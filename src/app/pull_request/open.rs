use std::time::Duration;

use tokio::task;

use crate::{
    event::Event,
    forge::{ForgeError, PullRequest, PullRequestSummary, Snapshot},
    git::{self, FetchedPullRequest, PullRequestFetch, PullRequestFetchError},
};

use super::{
    super::{ActivePane, App, ReviewMode, Screen, SnackbarVariant},
    PullRequestEvent, PullRequestSelection, PullRequestTimer,
    prefetch::{OPEN_WAITS_FOR_PREFETCH, wait_for_landing},
    saved::request_time,
    state::{DetailOutcome, PollOutcome, PullRequestPage, ReviewOrigin},
    task::spawn_ticker,
};

/// How often the pull request under review is checked for new commits.
const OPEN_PULL_REQUEST_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Where opening a pull request gets its commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitSource {
    /// The head GitHub last reported, straight from the object store when
    /// its commits are already there; a fetch only when they are not.
    LocalFirst,
    /// Always fetch, for when the user asks for the newest commits.
    Fetch,
}

impl App {
    /// Opens `summary` in the review screen. When its head and base commits
    /// are already local the review switches over in milliseconds; otherwise
    /// they are fetched first, and until the fetch lands the current review
    /// stays up. Its saved detail, if any, shows until the live one loads.
    /// `origin` decides where Esc goes back to.
    ///
    /// `summary` must name the pull request's current head and base, as a
    /// lookup, the current-branch chip, or a current list row does (see
    /// [`RowCurrency`](super::list::RowCurrency)): its commits are trusted
    /// when they are local. An outdated list row is looked up first instead.
    pub(in crate::app) fn open_pull_request(
        &mut self,
        summary: PullRequestSummary,
        origin: ReviewOrigin,
    ) {
        self.start_opening_pull_request(summary, origin, CommitSource::LocalFirst);
    }

    fn start_opening_pull_request(
        &mut self,
        summary: PullRequestSummary,
        origin: ReviewOrigin,
        source: CommitSource,
    ) {
        if !self.request_forge_connection() {
            return;
        }
        let Some(github) = self.pull_requests.github().cloned() else {
            return;
        };
        let number = summary.number;
        let request = PullRequestFetch {
            repository: github.repository().clone(),
            number,
            head_oid: summary.head_oid.clone(),
            base_oid: summary.base_oid.clone(),
            base_ref_name: summary.base_ref_name.clone(),
        };
        if self.pull_requests.lookup_in_flight() {
            // `begin_open` drops it; its "looking up" status goes too.
            self.status_message = Some(self.current_status_message());
        }
        let (fetch_id, detail_id) = self.pull_requests.begin_open(summary, origin);
        // A prefetch fetching these commits would race this open for the
        // same ref; let it land, then take what it brought. A prefetch that
        // does not land soon is left behind.
        let prefetch_landing = self.pull_requests.prefetch().commits_landing(number);

        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        let fetch = task::spawn(async move {
            if let Some(landing) = prefetch_landing {
                let _ = sender.send(Event::PullRequest(PullRequestEvent::FetchStarted {
                    request_id: fetch_id,
                }));
                wait_for_landing(landing, OPEN_WAITS_FOR_PREFETCH).await;
            }
            if source == CommitSource::LocalFirst
                && let Some(local) = git::resolve_local_pull_request(&repo_root, &request).await
            {
                let _ = sender.send(Event::PullRequest(PullRequestEvent::Fetched {
                    request_id: fetch_id,
                    result: Ok(local),
                }));
                return;
            }
            let _ = sender.send(Event::PullRequest(PullRequestEvent::FetchStarted {
                request_id: fetch_id,
            }));
            let result = git::fetch_pull_request(&repo_root, &request).await;
            let _ = sender.send(Event::PullRequest(PullRequestEvent::Fetched {
                request_id: fetch_id,
                result,
            }));
        });
        self.pull_requests.attach_fetch(fetch_id, fetch);
        self.spawn_pull_request_detail_load(github, detail_id, number);
        self.read_saved_pull_request(number);
    }

    /// The commits were not local, so opening waits on the network: say so.
    pub(super) fn handle_pull_request_fetch_started(&mut self, request_id: u64) -> bool {
        let Some(number) = self.pull_requests.fetching_number(request_id) else {
            return false;
        };
        self.status_message = Some(format!("fetching pull request #{number}…"));
        true
    }

    fn spawn_pull_request_detail_load(
        &mut self,
        github: crate::forge::GitHub,
        request_id: u64,
        number: u64,
    ) {
        let sender = self.events.sender();
        let handle = task::spawn(async move {
            let result = github.load_pull_request(number).await;
            let _ = sender.send(Event::PullRequest(PullRequestEvent::DetailLoaded {
                request_id,
                result: Box::new(result),
            }));
        });
        self.pull_requests.attach_detail(request_id, handle);
    }

