use crate::{
    forge::{ForgeError, GitHub, PullRequest, PullRequestSummary},
    git::{FetchedPullRequest, PullRequestFetchError},
};

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
    Tick(PullRequestTimer),
}

/// Which periodic refresh a [`PullRequestEvent::Tick`] is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestTimer {
    /// Checks the pull request under review for new commits and activity.
    OpenPullRequest,
}
