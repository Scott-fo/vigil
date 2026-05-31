use crossterm::event::{KeyCode, KeyEvent};
use tokio::task;

use crate::{
    event::Event,
    review::{
        self as review_domain, BuildReviewSnapshotOptions, CodexAppReviewProvider, PersistedReview,
        ReviewDisplayComment, ReviewFinding, ReviewProvider, ReviewScope, ReviewStore,
        ReviewTarget,
    },
};

use super::{App, DiffViewMode, ReviewMode};

const DEFAULT_REVIEW_INSTRUCTIONS: &str =
    "Focus on correctness bugs, regressions, missing tests, and risky edge cases.";

impl App {
    pub(super) fn start_codex_review(&mut self) {
        if self.files.is_empty() {
            self.status_message = Some("no changed files to review".to_string());
            return;
        }

        self.cancel_inflight_review();
        self.review_request_id = self.review_request_id.saturating_add(1);
        let request_id = self.review_request_id;
        self.review_loading = true;
        self.review_error = None;
        self.review_report = None;
        self.review_snapshot_id = None;
        self.review_provider_session_id = None;
        self.review_summary_scroll = 0;
        self.status_message = Some("Preparing Codex review...".to_string());

        let Some(scope) = review_scope_from_mode(&self.review_mode) else {
            self.review_loading = false;
            self.status_message = Some("review is unavailable for this mode".to_string());
            return;
        };

        let repo_root = self.repo_root.clone();
        let files = self.files.clone();
        let sender = self.events.sender();

        self.review_task = Some(task::spawn(async move {
            let result = run_codex_review(repo_root, scope, files)
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::ReviewFinished { request_id, result });
        }));
    }

    pub(in crate::app) fn handle_review_finished(
        &mut self,
        request_id: u64,
        result: Result<PersistedReview, String>,
    ) -> bool {
        if request_id != self.review_request_id {
            return false;
        }

        self.review_task = None;
        self.review_loading = false;
        match result {
            Ok(review) => {
                let comment_count = review.report.findings.len();
                let headline = review.report.summary.headline.clone();
                self.review_snapshot_id = Some(review.snapshot.id);
                self.review_provider_session_id = review.provider_session_id;
                self.review_report = Some(review.report);
                self.review_error = None;
                self.review_summary_modal_open = true;
                self.review_summary_scroll = 0;
                self.status_message = Some(format!(
                    "Codex review: {headline} ({comment_count} comment{})",
                    if comment_count == 1 { "" } else { "s" }
                ));
            }
            Err(error) => {
                self.review_error = Some(error.clone());
                self.status_message = Some(format!("Codex review failed: {error}"));
            }
        }
        true
    }

    pub(in crate::app) fn queue_review_restore_for_current_snapshot(&mut self) {
        if self.files.is_empty() {
            return;
        }

        let Some(scope) = review_scope_from_mode(&self.review_mode) else {
            return;
        };

        let request_id = self.review_request_id;
        let repo_root = self.repo_root.clone();
        let files = self.files.clone();
        let sender = self.events.sender();

        self.review_task = Some(task::spawn(async move {
            let result = load_persisted_review(repo_root, scope, files)
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::ReviewLoaded { request_id, result });
        }));
    }

    pub(in crate::app) fn handle_review_loaded(
        &mut self,
        request_id: u64,
        result: Result<Option<PersistedReview>, String>,
    ) -> bool {
        if request_id != self.review_request_id {
            return false;
        }

        self.review_task = None;
        match result {
            Ok(Some(review)) => {
                let comment_count = review.report.findings.len();
                self.review_snapshot_id = Some(review.snapshot.id);
                self.review_provider_session_id = review.provider_session_id;
                self.review_report = Some(review.report);
                self.review_error = None;
                self.status_message = Some(format!(
                    "Loaded saved Codex review ({comment_count} comment{})",
                    if comment_count == 1 { "" } else { "s" }
                ));
            }
            Ok(None) => {
                self.review_report = None;
                self.review_error = None;
                self.review_snapshot_id = None;
                self.review_provider_session_id = None;
            }
            Err(error) => {
                self.review_report = None;
                self.review_error = Some(format!("failed to load saved review: {error}"));
                self.review_snapshot_id = None;
                self.review_provider_session_id = None;
            }
        }
        true
    }

    pub(in crate::app) fn cancel_inflight_review(&mut self) {
        if let Some(task) = self.review_task.take() {
            task.abort();
        }
        self.review_loading = false;
    }

    pub(in crate::app) fn invalidate_review_snapshot(&mut self) {
        self.cancel_inflight_review();
        self.review_request_id = self.review_request_id.saturating_add(1);
        self.review_report = None;
        self.review_error = None;
        self.review_snapshot_id = None;
        self.review_provider_session_id = None;
        self.review_summary_modal_open = false;
        self.review_summary_scroll = 0;
    }

    pub fn open_review_summary_modal(&mut self) {
        if self.review_report.is_some() || self.review_error.is_some() || self.review_loading {
            self.review_summary_modal_open = true;
            self.review_summary_scroll = 0;
        } else {
            self.status_message = Some("no Codex review loaded; press R to run one".to_string());
        }
    }

    pub(in crate::app) fn close_review_summary_modal(&mut self) {
        self.review_summary_modal_open = false;
    }

    pub(in crate::app) fn scroll_review_summary_modal(&mut self, delta: i32) {
        if delta < 0 {
            let amount = u16::try_from(delta.unsigned_abs()).unwrap_or(u16::MAX);
            self.review_summary_scroll = self.review_summary_scroll.saturating_sub(amount);
        } else {
            let amount = u16::try_from(delta).unwrap_or(u16::MAX);
            self.review_summary_scroll = self.review_summary_scroll.saturating_add(amount);
        }
    }

    pub(in crate::app) fn handle_review_summary_modal_key(&mut self, key_event: KeyEvent) -> bool {
        if !self.review_summary_modal_open {
            return false;
        }

        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('S') => {
                self.close_review_summary_modal();
            }
            KeyCode::Char('c') => {
                let _ = self.copy_review_summary_to_clipboard();
            }
            KeyCode::Up | KeyCode::Char('k') => self.scroll_review_summary_modal(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_review_summary_modal(1),
            KeyCode::PageUp => self.scroll_review_summary_modal(-10),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_review_summary_modal(10),
            KeyCode::Home => self.review_summary_scroll = 0,
            _ => {}
        }

        true
    }

    pub fn review_summary_headline(&self) -> Option<String> {
        if self.review_loading {
            return Some("Codex review running...".to_string());
        }
        if let Some(error) = self.review_error.as_deref() {
            return Some(format!("Codex review failed: {error}"));
        }

        let report = self.review_report.as_ref()?;
        let comment_count = report.findings.len();
        Some(format!(
            "{}  {} comment{}",
            report.summary.headline,
            comment_count,
            if comment_count == 1 { "" } else { "s" }
        ))
    }

    pub fn review_summary_body(&self) -> Option<String> {
        self.review_report
            .as_ref()
            .map(|report| report.summary.body.clone())
    }

    pub fn review_comments_for_display_index(
        &mut self,
        display_index: usize,
        width: usize,
    ) -> Vec<ReviewDisplayComment> {
        self.review_comments_for_display_index_in_mode(self.diff_view_mode, display_index, width)
    }

    pub(crate) fn review_comments_for_display_index_in_mode(
        &mut self,
        mode: DiffViewMode,
        display_index: usize,
        width: usize,
    ) -> Vec<ReviewDisplayComment> {
        let Some(file_path) = self.selected_file().map(|file| file.path.clone()) else {
            return Vec::new();
        };
        let Some(anchor) = self
            .diff_view
            .display_line_anchor(mode, width, display_index)
        else {
            return Vec::new();
        };
        review_domain::comments_for_display_line(self.active_review_findings(), &file_path, anchor)
    }

    pub fn review_comment_count_for_file(&self, path: &str) -> usize {
        self.active_review_findings()
            .iter()
            .filter(|finding| finding.path == path)
            .count()
    }

    fn active_review_findings(&self) -> &[ReviewFinding] {
        self.review_report
            .as_ref()
            .map(|report| report.findings.as_slice())
            .unwrap_or(&[])
    }
}

