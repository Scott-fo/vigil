//! GitHub pull requests in the review screen.
//!
//! This module owns the app's pull request state, kept in one
//! [`PullRequests`](state::PullRequests) value on `App`:
//!
//! - **The GitHub connection.** vigil connects lazily in the background the
//!   first time a branch snapshot loads. When the repository is not on
//!   GitHub, or `gh` is missing or logged out, pull request features stay off
//!   quietly; only an explicit request (`O`, a click on the footer chip)
//!   retries and explains why.
//! - **The current branch's pull request**, shown as a footer chip. It is
//!   looked up again whenever the branch snapshot reloads, at most every few
//!   seconds for the same branch tip.
//! - **The pull request under review.** Opening one fetches its commits
//!   (`git::fetch_pull_request`) while its detail loads, then switches to
//!   [`ReviewMode::PullRequest`](super::ReviewMode) with a
//!   [`PullRequestSelection`]. The sidebar pins an overview page first. While
//!   open, it is polled every 30 seconds; new commits raise a notice and `r`
//!   refetches and reloads. Any other review mode ends it.
//!
//! Every request is matched by id: a response to a superseded request is
//! dropped, and dropping the state aborts its tasks (and their `gh`
//! processes). Nothing here writes to GitHub.

mod connect;
mod current_branch;
mod event;
mod open;
mod selection;
mod state;
mod task;
mod view;

pub use self::event::{PullRequestEvent, PullRequestTimer};
pub use self::selection::PullRequestSelection;
pub use self::state::PullRequestPage;
pub(super) use self::state::PullRequests;
pub use self::view::PullRequestOverview;

use super::App;

impl App {
    /// Applies a pull request event. Returns whether anything visible changed.
    pub(super) async fn handle_pull_request_event(
        &mut self,
        event: PullRequestEvent,
    ) -> color_eyre::Result<bool> {
        match event {
            PullRequestEvent::Connected { request_id, result } => {
                Ok(self.handle_forge_connected(request_id, result))
            }
            PullRequestEvent::CurrentBranchLoaded { request_id, result } => {
                Ok(self.handle_current_branch_pull_request_loaded(request_id, result))
            }
            PullRequestEvent::Fetched { request_id, result } => {
                self.handle_pull_request_fetched(request_id, result).await
            }
            PullRequestEvent::DetailLoaded { request_id, result } => {
                Ok(self.handle_pull_request_detail_loaded(request_id, *result))
            }
            PullRequestEvent::Polled { request_id, result } => {
                Ok(self.handle_pull_request_polled(request_id, result))
            }
            PullRequestEvent::Tick(PullRequestTimer::OpenPullRequest) => {
                Ok(self.handle_open_pull_request_tick())
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
