//! GitHub response shapes and their conversion into domain types.
//!
//! Wire structs mirror the GraphQL documents field for field and stay
//! private. Wire enums carry a `#[serde(other)]` fallback wherever GitHub
//! might add values, so a new check conclusion degrades one field instead of
//! failing a whole load; states that must never be guessed (pull request
//! state, diff side) have no fallback and fail decoding instead.

use std::future::Future;

use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;

use super::{
    error::ForgeError,
    request::pull_request_alias,
    types::{
        AutoMerge, Check, CheckCounts, CheckRollup, CheckState, CommentState, ConversationComment,
        DiffPosition, DiffSide, MergeMethod, MergeSettings, MergeStateStatus, Mergeability,
        PendingReview, PullRequest, PullRequestState, PullRequestSummary, RequestedReviewer,
        Review, ReviewDecision, ReviewState, ReviewThread, ThreadComment, ThreadId, ThreadSubject,
        Timestamp, Viewer,
    },
};

/// Login GitHub shows for deleted accounts, which the API reports as null.
const GHOST_LOGIN: &str = "ghost";

pub(super) fn decode<T: DeserializeOwned>(context: &str, bytes: &[u8]) -> Result<T, ForgeError> {
    serde_json::from_slice(bytes).map_err(|error| ForgeError::decode(context, error))
}

pub(super) fn decode_value<T: DeserializeOwned>(
    context: &str,
    value: Value,
) -> Result<T, ForgeError> {
    serde_json::from_value(value).map_err(|error| ForgeError::decode(context, error))
}

/// Extracts `data` from a GraphQL response. `gh` exits non-zero when the
/// response has `errors`, so a successful run always carries data.
pub(super) fn graphql_data(context: &str, bytes: &[u8]) -> Result<Value, ForgeError> {
    let mut envelope: Value = decode(context, bytes)?;
    match envelope.get_mut("data").map(Value::take) {
        Some(data) if !data.is_null() => Ok(data),
        _ => Err(ForgeError::decode(context, "response has no data")),
    }
}

/// Takes the value at a JSON pointer, failing with `context` if absent.
pub(super) fn take_at(context: &str, mut value: Value, pointer: &str) -> Result<Value, ForgeError> {
    match value.pointer_mut(pointer).map(Value::take) {
        Some(found) if !found.is_null() => Ok(found),
        _ => Err(ForgeError::decode(context, format!("missing {pointer}"))),
    }
}

// ---------------------------------------------------------------------------
// Pagination

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct PageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Connection<T> {
    pub page_info: PageInfo,
    pub nodes: Vec<T>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Nodes<T> {
    pub nodes: Vec<T>,
}

/// Follows a connection's cursors until the last page, appending nodes in
/// order. `fetch_next` receives the cursor to continue after.
pub(super) async fn collect_pages<T, F, Fut>(
    context: &str,
    first: Connection<T>,
    mut fetch_next: F,
) -> Result<Vec<T>, ForgeError>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<Connection<T>, ForgeError>>,
{
    let mut items = first.nodes;
    let mut page = first.page_info;
    while page.has_next_page {
        let Some(cursor) = page.end_cursor else {
            return Err(ForgeError::decode(context, "next page without a cursor"));
        };
        let next = fetch_next(cursor.clone()).await?;
        if next.page_info.has_next_page && next.page_info.end_cursor.as_deref() == Some(&cursor) {
            return Err(ForgeError::decode(
                context,
                "pagination cursor did not advance",
            ));
        }
        items.extend(next.nodes);
        page = next.page_info;
    }
    Ok(items)
}

// ---------------------------------------------------------------------------
// Enums

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WirePullRequestState {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WireReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
    #[serde(other)]
    Unknown,
}

/// `StatusState`, used by commit statuses and the rollup.
#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum WireStatusState {
    Expected,
    Error,
    Failure,
    Pending,
    Success,
    #[serde(other)]
    Unknown,
}

