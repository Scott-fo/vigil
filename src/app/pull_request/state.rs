use std::{
    path::Path,
    time::{Duration, Instant},
};

use crate::{
    forge::{ForgeError, GitHub, Mergeability, PullRequest, PullRequestState, PullRequestSummary},
    review::ReviewThreads,
};

use super::{
    drafts::{DraftBook, DraftPersistence, DraftWriter},
    gateway::{ForgeGateway, ForgeMutation},
    list::PullRequestListState,
    modal::PullRequestModal,
    task::{OwnedTask, RequestSlot},
};

/// How the pull request under review was opened, which decides where Esc
/// goes back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum ReviewOrigin {
    /// From the pull request list; Esc returns to it.
    PullRequestList,
    /// From the footer chip, `O`, or anywhere else; Esc keeps its old
    /// meaning.
    Elsewhere,
}

/// Whether the open pull request's drafts have loaded from the database.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::app) enum DraftLoad {
    #[default]
    Loading,
    Loaded,
    Failed(String),
}

/// A current-branch lookup for the same branch and tip is skipped when the
/// last one finished this recently. Branch snapshots reload on every working
/// tree change, and each lookup spends two `gh` calls.
const CURRENT_BRANCH_RELOAD_INTERVAL: Duration = Duration::from_secs(15);

/// Whether vigil can talk to GitHub for this repository.
#[derive(Debug)]
pub(in crate::app) enum ForgeConnection {
    /// Not probed yet; the first branch snapshot or PR request probes.
    Idle,
    /// Never probe. Used where spawning `gh` is unwanted (benchmarks, tests).
    Disabled,
    Connecting,
    Connected(GitHub),
    /// `gh` is missing or logged out, or the repository is not on GitHub.
    /// Retried only when the user asks for a pull request feature.
    Unavailable(ForgeError),
}

/// Who asked for the connection, which decides whether a failure is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum ConnectReason {
    Background,
    UserRequest,
}

/// A user request waiting on the connection or the current-branch lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum PendingAction {
    OpenCurrentBranch,
}

/// Which page of an open pull request the diff pane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestPage {
    /// Title, reviews, checks, description, conversation, and threads that
    /// cannot be shown in the diff.
    Overview,
    /// The selected file's diff.
    Files,
}

/// Identity of a current-branch lookup: the branch and its tip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) struct BranchKey {
    pub(in crate::app) branch: String,
    pub(in crate::app) tip: String,
}

#[derive(Debug, Default)]
struct CurrentBranch {
    /// The branch `summary` belongs to; a different checked-out branch hides
    /// it until its own lookup finishes.
    branch: Option<String>,
    summary: Option<PullRequestSummary>,
    requested: Option<BranchKey>,
    last_load: Option<(BranchKey, Instant)>,
    request: RequestSlot,
}

/// What a finished connection attempt means for the app.
#[derive(Debug)]
pub(in crate::app) enum ConnectOutcome {
    Connected,
    /// `report` is set when the user asked for a pull request feature and
    /// should hear why it is unavailable.
    Failed {
        report: Option<ForgeError>,
    },
}

/// What a poll of the open pull request found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) enum PollOutcome {
    Unchanged,
    /// New commits were pushed. `first_notice` is true the first time this
    /// head is seen, so the notice is announced once.
    HeadMoved {
        first_notice: bool,
    },
    /// Same commits, but reviews, comments, or checks changed, or GitHub
    /// was still computing mergeability; the detail should reload.
    Updated,
}

/// The pull request under review.
#[derive(Debug)]
pub(in crate::app) struct OpenPullRequest {
    summary: PullRequestSummary,
    /// The head commit the review shows.
    reviewed_head: String,
    detail: Option<PullRequest>,
    detail_error: Option<ForgeError>,
    threads: ReviewThreads,
    page: PullRequestPage,
    overview_scroll: usize,
    /// A newer head GitHub reported since the review was fetched.
    newer_head: Option<String>,
    poll: RequestSlot,
    ticker: Option<OwnedTask>,
    origin: ReviewOrigin,
    drafts: DraftBook,
    drafts_load: RequestSlot,
}

