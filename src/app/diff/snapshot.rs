use std::sync::Arc;

use tokio::task;

use super::*;

impl App {
    pub(in crate::app) fn queue_review_diff_snapshot_load(&mut self) {
        self.cancel_inflight_review_diff_snapshot();
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
            let result = load_review_diff_snapshot(&repo_root, &files, &review_mode)
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
        self.review_diff_snapshot = None;
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

async fn load_review_diff_snapshot(
    repo_root: &std::path::Path,
    files: &[git::FileEntry],
    review_mode: &ReviewMode,
) -> color_eyre::Result<git::ReviewDiffSnapshot> {
    match review_mode {
        ReviewMode::WorkingTree => {
            git::load_review_diff_snapshot_for_working_tree(repo_root, files).await
        }
        ReviewMode::CommitCompare(selection) => {
            git::load_review_diff_snapshot_for_commit_compare(repo_root, selection).await
        }
        ReviewMode::BranchCompare(selection) => {
            git::load_review_diff_snapshot_for_branch_compare(repo_root, selection).await
        }
    }
}
