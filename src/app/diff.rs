use std::collections::VecDeque;

use tokio::task;

use super::*;

pub(super) const DIFF_CACHE_CAPACITY: usize = 32;
pub(super) const DIFF_PREFETCH_DISTANCE: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffCacheKey {
    review_scope: String,
    file_path: String,
    file_status: String,
}

#[derive(Debug, Clone)]
struct DiffCacheEntry {
    key: DiffCacheKey,
    plain: DiffView,
    highlighted: Option<DiffView>,
    highlight_complete: bool,
}

#[derive(Debug, Default)]
pub(super) struct DiffViewCache {
    entries: VecDeque<DiffCacheEntry>,
}

impl DiffViewCache {
    fn contains(&self, key: &DiffCacheKey) -> bool {
        self.entries.iter().any(|entry| &entry.key == key)
    }

    fn get_plain(&mut self, key: &DiffCacheKey) -> Option<DiffView> {
        self.touch_entry(key).map(|entry| entry.plain.clone())
    }

    fn get_highlighted(&mut self, key: &DiffCacheKey) -> Option<(DiffView, bool)> {
        self.touch_entry(key).and_then(|entry| {
            entry
                .highlighted
                .clone()
                .map(|view| (view, entry.highlight_complete))
        })
    }

    pub(super) fn insert_plain(&mut self, key: DiffCacheKey, plain: DiffView) {
        let (highlighted, highlight_complete) = self
            .remove_entry(&key)
            .map(|entry| (entry.highlighted, entry.highlight_complete))
            .unwrap_or((None, false));
        self.entries.push_back(DiffCacheEntry {
            key,
            plain,
            highlighted,
            highlight_complete,
        });
        self.trim();
    }

    pub(super) fn insert_highlighted(
        &mut self,
        key: DiffCacheKey,
        highlighted: DiffView,
        complete: bool,
    ) {
        if let Some(mut entry) = self.remove_entry(&key) {
            match entry.highlighted.as_mut() {
                Some(existing) if !complete => existing.merge_highlighting_from(&highlighted),
                Some(existing) if complete => *existing = highlighted,
                Some(_) => {}
                None => entry.highlighted = Some(highlighted),
            }
            entry.highlight_complete |= complete;
            self.entries.push_back(entry);
        } else {
            self.entries.push_back(DiffCacheEntry {
                key,
                plain: highlighted.clone(),
                highlighted: Some(highlighted),
                highlight_complete: complete,
            });
        }
        self.trim();
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }

    fn touch_entry(&mut self, key: &DiffCacheKey) -> Option<&DiffCacheEntry> {
        let entry = self.remove_entry(key)?;
        self.entries.push_back(entry);
        self.entries.back()
    }

    fn remove_entry(&mut self, key: &DiffCacheKey) -> Option<DiffCacheEntry> {
        let index = self.entries.iter().position(|entry| &entry.key == key)?;
        self.entries.remove(index)
    }