impl OpenPullRequest {
    fn new(summary: PullRequestSummary, reviewed_head: String, origin: ReviewOrigin) -> Self {
        Self {
            summary,
            reviewed_head,
            detail: None,
            detail_error: None,
            threads: ReviewThreads::default(),
            page: PullRequestPage::Overview,
            overview_scroll: 0,
            newer_head: None,
            poll: RequestSlot::default(),
            ticker: None,
            origin,
            drafts: DraftBook::default(),
            drafts_load: RequestSlot::default(),
        }
    }

    /// The head commit the review's diff shows, which new drafts pin to.
    pub(in crate::app) fn reviewed_head(&self) -> &str {
        &self.reviewed_head
    }

    pub(in crate::app) fn origin(&self) -> ReviewOrigin {
        self.origin
    }

    pub(in crate::app) fn drafts(&self) -> &DraftBook {
        &self.drafts
    }

    pub(in crate::app) fn drafts_mut(&mut self) -> &mut DraftBook {
        &mut self.drafts
    }

    pub(in crate::app) fn finish_drafts_load(&mut self, id: u64) -> bool {
        self.drafts_load.complete(id)
    }

    #[cfg(test)]
    pub(in crate::app) fn set_newer_head_for_test(&mut self, head: &str) {
        self.newer_head = Some(head.to_string());
    }

    /// Redraws the thread model from GitHub's threads and the drafts.
    pub(in crate::app) fn rebuild_threads(&mut self) {
        let review_threads = self
            .detail
            .as_ref()
            .map_or(&[][..], |detail| detail.review_threads.as_slice());
        let head = self.reviewed_head.as_str();
        self.threads = ReviewThreads::with_drafts(
            review_threads,
            self.drafts
                .all()
                .iter()
                .map(|draft| (draft, draft.head_oid == head)),
        );
    }

    pub(in crate::app) fn summary(&self) -> &PullRequestSummary {
        &self.summary
    }

    pub(in crate::app) fn detail(&self) -> Option<&PullRequest> {
        self.detail.as_ref()
    }

    pub(in crate::app) fn detail_error(&self) -> Option<&ForgeError> {
        self.detail_error.as_ref()
    }

    pub(in crate::app) fn threads(&self) -> &ReviewThreads {
        &self.threads
    }

    pub(in crate::app) fn newer_head(&self) -> Option<&str> {
        self.newer_head.as_deref()
    }

    pub(in crate::app) fn overview_scroll(&self) -> usize {
        self.overview_scroll
    }

    pub(in crate::app) fn set_overview_scroll(&mut self, scroll: usize) {
        self.overview_scroll = scroll;
    }

    pub(in crate::app) fn set_ticker(&mut self, ticker: OwnedTask) {
        self.ticker = Some(ticker);
    }

    fn apply_detail(&mut self, detail: PullRequest) {
        self.summary = detail.summary.clone();
        self.detail = Some(detail);
        self.detail_error = None;
        self.rebuild_threads();
    }
}

/// Pull request state behind the app: the GitHub connection, the current
/// branch's pull request, and the pull request under review.
///
/// Every method here is a pure state transition; `App` methods spawn the
/// work and feed results back. Requests are matched by id, so responses to
/// superseded requests are dropped.
#[derive(Debug)]
pub(in crate::app) struct PullRequests {
    connection: ForgeConnection,
    connect: RequestSlot,
    connect_reason: ConnectReason,
    pending: Option<PendingAction>,
    current_branch: CurrentBranch,
    /// The pull request being fetched before its review opens.
    opening: Option<PullRequestSummary>,
    fetch: RequestSlot,
    detail: RequestSlot,
    detail_number: Option<u64>,
    /// Detail that arrived while its pull request was still being fetched.
    early_detail: Option<Result<PullRequest, ForgeError>>,
    open: Option<OpenPullRequest>,
    list: PullRequestListState,
    /// A pull request looked up by number before it opens.
    lookup: RequestSlot,
    opening_origin: ReviewOrigin,
    /// The review-action modal on screen, if any.
    modal: Option<PullRequestModal>,
    /// The write to GitHub in flight; at most one runs at a time.
    mutation: RequestSlot,
    in_flight: Option<ForgeMutation>,
    gateway: ForgeGateway,
    draft_persistence: DraftPersistence,
    draft_writer: Option<DraftWriter>,
}

