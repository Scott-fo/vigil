use std::{cmp::Ordering, collections::HashSet, sync::Arc};

use tokio::task;

use super::*;

const DIFF_SNAPSHOT_PREFETCH_CONCURRENCY: usize = 4;

impl App {
    pub(super) fn build_diff_cache_key(review_mode: &ReviewMode, file: &FileEntry) -> DiffCacheKey {
        let review_scope = match review_mode {
            ReviewMode::WorkingTree => "working-tree".to_string(),
            ReviewMode::CommitCompare(selection) => {
                format!("commit:{}:{}", selection.base_ref, selection.commit_hash)
            }
            ReviewMode::BranchCompare(selection) => format!(
                "branch:{}:{}",
                selection.source_ref, selection.destination_ref
            ),
        };

        DiffCacheKey {
            review_scope,
            file_path: file.path.clone(),
            file_status: file.status.clone(),
        }
    }

    pub(in crate::app) fn diff_cache_key(&self, file: &FileEntry) -> DiffCacheKey {
        Self::build_diff_cache_key(&self.review_mode, file)
    }

    fn update_diff_prefetch_direction(&mut self) {
        let current_index = self.selected_file_index;
        self.diff_prefetch_direction = match self.diff_prefetch_anchor_file_index {
            Some(previous_index) => match current_index.cmp(&previous_index) {
                Ordering::Greater => DiffPrefetchDirection::Forward,
                Ordering::Less => DiffPrefetchDirection::Backward,
                Ordering::Equal => self.diff_prefetch_direction,
            },
            None => DiffPrefetchDirection::Neutral,
        };
        self.diff_prefetch_anchor_file_index = Some(current_index);
    }

    pub(in crate::app) fn spawn_diff_prefetch(&mut self) {
        self.cancel_inflight_diff_prefetch();

        let Some(selected_visible_index) = self.selected_visible_file_index() else {
            return;
        };

        let visible_paths = self.visible_file_paths();
        if visible_paths.is_empty() {
            return;
        }

        let prefetch_files = self.diff_prefetch_files(selected_visible_index, &visible_paths);

        if prefetch_files.is_empty() {
            return;
        }

        let generation = self.diff_cache_generation;
        let review_mode = self.review_mode.clone();
        let review_diff_snapshot = self.review_diff_snapshot.clone();
        let review_diff_text_index = self.review_diff_text_index.clone();
        let whole_diff_inflight =
            review_diff_text_index.is_none() && self.review_diff_snapshot_task.is_some();
        let repo_root = self.repo_root.clone();
        let highlight_registry = self.highlight_registry.clone();
        let sender = self.events.sender();

        self.diff_prefetch_task = Some(task::spawn(async move {
            let mut memory_jobs = task::JoinSet::new();
            for (cache_key, file, should_prefetch_highlight) in prefetch_files {
                if let Some(snapshot) = review_diff_snapshot
                    .as_ref()
                    .filter(|snapshot| snapshot.contains_file(&file.path))
                    .cloned()
                    .filter(|_| !should_prefetch_highlight)
                {
                    memory_jobs.spawn_blocking(move || {
                        build_snapshot_prefetch_event(generation, cache_key, file, snapshot)
                    });
                    if memory_jobs.len() >= DIFF_SNAPSHOT_PREFETCH_CONCURRENCY {
                        send_next_memory_prefetch(&mut memory_jobs, &sender).await;
                    }
                    continue;
                }

                if let Some(text_index) = review_diff_text_index
                    .as_ref()
                    .filter(|text_index| text_index.contains_file(&file.path))
                    .cloned()
                    .filter(|_| !should_prefetch_highlight)
                {
                    memory_jobs.spawn_blocking(move || {
                        build_text_index_prefetch_event(generation, cache_key, file, text_index)
                    });
                    if memory_jobs.len() >= DIFF_SNAPSHOT_PREFETCH_CONCURRENCY {
                        send_next_memory_prefetch(&mut memory_jobs, &sender).await;
                    }
                    continue;
                }

                if whole_diff_inflight && !file.status.contains('U') {
                    continue;
                }

                let include_exact_context = should_prefetch_highlight;
                let preview_result = match &review_mode {
                    ReviewMode::WorkingTree => {
                        git::load_diff_preview_for_working_tree(
                            &repo_root,
                            &file,
                            include_exact_context,
                        )
                        .await
                    }
                    ReviewMode::CommitCompare(selection) => {
                        git::load_diff_preview_for_commit_compare(
                            &repo_root,
                            &file,
                            selection,
                            include_exact_context,
                        )
                        .await
                    }
                    ReviewMode::BranchCompare(selection) => {
                        git::load_diff_preview_for_branch_compare(
                            &repo_root,
                            &file,
                            selection,
                            include_exact_context,
                        )
                        .await
                    }
                };

                let Ok(preview) = preview_result else {
                    continue;
                };

                let registry = highlight_registry.clone();
                let build_result = task::spawn_blocking(move || {
                    let plain = git::build_diff_view_from_preview_data(&preview, &file, None)?;
                    let highlighted = if should_prefetch_highlight {
                        registry.map(|registry| {
                            let mut highlighted = plain.clone();
                            highlighted
                                .apply_exact_syntax_highlighting(file.filetype, registry.as_ref());
                            highlighted
                        })
                    } else {
                        None
                    };
                    Ok::<_, color_eyre::Report>((plain, highlighted))
                })
                .await;

                let Ok(Ok((plain_view, highlighted_view))) = build_result else {
                    continue;
                };
                let highlight_complete = highlighted_view.is_some();

                let _ = sender.send(Event::DiffPrefetched(Box::new(DiffPrefetchedEvent {
                    generation,
                    key: cache_key,
                    plain: plain_view,
                    highlighted: highlighted_view,
                    highlight_complete,
                })));
            }
            drain_memory_prefetches(&mut memory_jobs, &sender).await;
        }));
    }

