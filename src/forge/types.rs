//! Public domain types for pull requests, reviews, and checks.
//!
//! Every finite GitHub state is an enum here. Wire strings never leave the
//! `forge` module; unknown values GitHub may add later map to the closest
//! documented fallback on each type.

use std::fmt;

/// The GitHub repository a [`GitHub`](super::GitHub) client talks to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepositoryRef {
    /// API host, `github.com` unless the remote points at GitHub Enterprise.
    pub host: String,
    pub owner: String,
    pub name: String,
}

impl RepositoryRef {
    /// `owner/name`, as GitHub search and `gh --repo` expect it.
    pub fn name_with_owner(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

impl fmt::Display for RepositoryRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.owner, self.name)
    }
}

/// An RFC 3339 UTC timestamp as GitHub reports it, e.g. `2026-09-06T21:34:42Z`.
///
/// GitHub always reports UTC with a `Z` suffix, so ordering the strings
/// orders the instants.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(String);

impl Timestamp {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// GraphQL node id of a review thread; the handle for reply and resolve.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThreadId(String);

impl ThreadId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ThreadId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

/// The branch-protection review verdict. Absent when the base branch does not
/// require reviews.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

/// The state of one check, or of a whole commit's checks.
///
/// Check runs report a status and a conclusion; they collapse here as:
/// anything not yet completed is `Pending`; `TIMED_OUT`, `ACTION_REQUIRED`,
/// and `STARTUP_FAILURE` are `Failure`; `STALE` and unknown conclusions are
/// `Neutral`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckState {
    Pending,
    Success,
    Failure,
    Cancelled,
    Neutral,
    Skipped,
}

/// Per-state counts for every check run and commit status on a commit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CheckCounts {
    pub pending: u32,
    pub success: u32,
    pub failure: u32,
    pub cancelled: u32,
    pub neutral: u32,
    pub skipped: u32,
}

impl CheckCounts {
    pub fn total(&self) -> u32 {
        self.pending + self.success + self.failure + self.cancelled + self.neutral + self.skipped
    }

    pub fn get(&self, state: CheckState) -> u32 {
        match state {
            CheckState::Pending => self.pending,
            CheckState::Success => self.success,
            CheckState::Failure => self.failure,
            CheckState::Cancelled => self.cancelled,
            CheckState::Neutral => self.neutral,
            CheckState::Skipped => self.skipped,
        }
    }

    pub(super) fn add(&mut self, state: CheckState, count: u32) {
        let slot = match state {
            CheckState::Pending => &mut self.pending,
            CheckState::Success => &mut self.success,
            CheckState::Failure => &mut self.failure,
            CheckState::Cancelled => &mut self.cancelled,
            CheckState::Neutral => &mut self.neutral,
            CheckState::Skipped => &mut self.skipped,
        };
        *slot += count;
    }
}

/// GitHub's combined verdict for the head commit's checks.
///
/// `state` is GitHub's own rollup (what the PR list icon shows), which can
/// only be `Pending`, `Success`, or `Failure`; `counts` breaks it down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CheckRollup {
    pub state: CheckState,
    pub counts: CheckCounts,
}

/// One row of a pull request list: enough to render and to open its diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestSummary {
    pub number: u64,
    pub title: String,
    /// Login of the author; `ghost` when the account was deleted.
    pub author: String,
    pub url: String,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub head_oid: String,
    /// The base branch tip GitHub last computed the diff against. It moves
    /// as the base branch advances; use the merge base for a local diff.
    pub base_oid: String,
    pub is_draft: bool,
    pub state: PullRequestState,
    pub review_decision: Option<ReviewDecision>,
    /// `None` when the head commit has no checks or statuses at all.
    pub checks: Option<CheckRollup>,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    pub updated_at: Timestamp,
    /// The head branch lives in a fork.
    pub is_cross_repository: bool,
}

/// Which pull requests [`GitHub::list_pull_requests`](super::GitHub::list_pull_requests) returns.
/// Every filter is limited to open pull requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PullRequestListFilter {
    /// Review requested from the viewer, directly or through a team.
    NeedsMyReview,
    /// Authored by the viewer.
    Mine,
    AllOpen,
}