async fn run_codex_review(
    repo_root: std::path::PathBuf,
    scope: ReviewScope,
    files: Vec<crate::git::FileEntry>,
) -> color_eyre::Result<PersistedReview> {
    let snapshot = review_domain::build_review_snapshot(BuildReviewSnapshotOptions {
        repo_root: repo_root.clone(),
        worktree_root: repo_root,
        scope,
        files,
    })
    .await?;
    let target = ReviewTarget {
        snapshot: snapshot.clone(),
        instructions: DEFAULT_REVIEW_INSTRUCTIONS.to_string(),
    };
    let provider = CodexAppReviewProvider::from_env();
    let provider_review = provider.review(&target).await?;
    let provider_session_id = provider_review.provider_session_id.clone();
    let report = provider_review.report;

    task::spawn_blocking(move || {
        let store = ReviewStore::open_default()?;
        store.save_report(
            &snapshot,
            &report,
            "codex-app",
            provider_session_id.as_deref(),
        )
    })
    .await
    .map_err(color_eyre::Report::from)?
}

async fn load_persisted_review(
    repo_root: std::path::PathBuf,
    scope: ReviewScope,
    files: Vec<crate::git::FileEntry>,
) -> color_eyre::Result<Option<PersistedReview>> {
    let snapshot = review_domain::build_review_snapshot(BuildReviewSnapshotOptions {
        repo_root: repo_root.clone(),
        worktree_root: repo_root,
        scope,
        files,
    })
    .await?;
    let snapshot_id = snapshot.id;

    task::spawn_blocking(move || {
        let store = ReviewStore::open_default()?;
        store.load_latest_for_snapshot(&snapshot_id)
    })
    .await
    .map_err(color_eyre::Report::from)?
}

fn review_scope_from_mode(mode: &ReviewMode) -> Option<ReviewScope> {
    match mode {
        ReviewMode::WorkingTree => Some(ReviewScope::WorkingTree),
        ReviewMode::CommitCompare(selection) => Some(ReviewScope::CommitCompare {
            base_ref: selection.base_ref.clone(),
            base_sha: None,
            commit_hash: selection.commit_hash.clone(),
            short_hash: selection.short_hash.clone(),
            subject: selection.subject.clone(),
        }),
        ReviewMode::BranchCompare(selection) => Some(ReviewScope::BranchCompare {
            source_ref: selection.source_ref.clone(),
            source_sha: None,
            destination_ref: selection.destination_ref.clone(),
            destination_sha: None,
            merge_base: None,
        }),
    }
}
