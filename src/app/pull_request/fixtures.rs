//! Pull request fixtures for app and UI tests.

use crate::{
    app::{App, ReviewMode},
    forge::{
        Check, CheckCounts, CheckRollup, CheckState, CommentState, ConversationComment, DiffSide,
        MergeMethod, MergeSettings, MergeStateStatus, Mergeability, PullRequest, PullRequestList,
        PullRequestState, PullRequestSummary, Review, ReviewDecision, ReviewState, ReviewThread,
        Snapshot, ThreadComment, ThreadId, ThreadSubject, Timestamp, Viewer,
    },
    git::{
        BranchEntry, BranchLocation, BranchSnapshot, BranchTip, FetchedPullRequest, FileEntry,
        HeadState,
    },
};

use super::PullRequestSelection;

pub(crate) fn summary(number: u64) -> PullRequestSummary {
    PullRequestSummary {
        number,
        title: "Remove Codex review integration".to_string(),
        author: "Scott-fo".to_string(),
        url: format!("https://github.com/Scott-fo/vigil/pull/{number}"),
        head_ref_name: "review/remove-codex".to_string(),
        base_ref_name: "master".to_string(),
        head_oid: "f076e8b7b9fc5ccdf10f7f2af9ca76d7298e8807".to_string(),
        base_oid: "fbc257f166313abf0b2a1399995bdba3806054d4".to_string(),
        is_draft: false,
        state: PullRequestState::Open,
        review_decision: Some(ReviewDecision::Approved),
        checks: Some(CheckRollup {
            state: CheckState::Success,
            counts: CheckCounts {
                success: 2,
                ..CheckCounts::default()
            },
        }),
        additions: 211,
        deletions: 3143,
        changed_files: 2,
        updated_at: Timestamp::new("2026-09-29T16:00:00Z"),
        is_cross_repository: false,
    }
}

pub(crate) fn thread(id: &str, path: &str, side: DiffSide, line: Option<u32>) -> ReviewThread {
    ReviewThread {
        id: ThreadId::new(id),
        path: path.to_string(),
        subject: ThreadSubject::Line,
        side,
        line,
        start: None,
        original_line: line,
        original_start_line: None,
        is_resolved: false,
        resolved_by: None,
        is_outdated: false,
        viewer_can_reply: true,
        viewer_can_resolve: true,
        viewer_can_unresolve: false,
        diff_hunk: String::new(),
        comments: vec![ThreadComment {
            id: format!("{id}-comment"),
            author: "reviewer".to_string(),
            body: "Should this handle the empty case?".to_string(),
            created_at: Timestamp::new("2026-09-29T12:00:00Z"),
            url: String::new(),
            state: CommentState::Submitted,
        }],
    }
}

pub(crate) fn pull_request(number: u64, review_threads: Vec<ReviewThread>) -> PullRequest {
    PullRequest {
        summary: summary(number),
        body: "## Summary\n\nDrops the **Codex** review provider.".to_string(),
        created_at: Timestamp::new("2026-09-28T10:00:00Z"),
        commit_count: 1,
        mergeable: Mergeability::Unknown,
        merge_state: MergeStateStatus::Unknown,
        auto_merge: None,
        requested_reviewers: Vec::new(),
        latest_reviews: vec![Review {
            id: "R1".to_string(),
            author: "reviewer".to_string(),
            state: ReviewState::Approved,
            body: String::new(),
            submitted_at: Some(Timestamp::new("2026-09-29T15:00:00Z")),
            url: String::new(),
        }],
        conversation: vec![ConversationComment {
            id: "IC1".to_string(),
            author: "Scott-fo".to_string(),
            body: "Ready for review.".to_string(),
            created_at: Timestamp::new("2026-09-28T11:00:00Z"),
            url: String::new(),
        }],
        checks: vec![
            Check {
                name: "test".to_string(),
                workflow: Some("CI".to_string()),
                state: CheckState::Success,
                summary: None,
                url: None,
                is_required: true,
            },
            Check {
                name: "lint".to_string(),
                workflow: Some("CI".to_string()),
                state: CheckState::Failure,
                summary: Some("2 warnings".to_string()),
                url: None,
                is_required: false,
            },
        ],
        review_threads,
        viewer: Viewer {
            login: "reviewer".to_string(),
            is_author: false,
            pending_review: None,
            can_update: false,
            can_close: false,
            can_reopen: false,
            can_enable_auto_merge: false,
            can_disable_auto_merge: false,
            can_delete_head_ref: false,
        },
        merge_settings: MergeSettings {
            allowed_methods: vec![MergeMethod::Squash],
            viewer_default_method: MergeMethod::Squash,
            auto_merge_allowed: false,
            delete_branch_on_merge: true,
        },
    }
}

