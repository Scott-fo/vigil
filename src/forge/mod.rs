//! GitHub pull requests: browse, load, review, and merge.
//!
//! This module is the only place vigil talks to GitHub. It drives the `gh`
//! CLI, which must be installed and logged in, so vigil needs no tokens or
//! HTTP stack of its own and honors the user's `gh` configuration, including
//! GitHub Enterprise hosts. Callers get typed pull request facts and typed
//! mutations; GraphQL documents, REST payloads, and `gh` output never leave
//! the module.
//!
//! Start with [`GitHub::connect`], which resolves the repository behind a
//! checkout. Then:
//!
//! - [`GitHub::current_branch_pull_request`] and
//!   [`GitHub::list_pull_requests`] return [`PullRequestSummary`] rows.
//! - [`GitHub::load_pull_request`] returns a complete [`PullRequest`]:
//!   reviews, conversation, checks, merge state, and every
//!   [`ReviewThread`] with every comment.
//! - [`GitHub::submit_review`], [`GitHub::reply_to_thread`],
//!   [`GitHub::resolve_thread`], [`GitHub::merge_pull_request`], and friends
//!   write back.
//!
//! # Cost and staleness
//!
//! [`GitHub::connect`] usually costs two local `git` processes: when the
//! remotes settle the repository unambiguously on `github.com`, it never runs
//! `gh`. Otherwise (GitHub Enterprise, ssh host aliases, ties between
//! remotes, `GH_REPO`/`GH_HOST` overrides) it asks `gh repo view`, one API
//! round trip. Either way it happens once per session, and because the fast
//! path does not run `gh`, a missing or logged-out `gh` first shows up as
//! the error of the first request rather than of `connect`.
//!
//! Every other operation is async and spawns `gh` (tens to hundreds of
//! milliseconds plus network). Nothing is cached: each result is a
//! snapshot, and callers that display long-lived state own refreshing it.
//! Reads spend GitHub's GraphQL rate limit (5,000 points an hour); a full
//! pull request load costs about one point, plus one request per extra page
//! on pull requests with more than 50 threads or 100 comments or checks.
//! GitHub computes mergeability in the background, so a fresh load may
//! report [`Mergeability::Unknown`] until a later one. Dropping a future
//! kills its `gh` process; for writes that leaves it unknown whether the
//! change landed, so callers should reload rather than retry blindly.
//!
//! # Errors
//!
//! Every operation returns [`ForgeError`], which separates a missing or
//! logged-out `gh`, a checkout without a GitHub remote, missing objects,
//! rate limiting, and invalid input from other failures. Invalid review
//! input is rejected before any request is sent.
//! [`ForgeError::is_gh_unavailable`] picks out the failures that will repeat
//! on every request until the user installs or logs in to `gh`.
//!
//! # What callers should not rely on
//!
//! Merging and deleting head branches act on GitHub only; this module never
//! touches local branches, the working tree, or git refs, so after a merge
//! the caller decides whether to fetch or switch branches.

mod client;
mod error;
mod gh;
mod repository;
mod request;
mod types;
mod wire;

pub use self::client::GitHub;
pub use self::error::ForgeError;
pub use self::types::{
    AutoMerge, Check, CheckCounts, CheckRollup, CheckState, CommentState, ConversationComment,
    DiffPosition, DiffSide, DraftReviewComment, HeadBranchAction, HeadBranchOutcome,
    MergeCommitMessage, MergeMethod, MergeOptions, MergeOutcome, MergeSettings, MergeStateStatus,
    MergeTiming, Mergeability, PendingReview, PullRequest, PullRequestList, PullRequestListFilter,
    PullRequestState, PullRequestSummary, RepositoryRef, RequestedReviewer, Review, ReviewDecision,
    ReviewEvent, ReviewState, ReviewThread, SubmitReview, ThreadComment, ThreadId, ThreadSubject,
    Timestamp, Viewer,
};