/// A page of pull requests, newest update first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestList {
    pub pull_requests: Vec<PullRequestSummary>,
    /// How many pull requests match the filter. Larger than
    /// `pull_requests.len()` when the list was capped.
    pub total_count: u64,
}

impl PullRequestList {
    pub fn is_truncated(&self) -> bool {
        (self.pull_requests.len() as u64) < self.total_count
    }
}

/// Whether the head can merge into the base without conflicts. GitHub
/// computes this in the background, so a fresh or recently pushed pull
/// request reports `Unknown` until a later load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mergeability {
    Mergeable,
    Conflicting,
    Unknown,
}

/// What stands between the pull request and a merge right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MergeStateStatus {
    /// Mergeable with passing checks.
    Clean,
    /// Branch protection blocks the merge (reviews, required checks).
    Blocked,
    /// The head is behind the base and protection requires it up to date.
    Behind,
    /// Merge conflicts.
    Dirty,
    /// Mergeable, but non-required checks are failing.
    Unstable,
    /// Mergeable with passing checks and pre-receive hooks.
    HasHooks,
    /// Draft pull requests cannot merge. Older GitHub Enterprise servers
    /// report this; github.com reports `Blocked` instead.
    Draft,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoMerge {
    pub method: MergeMethod,
    pub enabled_by: Option<String>,
    pub enabled_at: Option<Timestamp>,
}

/// Someone whose review was requested and has not yet been given.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RequestedReviewer {
    /// A user, bot, or mannequin account.
    User {
        login: String,
    },
    Team {
        slug: String,
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    Pending,
}

/// A submitted (or, for the viewer, pending) review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub id: String,
    pub author: String,
    pub state: ReviewState,
    pub body: String,
    /// `None` while the review is pending.
    pub submitted_at: Option<Timestamp>,
    pub url: String,
}

/// A top-level comment on the pull request's conversation tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created_at: Timestamp,
    pub url: String,
}

/// One check run or commit status on the head commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Job name for check runs, context for commit statuses.
    pub name: String,
    /// The Actions workflow that produced the check run, if any.
    pub workflow: Option<String>,
    pub state: CheckState,
    /// Check run title or commit status description.
    pub summary: Option<String>,
    pub url: Option<String>,
    /// Branch protection requires this check for this pull request.
    pub is_required: bool,
}

/// Which side of a split diff a line lives on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffSide {
    /// The base (old) file. Deleted and unchanged lines by old line number.
    Left,
    /// The head (new) file. Added and unchanged lines by new line number.
    Right,
}

/// A line in the pull request diff, numbered in the file on `side`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiffPosition {
    pub side: DiffSide,
    pub line: u32,
}

/// What a review thread is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadSubject {
    /// One line or a range of lines.
    Line,
    /// The file as a whole; no line numbers.
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentState {
    Submitted,
    /// Part of the viewer's pending review; nobody else can see it yet.
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created_at: Timestamp,
    pub url: String,
    pub state: CommentState,
}

/// An inline discussion anchored to a file in the diff.
///
/// Line numbers describe the thread against the current head (`line`,
/// `start`) and against the commit it was written on (`original_*`). When the
/// commented lines no longer exist in the current diff, `line` and `start`
/// are `None` and only the originals remain; `is_outdated` alone does not
/// imply that, because GitHub also marks threads outdated whose lines still
/// map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewThread {
    pub id: ThreadId,
    pub path: String,
    pub subject: ThreadSubject,
    /// Side of `line`, the last line of the range.
    pub side: DiffSide,
    pub line: Option<u32>,
    /// First line of a multi-line range. `None` for single-line threads.
    pub start: Option<DiffPosition>,
    pub original_line: Option<u32>,
    pub original_start_line: Option<u32>,
    pub is_resolved: bool,
    pub resolved_by: Option<String>,
    pub is_outdated: bool,
    pub viewer_can_reply: bool,
    pub viewer_can_resolve: bool,
    pub viewer_can_unresolve: bool,
    /// The diff hunk the first comment was written against, ending at the
    /// commented line. Useful for rendering outdated threads.
    pub diff_hunk: String,
    pub comments: Vec<ThreadComment>,
}