pub(crate) fn file(path: &str) -> FileEntry {
    FileEntry {
        status: "M".to_string(),
        path: path.to_string(),
        label: path.to_string(),
        filetype: Some("rust"),
    }
}

/// A branch snapshot with `branch` checked out.
pub(crate) fn branch_snapshot(branch: &str) -> BranchSnapshot {
    BranchSnapshot {
        head: HeadState::Branch(branch.to_string()),
        branches: vec![BranchEntry {
            name: branch.to_string(),
            location: BranchLocation::Local,
            is_head: true,
            upstream: None,
            tip: BranchTip {
                short_hash: "f076e8b".to_string(),
                subject: "Remove Codex review integration".to_string(),
                committed_at: 0,
            },
        }],
        previous_branch: None,
        remotes: vec!["origin".to_string()],
        last_fetch: None,
        operation: None,
    }
}

/// A list page of `numbers`, capped from `total_count` matches.
pub(crate) fn pull_request_list(numbers: &[u64], total_count: u64) -> PullRequestList {
    PullRequestList {
        pull_requests: numbers
            .iter()
            .map(|number| {
                let mut row = summary(*number);
                row.title = format!("Pull request number {number}");
                row.author = format!("author{number}");
                row
            })
            .collect(),
        total_count,
    }
}

impl App {
    /// Shows the pull request list with `page` loaded for its first tab, as
    /// if GitHub had answered. Nothing is spawned; keys that reload must not
    /// be pressed afterwards.
    pub(crate) fn show_pull_request_list_for_test(&mut self, page: PullRequestList) {
        self.show_empty_pull_request_list_for_test();
        let list = self.pull_requests.list_mut();
        let (id, filter) = list.begin_load();
        list.finish_load(id, filter, Ok(page));
    }

    /// Like [`Self::show_pull_request_list_for_test`], showing `saved` from
    /// the forge cache while the live load still runs.
    pub(crate) fn show_saved_pull_request_list_for_test(
        &mut self,
        saved: Snapshot<PullRequestList>,
    ) {
        self.show_empty_pull_request_list_for_test();
        let list = self.pull_requests.list_mut();
        let (_, filter) = list.begin_load();
        list.show_saved(filter, saved);
    }

    fn show_empty_pull_request_list_for_test(&mut self) {
        self.pull_requests
            .connect_for_test(crate::forge::GitHub::new(
                self.repo_root.clone(),
                crate::forge::RepositoryRef {
                    host: "github.com".to_string(),
                    owner: "Scott-fo".to_string(),
                    name: "vigil".to_string(),
                },
            ));
        self.screen = crate::app::Screen::PullRequestList;
    }

    pub(crate) fn select_pull_request_page_for_test(&mut self, page: super::PullRequestPage) {
        self.pull_requests.set_page(page);
        self.sync_sidebar_state();
    }

    /// Checks out `branch` with `summary` as its pull request, as if the
    /// lookup had finished.
    pub(crate) fn set_current_branch_pull_request_for_test(
        &mut self,
        branch: &str,
        summary: PullRequestSummary,
    ) {
        self.set_branch_snapshot(branch_snapshot(branch));
        let key = super::state::BranchKey {
            branch: branch.to_string(),
            tip: "f076e8b".to_string(),
        };
        let now = std::time::Instant::now();
        let id = self
            .pull_requests
            .begin_current_branch_load(key, true, now)
            .expect("lookup starts");
        self.pull_requests
            .finish_current_branch_load(id, &Ok(Some(summary)), now);
    }