    pub(in crate::app) fn cancel_inflight_diff_prefetch(&mut self) {
        if let Some(task) = self.diff_prefetch_task.take() {
            task.abort();
        }
    }

    pub(in crate::app) fn diff_prefetch_files(
        &self,
        selected_visible_index: usize,
        visible_paths: &[String],
    ) -> Vec<(DiffCacheKey, FileEntry, bool)> {
        let selected_path = self.selected_file().map(|file| file.path.as_str());
        let mut seen_paths = HashSet::new();
        let mut prefetch_files = Vec::new();

        let directional_distance = if self.review_diff_snapshot.is_some() {
            DIFF_DIRECTIONAL_PREFETCH_DISTANCE
        } else {
            DIFF_PREFETCH_DISTANCE
        };

        match self.diff_prefetch_direction {
            DiffPrefetchDirection::Forward => {
                self.push_diff_prefetch_range(
                    selected_visible_index,
                    visible_paths,
                    1,
                    directional_distance,
                    selected_path,
                    &mut seen_paths,
                    &mut prefetch_files,
                );
                self.push_diff_prefetch_range(
                    selected_visible_index,
                    visible_paths,
                    -1,
                    DIFF_PREFETCH_DISTANCE,
                    selected_path,
                    &mut seen_paths,
                    &mut prefetch_files,
                );
            }
            DiffPrefetchDirection::Backward => {
                self.push_diff_prefetch_range(
                    selected_visible_index,
                    visible_paths,
                    -1,
                    directional_distance,
                    selected_path,
                    &mut seen_paths,
                    &mut prefetch_files,
                );
                self.push_diff_prefetch_range(
                    selected_visible_index,
                    visible_paths,
                    1,
                    DIFF_PREFETCH_DISTANCE,
                    selected_path,
                    &mut seen_paths,
                    &mut prefetch_files,
                );
            }
            DiffPrefetchDirection::Neutral => {
                for distance in 1..=DIFF_PREFETCH_DISTANCE {
                    self.push_diff_prefetch_offset(
                        selected_visible_index,
                        visible_paths,
                        distance as isize,
                        selected_path,
                        &mut seen_paths,
                        &mut prefetch_files,
                    );
                    self.push_diff_prefetch_offset(
                        selected_visible_index,
                        visible_paths,
                        -(distance as isize),
                        selected_path,
                        &mut seen_paths,
                        &mut prefetch_files,
                    );
                }
            }
        }

        let visible_start = self.sidebar_scroll.saturating_sub(DIFF_PREFETCH_DISTANCE);
        let visible_end = self
            .sidebar_scroll
            .saturating_add(self.sidebar_viewport_height)
            .saturating_add(DIFF_PREFETCH_DISTANCE)
            .min(self.sidebar_items.len());

        for item in self
            .sidebar_items
            .get(visible_start..visible_end)
            .unwrap_or_default()
        {
            let Some(file) = item.file() else {
                continue;
            };
            self.push_diff_prefetch_file(
                &file.path,
                selected_path,
                &mut seen_paths,
                &mut prefetch_files,
            );
        }

        prefetch_files
    }