    /// Refetches the pull request under review and reloads its diff and
    /// detail (the `r` key in pull request mode). Always fetches, even when
    /// the known head is local: the user is asking for newer commits.
    pub(in crate::app) fn reload_open_pull_request(&mut self) {
        let Some((summary, origin)) = self
            .pull_requests
            .open()
            .map(|open| (open.summary().clone(), open.origin()))
        else {
            return;
        };
        self.start_opening_pull_request(summary, origin, CommitSource::Fetch);
    }

    /// Esc in a review opened from the list: ends the review and shows the
    /// list again, with its tab, filter, and selection as they were.
    pub(in crate::app) async fn return_to_pull_request_list(&mut self) -> color_eyre::Result<()> {
        self.review_mode = ReviewMode::WorkingTree;
        self.refresh().await?;
        self.open_pull_request_list();
        Ok(())
    }

    /// Deletes the refs a finished review's fetch wrote, in the background.
    fn spawn_pull_request_ref_cleanup(&mut self, number: u64) {
        let repo_root = self.repo_root.clone();
        self.track_background_task(task::spawn(async move {
            let _ = git::delete_pull_request_refs(&repo_root, number).await;
        }));
    }

    /// Deletes pull request refs an earlier session left behind. Runs once
    /// at startup, before any review opens.
    pub(in crate::app) fn spawn_stale_pull_request_ref_prune(&mut self) {
        let repo_root = self.repo_root.clone();
        self.track_background_task(task::spawn(async move {
            let _ = git::prune_pull_request_refs(&repo_root).await;
        }));
    }

    pub(super) async fn handle_pull_request_fetched(
        &mut self,
        request_id: u64,
        result: Result<FetchedPullRequest, PullRequestFetchError>,
    ) -> color_eyre::Result<bool> {
        let Some(summary) = self.pull_requests.finish_fetch(request_id) else {
            return Ok(false);
        };
        let fetched = match result {
            Ok(fetched) => fetched,
            Err(error) => {
                self.status_message = Some(self.current_status_message());
                self.show_snackbar(
                    format!("could not fetch pull request #{}: {error}", summary.number),
                    SnackbarVariant::Error,
                );
                return Ok(true);
            }
        };
        self.enter_pull_request_review(summary, fetched).await?;
        Ok(true)
    }

    /// Switches the review to a fetched pull request. Reloading the pull
    /// request already under review keeps the selected file and page.
    pub(in crate::app) async fn enter_pull_request_review(
        &mut self,
        summary: PullRequestSummary,
        fetched: FetchedPullRequest,
    ) -> color_eyre::Result<()> {
        let selection = PullRequestSelection::new(&summary, &fetched);
        let reloading = self.pull_requests.open_number() == Some(summary.number);
        let number = summary.number;
        if let Some(replaced) = self.pull_requests.enter(summary, fetched.head_oid.clone()) {
            self.spawn_pull_request_ref_cleanup(replaced);
        }
        // Detail that landed during the fetch may already know of a newer
        // head or base; `enter` clears the notices otherwise.
        if self.pull_request_has_newer_head() {
            self.announce_newer_head(number);
        } else if self.pull_request_base_moved() {
            self.announce_moved_base(number);
        }
        if !reloading {
            self.load_pull_request_drafts();
            self.active_pane = ActivePane::Sidebar;
            self.clear_diff_text_selection();
            let ticker = spawn_ticker(
                self.events.sender(),
                OPEN_PULL_REQUEST_POLL_INTERVAL,
                || Event::PullRequest(PullRequestEvent::Tick(PullRequestTimer::OpenPullRequest)),
            );
            if let Some(open) = self.pull_requests.open_mut() {
                open.set_ticker(ticker);
            }
        }
        self.review_mode = ReviewMode::PullRequest(selection);
        if self.screen == Screen::PullRequestList {
            self.close_pull_request_list();
        }
        if let Err(error) = self.refresh().await {
            self.show_snackbar(
                format!("could not load the pull request diff: {error}"),
                SnackbarVariant::Error,
            );
            self.review_mode = ReviewMode::WorkingTree;
            self.refresh().await?;
        }
        Ok(())
    }