    /// Puts the app in pull request review of `detail` as if it had been
    /// fetched and loaded, without spawning anything.
    pub(crate) fn open_pull_request_for_test(
        &mut self,
        detail: PullRequest,
        files: Vec<FileEntry>,
    ) {
        self.open_pull_request_from_for_test(detail, files, super::state::ReviewOrigin::Elsewhere);
    }

    /// Like [`Self::open_pull_request_for_test`], opened from `origin`.
    pub(in crate::app) fn open_pull_request_from_for_test(
        &mut self,
        detail: PullRequest,
        files: Vec<FileEntry>,
        origin: super::state::ReviewOrigin,
    ) {
        let summary = detail.summary.clone();
        let (_, detail_id) = self.pull_requests.begin_open(summary.clone(), origin);
        self.pull_requests.finish_detail(detail_id, Ok(detail));
        self.enter_pull_request_for_test(summary, files);
    }

    /// Opens `summary`'s review as if its commits were fetched, with
    /// `saved` read from the forge cache during the fetch and the live
    /// detail still loading. Returns the detail request id, to deliver the
    /// live detail with.
    pub(crate) fn open_pull_request_with_saved_detail_for_test(
        &mut self,
        summary: PullRequestSummary,
        saved: Snapshot<PullRequest>,
        files: Vec<FileEntry>,
    ) -> u64 {
        let (fetch_id, detail_id) = self
            .pull_requests
            .begin_open(summary.clone(), super::state::ReviewOrigin::Elsewhere);
        self.pull_requests.show_saved_detail(summary.number, saved);
        let summary = self
            .pull_requests
            .finish_fetch(fetch_id)
            .expect("the fetch is current");
        self.enter_pull_request_for_test(summary, files);
        detail_id
    }

    fn enter_pull_request_for_test(&mut self, summary: PullRequestSummary, files: Vec<FileEntry>) {
        let fetched = FetchedPullRequest {
            remote: "origin".to_string(),
            head_ref: crate::git::pull_request_head_ref(summary.number),
            head_oid: summary.head_oid.clone(),
            base_oid: summary.base_oid.clone(),
        };
        self.pull_requests
            .enter(summary.clone(), fetched.head_oid.clone());
        self.review_mode = ReviewMode::PullRequest(PullRequestSelection::new(&summary, &fetched));
        self.loaded_files = files.clone();
        self.files = files;
        self.selected_file_index = 0;
        self.rebuild_sidebar_items();
        self.sync_sidebar_state();
        self.load_pull_request_drafts();
    }

    /// Stores a draft on the open pull request, in memory only.
    pub(crate) fn save_draft_for_test(&mut self, draft: crate::review::DraftComment) {
        self.save_draft(draft);
    }

    /// Presses `c` on the cursor line, as the review key does.
    pub(crate) fn start_inline_comment_for_test(&mut self) {
        self.start_inline_comment();
    }

    pub(crate) fn open_submit_review_for_test(&mut self) {
        self.open_submit_review();
    }

    pub(crate) fn open_merge_form_for_test(&mut self) {
        self.open_merge_form();
    }

    /// Types `text` into the open review modal's text field.
    pub(crate) fn type_in_pull_request_modal_for_test(&mut self, text: &str) {
        if let Some(area) = self.pull_request_modal_text() {
            area.insert_str(text);
        }
    }

    /// Shows `diff` as the selected file's diff with the cursor on display
    /// row `cursor`, focused, as a reviewer reading the files page would.
    pub(crate) fn show_pull_request_diff_for_test(&mut self, diff: &str, cursor: usize) {
        self.select_pull_request_page_for_test(super::PullRequestPage::Files);
        self.diff_view = crate::git::build_diff_view_from_diff_text(diff, Some("rust"));
        self.diff_view_mode = crate::app::DiffViewMode::Unified;
        self.diff_line_wrap_mode = crate::app::DiffLineWrapMode::Wrap;
        self.active_pane = crate::app::ActivePane::Diff;
        self.selected_diff_line_index = cursor;
    }
}