impl Default for PullRequests {
    fn default() -> Self {
        Self::with_connection(ForgeConnection::Idle, DraftPersistence::Database)
    }
}

impl PullRequests {
    /// State that never spawns `gh` or touches the review database.
    pub(in crate::app) fn disabled() -> Self {
        Self::with_connection(ForgeConnection::Disabled, DraftPersistence::Off)
    }

    fn with_connection(connection: ForgeConnection, draft_persistence: DraftPersistence) -> Self {
        Self {
            opening_origin: ReviewOrigin::Elsewhere,
            modal: None,
            mutation: RequestSlot::default(),
            in_flight: None,
            gateway: ForgeGateway::default(),
            draft_persistence,
            draft_writer: None,
            connection,
            connect: RequestSlot::default(),
            connect_reason: ConnectReason::Background,
            pending: None,
            current_branch: CurrentBranch::default(),
            opening: None,
            fetch: RequestSlot::default(),
            detail: RequestSlot::default(),
            detail_number: None,
            early_detail: None,
            open: None,
            list: PullRequestListState::default(),
            lookup: RequestSlot::default(),
        }
    }

    pub(in crate::app) fn connection(&self) -> &ForgeConnection {
        &self.connection
    }

    /// Marks GitHub connected without probing it. Tests must not trigger
    /// loads afterwards: they would run `gh` against the real repository.
    #[cfg(test)]
    pub(in crate::app) fn connect_for_test(&mut self, github: GitHub) {
        self.connection = ForgeConnection::Connected(github);
    }

    pub(in crate::app) fn github(&self) -> Option<&GitHub> {
        match &self.connection {
            ForgeConnection::Connected(github) => Some(github),
            _ => None,
        }
    }

    /// Starts a connection attempt unless one is running or done. Returns the
    /// request id to spawn `GitHub::connect` with. A user request upgrades a
    /// running background attempt so its failure is reported.
    pub(in crate::app) fn begin_connect(&mut self, reason: ConnectReason) -> Option<u64> {
        match self.connection {
            ForgeConnection::Idle => {}
            ForgeConnection::Unavailable(_) if reason == ConnectReason::UserRequest => {}
            ForgeConnection::Connecting => {
                if reason == ConnectReason::UserRequest {
                    self.connect_reason = reason;
                }
                return None;
            }
            ForgeConnection::Disabled
            | ForgeConnection::Connected(_)
            | ForgeConnection::Unavailable(_) => return None,
        }
        self.connection = ForgeConnection::Connecting;
        self.connect_reason = reason;
        Some(self.connect.begin())
    }

    pub(in crate::app) fn attach_connect(&mut self, id: u64, handle: tokio::task::JoinHandle<()>) {
        self.connect.attach(id, handle);
    }

    pub(in crate::app) fn finish_connect(
        &mut self,
        id: u64,
        result: Result<GitHub, ForgeError>,
    ) -> Option<ConnectOutcome> {
        if !self.connect.complete(id) {
            return None;
        }
        match result {
            Ok(github) => {
                self.connection = ForgeConnection::Connected(github);
                Some(ConnectOutcome::Connected)
            }
            Err(error) => {
                let wanted = self.connect_reason == ConnectReason::UserRequest
                    || self.pending.take().is_some();
                self.connection = ForgeConnection::Unavailable(error.clone());
                Some(ConnectOutcome::Failed {
                    report: wanted.then_some(error),
                })
            }
        }
    }

    /// Keeps the client pointed at the checkout vigil shows. Worktrees of one
    /// repository share its GitHub identity, so no new probe is needed.
    pub(in crate::app) fn rebind_repo_root(&mut self, repo_root: &Path) {
        if let ForgeConnection::Connected(github) = &self.connection
            && github.repo_root() != repo_root
        {
            let repository = github.repository().clone();
            self.connection = ForgeConnection::Connected(GitHub::new(repo_root, repository));
            self.current_branch = CurrentBranch::default();
        }
    }

    pub(in crate::app) fn set_pending(&mut self, action: PendingAction) {
        self.pending = Some(action);
    }

    pub(in crate::app) fn pending(&self) -> Option<PendingAction> {
        self.pending
    }

