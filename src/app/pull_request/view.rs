use crate::{
    app::{DiffViewMode, ReviewMode},
    forge::{ForgeError, PullRequest, PullRequestSummary},
    git,
    review::{DisplayThread, ThreadRow, UnplacedReason},
};

use super::{
    super::{App, Screen, SnackbarVariant, clipboard::write_osc52_clipboard},
    PullRequestSelection,
    modal::DraftEntry,
};

/// Everything the overview page draws, prepared from the open pull request.
#[derive(Debug)]
pub struct PullRequestOverview<'a> {
    pub selection: &'a PullRequestSelection,
    /// The latest summary: from the list or branch lookup, then from each
    /// detail load and poll.
    pub summary: &'a PullRequestSummary,
    /// `None` until the first detail load finishes.
    pub detail: Option<&'a PullRequest>,
    pub detail_error: Option<&'a ForgeError>,
    /// Threads the diff cannot show, with why.
    pub unplaced_threads: Vec<(UnplacedReason, &'a DisplayThread)>,
    /// The reviewer's draft comments, oldest first.
    pub drafts: Vec<DraftEntry<'a>>,
    /// Rows scrolled off the top.
    pub scroll: usize,
}

impl App {
    /// Copies [`Self::pull_request_branch_in_view`] to the clipboard.
    pub(in crate::app) fn copy_pull_request_branch(&mut self) {
        let Some(branch) = self.pull_request_branch_in_view() else {
            self.status_message = Some("no pull request to copy a branch from".to_string());
            return;
        };
        match write_osc52_clipboard(&branch) {
            Ok(()) => self.show_snackbar(format!("copied {branch}"), SnackbarVariant::Info),
            Err(error) => self.show_snackbar(
                format!("could not copy the branch name: {error}"),
                SnackbarVariant::Error,
            ),
        }
    }

    /// `K`: checks out the pull request under review as a local branch, at
    /// the head that was fetched for review. See
    /// [`PullRequestSelection::checkout`] for the branch it uses.
    pub(in crate::app) fn checkout_pull_request_branch(&mut self) {
        let Some(checkout) = self
            .pull_request_selection()
            .map(PullRequestSelection::checkout)
        else {
            return;
        };
        self.start_branch_operation(git::BranchOperation::CheckoutPullRequest(checkout));
    }

    /// The head branch of the pull request in view: the list's selected row,
    /// else the pull request under review, else the checked-out branch's
    /// pull request.
    pub(in crate::app) fn pull_request_branch_in_view(&self) -> Option<String> {
        let summary_branch = |summary: &PullRequestSummary| summary.head_ref_name.clone();
        match self.screen {
            Screen::PullRequestList => self
                .pull_requests
                .list()
                .selected_summary()
                .map(summary_branch),
            Screen::Review => self
                .pull_request_selection()
                .map(|selection| selection.head_ref_name.clone())
                .or_else(|| self.current_branch_pull_request().map(summary_branch)),
        }
    }

    /// The pull request under review, if the review shows one.
    pub fn pull_request_selection(&self) -> Option<&PullRequestSelection> {
        match &self.review_mode {
            ReviewMode::PullRequest(selection) => Some(selection),
            ReviewMode::WorkingTree
            | ReviewMode::CommitCompare(_)
            | ReviewMode::BranchCompare(_) => None,
        }
    }

    /// The overview page, while it is the page shown.
    pub fn pull_request_overview(&self) -> Option<PullRequestOverview<'_>> {
        if !self.pull_request_overview_visible() {
            return None;
        }
        let selection = self.pull_request_selection()?;
        let open = self.pull_requests.open()?;
        let unplaced_threads = open
            .threads()
            .unplaced(|path| self.files.iter().any(|file| file.path == path));
        Some(PullRequestOverview {
            selection,
            summary: open.summary(),
            detail: open.detail(),
            detail_error: open.detail_error(),
            unplaced_threads,
            drafts: self.draft_entries(),
            scroll: open.overview_scroll(),
        })
    }

    /// Stores the overview scroll after the renderer clamps it to the content.
    pub fn set_pull_request_overview_scroll(&mut self, scroll: usize) {
        if let Some(open) = self.pull_requests.open_mut() {
            open.set_overview_scroll(scroll);
        }
    }

    pub(in crate::app) fn scroll_pull_request_overview(&mut self, delta: i32) {
        if let Some(open) = self.pull_requests.open_mut() {
            let scroll = open.overview_scroll();
            let next = if delta < 0 {
                scroll.saturating_sub(delta.unsigned_abs() as usize)
            } else {
                scroll.saturating_add(delta as usize)
            };
            open.set_overview_scroll(next);
        }
    }

    /// A newer head GitHub reported for the pull request under review, which
    /// `r` would load.
    pub fn pull_request_has_newer_head(&self) -> bool {
        self.pull_requests
            .open()
            .is_some_and(|open| open.newer_head().is_some())
    }

    /// Unresolved review threads on `path`, for the sidebar marker.
    pub fn unresolved_thread_count(&self, path: &str) -> usize {
        self.pull_requests
            .open()
            .map_or(0, |open| open.threads().unresolved_count(path))
    }

    /// Unresolved threads the overview lists because the diff cannot show
    /// them, plus drafts that need attention, for the overview row's marker.
    pub fn unresolved_unplaced_thread_count(&self) -> usize {
        self.pull_requests.open().map_or(0, |open| {
            let threads = open
                .threads()
                .unplaced(|path| self.files.iter().any(|file| file.path == path))
                .into_iter()
                .filter(|(_, thread)| !thread.is_resolved())
                .count();
            threads
                + open
                    .drafts()
                    .needing_attention(open.reviewed_head())
                    .count()
        })
    }

    /// Drafts a review of the shown head would send, for the footer.
    pub fn pending_draft_count(&self) -> usize {
        self.pull_requests.open().map_or(0, |open| {
            open.drafts().attached(open.reviewed_head()).count()
        })
    }

    /// Whether the selected file's diff has threads drawn inside it, which
    /// makes its rows taller than one line per diff row.
    pub fn selected_file_has_review_threads(&self) -> bool {
        let Some(path) = self.selected_file().map(|file| file.path.as_str()) else {
            return false;
        };
        self.pull_request_selection().is_some()
            && self
                .pull_requests
                .open()
                .is_some_and(|open| open.threads().has_inline_threads(path))
    }

    /// Rows of the review threads drawn under display line `display_index`
    /// of the selected file, laid out for a pane `width` columns wide.
    pub fn review_thread_rows_at(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        display_index: usize,
    ) -> Vec<ThreadRow> {
        if !self.selected_file_has_review_threads() {
            return Vec::new();
        }
        let Some(anchor) = self.diff_view.display_line_anchor(
            mode,
            width,
            self.diff_line_wrap_mode,
            display_index,
        ) else {
            return Vec::new();
        };
        let (Some(open), Some(file)) = (self.pull_requests.open(), self.selected_file()) else {
            return Vec::new();
        };
        open.threads()
            .threads_at(&file.path, anchor)
            .into_iter()
            .flat_map(|thread| thread.rows(width))
            .collect()
    }
}