/// The viewer's unsubmitted review on github.com. GitHub allows one per
/// reviewer, and [`GitHub::submit_review`](super::GitHub::submit_review)
/// fails while it exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReview {
    pub id: String,
    pub created_at: Timestamp,
    pub body: String,
    pub comment_count: u32,
}

/// What the authenticated user is and may do on this pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    pub login: String,
    pub is_author: bool,
    pub pending_review: Option<PendingReview>,
    /// May edit title, body, and draft state.
    pub can_update: bool,
    pub can_close: bool,
    pub can_reopen: bool,
    pub can_enable_auto_merge: bool,
    pub can_disable_auto_merge: bool,
    pub can_delete_head_ref: bool,
}

/// Repository-level merge configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeSettings {
    /// Allowed methods in `Merge`, `Squash`, `Rebase` order.
    pub allowed_methods: Vec<MergeMethod>,
    /// The method the viewer last used here, GitHub's preselection.
    pub viewer_default_method: MergeMethod,
    pub auto_merge_allowed: bool,
    /// GitHub deletes head branches after merge by itself.
    pub delete_branch_on_merge: bool,
}

/// Everything the review screen needs about one pull request.
///
/// Collections are complete: `load_pull_request` follows every pagination
/// cursor rather than truncating large pull requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub summary: PullRequestSummary,
    /// Markdown.
    pub body: String,
    pub created_at: Timestamp,
    pub commit_count: u32,
    pub mergeable: Mergeability,
    pub merge_state: MergeStateStatus,
    pub auto_merge: Option<AutoMerge>,
    pub requested_reviewers: Vec<RequestedReviewer>,
    /// The latest review from each reviewer, oldest first.
    pub latest_reviews: Vec<Review>,
    /// Conversation-tab comments, oldest first.
    pub conversation: Vec<ConversationComment>,
    /// Checks and statuses on the head commit.
    pub checks: Vec<Check>,
    pub review_threads: Vec<ReviewThread>,
    pub viewer: Viewer,
    pub merge_settings: MergeSettings,
}

/// An inline comment to post as part of a review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftReviewComment {
    pub path: String,
    /// The commented line, or the last line of a range.
    pub end: DiffPosition,
    /// The first line of a multi-line range. Must come before `end`.
    pub start: Option<DiffPosition>,
    /// Markdown.
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
}

/// A complete review, submitted in one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitReview {
    /// The head commit the reviewer looked at. Line numbers in `comments`
    /// are resolved against this commit's diff, so a later push does not
    /// shift them.
    pub commit_oid: String,
    pub event: ReviewEvent,
    /// Markdown summary; may be empty for `Approve`, or when there are
    /// inline comments.
    pub body: String,
    pub comments: Vec<DraftReviewComment>,
}

/// What to do with the head branch after an immediate merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeadBranchAction {
    Keep,
    /// Delete the branch on GitHub. Local branches are never touched.
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MergeTiming {
    /// Merge now; fails if the pull request is not mergeable.
    Now { head_branch: HeadBranchAction },
    /// Enable auto-merge; GitHub merges once requirements pass and deletes
    /// the head branch only if the repository is configured to.
    WhenReady,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeCommitMessage {
    pub headline: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOptions {
    pub method: MergeMethod,
    /// The head the user reviewed. GitHub refuses the merge if the branch
    /// has moved since.
    pub expected_head_oid: String,
    pub timing: MergeTiming,
    /// `None` uses GitHub's default message for the method.
    pub commit_message: Option<MergeCommitMessage>,
}

/// What happened to the head branch after a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadBranchOutcome {
    Kept,
    Deleted,
    /// Already gone, usually because the repository deletes merged branches.
    AlreadyDeleted,
    /// Not deleted because it lives in a fork or the viewer lacks permission.
    NotDeletable,
    /// The merge succeeded but deleting the branch failed.
    DeleteFailed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    Merged { head_branch: HeadBranchOutcome },
    AutoMergeEnabled,
}
