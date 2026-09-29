//! Pull request fixtures for app and UI tests.

use crate::{
    app::{App, ReviewMode},
    forge::{
        Check, CheckCounts, CheckRollup, CheckState, CommentState, ConversationComment, DiffSide,
        MergeMethod, MergeSettings, MergeStateStatus, Mergeability, PullRequest, PullRequestState,
        PullRequestSummary, Review, ReviewDecision, ReviewState, ReviewThread, ThreadComment,
        ThreadId, ThreadSubject, Timestamp, Viewer,
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

impl App {
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
        let summary = detail.summary.clone();
        let fetched = FetchedPullRequest {
            remote: "origin".to_string(),
            head_ref: crate::git::pull_request_head_ref(summary.number),
            head_oid: summary.head_oid.clone(),
            base_oid: summary.base_oid.clone(),
        };
        let (_, detail_id) = self.pull_requests.begin_open(summary.clone());
        self.pull_requests.finish_detail(detail_id, Ok(detail));
        self.pull_requests
            .enter(summary.clone(), fetched.head_oid.clone());
        self.review_mode = ReviewMode::PullRequest(PullRequestSelection::new(&summary, &fetched));
        self.loaded_files = files.clone();
        self.files = files;
        self.selected_file_index = 0;
        self.rebuild_sidebar_items();
        self.sync_sidebar_state();
    }
}
