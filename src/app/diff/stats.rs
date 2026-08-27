//! Diff statistics modal state and background task lifecycle.
//!
//! The app keeps aggregate stats separate from the full review snapshot so the
//! modal can show useful totals as soon as a cheap `git diff --numstat` pass
//! completes. Snapshot totals still win once the parsed review snapshot is
//! available, because they include the exact rendered-line counts.

use crossterm::event::{KeyCode, KeyEvent};
use tokio::task;

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatsState {
    Ready(git::ReviewDiffStats),
    Loading { file_count: usize },
    Unavailable { file_count: usize },
}

impl App {
    pub fn diff_stats_state(&self) -> DiffStatsState {
        if let Some(snapshot) = self.review_diff_snapshot.as_ref() {
            return DiffStatsState::Ready(match self.review_mode {
                ReviewMode::WorkingTree => snapshot.stats_for_working_tree(&self.files),
                ReviewMode::CommitCompare(_) | ReviewMode::BranchCompare(_) => snapshot.stats(),
            });
        }

        if let Some(stats) = self.review_diff_stats {
            return DiffStatsState::Ready(stats);
        }

        if self.review_diff_snapshot_task.is_some() {
            return DiffStatsState::Loading {
                file_count: self.files.len(),
            };
        }

        if self.review_diff_stats_task.is_some() {
            return DiffStatsState::Loading {
                file_count: self.files.len(),
            };
        }

        DiffStatsState::Unavailable {
            file_count: self.files.len(),
        }
    }

    pub(in crate::app) fn open_diff_stats_modal(&mut self) {
        self.diff_stats_modal_open = true;
    }

    pub(in crate::app) fn queue_review_diff_stats_load(&mut self) {
        self.cancel_inflight_review_diff_stats();
        self.review_diff_stats = None;
        self.review_diff_stats_error = None;
        self.review_diff_stats_request_id = self.review_diff_stats_request_id.saturating_add(1);

        if self.files.is_empty() {
            self.review_diff_stats = Some(git::ReviewDiffStats::default());
            return;
        }

        let request_id = self.review_diff_stats_request_id;
        let generation = self.diff_cache_generation;
        let repo_root = self.repo_root.clone();
        let files = self.files.clone();
        let review_mode = self.review_mode.clone();
        let sender = self.events.sender();

        self.review_diff_stats_task = Some(task::spawn(async move {
            let result = load_review_diff_stats(&repo_root, &files, &review_mode)
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::ReviewDiffStatsLoaded {
                request_id,
                generation,
                result,
            });
        }));
    }

    pub(in crate::app) fn cancel_inflight_review_diff_stats(&mut self) {
        if let Some(task) = self.review_diff_stats_task.take() {
            task.abort();
        }
    }

    pub(in crate::app) fn clear_review_diff_stats(&mut self) {
        self.cancel_inflight_review_diff_stats();
        self.review_diff_stats_request_id = self.review_diff_stats_request_id.saturating_add(1);
        self.review_diff_stats = None;
        self.review_diff_stats_error = None;
    }

    pub(in crate::app) fn handle_review_diff_stats_loaded(
        &mut self,
        request_id: u64,
        generation: u64,
        result: Result<git::ReviewDiffStats, String>,
    ) -> bool {
        if request_id != self.review_diff_stats_request_id
            || generation != self.diff_cache_generation
        {
            return false;
        }

        self.review_diff_stats_task = None;
        match result {
            Ok(stats) => {
                self.review_diff_stats = Some(stats);
                self.review_diff_stats_error = None;
            }
            Err(error) => {
                self.review_diff_stats = None;
                self.review_diff_stats_error = Some(error);
            }
        }
        true
    }

    pub(in crate::app) fn handle_diff_stats_modal_key(&mut self, key_event: KeyEvent) -> bool {
        if !self.diff_stats_modal_open {
            return false;
        }

        match key_event.code {
            KeyCode::Esc | KeyCode::F(2) | KeyCode::Enter | KeyCode::Char('q') => {
                self.diff_stats_modal_open = false;
            }
            _ => {}
        }

        true
    }
}

async fn load_review_diff_stats(
    repo_root: &std::path::Path,
    files: &[git::FileEntry],
    review_mode: &ReviewMode,
) -> color_eyre::Result<git::ReviewDiffStats> {
    match review_mode {
        ReviewMode::WorkingTree => {
            git::load_review_diff_stats_for_working_tree(repo_root, files).await
        }
        ReviewMode::CommitCompare(selection) => {
            git::load_review_diff_stats_for_commit_compare(repo_root, selection).await
        }
        ReviewMode::BranchCompare(selection) => {
            git::load_review_diff_stats_for_branch_compare(repo_root, selection).await
        }
    }
}