    fn push_diff_prefetch_range(
        &self,
        selected_visible_index: usize,
        visible_paths: &[String],
        direction: isize,
        distance: usize,
        selected_path: Option<&str>,
        seen_paths: &mut HashSet<String>,
        prefetch_files: &mut Vec<(DiffCacheKey, FileEntry, bool)>,
    ) {
        for offset in 1..=distance {
            self.push_diff_prefetch_offset(
                selected_visible_index,
                visible_paths,
                direction * offset as isize,
                selected_path,
                seen_paths,
                prefetch_files,
            );
        }
    }

    fn push_diff_prefetch_offset(
        &self,
        selected_visible_index: usize,
        visible_paths: &[String],
        offset: isize,
        selected_path: Option<&str>,
        seen_paths: &mut HashSet<String>,
        prefetch_files: &mut Vec<(DiffCacheKey, FileEntry, bool)>,
    ) {
        let Some(candidate_index) = selected_visible_index.checked_add_signed(offset) else {
            return;
        };
        let Some(path) = visible_paths.get(candidate_index) else {
            return;
        };
        self.push_diff_prefetch_file(path, selected_path, seen_paths, prefetch_files);
    }

    fn push_diff_prefetch_file(
        &self,
        path: &str,
        selected_path: Option<&str>,
        seen_paths: &mut HashSet<String>,
        prefetch_files: &mut Vec<(DiffCacheKey, FileEntry, bool)>,
    ) {
        if Some(path) == selected_path || !seen_paths.insert(path.to_string()) {
            return;
        }

        let Some(file_index) = self.file_index_by_path(path) else {
            return;
        };
        let file = self.files[file_index].clone();
        let cache_key = self.diff_cache_key(&file);
        let has_plain = self.diff_view_cache.has_plain(&cache_key);
        let needs_highlight = self.highlight_registry.is_some()
            && file.filetype.is_some()
            && !self.diff_view_cache.has_complete_highlight(&cache_key);

        if has_plain && !needs_highlight {
            return;
        }

        let should_prefetch_highlight =
            has_plain && needs_highlight && prefetch_files.len() < DIFF_PREFETCH_DISTANCE;
        prefetch_files.push((cache_key, file, should_prefetch_highlight));
    }