    fn trim(&mut self) {
        while self.entries.len() > DIFF_CACHE_CAPACITY {
            let _ = self.entries.pop_front();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DiffViewport {
    mode: DiffViewMode,
    width: usize,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedDiffViewport {
    pub mode: DiffViewMode,
    pub width: usize,
    pub start: usize,
    pub end: usize,
    pub rendered_line_count: usize,
    pub selected_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiffHighlightJobKind {
    Viewport(DiffViewport),
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffHighlightJob {
    request_id: u64,
    key: DiffCacheKey,
    kind: DiffHighlightJobKind,
}

impl App {
    pub(super) fn current_diff_display_width(&self) -> usize {
        self.diff_viewport
            .map(|viewport| viewport.width)
            .unwrap_or(usize::MAX)
    }

    fn build_diff_cache_key(review_mode: &ReviewMode, file: &FileEntry) -> DiffCacheKey {
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

    fn diff_cache_key(&self, file: &FileEntry) -> DiffCacheKey {
        Self::build_diff_cache_key(&self.review_mode, file)
    }

    fn spawn_diff_prefetch(&mut self) {
        let Some(selected_visible_index) = self.selected_visible_file_index() else {
            return;
        };

        let visible_paths = self.visible_file_paths();
        if visible_paths.is_empty() {
            return;
        }

        let mut prefetch_files = Vec::new();
        for distance in 1..=DIFF_PREFETCH_DISTANCE {
            for candidate_index in [
                selected_visible_index.checked_sub(distance),
                selected_visible_index.checked_add(distance),
            ] {
                let Some(candidate_index) = candidate_index else {
                    continue;
                };
                let Some(path) = visible_paths.get(candidate_index) else {
                    continue;
                };
                let Some(file_index) = self.file_index_by_path(path) else {
                    continue;
                };
                let file = self.files[file_index].clone();
                let cache_key = self.diff_cache_key(&file);
                if self.diff_view_cache.contains(&cache_key) {
                    continue;
                }
                prefetch_files.push((cache_key, file));
            }
        }

        if prefetch_files.is_empty() {
            return;
        }

        let generation = self.diff_cache_generation;
        let review_mode = self.review_mode.clone();
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();

        self.track_background_task(task::spawn(async move {
            for (cache_key, file) in prefetch_files {
                let preview_result = match &review_mode {
                    ReviewMode::WorkingTree => {
                        git::load_diff_preview_for_working_tree(&repo_root, &file, false).await
                    }
                    ReviewMode::CommitCompare(selection) => {
                        git::load_diff_preview_for_commit_compare(
                            &repo_root, &file, selection, false,
                        )
                        .await
                    }
                    ReviewMode::BranchCompare(selection) => {
                        git::load_diff_preview_for_branch_compare(
                            &repo_root, &file, selection, false,
                        )
                        .await
                    }
                };

                let Ok(preview) = preview_result else {
                    continue;
                };

                let plain_file = file.clone();
                let plain_result = task::spawn_blocking(move || {
                    git::build_diff_view_from_preview_data(&preview, &plain_file, None)
                })
                .await;

                let Ok(Ok(plain_view)) = plain_result else {
                    continue;
                };

                let _ = sender.send(Event::DiffPrefetched(Box::new(DiffPrefetchedEvent {
                    generation,
                    key: cache_key,
                    plain: plain_view,
                    highlighted: None,
                })));
            }
        }));
    }

    pub(super) fn queue_selected_diff_load(&mut self, show_loading: bool, reset_viewport: bool) {
        self.cancel_inflight_diff_load();
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

        self.spawn_diff_prefetch();

        let cache_key = self.diff_cache_key(&file);
        self.pending_diff_cache_key = Some(cache_key.clone());
        if let Some((mut diff_view, highlight_complete)) =
            self.diff_view_cache.get_highlighted(&cache_key)
        {
            let max_index = diff_view
                .last_selectable_index(self.diff_view_mode, self.current_diff_display_width());
            self.selected_diff_line_index = self.selected_diff_line_index.min(max_index);
            self.diff_view = diff_view;
            self.diff_highlight_complete = highlight_complete;
            self.status_message = Some(self.current_status_message());
            return;
        }

        if let Some(mut plain_diff_view) = self.diff_view_cache.get_plain(&cache_key) {
            let max_index = plain_diff_view
                .last_selectable_index(self.diff_view_mode, self.current_diff_display_width());
            self.selected_diff_line_index = self.selected_diff_line_index.min(max_index);
            self.diff_view = plain_diff_view;
            self.status_message = Some(self.current_status_message());
            return;
        }

        if show_loading {
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

    pub(super) fn cancel_inflight_diff_load(&mut self) {
        if let Some(task) = self.diff_load_task.take() {
            task.abort();
        }
        self.cancel_inflight_diff_highlight();
        self.pending_diff_cache_key = None;
        self.diff_highlight_complete = false;
    }

    fn cancel_inflight_diff_highlight(&mut self) {
        if let Some(task) = self.diff_highlight_task.take() {
            task.abort();
        }
        self.diff_highlight_job = None;
    }

    pub(super) fn maybe_queue_diff_highlight(&mut self) {
        let Some(highlight_registry) = self.highlight_registry.clone() else {
            self.diff_highlight_complete = true;
            return;
        };
        let Some(cache_key) = self.pending_diff_cache_key.clone() else {
            return;
        };
        let Some(file) = self.selected_file().cloned() else {
            self.diff_highlight_complete = true;
            return;
        };
        let Some(_filetype) = file.filetype else {
            self.diff_highlight_complete = true;
            return;
        };
        let Some(viewport) = self.diff_viewport else {
            return;
        };

        let viewport_ready = self.diff_view.is_display_range_fully_highlighted(
            viewport.mode,
            viewport.width,
            viewport.start,
            viewport.end,
        );

        if !viewport_ready {
            let needs_new_viewport_job = !matches!(
                self.diff_highlight_job.as_ref(),
                Some(DiffHighlightJob {
                    request_id,
                    key,
                    kind: DiffHighlightJobKind::Viewport(existing_viewport),
                }) if *request_id == self.diff_request_id
                    && *key == cache_key
                    && *existing_viewport == viewport
            );
            if needs_new_viewport_job {
                self.cancel_inflight_diff_highlight();
                self.spawn_selected_diff_highlight(
                    cache_key,
                    file.clone(),
                    highlight_registry,
                    DiffHighlightJobKind::Viewport(viewport),
                );
            }
            return;
        }

        if self.diff_highlight_complete {
            return;
        }

        let full_inflight = matches!(
            self.diff_highlight_job.as_ref(),
            Some(DiffHighlightJob {
                request_id,
                key,
                kind: DiffHighlightJobKind::Full,
            }) if *request_id == self.diff_request_id && *key == cache_key
        );
        if full_inflight {
            return;
        }

        self.cancel_inflight_diff_highlight();
        self.spawn_selected_diff_highlight(
            cache_key,
            file,
            highlight_registry,
            DiffHighlightJobKind::Full,
        );
    }

    fn spawn_selected_diff_highlight(
        &mut self,
        cache_key: DiffCacheKey,
        file: FileEntry,
        highlight_registry: SharedHighlightRegistry,
        kind: DiffHighlightJobKind,
    ) {
        let request_id = self.diff_request_id;
        let sender = self.events.sender();
        let mut diff_view = self.diff_view.clone();
        let review_mode = self.review_mode.clone();
        let repo_root = self.repo_root.clone();
        self.diff_highlight_job = Some(DiffHighlightJob {
            request_id,
            key: cache_key,
            kind: kind.clone(),
        });
        self.diff_highlight_task = Some(task::spawn(async move {
            let complete = matches!(kind, DiffHighlightJobKind::Full);
            let result = match kind {
                DiffHighlightJobKind::Viewport(viewport) => task::spawn_blocking(move || {
                    diff_view.apply_syntax_highlighting_for_display_range(
                        viewport.mode,
                        viewport.width,
                        viewport.start,
                        viewport.end,
                        file.filetype,
                        highlight_registry.as_ref(),
                    );
                    Ok::<_, String>(diff_view)
                })
                .await
                .unwrap_or_else(|error| Err(error.to_string())),
                DiffHighlightJobKind::Full => {
                    let preview_result = match &review_mode {
                        ReviewMode::WorkingTree => {
                            git::load_diff_preview_for_working_tree(&repo_root, &file, true).await
                        }
                        ReviewMode::CommitCompare(selection) => {
                            git::load_diff_preview_for_commit_compare(
                                &repo_root, &file, selection, true,
                            )
                            .await
                        }
                        ReviewMode::BranchCompare(selection) => {
                            git::load_diff_preview_for_branch_compare(
                                &repo_root, &file, selection, true,
                            )
                            .await
                        }
                    };

                    let preview = match preview_result {
                        Ok(preview) => preview,
                        Err(error) => {
                            let _ = sender.send(Event::DiffHighlightUpdated {
                                request_id,
                                complete,
                                result: Err(error.to_string()),
                            });
                            return;
                        }
                    };

                    task::spawn_blocking(move || {
                        let mut diff_view =
                            git::build_diff_view_from_preview_data(&preview, &file, None)
                                .map_err(|error| error.to_string())?;
                        diff_view.apply_exact_syntax_highlighting(
                            file.filetype,
                            highlight_registry.as_ref(),
                        );
                        Ok::<_, String>(diff_view)
                    })
                    .await
                    .unwrap_or_else(|error| Err(error.to_string()))
                }
            };
            let _ = sender.send(Event::DiffHighlightUpdated {
                request_id,
                complete,
                result,
            });
        }));
    }

    pub(super) fn move_diff_selection(&mut self, delta: i32) {
        self.selected_diff_line_index = self.diff_view.move_selection(
            self.diff_view_mode,
            self.current_diff_display_width(),
            self.selected_diff_line_index,
            delta,
        );
    }

    pub fn update_diff_viewport(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        visible_start: usize,
        visible_end: usize,
    ) {
        self.diff_viewport = (width > 0 && visible_start < visible_end).then_some(DiffViewport {
            mode,
            width,
            start: visible_start,
            end: visible_end,
        });
    }

    pub fn prepare_diff_viewport(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        viewport_height: usize,
    ) -> Option<PreparedDiffViewport> {
        if width == 0 || viewport_height == 0 {
            return None;
        }

        let rendered_lines = self.diff_view.rendered_lines(mode, width);
        if rendered_lines.is_empty() {
            return None;
        }

        let max_scroll = rendered_lines
            .len()
            .saturating_sub(viewport_height)
            .min(u16::MAX as usize) as u16;
        if self.diff_scroll > max_scroll {
            self.diff_scroll = max_scroll;
        }

        let selected_index = self
            .selected_diff_line_index
            .min(rendered_lines.len().saturating_sub(1));
        if self.active_pane == ActivePane::Diff {
            if selected_index < self.diff_scroll as usize {
                self.diff_scroll = selected_index.min(max_scroll as usize) as u16;
            } else {
                let visible_end = (self.diff_scroll as usize).saturating_add(viewport_height);
                if selected_index >= visible_end {
                    self.diff_scroll = selected_index
                        .saturating_add(1)
                        .saturating_sub(viewport_height)
                        .min(max_scroll as usize) as u16;
                }
            }
        }

        let visible_start = (self.diff_scroll as usize).min(max_scroll as usize);
        let visible_end = (visible_start + viewport_height).min(rendered_lines.len());
        if visible_start >= visible_end {
            return None;
        }

        Some(PreparedDiffViewport {
            mode,
            width,
            start: visible_start,
            end: visible_end,
            rendered_line_count: rendered_lines.len(),
            selected_index,
        })
    }

    pub(super) fn page_diff(&mut self, delta: i32) {
        self.move_diff_selection(delta);
    }

    fn scroll_diff(&mut self, delta: i32) {
        self.diff_scroll = if delta.is_negative() {
            self.diff_scroll.saturating_sub(delta.unsigned_abs() as u16)
        } else {
            self.diff_scroll.saturating_add(delta as u16)
        };
    }

    pub(super) fn page_or_scroll_diff(&mut self, delta: i32) {
        match self.active_pane {
            ActivePane::Diff => self.page_diff(delta),
            ActivePane::Sidebar => self.scroll_diff(delta),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_test_app() -> App {
        App::new_for_benchmarks(PathBuf::from("/tmp/vigil-app-tests"))
    }

    fn build_cache_key(index: usize) -> DiffCacheKey {
        DiffCacheKey {
            review_scope: "working-tree".to_string(),
            file_path: format!("src/file-{index}.rs"),
            file_status: "M ".to_string(),
        }
    }

    fn build_diff_view(line_count: usize) -> DiffView {
        let mut diff = format!(
            "diff --git a/src/app.rs b/src/app.rs\n\
--- a/src/app.rs\n\
+++ b/src/app.rs\n\
@@ -1,0 +1,{} @@\n",
            line_count
        );
        for index in 0..line_count {
            diff.push_str(&format!("+fn line_{index}() {{}}\n"));
        }
        git::build_diff_view_from_diff_text(&diff, Some("rust"))
    }

    #[test]
    fn diff_view_cache_touches_recent_entries_before_trimming() {
        let mut cache = DiffViewCache::default();

        for index in 0..DIFF_CACHE_CAPACITY {
            cache.insert_plain(
                build_cache_key(index),
                DiffView::empty(format!("plain-{index}")),
            );
        }

        let touched_key = build_cache_key(0);
        let evicted_key = build_cache_key(1);
        assert!(cache.get_plain(&touched_key).is_some());

        cache.insert_plain(
            build_cache_key(DIFF_CACHE_CAPACITY),
            DiffView::empty("overflow"),
        );

        assert!(cache.get_plain(&touched_key).is_some());
        assert!(cache.get_plain(&evicted_key).is_none());
    }

    #[test]
    fn prepare_diff_viewport_keeps_selection_visible_in_diff_pane() {
        let mut app = build_test_app();
        app.active_pane = ActivePane::Diff;
        app.diff_view = build_diff_view(120);

        let rendered_line_count = app.diff_view.display_line_count(DiffViewMode::Split, 160);
        app.selected_diff_line_index = rendered_line_count.saturating_sub(1);
        app.diff_scroll = 0;

        let viewport = app
            .prepare_diff_viewport(DiffViewMode::Split, 160, 12)
            .expect("viewport should be available");

        assert!(viewport.start <= viewport.selected_index);
        assert!(viewport.selected_index < viewport.end);
        assert!(app.diff_scroll > 0);
    }

    #[test]
    fn prepare_diff_viewport_does_not_auto_scroll_from_sidebar_pane() {
        let mut app = build_test_app();
        app.active_pane = ActivePane::Sidebar;
        app.diff_view = build_diff_view(120);
        app.selected_diff_line_index = 40;
        app.diff_scroll = 0;

        let viewport = app
            .prepare_diff_viewport(DiffViewMode::Split, 160, 12)
            .expect("viewport should be available");

        assert_eq!(viewport.start, 0);
        assert_eq!(app.diff_scroll, 0);
    }

    #[test]
    fn build_diff_cache_key_includes_review_scope() {
        let file = FileEntry {
            status: "M ".to_string(),
            path: "src/app.rs".to_string(),
            label: "app.rs".to_string(),
            filetype: Some("rust"),
        };
        let commit_key = App::build_diff_cache_key(
            &ReviewMode::CommitCompare(CommitCompareSelection {
                base_ref: "base".to_string(),
                commit_hash: "commit".to_string(),
                short_hash: "abc123".to_string(),
                subject: "subject".to_string(),
            }),
            &file,
        );
        let branch_key = App::build_diff_cache_key(
            &ReviewMode::BranchCompare(BranchCompareSelection {
                source_ref: "feature".to_string(),
                destination_ref: "main".to_string(),
            }),
            &file,
        );

        assert_eq!(commit_key.review_scope, "commit:base:commit");
        assert_eq!(branch_key.review_scope, "branch:feature:main");
    }
}