/// `CheckRunState`, the union of check run statuses and conclusions used by
/// the rollup counts.
#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WireCheckRunState {
    ActionRequired,
    Cancelled,
    Completed,
    Failure,
    InProgress,
    Neutral,
    Pending,
    Queued,
    Skipped,
    Stale,
    StartupFailure,
    Success,
    TimedOut,
    Waiting,
    Requested,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum WireCheckStatus {
    Completed,
    #[serde(other)]
    Incomplete,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum WireCheckConclusion {
    ActionRequired,
    TimedOut,
    Cancelled,
    Failure,
    Success,
    Neutral,
    Skipped,
    StartupFailure,
    Stale,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WireMergeable {
    Mergeable,
    Conflicting,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WireMergeStateStatus {
    Clean,
    Blocked,
    Behind,
    Dirty,
    Unstable,
    HasHooks,
    Draft,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WireMergeMethod {
    Merge,
    Squash,
    Rebase,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WireReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    Pending,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WireDiffSide {
    Left,
    Right,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WireSubjectType {
    File,
    #[serde(other)]
    Line,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WireCommentState {
    Pending,
    #[serde(other)]
    Submitted,
}

impl From<WirePullRequestState> for PullRequestState {
    fn from(state: WirePullRequestState) -> Self {
        match state {
            WirePullRequestState::Open => Self::Open,
            WirePullRequestState::Closed => Self::Closed,
            WirePullRequestState::Merged => Self::Merged,
        }
    }
}

fn review_decision(decision: WireReviewDecision) -> Option<ReviewDecision> {
    match decision {
        WireReviewDecision::Approved => Some(ReviewDecision::Approved),
        WireReviewDecision::ChangesRequested => Some(ReviewDecision::ChangesRequested),
        WireReviewDecision::ReviewRequired => Some(ReviewDecision::ReviewRequired),
        WireReviewDecision::Unknown => None,
    }
}

impl From<WireStatusState> for CheckState {
    fn from(state: WireStatusState) -> Self {
        match state {
            WireStatusState::Expected | WireStatusState::Pending => Self::Pending,
            WireStatusState::Error | WireStatusState::Failure => Self::Failure,
            WireStatusState::Success => Self::Success,
            WireStatusState::Unknown => Self::Neutral,
        }
    }
}

impl From<WireCheckRunState> for CheckState {
    fn from(state: WireCheckRunState) -> Self {
        match state {
            WireCheckRunState::InProgress
            | WireCheckRunState::Pending
            | WireCheckRunState::Queued
            | WireCheckRunState::Waiting
            | WireCheckRunState::Requested => Self::Pending,
            WireCheckRunState::Success => Self::Success,
            WireCheckRunState::Failure
            | WireCheckRunState::ActionRequired
            | WireCheckRunState::StartupFailure
            | WireCheckRunState::TimedOut => Self::Failure,
            WireCheckRunState::Cancelled => Self::Cancelled,
            WireCheckRunState::Skipped => Self::Skipped,
            WireCheckRunState::Neutral
            | WireCheckRunState::Stale
            | WireCheckRunState::Completed
            | WireCheckRunState::Unknown => Self::Neutral,
        }
    }
}

fn check_run_state(status: WireCheckStatus, conclusion: Option<WireCheckConclusion>) -> CheckState {
    if status != WireCheckStatus::Completed {
        return CheckState::Pending;
    }
    match conclusion {
        Some(WireCheckConclusion::Success) => CheckState::Success,
        Some(
            WireCheckConclusion::Failure
            | WireCheckConclusion::ActionRequired
            | WireCheckConclusion::TimedOut
            | WireCheckConclusion::StartupFailure,
        ) => CheckState::Failure,
        Some(WireCheckConclusion::Cancelled) => CheckState::Cancelled,
        Some(WireCheckConclusion::Skipped) => CheckState::Skipped,
        Some(
            WireCheckConclusion::Neutral
            | WireCheckConclusion::Stale
            | WireCheckConclusion::Unknown,
        )
        | None => CheckState::Neutral,
    }
}

impl From<WireMergeable> for Mergeability {
    fn from(state: WireMergeable) -> Self {
        match state {
            WireMergeable::Mergeable => Self::Mergeable,
            WireMergeable::Conflicting => Self::Conflicting,
            WireMergeable::Unknown => Self::Unknown,
        }
    }
}

impl From<WireMergeStateStatus> for MergeStateStatus {
    fn from(state: WireMergeStateStatus) -> Self {
        match state {
            WireMergeStateStatus::Clean => Self::Clean,
            WireMergeStateStatus::Blocked => Self::Blocked,
            WireMergeStateStatus::Behind => Self::Behind,
            WireMergeStateStatus::Dirty => Self::Dirty,
            WireMergeStateStatus::Unstable => Self::Unstable,
            WireMergeStateStatus::HasHooks => Self::HasHooks,
            WireMergeStateStatus::Draft => Self::Draft,
            WireMergeStateStatus::Unknown => Self::Unknown,
        }
    }
}

impl From<WireMergeMethod> for MergeMethod {
    fn from(method: WireMergeMethod) -> Self {
        match method {
            WireMergeMethod::Merge => Self::Merge,
            WireMergeMethod::Squash => Self::Squash,
            WireMergeMethod::Rebase => Self::Rebase,
        }
    }
}

impl From<WireReviewState> for ReviewState {
    fn from(state: WireReviewState) -> Self {
        match state {
            WireReviewState::Approved => Self::Approved,
            WireReviewState::ChangesRequested => Self::ChangesRequested,
            WireReviewState::Dismissed => Self::Dismissed,
            WireReviewState::Pending => Self::Pending,
            WireReviewState::Commented | WireReviewState::Unknown => Self::Commented,
        }
    }
}

impl From<WireDiffSide> for DiffSide {
    fn from(side: WireDiffSide) -> Self {
        match side {
            WireDiffSide::Left => Self::Left,
            WireDiffSide::Right => Self::Right,
        }
    }
}

// ---------------------------------------------------------------------------
// Shared objects

#[derive(Debug, Clone, Deserialize)]
struct WireActor {
    login: String,
}

fn login(actor: Option<WireActor>) -> String {
    actor.map_or_else(|| GHOST_LOGIN.to_string(), |actor| actor.login)
}

#[derive(Debug, Deserialize)]
pub(super) struct RepositoryView {
    #[serde(rename = "nameWithOwner")]
    pub name_with_owner: String,
    pub url: String,
}

// ---------------------------------------------------------------------------
// Summary

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireSummary {
    number: u64,
    title: String,
    url: String,
    state: WirePullRequestState,
    is_draft: bool,
    review_decision: Option<WireReviewDecision>,
    author: Option<WireActor>,
    head_ref_name: String,
    base_ref_name: String,
    head_ref_oid: String,
    base_ref_oid: String,
    additions: u64,
    deletions: u64,
    changed_files: u64,
    updated_at: String,
    is_cross_repository: bool,
    check_rollup: Nodes<WireRollupCommit>,
}

#[derive(Debug, Deserialize)]
struct WireRollupCommit {
    commit: WireRollupCommitInner,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireRollupCommitInner {
    status_check_rollup: Option<WireRollup>,
}

#[derive(Debug, Deserialize)]
struct WireRollup {
    state: WireStatusState,
    contexts: WireRollupCounts,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireRollupCounts {
    check_run_counts_by_state: Vec<WireCount<WireCheckRunState>>,
    status_context_counts_by_state: Vec<WireCount<WireStatusState>>,
}

#[derive(Debug, Deserialize)]
struct WireCount<S> {
    state: S,
    count: u32,
}

impl From<WireSummary> for PullRequestSummary {
    fn from(wire: WireSummary) -> Self {
        let checks = wire
            .check_rollup
            .nodes
            .into_iter()
            .next()
            .and_then(|node| node.commit.status_check_rollup)
            .map(|rollup| {
                let mut counts = CheckCounts::default();
                for entry in rollup.contexts.check_run_counts_by_state {
                    counts.add(entry.state.into(), entry.count);
                }
                for entry in rollup.contexts.status_context_counts_by_state {
                    counts.add(entry.state.into(), entry.count);
                }
                CheckRollup {
                    state: rollup.state.into(),
                    counts,
                }
            });
        Self {
            number: wire.number,
            title: wire.title,
            author: login(wire.author),
            url: wire.url,
            head_ref_name: wire.head_ref_name,
            base_ref_name: wire.base_ref_name,
            head_oid: wire.head_ref_oid,
            base_oid: wire.base_ref_oid,
            is_draft: wire.is_draft,
            state: wire.state.into(),
            review_decision: wire.review_decision.and_then(review_decision),
            checks,
            additions: wire.additions,
            deletions: wire.deletions,
            changed_files: wire.changed_files,
            updated_at: Timestamp::new(wire.updated_at),
            is_cross_repository: wire.is_cross_repository,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SearchPage {
    pub issue_count: u64,
    pub page_info: PageInfo,
    pub nodes: Vec<SearchNode>,
}

/// Search can return issues too; they decode as `Other` and are skipped.
#[derive(Debug, Deserialize)]
#[serde(tag = "__typename")]
pub(super) enum SearchNode {
    PullRequest(Box<WireSummary>),
    #[serde(other)]
    Other,
}

impl SearchNode {
    pub fn into_summary(self) -> Option<PullRequestSummary> {
        match self {
            Self::PullRequest(summary) => Some((*summary).into()),
            Self::Other => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Detail

/// `data` of the `PullRequest` query.
#[derive(Debug, Deserialize)]
pub(super) struct PullRequestData {
    viewer: WireActor,
    repository: WireRepository,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireRepository {
    #[serde(flatten)]
    settings: WireMergeSettings,
    pull_request: Option<WirePullRequest>,
}

/// `MergeSettingsFields`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireMergeSettings {
    merge_commit_allowed: bool,
    squash_merge_allowed: bool,
    rebase_merge_allowed: bool,
    auto_merge_allowed: bool,
    delete_branch_on_merge: bool,
    viewer_default_merge_method: WireMergeMethod,
}

/// `data` of the batched `PullRequests` query: the viewer, the merge
/// settings, and one aliased pull request per number.
#[derive(Debug, Deserialize)]
pub(super) struct PullRequestsData {
    viewer: WireActor,
    repository: serde_json::Map<String, Value>,
}

impl PullRequestsData {
    /// One [`PullRequestData`] per number in `numbers`, as if each had been
    /// loaded alone, read from the field [`pull_request_alias`] names. A
    /// missing or null alias splits into [`ForgeError::NotFound`].
    pub fn into_each(self, numbers: &[u64]) -> Result<Vec<(u64, PullRequestData)>, ForgeError> {
        let mut repository = self.repository;
        let pull_requests = numbers
            .iter()
            .map(|number| (*number, repository.remove(&pull_request_alias(*number))))
            .collect::<Vec<_>>();
        let settings: WireMergeSettings = decode_value("pull requests", Value::Object(repository))?;
        pull_requests
            .into_iter()
            .map(|(number, pull_request)| {
                let pull_request = match pull_request {
                    Some(value) if !value.is_null() => Some(decode_value("pull requests", value)?),
                    _ => None,
                };
                Ok((
                    number,
                    PullRequestData {
                        viewer: self.viewer.clone(),
                        repository: WireRepository {
                            settings: settings.clone(),
                            pull_request,
                        },
                    },
                ))
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WirePullRequest {
    #[serde(flatten)]
    summary: WireSummary,
    body: String,
    created_at: String,
    mergeable: WireMergeable,
    merge_state_status: WireMergeStateStatus,
    viewer_did_author: bool,
    viewer_can_update: bool,
    viewer_can_close: bool,
    viewer_can_reopen: bool,
    viewer_can_enable_auto_merge: bool,
    viewer_can_disable_auto_merge: bool,
    viewer_can_delete_head_ref: bool,
    commit_count: WireTotalCount,
    auto_merge_request: Option<WireAutoMerge>,
    review_requests: Connection<WireReviewRequest>,
    latest_reviews: Connection<WireReview>,
    viewer_pending_reviews: Nodes<WirePendingReview>,
    comments: Connection<WireConversationComment>,
    review_threads: Connection<WireThread>,
    checks: Nodes<WireChecksCommit>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireTotalCount {
    total_count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireAutoMerge {
    merge_method: WireMergeMethod,
    enabled_at: Option<String>,
    enabled_by: Option<WireActor>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireReviewRequest {
    requested_reviewer: Option<WireRequestedReviewer>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "__typename")]
enum WireRequestedReviewer {
    User { login: String },
    Bot { login: String },
    Mannequin { login: String },
    Team { slug: String, name: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireReview {
    id: String,
    author: Option<WireActor>,
    state: WireReviewState,
    body: String,
    submitted_at: Option<String>,
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WirePendingReview {
    id: String,
    created_at: String,
    body: String,
    comments: WireTotalCount,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireConversationComment {
    id: String,
    author: Option<WireActor>,
    body: String,
    created_at: String,
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireThread {
    id: String,
    path: String,
    diff_side: WireDiffSide,
    line: Option<u32>,
    start_line: Option<u32>,
    start_diff_side: Option<WireDiffSide>,
    original_line: Option<u32>,
    original_start_line: Option<u32>,
    subject_type: WireSubjectType,
    is_resolved: bool,
    is_outdated: bool,
    resolved_by: Option<WireActor>,
    viewer_can_resolve: bool,
    viewer_can_unresolve: bool,
    viewer_can_reply: bool,
    comments: Connection<WireThreadComment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireThreadComment {
    id: String,
    author: Option<WireActor>,
    body: String,
    created_at: String,
    url: String,
    state: WireCommentState,
    diff_hunk: String,
}

#[derive(Debug, Deserialize)]
struct WireChecksCommit {
    commit: WireChecksCommitInner,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireChecksCommitInner {
    status_check_rollup: Option<WireChecksRollup>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WireChecksRollup {
    pub contexts: Connection<WireCheck>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "__typename")]
pub(super) enum WireCheck {
    #[serde(rename_all = "camelCase")]
    CheckRun {
        name: String,
        status: WireCheckStatus,
        conclusion: Option<WireCheckConclusion>,
        details_url: Option<String>,
        title: Option<String>,
        is_required: bool,
        check_suite: Option<WireCheckSuite>,
    },
    #[serde(rename_all = "camelCase")]
    StatusContext {
        context: String,
        state: WireStatusState,
        target_url: Option<String>,
        description: Option<String>,
        is_required: bool,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireCheckSuite {
    workflow_run: Option<WireWorkflowRun>,
}

#[derive(Debug, Deserialize)]
struct WireWorkflowRun {
    workflow: WireActorName,
}

#[derive(Debug, Deserialize)]
struct WireActorName {
    name: String,
}

impl From<WireReview> for Review {
    fn from(wire: WireReview) -> Self {
        Self {
            id: wire.id,
            author: login(wire.author),
            state: wire.state.into(),
            body: wire.body,
            submitted_at: wire.submitted_at.map(Timestamp::new),
            url: wire.url,
        }
    }
}

impl From<WireConversationComment> for ConversationComment {
    fn from(wire: WireConversationComment) -> Self {
        Self {
            id: wire.id,
            author: login(wire.author),
            body: wire.body,
            created_at: Timestamp::new(wire.created_at),
            url: wire.url,
        }
    }
}

impl WireReviewRequest {
    /// `None` when the reviewer is no longer visible (a deleted account).
    fn into_reviewer(self) -> Option<RequestedReviewer> {
        Some(match self.requested_reviewer? {
            WireRequestedReviewer::User { login }
            | WireRequestedReviewer::Bot { login }
            | WireRequestedReviewer::Mannequin { login } => RequestedReviewer::User { login },
            WireRequestedReviewer::Team { slug, name } => RequestedReviewer::Team { slug, name },
        })
    }
}

impl From<WireThreadComment> for ThreadComment {
    fn from(wire: WireThreadComment) -> Self {
        Self {
            id: wire.id,
            author: login(wire.author),
            body: wire.body,
            created_at: Timestamp::new(wire.created_at),
            url: wire.url,
            state: match wire.state {
                WireCommentState::Pending => CommentState::Pending,
                WireCommentState::Submitted => CommentState::Submitted,
            },
        }
    }
}

impl From<WireCheck> for Check {
    fn from(wire: WireCheck) -> Self {
        match wire {
            WireCheck::CheckRun {
                name,
                status,
                conclusion,
                details_url,
                title,
                is_required,
                check_suite,
            } => Self {
                name,
                workflow: check_suite
                    .and_then(|suite| suite.workflow_run)
                    .map(|run| run.workflow.name),
                state: check_run_state(status, conclusion),
                summary: title.filter(|title| !title.is_empty()),
                url: details_url,
                is_required,
            },
            WireCheck::StatusContext {
                context,
                state,
                target_url,
                description,
                is_required,
            } => Self {
                name: context,
                workflow: None,
                state: state.into(),
                summary: description.filter(|description| !description.is_empty()),
                url: target_url,
                is_required,
            },
        }
    }
}

/// A review thread whose comment list may still have later pages.
pub(super) struct PartialThread {
    pub id: String,
    pub comments_page: PageInfo,
    wire: WireThread,
    comments: Vec<WireThreadComment>,
}

impl From<WireThread> for PartialThread {
    fn from(mut wire: WireThread) -> Self {
        let comments = std::mem::take(&mut wire.comments.nodes);
        let comments_page = std::mem::replace(
            &mut wire.comments.page_info,
            PageInfo {
                has_next_page: false,
                end_cursor: None,
            },
        );
        Self {
            id: wire.id.clone(),
            comments_page,
            wire,
            comments,
        }
    }
}

impl PartialThread {
    pub fn extend_comments(&mut self, more: impl IntoIterator<Item = WireThreadComment>) {
        self.comments.extend(more);
    }

    pub fn finish(self) -> ReviewThread {
        let wire = self.wire;
        let side = DiffSide::from(wire.diff_side);
        // GitHub reports `startLine == line` with no start side for
        // single-line threads; only a real range gets a start.
        let start = wire.start_line.and_then(|start_line| {
            let start_side = wire.start_diff_side.map_or(side, DiffSide::from);
            let is_range = wire.start_diff_side.is_some() || Some(start_line) != wire.line;
            is_range.then_some(DiffPosition {
                side: start_side,
                line: start_line,
            })
        });
        let original_start_line = wire
            .original_start_line
            .filter(|start| Some(*start) != wire.original_line);
        let diff_hunk = self
            .comments
            .first()
            .map(|comment| comment.diff_hunk.clone())
            .unwrap_or_default();
        ReviewThread {
            id: ThreadId::new(wire.id),
            path: wire.path,
            subject: match wire.subject_type {
                WireSubjectType::File => ThreadSubject::File,
                WireSubjectType::Line => ThreadSubject::Line,
            },
            side,
            line: wire.line,
            start,
            original_line: wire.original_line,
            original_start_line,
            is_resolved: wire.is_resolved,
            resolved_by: wire.resolved_by.map(|actor| actor.login),
            is_outdated: wire.is_outdated,
            viewer_can_reply: wire.viewer_can_reply,
            viewer_can_resolve: wire.viewer_can_resolve,
            viewer_can_unresolve: wire.viewer_can_unresolve,
            diff_hunk,
            comments: self.comments.into_iter().map(ThreadComment::from).collect(),
        }
    }
}

/// The first page of every collection on a pull request. Each connection
/// may have later pages the caller must collect before
/// [`PullRequestShell::finish`].
pub(super) struct FirstPages {
    pub review_requests: Connection<WireReviewRequest>,
    pub latest_reviews: Connection<WireReview>,
    pub conversation: Connection<WireConversationComment>,
    pub review_threads: Connection<WireThread>,
    /// `None` when the head commit has no checks.
    pub checks: Option<Connection<WireCheck>>,
}

/// The scalar fields of a pull request, waiting for its collections.
pub(super) struct PullRequestShell {
    /// The commit the checks belong to; check pages are pinned to it so a
    /// push during loading cannot mix two commits' checks.
    pub head_oid: String,
    viewer_login: String,
    summary: WireSummary,
    body: String,
    created_at: String,
    mergeable: WireMergeable,
    merge_state_status: WireMergeStateStatus,
    viewer_did_author: bool,
    viewer_can_update: bool,
    viewer_can_close: bool,
    viewer_can_reopen: bool,
    viewer_can_enable_auto_merge: bool,
    viewer_can_disable_auto_merge: bool,
    viewer_can_delete_head_ref: bool,
    commit_count: u32,
    auto_merge_request: Option<WireAutoMerge>,
    pending_review: Option<WirePendingReview>,
    merge_settings: MergeSettings,
}

/// Every page of a pull request's collections, fully collected.
pub(super) struct CollectedPages {
    pub review_requests: Vec<WireReviewRequest>,
    pub latest_reviews: Vec<WireReview>,
    pub conversation: Vec<WireConversationComment>,
    pub review_threads: Vec<ReviewThread>,
    pub checks: Vec<WireCheck>,
}

impl PullRequestData {
    pub fn split(self, number: u64) -> Result<(PullRequestShell, FirstPages), ForgeError> {
        let repository = self.repository;
        let pull_request = repository
            .pull_request
            .ok_or_else(|| ForgeError::NotFound {
                message: format!("pull request #{number}"),
            })?;
        let mut allowed_methods = Vec::new();
        if repository.settings.merge_commit_allowed {
            allowed_methods.push(MergeMethod::Merge);
        }
        if repository.settings.squash_merge_allowed {
            allowed_methods.push(MergeMethod::Squash);
        }
        if repository.settings.rebase_merge_allowed {
            allowed_methods.push(MergeMethod::Rebase);
        }
        let merge_settings = MergeSettings {
            allowed_methods,
            viewer_default_method: repository.settings.viewer_default_merge_method.into(),
            auto_merge_allowed: repository.settings.auto_merge_allowed,
            delete_branch_on_merge: repository.settings.delete_branch_on_merge,
        };
        let first = FirstPages {
            review_requests: pull_request.review_requests,
            latest_reviews: pull_request.latest_reviews,
            conversation: pull_request.comments,
            review_threads: pull_request.review_threads,
            checks: pull_request
                .checks
                .nodes
                .into_iter()
                .next()
                .and_then(|node| node.commit.status_check_rollup)
                .map(|rollup| rollup.contexts),
        };
        let shell = PullRequestShell {
            head_oid: pull_request.summary.head_ref_oid.clone(),
            viewer_login: self.viewer.login,
            summary: pull_request.summary,
            body: pull_request.body,
            created_at: pull_request.created_at,
            mergeable: pull_request.mergeable,
            merge_state_status: pull_request.merge_state_status,
            viewer_did_author: pull_request.viewer_did_author,
            viewer_can_update: pull_request.viewer_can_update,
            viewer_can_close: pull_request.viewer_can_close,
            viewer_can_reopen: pull_request.viewer_can_reopen,
            viewer_can_enable_auto_merge: pull_request.viewer_can_enable_auto_merge,
            viewer_can_disable_auto_merge: pull_request.viewer_can_disable_auto_merge,
            viewer_can_delete_head_ref: pull_request.viewer_can_delete_head_ref,
            commit_count: pull_request.commit_count.total_count,
            auto_merge_request: pull_request.auto_merge_request,
            pending_review: pull_request.viewer_pending_reviews.nodes.into_iter().next(),
            merge_settings,
        };
        Ok((shell, first))
    }
}

impl PullRequestShell {
    /// Builds the pull request from its fully collected pages.
    pub fn finish(self, pages: CollectedPages) -> PullRequest {
        let rest = self;
        PullRequest {
            summary: rest.summary.into(),
            body: rest.body,
            created_at: Timestamp::new(rest.created_at),
            commit_count: rest.commit_count,
            mergeable: rest.mergeable.into(),
            merge_state: rest.merge_state_status.into(),
            auto_merge: rest.auto_merge_request.map(|request| AutoMerge {
                method: request.merge_method.into(),
                enabled_by: request.enabled_by.map(|actor| actor.login),
                enabled_at: request.enabled_at.map(Timestamp::new),
            }),
            requested_reviewers: pages
                .review_requests
                .into_iter()
                .filter_map(WireReviewRequest::into_reviewer)
                .collect(),
            latest_reviews: pages.latest_reviews.into_iter().map(Review::from).collect(),
            conversation: pages
                .conversation
                .into_iter()
                .map(ConversationComment::from)
                .collect(),
            checks: pages.checks.into_iter().map(Check::from).collect(),
            review_threads: pages.review_threads,
            viewer: Viewer {
                login: rest.viewer_login,
                is_author: rest.viewer_did_author,
                pending_review: rest.pending_review.map(|review| PendingReview {
                    id: review.id,
                    created_at: Timestamp::new(review.created_at),
                    body: review.body,
                    comment_count: review.comments.total_count,
                }),
                can_update: rest.viewer_can_update,
                can_close: rest.viewer_can_close,
                can_reopen: rest.viewer_can_reopen,
                can_enable_auto_merge: rest.viewer_can_enable_auto_merge,
                can_disable_auto_merge: rest.viewer_can_disable_auto_merge,
                can_delete_head_ref: rest.viewer_can_delete_head_ref,
            },
            merge_settings: rest.merge_settings,
        }
    }
}

/// `data.repository.pullRequest` of the `MergeTarget` query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MergeTarget {
    pub id: String,
    pub head_ref_name: String,
    pub is_cross_repository: bool,
    pub viewer_can_delete_head_ref: bool,
}

/// The REST review object returned by `POST .../pulls/{n}/reviews`.
#[derive(Debug, Deserialize)]
pub(super) struct RestReview {
    node_id: String,
    user: Option<WireActor>,
    state: WireReviewState,
    body: Option<String>,
    submitted_at: Option<String>,
    html_url: String,
}

impl From<RestReview> for Review {
    fn from(wire: RestReview) -> Self {
        Self {
            id: wire.node_id,
            author: login(wire.user),
            state: wire.state.into(),
            body: wire.body.unwrap_or_default(),
            submitted_at: wire.submitted_at.map(Timestamp::new),
            url: wire.html_url,
        }
    }
}

#[cfg(test)]
mod tests;