    pub(in crate::app) fn take_pending(&mut self) -> Option<PendingAction> {
        self.pending.take()
    }

    // Current branch --------------------------------------------------------

    /// The pull request for `branch`, if the last lookup for it found one.
    pub(in crate::app) fn current_branch_summary(
        &self,
        branch: &str,
    ) -> Option<&PullRequestSummary> {
        (self.current_branch.branch.as_deref() == Some(branch))
            .then_some(self.current_branch.summary.as_ref())
            .flatten()
    }

    /// Starts a lookup of `key`'s pull request unless the same lookup is
    /// running, or finished within [`CURRENT_BRANCH_RELOAD_INTERVAL`] and
    /// `force` is false.
    pub(in crate::app) fn begin_current_branch_load(
        &mut self,
        key: BranchKey,
        force: bool,
        now: Instant,
    ) -> Option<u64> {
        let current = &mut self.current_branch;
        if current.request.in_flight() && current.requested.as_ref() == Some(&key) {
            return None;
        }
        if !force
            && current.last_load.as_ref().is_some_and(|(loaded, at)| {
                *loaded == key && now.duration_since(*at) < CURRENT_BRANCH_RELOAD_INTERVAL
            })
        {
            return None;
        }
        current.requested = Some(key);
        Some(current.request.begin())
    }

    pub(in crate::app) fn attach_current_branch(
        &mut self,
        id: u64,
        handle: tokio::task::JoinHandle<()>,
    ) {
        self.current_branch.request.attach(id, handle);
    }

    /// Records a finished lookup. Returns false for stale responses.
    pub(in crate::app) fn finish_current_branch_load(
        &mut self,
        id: u64,
        result: &Result<Option<PullRequestSummary>, ForgeError>,
        now: Instant,
    ) -> bool {
        let current = &mut self.current_branch;
        if !current.request.complete(id) {
            return false;
        }
        let Some(key) = current.requested.take() else {
            return false;
        };
        if let Ok(summary) = result {
            current.branch = Some(key.branch.clone());
            current.summary = summary.clone();
            current.last_load = Some((key, now));
        }
        true
    }

    /// Forgets the current branch's pull request, as when HEAD is detached.
    pub(in crate::app) fn clear_current_branch(&mut self) {
        self.current_branch.request.cancel();
        self.current_branch.requested = None;
        self.current_branch.branch = None;
        self.current_branch.summary = None;
    }

    // Opening a pull request -------------------------------------------------

    /// Starts fetching `summary`'s commits and loading its detail. Returns the
    /// fetch and detail request ids.
    pub(in crate::app) fn begin_open(
        &mut self,
        summary: PullRequestSummary,
        origin: ReviewOrigin,
    ) -> (u64, u64) {
        self.detail_number = Some(summary.number);
        self.early_detail = None;
        self.opening = Some(summary);
        self.opening_origin = origin;
        (self.fetch.begin(), self.detail.begin())
    }

    /// The number of the pull request whose commits are being fetched.
    pub(in crate::app) fn opening_number(&self) -> Option<u64> {
        self.opening.as_ref().map(|summary| summary.number)
    }

    pub(in crate::app) fn list(&self) -> &PullRequestListState {
        &self.list
    }

    pub(in crate::app) fn list_mut(&mut self) -> &mut PullRequestListState {
        &mut self.list
    }

    /// Starts looking up a pull request by number, superseding any earlier
    /// lookup.
    pub(in crate::app) fn begin_lookup(&mut self) -> u64 {
        self.lookup.begin()
    }

    pub(in crate::app) fn attach_lookup(&mut self, id: u64, handle: tokio::task::JoinHandle<()>) {
        self.lookup.attach(id, handle);
    }

    pub(in crate::app) fn finish_lookup(&mut self, id: u64) -> bool {
        self.lookup.complete(id)
    }

    pub(in crate::app) fn attach_fetch(&mut self, id: u64, handle: tokio::task::JoinHandle<()>) {
        self.fetch.attach(id, handle);
    }

    pub(in crate::app) fn attach_detail(&mut self, id: u64, handle: tokio::task::JoinHandle<()>) {
        self.detail.attach(id, handle);
    }

