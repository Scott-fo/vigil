//! GitHub pull requests in the review screen.
//!
//! This module owns the app's pull request state, kept in one
//! [`PullRequests`](state::PullRequests) value on `App`:
//!
//! - **The GitHub connection.** vigil connects lazily in the background the
//!   first time a branch snapshot loads; usually that reads only git config
//!   (see [`GitHub::connect`](crate::forge::GitHub::connect)). When the
//!   repository is not on GitHub, or `gh` is missing or logged out, pull
//!   request features stay off quietly; only an explicit request (`O`, a
//!   click on the footer chip, the list) retries and explains why. Since
//!   connecting may not run `gh`, a missing or logged-out `gh`, or a
//!   repository `gh` cannot see, is often found by the first request
//!   instead, which turns the connection off the same way.
//! - **The current branch's pull request**, shown as a footer chip. It is
//!   looked up again whenever the branch snapshot reloads, at most every few
//!   seconds for the same branch tip.
//! - **The pull request under review.** Opening one uses its head and base
//!   commits straight from the object store when they are there
//!   (`git::resolve_local_pull_request`), and fetches them
//!   (`git::fetch_pull_request`) only when they are not, while its detail
//!   loads; then it switches to [`ReviewMode::PullRequest`](super::ReviewMode)
//!   with a [`PullRequestSelection`]. The sidebar pins an overview page
//!   first. While open, it is polled every 30 seconds; new commits raise a
//!   notice and `r` always refetches and reloads. Any other review mode ends
//!   it and deletes the refs its fetch wrote; refs left by an earlier session
//!   are pruned at startup. Opened from the list, Esc goes back to the list.
//!   `K` checks the reviewed head out as a local branch through the shared
//!   branch operation (see [`PullRequestSelection::checkout`]); the review
//!   stays open, pinned to the same commits.
//! - **The pull request list screen**
//!   ([`Screen::PullRequestList`](super::Screen)): open pull requests per
//!   [`PullRequestListFilter`](crate::forge::PullRequestListFilter) tab, kept
//!   per tab, filtered by typed text, and reloaded every 60 seconds while on
//!   screen. Enter opens the selected row; a query such as `#17` opens that
//!   pull request by number even when it is closed or merged.
//! - **Reviewing and acting on it.** Draft comments (`c`) are local and
//!   persisted in the review database until a review (`S`) sends them;
//!   replies (`R`), resolving (`T`), conversation comments (`C`), merging
//!   (`M`), and state changes (`A`) write to GitHub at once, after
//!   confirmation where a mistake is costly. Each write is a typed
//!   [`ForgeMutation`](gateway::ForgeMutation) run through one gateway, one
//!   at a time, and reloads what it changed.
//!
//! Every request is matched by id: a response to a superseded request is
//! dropped, and dropping the state aborts its reads (and their `gh`
//! processes). Writes are never aborted once started.

mod actions;
mod composer;
mod connect;
mod current_branch;
mod draft_list;
mod drafts;
mod event;
mod gateway;
mod list;
mod list_screen;
mod merge;
mod modal;
mod open;
mod selection;
mod state;
mod submit;
mod task;
mod threads;
mod view;

pub use self::actions::{ActionsMenu, PullRequestAction};
pub use self::composer::{Composer, ComposerStatus, ComposerTarget};
pub use self::draft_list::DraftList;
pub use self::event::{PullRequestEvent, PullRequestTimer};
pub use self::gateway::MutationOutcome;
pub use self::list::{PULL_REQUEST_LIST_FILTERS, QueryInput};
pub use self::list_screen::{PullRequestListStatus, PullRequestListView};
pub use self::merge::{AutoMergeChoice, MergeBlocker, MergeForm, MergeStep, MergeWhen};
pub use self::modal::{DraftEntry, PullRequestModalView};
pub use self::selection::PullRequestSelection;
pub use self::state::PullRequestPage;
pub(super) use self::state::PullRequests;
pub use self::submit::{REVIEW_EVENTS, SubmitForm, SubmitWarning, event_allowed};
pub use self::view::PullRequestOverview;

use super::App;

impl App {
    /// Applies a pull request event. Returns whether anything visible changed.
    pub(super) async fn handle_pull_request_event(
        &mut self,
        event: PullRequestEvent,
    ) -> color_eyre::Result<bool> {
        if let Some(error) = event.request_error()
            && error.leaves_forge_unavailable()
        {
            self.pull_requests.mark_unavailable(error.clone());
        }
        match event {
            PullRequestEvent::Connected { request_id, result } => {
                Ok(self.handle_forge_connected(request_id, result))
            }
            PullRequestEvent::FetchStarted { request_id } => {
                Ok(self.handle_pull_request_fetch_started(request_id))
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
            PullRequestEvent::ListLoaded {
                request_id,
                filter,
                result,
            } => Ok(self.handle_pull_request_list_loaded(request_id, filter, result)),
            PullRequestEvent::LookedUp { request_id, result } => {
                Ok(self.handle_pull_request_looked_up(request_id, result))
            }
            PullRequestEvent::DraftsLoaded { request_id, result } => {
                Ok(self.handle_drafts_loaded(request_id, result))
            }
            PullRequestEvent::DraftWriteFailed(error) => Ok(self.handle_draft_write_failed(error)),
            PullRequestEvent::MutationFinished { request_id, result } => {
                Ok(self.handle_mutation_finished(request_id, result))
            }
            PullRequestEvent::Tick(PullRequestTimer::OpenPullRequest) => {
                Ok(self.handle_open_pull_request_tick())
            }
            PullRequestEvent::Tick(PullRequestTimer::List) => {
                Ok(self.handle_pull_request_list_tick())
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod tests;