    pub(in crate::app) fn queue_selected_diff_load(
        &mut self,
        show_loading: bool,
        reset_viewport: bool,
    ) {
        self.update_diff_prefetch_direction();
        self.cancel_inflight_selected_diff_load();
        self.clear_diff_text_selection();
        self.diff_request_id = self.diff_request_id.saturating_add(1);
        let request_id = self.diff_request_id;
        self.diff_highlight_complete = false;
        self.pending_diff_cache_key = None;

        if reset_viewport {
            self.diff_scroll = 0;
            self.selected_diff_line_index = 0;
        }

        let Some(file) = self.selected_file().cloned() else {
            self.diff_view = DiffView::empty("No changed files found.");
            return;
        };

        let cache_key = self.diff_cache_key(&file);
        self.pending_diff_cache_key = Some(cache_key.clone());
        if let Some((diff_view, highlight_complete)) =
            self.diff_view_cache.get_highlighted(&cache_key)
        {
            self.diff_view = diff_view;
            self.apply_pending_diff_search_target();
            self.diff_highlight_complete = highlight_complete;
            self.status_message = Some(self.current_status_message());
            self.spawn_diff_prefetch();
            return;
        }

        if let Some(plain_diff_view) = self.diff_view_cache.get_plain(&cache_key) {
            self.diff_view = plain_diff_view;
            self.apply_pending_diff_search_target();
            self.status_message = Some(self.current_status_message());
            self.spawn_diff_prefetch();
            return;
        }

        if self.load_selected_diff_from_review_snapshot_cache(&file, &cache_key) {
            return;
        }

        if self.load_selected_diff_from_review_text_index_cache(&file, &cache_key) {
            return;
        }

        if self.load_selected_diff_from_review_stream_cache(&file, &cache_key) {
            return;
        }

        if self.review_diff_text_index.is_none()
            && self.review_diff_snapshot_task.is_some()
            && !file.status.contains('U')
        {
            self.diff_view = DiffView::empty("");
            self.status_message = Some(self.current_status_message());
            return;
        }

        if show_loading && !self.diff_view.has_diff_rows() {
            self.diff_view = DiffView::empty("Loading diff...");
        }

        let review_mode = self.review_mode.clone();
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        let plain_file = file.clone();

        self.diff_load_task = Some(task::spawn(async move {
            let preview_result = match &review_mode {
                ReviewMode::WorkingTree => {
                    git::load_diff_preview_for_working_tree(&repo_root, &file, false).await
                }
                ReviewMode::CommitCompare(selection) => {
                    git::load_diff_preview_for_commit_compare(&repo_root, &file, selection, false)
                        .await
                }
                ReviewMode::BranchCompare(selection) => {
                    git::load_diff_preview_for_branch_compare(&repo_root, &file, selection, false)
                        .await
                }
            };

            let preview = match preview_result {
                Ok(preview) => preview,
                Err(error) => {
                    let _ = sender.send(Event::DiffLoaded {
                        request_id,
                        result: Err(error.to_string()),
                    });
                    return;
                }
            };

            let plain_result = task::spawn_blocking(move || {
                git::build_diff_view_from_preview_data(&preview, &plain_file, None)
                    .map_err(|error| error.to_string())
            })
            .await
            .unwrap_or_else(|error| Err(error.to_string()));

            let plain_diff_view = match plain_result {
                Ok(diff_view) => {
                    let _ = sender.send(Event::DiffLoaded {
                        request_id,
                        result: Ok(diff_view.clone()),
                    });
                    diff_view
                }
                Err(error) => {
                    let _ = sender.send(Event::DiffLoaded {
                        request_id,
                        result: Err(error),
                    });
                    return;
                }
            };
            let _ = plain_diff_view;
        }));
    }

    fn load_selected_diff_from_review_snapshot_cache(
        &mut self,
        file: &FileEntry,
        cache_key: &DiffCacheKey,
    ) -> bool {
        let Some(snapshot) = self
            .review_diff_snapshot
            .as_ref()
            .filter(|snapshot| snapshot.contains_file(&file.path))
        else {
            return false;
        };

        let Some(diff_view) = snapshot.build_diff_view(file) else {
            return false;
        };
        self.diff_view_cache
            .insert_plain(cache_key.clone(), diff_view.clone());
        self.diff_view = diff_view;
        self.apply_pending_diff_search_target();
        self.diff_highlight_complete = self.highlight_registry.is_none() || file.filetype.is_none();
        self.status_message = Some(self.current_status_message());
        true
    }