    pub(super) fn handle_pull_request_detail_loaded(
        &mut self,
        request_id: u64,
        result: Result<PullRequest, ForgeError>,
    ) -> bool {
        let live = result.as_ref().ok().cloned();
        let requested_at = self.pull_requests.detail_requested_at();
        let Some(outcome) = self.pull_requests.finish_detail(request_id, result) else {
            return false;
        };
        if let Some(detail) = live {
            self.save_pull_request(Snapshot::new(detail, request_time(requested_at)));
        }
        if let Some(number) = self.pull_requests.open_number() {
            match outcome {
                DetailOutcome::HeadMoved => self.announce_newer_head(number),
                DetailOutcome::BaseMoved => self.announce_moved_base(number),
                DetailOutcome::Applied => {}
            }
        }
        if let Some(error) = self
            .pull_requests
            .open()
            .and_then(|open| open.detail_error())
        {
            let message = format!("could not load pull request details: {error}");
            self.show_snackbar(message, SnackbarVariant::Error);
        }
        true
    }

    /// Ends pull request state that no longer matches the review mode, as
    /// after `Ctrl-L`, a branch compare, or a worktree switch. Called on
    /// every refresh, the one path every mode change goes through.
    pub(in crate::app) fn sync_pull_request_with_review_mode(&mut self) {
        let reviewing = match &self.review_mode {
            ReviewMode::PullRequest(selection) => Some(selection.number),
            ReviewMode::WorkingTree
            | ReviewMode::CommitCompare(_)
            | ReviewMode::BranchCompare(_) => None,
        };
        if self.pull_requests.open_number().is_some()
            && self.pull_requests.open_number() != reviewing
            && let Some(closed) = self.pull_requests.close()
        {
            self.spawn_pull_request_ref_cleanup(closed);
        }
        self.pull_requests.rebind_repo_root(&self.repo_root);
    }

    pub(super) fn handle_open_pull_request_tick(&mut self) -> bool {
        let Some(github) = self.pull_requests.github().cloned() else {
            return false;
        };
        let Some((request_id, number)) = self.pull_requests.begin_poll() else {
            return false;
        };
        let sender = self.events.sender();
        let handle = task::spawn(async move {
            let result = github.load_summary(number).await;
            let _ = sender.send(Event::PullRequest(PullRequestEvent::Polled {
                request_id,
                result,
            }));
        });
        self.pull_requests.attach_poll(request_id, handle);
        false
    }

    pub(super) fn handle_pull_request_polled(
        &mut self,
        request_id: u64,
        result: Result<PullRequestSummary, ForgeError>,
    ) -> bool {
        match self.pull_requests.finish_poll(request_id, result) {
            None | Some(PollOutcome::Unchanged) => false,
            Some(PollOutcome::HeadMoved { first_notice }) => {
                if first_notice && let Some(number) = self.pull_requests.open_number() {
                    self.announce_newer_head(number);
                }
                true
            }
            Some(PollOutcome::Updated) => {
                self.reload_open_pull_request_detail();
                true
            }
        }
    }

    fn announce_newer_head(&mut self, number: u64) {
        self.show_snackbar(
            format!("pull request #{number} has new commits · r to reload"),
            SnackbarVariant::Info,
        );
    }

    fn announce_moved_base(&mut self, number: u64) {
        self.show_snackbar(
            format!("pull request #{number} base moved · r to reload"),
            SnackbarVariant::Info,
        );
    }

    /// Reloads reviews, comments, and checks for the pull request under
    /// review, leaving its diff alone.
    pub(in crate::app) fn reload_open_pull_request_detail(&mut self) {
        let Some(github) = self.pull_requests.github().cloned() else {
            return;
        };
        if let Some((request_id, number)) = self.pull_requests.begin_detail_reload() {
            self.spawn_pull_request_detail_load(github, request_id, number);
        }
    }

    /// Shows the pull request overview in the diff pane.
    ///
    /// The overview is a page to read, not a list of lines, so it never holds
    /// focus while the sidebar is shown: the sidebar keeps driving navigation
    /// and `Ctrl-d`/`Ctrl-u` scroll the page, as they scroll a file's diff.
    pub(in crate::app) fn show_pull_request_overview(&mut self) {
        self.pull_requests.set_page(PullRequestPage::Overview);
        self.clear_diff_text_selection();
        self.keep_overview_focus_in_sidebar();
    }

    /// Returns focus to the sidebar while the overview shows, unless the
    /// sidebar is hidden and the page is all there is to focus.
    pub(in crate::app) fn keep_overview_focus_in_sidebar(&mut self) {
        if self.pull_request_overview_visible() && !self.sidebar_hidden {
            self.active_pane = ActivePane::Sidebar;
        }
    }

    /// Shows the selected file's diff instead of the overview.
    pub(in crate::app) fn show_pull_request_files(&mut self) {
        self.pull_requests.set_page(PullRequestPage::Files);
    }

    /// Whether the diff pane shows the pull request overview.
    pub fn pull_request_overview_visible(&self) -> bool {
        self.pull_requests.page() == Some(PullRequestPage::Overview)
    }
}
