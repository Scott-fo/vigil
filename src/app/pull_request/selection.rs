use crate::{
    forge::PullRequestSummary,
    git::{BranchCompareSelection, FetchedPullRequest, PullRequestCheckout, RemoteBranch},
};

/// The pull request a [`ReviewMode::PullRequest`](crate::app::ReviewMode)
/// review shows, pinned to the commits that were fetched for it.
///
/// A pull request's diff is GitHub's three-dot diff, `base...head`, which is
/// exactly what branch compare runs. `compare` holds that comparison with the
/// fetched head commit as the source and the base commit as the destination,
/// so every diff, stats, and search path reuses the branch-compare loaders.
/// Pinning commit ids rather than refs keeps cached diffs honest: a reload that
/// fetches a new head produces a new selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestSelection {
    pub number: u64,
    pub title: String,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub head_oid: String,
    pub base_oid: String,
    /// The remote the commits were fetched from.
    pub remote: String,
    /// The head branch lives in a fork, which has no branch on `remote`.
    pub is_cross_repository: bool,
    pub compare: BranchCompareSelection,
}

impl PullRequestSelection {
    pub fn new(summary: &PullRequestSummary, fetched: &FetchedPullRequest) -> Self {
        Self {
            number: summary.number,
            title: summary.title.clone(),
            head_ref_name: summary.head_ref_name.clone(),
            base_ref_name: summary.base_ref_name.clone(),
            head_oid: fetched.head_oid.clone(),
            base_oid: fetched.base_oid.clone(),
            remote: fetched.remote.clone(),
            is_cross_repository: summary.is_cross_repository,
            compare: BranchCompareSelection {
                source_ref: fetched.head_oid.clone(),
                destination_ref: fetched.base_oid.clone(),
            },
        }
    }

    /// Checking out the reviewed head as a local branch. A branch on the
    /// base repository keeps its name and tracks its remote branch; a fork's
    /// head becomes `pr-<number>`, since its name (often `main`) may clash
    /// with a local branch and vigil cannot push to the fork by name.
    pub fn checkout(&self) -> PullRequestCheckout {
        let (branch, upstream) = if self.is_cross_repository {
            (format!("pr-{}", self.number), None)
        } else {
            (
                self.head_ref_name.clone(),
                Some(RemoteBranch {
                    remote: self.remote.clone(),
                    branch: self.head_ref_name.clone(),
                }),
            )
        };
        PullRequestCheckout {
            number: self.number,
            branch,
            head_oid: self.head_oid.clone(),
            upstream,
        }
    }
}
