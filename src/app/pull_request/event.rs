use crate::{
    forge::{
        ForgeError, GitHub, PullRequest, PullRequestList, PullRequestListFilter, PullRequestSummary,
    },
    git::{FetchedPullRequest, PullRequestFetchError},
    review::DraftComment,
};

use super::gateway::MutationOutcome;

/// Results of background pull request work, delivered through the app's event
/// loop. Each carries the id of the request that produced it so superseded
/// responses can be dropped.
#[derive(Debug)]
pub enum PullRequestEvent {
    Connected {
        request_id: u64,
        result: Result<GitHub, ForgeError>,
    },
    CurrentBranchLoaded {
        request_id: u64,
        result: Result<Option<PullRequestSummary>, ForgeError>,
    },
    /// The pull request's commits are not local, so a network fetch began.
    FetchStarted {
        request_id: u64,
    },
    /// The pull request's commits are ready, from the object store or a
    /// fetch.
    Fetched {
        request_id: u64,
        result: Result<FetchedPullRequest, PullRequestFetchError>,
    },
    DetailLoaded {
        request_id: u64,
        result: Box<Result<PullRequest, ForgeError>>,
    },
    Polled {
        request_id: u64,
        result: Result<PullRequestSummary, ForgeError>,
    },
    ListLoaded {
        request_id: u64,
        filter: PullRequestListFilter,
        result: Result<PullRequestList, ForgeError>,
    },
    /// A pull request looked up by number from the list's query.
    LookedUp {
        request_id: u64,
        result: Result<PullRequestSummary, ForgeError>,
    },
    /// The open pull request's drafts, read from the review database.
    DraftsLoaded {
        request_id: u64,
        result: Result<Vec<DraftComment>, String>,
    },
    /// A background draft write failed; the drafts on screen are newer than
    /// the database.
    DraftWriteFailed(String),
    /// A write to GitHub finished.
    MutationFinished {
        request_id: u64,
        result: Result<MutationOutcome, ForgeError>,
    },
    Tick(PullRequestTimer),
}

impl PullRequestEvent {
    /// The error a finished GitHub request reports, if it failed. Connecting
    /// is left out: its failures are handled as connection state.
    pub(super) fn request_error(&self) -> Option<&ForgeError> {
        match self {
            Self::CurrentBranchLoaded { result, .. } => result.as_ref().err(),
            Self::DetailLoaded { result, .. } => result.as_ref().as_ref().err(),
            Self::Polled { result, .. } | Self::LookedUp { result, .. } => result.as_ref().err(),
            Self::ListLoaded { result, .. } => result.as_ref().err(),
            Self::MutationFinished { result, .. } => result.as_ref().err(),
            Self::Connected { .. }
            | Self::FetchStarted { .. }
            | Self::Fetched { .. }
            | Self::DraftsLoaded { .. }
            | Self::DraftWriteFailed(_)
            | Self::Tick(_) => None,
        }
    }
}

/// Which periodic refresh a [`PullRequestEvent::Tick`] is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestTimer {
    /// Checks the pull request under review for new commits and activity.
    OpenPullRequest,
    /// Reloads the pull request list while it is on screen.
    List,
}
