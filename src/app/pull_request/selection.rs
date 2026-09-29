use crate::{
    forge::PullRequestSummary,
    git::{BranchCompareSelection, FetchedPullRequest},
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
            compare: BranchCompareSelection {
                source_ref: fetched.head_oid.clone(),
                destination_ref: fetched.base_oid.clone(),
            },
        }
    }
}
