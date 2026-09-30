use std::{
    path::Path,
    time::{Duration, Instant},
};

use crate::{
    forge::{
        ForgeError, GitHub, Mergeability, PullRequest, PullRequestState, PullRequestSummary,
        Snapshot, Timestamp,
    },
    review::ReviewThreads,
};

use super::{
    drafts::{DraftBook, DraftPersistence, DraftWriter},
    gateway::{ForgeGateway, ForgeMutation},
    list::PullRequestListState,
    modal::PullRequestModal,
    saved::{Freshness, SavedSnapshots},
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
    /// Found by connecting or, since connecting may not run `gh`, by the
    /// first request. Retried only when the user asks for a pull request
    /// feature.
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

/// What a finished detail load did to the review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum DetailOutcome {
    Applied,
    /// The detail reports a head the review does not show, seen for the
    /// first time; the reviewer should hear about it once.
    HeadMoved,
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

/// The detail a review shows, and whether it came from GitHub in this
/// session or from the cache.
#[derive(Debug)]
struct ShownDetail {
    value: PullRequest,
    freshness: Freshness,
}

/// The pull request under review.
#[derive(Debug)]
pub(in crate::app) struct OpenPullRequest {
    summary: PullRequestSummary,
    /// The head commit the review shows.
    reviewed_head: String,
    detail: Option<ShownDetail>,
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
            .map_or(&[][..], |shown| shown.value.review_threads.as_slice());
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

    /// The detail shown, live or saved. Fine for display; decisions that
    /// write to GitHub need [`Self::live_detail`].
    pub(in crate::app) fn detail(&self) -> Option<&PullRequest> {
        self.detail.as_ref().map(|shown| &shown.value)
    }

    /// The detail, once GitHub has confirmed it in this session.
    pub(in crate::app) fn live_detail(&self) -> Option<&PullRequest> {
        self.detail
            .as_ref()
            .filter(|shown| shown.freshness.is_live())
            .map(|shown| &shown.value)
    }

    /// When GitHub reported the detail shown, if it is a saved snapshot no
    /// load has confirmed yet.
    pub(in crate::app) fn detail_saved_at(&self) -> Option<&Timestamp> {
        self.detail
            .as_ref()
            .and_then(|shown| shown.freshness.saved_at())
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

    /// Shows a live detail. A head other than the reviewed one means the
    /// review is behind, as when it was opened from a saved list row;
    /// returns true the first time that head is seen. `requested_at` is
    /// when the load started.
    fn apply_detail(&mut self, detail: PullRequest, requested_at: Instant) -> bool {
        let head = &detail.summary.head_oid;
        let first_notice =
            *head != self.reviewed_head && self.newer_head.as_deref() != Some(head.as_str());
        if first_notice {
            self.newer_head = Some(head.clone());
        }
        self.summary = detail.summary.clone();
        self.detail = Some(ShownDetail {
            value: detail,
            freshness: Freshness::Live { requested_at },
        });
        self.detail_error = None;
        self.rebuild_threads();
        first_notice
    }

    /// Shows a saved detail while nothing else is shown. Only a snapshot of
    /// the reviewed head is used, so its threads and checks match the diff.
    /// The summary is kept: the one the review opened with is newer.
    fn apply_saved_detail(&mut self, snapshot: Snapshot<PullRequest>) -> bool {
        if self.detail.is_some()
            || snapshot.value.summary.number != self.summary.number
            || snapshot.value.summary.head_oid != self.reviewed_head
        {
            return false;
        }
        self.detail = Some(ShownDetail {
            value: snapshot.value,
            freshness: Freshness::Saved {
                fetched_at: snapshot.fetched_at,
            },
        });
        self.rebuild_threads();
        true
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
    /// Asks GitHub for the canonical name of a locally resolved repository.
    confirm: RequestSlot,
    pending: Option<PendingAction>,
    current_branch: CurrentBranch,
    /// The pull request being fetched before its review opens.
    opening: Option<PullRequestSummary>,
    fetch: RequestSlot,
    detail: RequestSlot,
    detail_number: Option<u64>,
    /// When the latest detail load started.
    detail_requested_at: Instant,
    /// Detail that arrived while its pull request was still being fetched.
    early_detail: Option<Result<PullRequest, ForgeError>>,
    /// A saved detail read while its pull request was still being fetched.
    early_saved_detail: Option<Snapshot<PullRequest>>,
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
    saved: SavedSnapshots,
}

impl Default for PullRequests {
    fn default() -> Self {
        Self::with_connection(
            ForgeConnection::Idle,
            DraftPersistence::Database,
            SavedSnapshots::user_cache(),
        )
    }
}

impl PullRequests {
    /// State that never spawns `gh` or touches the review database or the
    /// forge cache.
    pub(in crate::app) fn disabled() -> Self {
        Self::with_connection(
            ForgeConnection::Disabled,
            DraftPersistence::Off,
            SavedSnapshots::off(),
        )
    }

    fn with_connection(
        connection: ForgeConnection,
        draft_persistence: DraftPersistence,
        saved: SavedSnapshots,
    ) -> Self {
        Self {
            saved,
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
            confirm: RequestSlot::default(),
            pending: None,
            current_branch: CurrentBranch::default(),
            opening: None,
            fetch: RequestSlot::default(),
            detail: RequestSlot::default(),
            detail_number: None,
            detail_requested_at: Instant::now(),
            early_detail: None,
            early_saved_detail: None,
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

    /// Records that a request found GitHub unable to serve this checkout
    /// (`gh` missing or logged out, repository not visible) after a
    /// connection that never ran `gh`. Background work stops as if
    /// connecting had failed, and the next user request reconnects and
    /// retries.
    pub(in crate::app) fn mark_unavailable(&mut self, error: ForgeError) {
        if matches!(self.connection, ForgeConnection::Connected(_)) {
            self.connection = ForgeConnection::Unavailable(error);
        }
    }

    /// Keeps the client pointed at the checkout vigil shows. Worktrees of one
    /// repository share its GitHub identity, so no new probe is needed.
    pub(in crate::app) fn rebind_repo_root(&mut self, repo_root: &Path) {
        if let ForgeConnection::Connected(github) = &self.connection
            && github.repo_root() != repo_root
        {
            self.connection = ForgeConnection::Connected(github.with_repo_root(repo_root));
            self.current_branch = CurrentBranch::default();
        }
    }

    /// Starts confirming a locally resolved repository's canonical name.
    /// Returns the request id and the client to ask with, or `None` when
    /// nothing needs confirming.
    pub(in crate::app) fn begin_confirm(&mut self) -> Option<(u64, GitHub)> {
        let github = self
            .github()
            .filter(|github| !github.is_repository_confirmed())?
            .clone();
        Some((self.confirm.begin(), github))
    }

    pub(in crate::app) fn attach_confirm(&mut self, id: u64, handle: tokio::task::JoinHandle<()>) {
        self.confirm.attach(id, handle);
    }

    /// Switches to the confirmed client. Returns whether the repository was
    /// renamed or transferred (not merely spelled in another case), so
    /// results loaded under the old name are stale; false for stale or
    /// failed confirmations, which keep the local name.
    pub(in crate::app) fn finish_confirm(
        &mut self,
        id: u64,
        result: Result<GitHub, ForgeError>,
    ) -> bool {
        if !self.confirm.complete(id) {
            return false;
        }
        let (Ok(confirmed), ForgeConnection::Connected(github)) = (result, &self.connection) else {
            return false;
        };
        let (old, new) = (github.repository(), confirmed.repository());
        let renamed = !(old.host.eq_ignore_ascii_case(&new.host)
            && old.owner.eq_ignore_ascii_case(&new.owner)
            && old.name.eq_ignore_ascii_case(&new.name));
        self.connection =
            ForgeConnection::Connected(confirmed.with_repo_root(github.repo_root().to_path_buf()));
        renamed
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
        }
        // A failed lookup waits out the interval too, so a failing `gh` is
        // not rerun on every working tree change.
        current.last_load = Some((key, now));
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

    /// Starts getting `summary`'s commits and loading its detail. Returns the
    /// fetch and detail request ids.
    pub(in crate::app) fn begin_open(
        &mut self,
        summary: PullRequestSummary,
        origin: ReviewOrigin,
    ) -> (u64, u64) {
        self.detail_number = Some(summary.number);
        self.early_detail = None;
        self.early_saved_detail = None;
        self.opening = Some(summary);
        self.opening_origin = origin;
        self.detail_requested_at = Instant::now();
        (self.fetch.begin(), self.detail.begin())
    }

    /// The number of the pull request whose commits are being fetched.
    pub(in crate::app) fn opening_number(&self) -> Option<u64> {
        self.opening.as_ref().map(|summary| summary.number)
    }

    /// The number of the pull request being opened by request `id`, unless
    /// that request was superseded or finished.
    pub(in crate::app) fn fetching_number(&self, id: u64) -> Option<u64> {
        self.fetch
            .is_current(id)
            .then(|| self.opening_number())
            .flatten()
    }

    /// Drops the detail load an open started, so a test never reaches
    /// GitHub.
    #[cfg(test)]
    pub(in crate::app) fn cancel_detail_for_test(&mut self) {
        self.detail.cancel();
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
        let early_saved_detail = self.early_saved_detail.take();
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
        if let (Some(snapshot), Some(open)) = (early_saved_detail, self.open.as_mut()) {
            open.apply_saved_detail(snapshot);
        }
        if let Some(result) = early_detail {
            self.apply_detail_result(result);
        }
        replaced
    }

    /// Whether a saved detail of `number` could still be shown: its review
    /// is not open yet, or has no detail.
    pub(in crate::app) fn wants_saved_detail(&self, number: u64) -> bool {
        match self.open.as_ref() {
            Some(open) if open.summary.number == number => open.detail.is_none(),
            _ => true,
        }
    }

    /// Shows a saved detail of `number` if its review, open or opening, has
    /// no detail yet. Returns whether anything visible changed; a snapshot
    /// that arrives after the live detail is dropped.
    pub(in crate::app) fn show_saved_detail(
        &mut self,
        number: u64,
        snapshot: Snapshot<PullRequest>,
    ) -> bool {
        if let Some(open) = self
            .open
            .as_mut()
            .filter(|open| open.summary.number == number)
        {
            return open.apply_saved_detail(snapshot);
        }
        if self.opening_number() == Some(number) && !matches!(self.early_detail, Some(Ok(_))) {
            self.early_saved_detail = Some(snapshot);
        }
        false
    }

    /// When the latest detail load started: the time its answer is as of.
    pub(in crate::app) fn detail_requested_at(&self) -> Instant {
        self.detail_requested_at
    }

    /// Whether a detail load for the open pull request is running.
    pub(in crate::app) fn detail_refreshing(&self) -> bool {
        self.detail.in_flight()
            && self.detail_number.is_some()
            && self.detail_number == self.open_number()
    }

    /// Records a finished detail load for the open (or opening) pull request.
    /// `None` for stale responses.
    pub(in crate::app) fn finish_detail(
        &mut self,
        id: u64,
        result: Result<PullRequest, ForgeError>,
    ) -> Option<DetailOutcome> {
        if !self.detail.complete(id) {
            return None;
        }
        let number = self.detail_number;
        if self.opening.as_ref().map(|summary| summary.number) == number {
            self.early_detail = Some(result);
            return Some(DetailOutcome::Applied);
        }
        if self.open.as_ref().map(|open| open.summary.number) != number {
            return None;
        }
        Some(self.apply_detail_result(result))
    }

    fn apply_detail_result(&mut self, result: Result<PullRequest, ForgeError>) -> DetailOutcome {
        let requested_at = self.detail_requested_at;
        let Some(open) = self.open.as_mut() else {
            return DetailOutcome::Applied;
        };
        match result {
            Ok(detail) if detail.summary.number == open.summary.number => {
                if open.apply_detail(detail, requested_at) {
                    DetailOutcome::HeadMoved
                } else {
                    DetailOutcome::Applied
                }
            }
            Ok(_) => DetailOutcome::Applied,
            Err(error) => {
                open.detail_error = Some(error);
                DetailOutcome::Applied
            }
        }
    }

    /// Reloads the open pull request's detail without refetching commits.
    pub(in crate::app) fn begin_detail_reload(&mut self) -> Option<(u64, u64)> {
        let number = self.open.as_ref()?.summary.number;
        self.detail_number = Some(number);
        self.detail_requested_at = Instant::now();
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
        self.early_saved_detail = None;
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

    pub(in crate::app) fn saved(&self) -> &SavedSnapshots {
        &self.saved
    }

    pub(in crate::app) fn saved_mut(&mut self) -> &mut SavedSnapshots {
        &mut self.saved
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
        let mergeability_pending = open.detail().is_some_and(|detail| {
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
