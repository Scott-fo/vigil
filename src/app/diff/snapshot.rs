use std::sync::Arc;

use color_eyre::eyre::WrapErr;
use tokio::task;

use super::*;

impl App {
    pub(in crate::app) fn queue_review_diff_snapshot_load(&mut self) {
        self.cancel_inflight_review_diff_snapshot();
        self.review_diff_stream_index = None;
        self.review_diff_text_index = None;
        self.review_diff_snapshot = None;

        if self.files.is_empty() {
            self.review_diff_snapshot_request_id =
                self.review_diff_snapshot_request_id.saturating_add(1);
            return;
        }

        self.review_diff_snapshot_request_id =
            self.review_diff_snapshot_request_id.saturating_add(1);
        let request_id = self.review_diff_snapshot_request_id;
        let generation = self.diff_cache_generation;
        let repo_root = self.repo_root.clone();
        let files = self.files.clone();
        let review_mode = self.review_mode.clone();
        let sender = self.events.sender();

        self.review_diff_snapshot_task = Some(task::spawn(async move {
            let stream_sender = sender.clone();
            let on_file = move |file| {
                let _ = stream_sender.send(Event::ReviewDiffFileStreamed {
                    request_id,
                    generation,
                    file,
                });
            };
            let text_index =
                match load_review_diff_text_index(&repo_root, &files, &review_mode, on_file)
                    .await
                    .map(Arc::new)
                {
                    Ok(text_index) => {
                        let _ = sender.send(Event::ReviewDiffTextIndexLoaded {
                            request_id,
                            generation,
                            result: Ok(Arc::clone(&text_index)),
                        });
                        text_index
                    }
                    Err(error) => {
                        let error = error.to_string();
                        let _ = sender.send(Event::ReviewDiffTextIndexLoaded {
                            request_id,
                            generation,
                            result: Err(error.clone()),
                        });
                        let _ = sender.send(Event::ReviewDiffSnapshotLoaded {
                            request_id,
                            generation,
                            result: Err(error),
                        });
                        return;
                    }
                };

            let cache_key_prefix = review_diff_snapshot_cache_key_prefix(&review_mode);
            let result = build_review_diff_snapshot_from_text_index(text_index, cache_key_prefix)
                .await
                .map(|snapshot| snapshot.with_generation(generation))
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::ReviewDiffSnapshotLoaded {
                request_id,
                generation,
                result,
            });
        }));
    }

    pub(in crate::app) fn cancel_inflight_review_diff_snapshot(&mut self) {
        if let Some(task) = self.review_diff_snapshot_task.take() {
            task.abort();
        }
    }

    pub(in crate::app) fn clear_review_diff_snapshot(&mut self) {
        self.cancel_inflight_review_diff_snapshot();
        self.review_diff_snapshot_request_id =
            self.review_diff_snapshot_request_id.saturating_add(1);
        self.review_diff_stream_index = None;
        self.review_diff_text_index = None;
        self.review_diff_snapshot = None;
    }

    pub(in crate::app) fn handle_review_diff_text_index_loaded(
        &mut self,
        request_id: u64,
        generation: u64,
        result: Result<Arc<git::ReviewDiffTextIndex>, String>,
    ) -> bool {
        if request_id != self.review_diff_snapshot_request_id
            || generation != self.diff_cache_generation
        {
            return false;
        }

        match result {
            Ok(text_index) => {
                self.review_diff_stream_index = None;
                self.review_diff_text_index = Some(text_index);
                let changed = self.load_selected_diff_from_review_text_index();
                self.spawn_diff_prefetch();
                changed
            }
            Err(error) => {
                self.review_diff_stream_index = None;
                self.review_diff_text_index = None;
                self.status_message = Some(format!("review diff text index failed: {error}"));
                false
            }
        }
    }

    pub(in crate::app) fn handle_review_diff_file_streamed(
        &mut self,
        request_id: u64,
        generation: u64,
        file: git::ReviewDiffStreamedFile,
    ) -> bool {
        if request_id != self.review_diff_snapshot_request_id
            || generation != self.diff_cache_generation
            || self.review_diff_text_index.is_some()
        {
            return false;
        }

        let path = file.path.clone();
        self.review_diff_stream_index
            .get_or_insert_with(git::ReviewDiffPartialTextIndex::default)
            .insert_file_diff(file.path, file.diff);

        let Some(selected_file) = self.selected_file().cloned() else {
            return false;
        };

        if selected_file.path != path {
            let mut cached_visible_plain = false;
            if self.streamed_path_is_near_sidebar(&path)
                && let Some(file_index) = self.file_index_by_path(&path)
            {
                let file = self.files[file_index].clone();
                let cache_key = self.diff_cache_key(&file);
                if !self.diff_view_cache.has_plain(&cache_key)
                    && let Some(diff_view) = self
                        .review_diff_stream_index
                        .as_ref()
                        .and_then(|stream_index| stream_index.build_diff_view(&file))
                {
                    self.diff_view_cache.insert_plain(cache_key, diff_view);
                    cached_visible_plain = true;
                }
            }
            if cached_visible_plain && self.diff_highlight_complete {
                self.spawn_diff_prefetch();
            }
            return false;
        }

        let cache_key = self.diff_cache_key(&selected_file);
        self.load_selected_diff_from_review_stream_cache(&selected_file, &cache_key)
    }

    fn streamed_path_is_near_sidebar(&self, path: &str) -> bool {
        let visible_start = self.sidebar_scroll.saturating_sub(DIFF_PREFETCH_DISTANCE);
        let visible_end = self
            .sidebar_scroll
            .saturating_add(self.sidebar_viewport_height)
            .saturating_add(DIFF_PREFETCH_DISTANCE)
            .min(self.sidebar_items.len());

        self.sidebar_items
            .get(visible_start..visible_end)
            .unwrap_or_default()
            .iter()
            .any(|item| item.file().is_some_and(|file| file.path == path))
    }

    pub(in crate::app) fn handle_review_diff_snapshot_loaded(
        &mut self,
        request_id: u64,
        generation: u64,
        result: Result<git::ReviewDiffSnapshot, String>,
    ) -> bool {
        if request_id != self.review_diff_snapshot_request_id
            || generation != self.diff_cache_generation
        {
            return false;
        }

        self.review_diff_snapshot_task = None;
        match result {
            Ok(snapshot) => {
                self.review_diff_stats = Some(snapshot.stats());
                self.review_diff_stats_error = None;
                self.review_diff_snapshot = Some(Arc::new(snapshot));
                self.queue_diff_search_index_load();
                let changed = self.load_selected_diff_from_review_snapshot();
                self.spawn_diff_prefetch();
                changed || self.diff_search_modal_open
            }
            Err(error) => {
                self.review_diff_snapshot = None;
                if self.diff_search_index.is_none() {
                    self.queue_diff_search_index_load();
                }
                self.status_message = Some(format!("review diff snapshot failed: {error}"));
                self.diff_search_modal_open
            }
        }
    }

    pub(in crate::app) fn build_diff_view_from_review_snapshot(
        &self,
        file: &FileEntry,
    ) -> Option<DiffView> {
        self.review_diff_snapshot.as_ref()?.build_diff_view(file)
    }

    pub(in crate::app) fn load_selected_diff_from_review_snapshot(&mut self) -> bool {
        let Some(file) = self.selected_file().cloned() else {
            return false;
        };
        let cache_key = self.diff_cache_key(&file);

        if self.diff_view_cache.get_highlighted(&cache_key).is_some()
            || self.diff_view_cache.get_plain(&cache_key).is_some()
        {
            return false;
        }

        let Some(diff_view) = self.build_diff_view_from_review_snapshot(&file) else {
            return false;
        };

        self.diff_view_cache
            .insert_plain(cache_key.clone(), diff_view.clone());

        if self.pending_diff_cache_key.as_ref() != Some(&cache_key) {
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
}

async fn load_review_diff_text_index(
    repo_root: &std::path::Path,
    files: &[git::FileEntry],
    review_mode: &ReviewMode,
    on_file: impl FnMut(git::ReviewDiffStreamedFile) + Send,
) -> color_eyre::Result<git::ReviewDiffTextIndex> {
    match review_mode {
        ReviewMode::WorkingTree => {
            git::load_review_diff_text_index_for_working_tree_streaming(repo_root, files, on_file)
                .await
        }
        ReviewMode::CommitCompare(selection) => {
            git::load_review_diff_text_index_for_commit_compare_streaming(
                repo_root, selection, on_file,
            )
            .await
        }
        ReviewMode::BranchCompare(selection) => {
            git::load_review_diff_text_index_for_branch_compare_streaming(
                repo_root, selection, on_file,
            )
            .await
        }
    }
}

async fn build_review_diff_snapshot_from_text_index(
    text_index: Arc<git::ReviewDiffTextIndex>,
    cache_key_prefix: &'static str,
) -> color_eyre::Result<git::ReviewDiffSnapshot> {
    task::spawn_blocking(move || {
        git::ReviewDiffSnapshot::from_diff_text(text_index.diff_text(), Some(cache_key_prefix))
    })
    .await
    .wrap_err("review diff snapshot parse task failed")?
}

fn review_diff_snapshot_cache_key_prefix(review_mode: &ReviewMode) -> &'static str {
    match review_mode {
        ReviewMode::WorkingTree => "working-tree",
        ReviewMode::CommitCompare(_) => "commit",
        ReviewMode::BranchCompare(_) => "branch",
    }
}