    pub(in crate::app) fn load_selected_diff_from_review_stream_cache(
        &mut self,
        file: &FileEntry,
        cache_key: &DiffCacheKey,
    ) -> bool {
        let Some(stream_index) = self
            .review_diff_stream_index
            .as_ref()
            .filter(|stream_index| stream_index.contains_file(&file.path))
        else {
            return false;
        };

        let Some(diff_view) = stream_index.build_diff_view(file) else {
            return false;
        };
        self.diff_view_cache
            .insert_plain(cache_key.clone(), diff_view.clone());

        if self.pending_diff_cache_key.as_ref() != Some(cache_key) {
            return false;
        }

        if let Some(task) = self.diff_load_task.take() {
            task.abort();
        }

        self.diff_view = diff_view;
        self.apply_pending_diff_search_target();
        self.diff_highlight_complete = self.highlight_registry.is_none() || file.filetype.is_none();
        self.status_message = Some(self.current_status_message());
        self.spawn_diff_prefetch();
        true
    }

    pub(in crate::app) fn load_selected_diff_from_review_text_index(&mut self) -> bool {
        let Some(file) = self.selected_file().cloned() else {
            return false;
        };
        let cache_key = self.diff_cache_key(&file);

        if self.diff_view_cache.get_highlighted(&cache_key).is_some()
            || self.diff_view_cache.get_plain(&cache_key).is_some()
        {
            return false;
        }

        self.load_selected_diff_from_review_text_index_cache(&file, &cache_key)
    }

    fn load_selected_diff_from_review_text_index_cache(
        &mut self,
        file: &FileEntry,
        cache_key: &DiffCacheKey,
    ) -> bool {
        let Some(text_index) = self
            .review_diff_text_index
            .as_ref()
            .filter(|text_index| text_index.contains_file(&file.path))
        else {
            return false;
        };

        let Some(diff_view) = text_index.build_diff_view(file) else {
            return false;
        };
        self.diff_view_cache
            .insert_plain(cache_key.clone(), diff_view.clone());

        if self.pending_diff_cache_key.as_ref() != Some(cache_key) {
            return false;
        }

        if let Some(task) = self.diff_load_task.take() {
            task.abort();
        }

        self.diff_view = diff_view;
        self.apply_pending_diff_search_target();
        self.diff_highlight_complete = self.highlight_registry.is_none() || file.filetype.is_none();
        self.status_message = Some(self.current_status_message());
        true
    }

    pub(in crate::app) fn cancel_inflight_diff_load(&mut self) {
        self.cancel_inflight_selected_diff_load();
        self.cancel_inflight_diff_prefetch();
    }

    fn cancel_inflight_selected_diff_load(&mut self) {
        if let Some(task) = self.diff_load_task.take() {
            task.abort();
        }
        self.cancel_inflight_diff_highlight();
        self.pending_diff_cache_key = None;
        self.diff_highlight_complete = false;
    }
}

pub(super) fn build_snapshot_prefetch_event(
    generation: u64,
    key: DiffCacheKey,
    file: FileEntry,
    snapshot: Arc<git::ReviewDiffSnapshot>,
) -> Option<DiffPrefetchedEvent> {
    let plain = snapshot.build_diff_view(&file)?;

    Some(DiffPrefetchedEvent {
        generation,
        key,
        plain,
        highlighted: None,
        highlight_complete: false,
    })
}

pub(super) fn build_text_index_prefetch_event(
    generation: u64,
    key: DiffCacheKey,
    file: FileEntry,
    text_index: Arc<git::ReviewDiffTextIndex>,
) -> Option<DiffPrefetchedEvent> {
    let plain = text_index.build_diff_view(&file)?;

    Some(DiffPrefetchedEvent {
        generation,
        key,
        plain,
        highlighted: None,
        highlight_complete: false,
    })
}

async fn send_next_memory_prefetch(
    jobs: &mut task::JoinSet<Option<DiffPrefetchedEvent>>,
    sender: &tokio::sync::mpsc::UnboundedSender<Event>,
) {
    let Some(joined) = jobs.join_next().await else {
        return;
    };
    let Ok(Some(prefetched)) = joined else {
        return;
    };
    let _ = sender.send(Event::DiffPrefetched(Box::new(prefetched)));
}

async fn drain_memory_prefetches(
    jobs: &mut task::JoinSet<Option<DiffPrefetchedEvent>>,
    sender: &tokio::sync::mpsc::UnboundedSender<Event>,
) {
    while !jobs.is_empty() {
        send_next_memory_prefetch(jobs, sender).await;
    }
}