    /// Accepts a finished fetch. Returns the pull request it was for, which is
    /// no longer "opening"; `None` for stale responses.
    pub(in crate::app) fn finish_fetch(&mut self, id: u64) -> Option<PullRequestSummary> {
        if !self.fetch.complete(id) {
            return None;
        }
        self.opening.take()
    }

    /// Makes `summary` the pull request under review at `reviewed_head`. A
    /// reload of the same pull request keeps its page, scroll, detail, and
    /// drafts; a newly opened one starts on the overview. Returns the number
    /// of a different pull request this replaced, whose review ended.
    pub(in crate::app) fn enter(
        &mut self,
        summary: PullRequestSummary,
        reviewed_head: String,
    ) -> Option<u64> {
        let early_detail = self.early_detail.take();
        let origin = self.opening_origin;
        let mut replaced = None;
        match self.open.as_mut() {
            Some(open) if open.summary.number == summary.number => {
                open.summary = summary;
                open.reviewed_head = reviewed_head;
                open.newer_head = None;
                if origin == ReviewOrigin::PullRequestList {
                    open.origin = origin;
                }
                open.rebuild_threads();
            }
            previous => {
                replaced = previous.map(|open| open.summary.number);
                self.modal = None;
                self.open = Some(OpenPullRequest::new(summary, reviewed_head, origin));
            }
        }
        if let Some(result) = early_detail {
            self.apply_detail_result(result);
        }
        replaced
    }

    /// Records a finished detail load for the open (or opening) pull request.
    /// Returns false for stale responses.
    pub(in crate::app) fn finish_detail(
        &mut self,
        id: u64,
        result: Result<PullRequest, ForgeError>,
    ) -> bool {
        if !self.detail.complete(id) {
            return false;
        }
        let number = self.detail_number;
        if self.opening.as_ref().map(|summary| summary.number) == number {
            self.early_detail = Some(result);
            return true;
        }
        if self.open.as_ref().map(|open| open.summary.number) != number {
            return false;
        }
        self.apply_detail_result(result);
        true
    }

    fn apply_detail_result(&mut self, result: Result<PullRequest, ForgeError>) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        match result {
            Ok(detail) if detail.summary.number == open.summary.number => open.apply_detail(detail),
            Ok(_) => {}
            Err(error) => open.detail_error = Some(error),
        }
    }

    /// Reloads the open pull request's detail without refetching commits.
    pub(in crate::app) fn begin_detail_reload(&mut self) -> Option<(u64, u64)> {
        let number = self.open.as_ref()?.summary.number;
        self.detail_number = Some(number);
        Some((self.detail.begin(), number))
    }

    // The open pull request ---------------------------------------------------

    pub(in crate::app) fn open(&self) -> Option<&OpenPullRequest> {
        self.open.as_ref()
    }

    pub(in crate::app) fn open_mut(&mut self) -> Option<&mut OpenPullRequest> {
        self.open.as_mut()
    }

    pub(in crate::app) fn open_number(&self) -> Option<u64> {
        self.open.as_ref().map(|open| open.summary.number)
    }

    /// Ends the review of the open pull request, cancelling its loads and
    /// polling and closing its modals. A write to GitHub already in flight
    /// still finishes and reports, so a submitted review's drafts are
    /// deleted even after the review screen moved on. Returns the number of
    /// the pull request whose review ended.
    pub(in crate::app) fn close(&mut self) -> Option<u64> {
        let closed = self.open.take()?;
        self.detail.cancel();
        self.detail_number = None;
        self.early_detail = None;
        self.modal = None;
        Some(closed.summary.number)
    }

    // Review actions ----------------------------------------------------------

    pub(in crate::app) fn modal(&self) -> Option<&PullRequestModal> {
        self.modal.as_ref()
    }

    pub(in crate::app) fn modal_mut(&mut self) -> Option<&mut PullRequestModal> {
        self.modal.as_mut()
    }

    pub(in crate::app) fn set_modal(&mut self, modal: Option<PullRequestModal>) {
        self.modal = modal;
    }

    pub(in crate::app) fn mutation_in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    pub(in crate::app) fn in_flight_mutation(&self) -> Option<&ForgeMutation> {
        self.in_flight.as_ref()
    }

    pub(in crate::app) fn begin_mutation(&mut self, mutation: ForgeMutation) -> u64 {
        self.in_flight = Some(mutation);
        self.mutation.begin()
    }

    /// Keeps the task of a running mutation. Unlike reads, it is not aborted
    /// when superseded: a write is never cancelled midway.
    pub(in crate::app) fn attach_mutation(&mut self, id: u64, handle: tokio::task::JoinHandle<()>) {
        self.mutation.attach_detached(id, handle);
    }

    /// The mutation `id` finished; returns it unless it is stale.
    pub(in crate::app) fn finish_mutation(&mut self, id: u64) -> Option<ForgeMutation> {
        if !self.mutation.complete(id) {
            return None;
        }
        self.in_flight.take()
    }

    #[cfg(test)]
    pub(in crate::app) fn mutation_request_id(&self) -> u64 {
        self.mutation.current_id()
    }

    #[cfg(test)]
    pub(in crate::app) fn gateway(&self) -> &ForgeGateway {
        &self.gateway
    }

    pub(in crate::app) fn gateway_mut(&mut self) -> &mut ForgeGateway {
        &mut self.gateway
    }

    pub(in crate::app) fn draft_persistence(&self) -> DraftPersistence {
        self.draft_persistence
    }

    /// The draft writer, started on first use.
    pub(in crate::app) fn draft_writer(
        &mut self,
        spawn: impl FnOnce() -> DraftWriter,
    ) -> DraftWriter {
        self.draft_writer.get_or_insert_with(spawn).clone()
    }

    /// Starts loading the open pull request's drafts.
    pub(in crate::app) fn begin_drafts_load(&mut self) -> Option<u64> {
        Some(self.open.as_mut()?.drafts_load.begin())
    }

    pub(in crate::app) fn attach_drafts_load(
        &mut self,
        id: u64,
        handle: tokio::task::JoinHandle<()>,
    ) {
        if let Some(open) = self.open.as_mut() {
            open.drafts_load.attach(id, handle);
        }
    }

    pub(in crate::app) fn page(&self) -> Option<PullRequestPage> {
        self.open.as_ref().map(|open| open.page)
    }

    pub(in crate::app) fn set_page(&mut self, page: PullRequestPage) {
        if let Some(open) = self.open.as_mut() {
            open.page = page;
        }
    }

    /// Starts a poll of the open pull request unless one is running.
    pub(in crate::app) fn begin_poll(&mut self) -> Option<(u64, u64)> {
        let open = self.open.as_mut()?;
        if open.poll.in_flight() {
            return None;
        }
        Some((open.poll.begin(), open.summary.number))
    }

    pub(in crate::app) fn attach_poll(&mut self, id: u64, handle: tokio::task::JoinHandle<()>) {
        if let Some(open) = self.open.as_mut() {
            open.poll.attach(id, handle);
        }
    }

    /// Compares a polled summary with what the review shows. `None` for
    /// stale responses or failed polls, which are retried on the next tick.
    pub(in crate::app) fn finish_poll(
        &mut self,
        id: u64,
        result: Result<PullRequestSummary, ForgeError>,
    ) -> Option<PollOutcome> {
        let open = self.open.as_mut()?;
        if !open.poll.complete(id) {
            return None;
        }
        let summary = result.ok()?;
        if summary.number != open.summary.number {
            return None;
        }
        if summary.head_oid != open.reviewed_head {
            let first_notice = open.newer_head.as_deref() != Some(summary.head_oid.as_str());
            open.newer_head = Some(summary.head_oid.clone());
            open.summary = summary;
            return Some(PollOutcome::HeadMoved { first_notice });
        }
        open.newer_head = None;
        // GitHub computes mergeability in the background without touching
        // `updated_at`, so an unknown answer is worth asking again.
        let mergeability_pending = open.detail.as_ref().is_some_and(|detail| {
            detail.summary.state == PullRequestState::Open
                && detail.mergeable == Mergeability::Unknown
        });
        if summary.updated_at != open.summary.updated_at || mergeability_pending {
            open.summary = summary;
            return Some(PollOutcome::Updated);
        }
        Some(PollOutcome::Unchanged)
    }
}
