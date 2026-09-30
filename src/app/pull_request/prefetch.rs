//! Prefetching: making the pull requests a reviewer is likely to open local
//! before they ask.
//!
//! Opening a pull request is instant when its commits are in the object
//! store and its detail is in the forge cache; otherwise it waits on a
//! fetch and a GitHub load. So once a list page arrives live from GitHub
//! (never a saved page), the app prefetches its first [`PREFETCH_ROWS`]
//! rows in the background, and after each current-branch lookup, that pull
//! request's detail:
//!
//! - **Commits**, through `git::prefetch_pull_requests`: one `git cat-file`
//!   finds the rows whose head or base is missing, and one `git fetch`
//!   brings them all in. Rows already checked this session are skipped
//!   until their head changes, so a list refresh in which no head moved
//!   runs no process at all, and one where only local rows changed runs
//!   the `cat-file` and nothing that writes. A base commit alone moving,
//!   as it does whenever the base branch advances, fetches nothing; an
//!   open that then lacks the new base fetches it.
//! - **Details**, through `GitHub::load_pull_requests`: the forge cache
//!   first names the rows whose saved detail is missing or was saved for
//!   another `updated_at`, head, or check rollup
//!   (`ForgeCache::stale_pull_requests`, a lookup on the blocking pool),
//!   and only those are loaded, in one
//!   request, and saved through [`App::save_pull_request`] dated by when
//!   that request started. Without a cache there is nowhere to put them,
//!   so nothing is loaded. Saved details are keyed by the repository's
//!   name, so a confirmed rename forgets which were saved and the next
//!   live page prefetches them again under the new name.
//!
//! Prefetching never changes the screen. It only fills refs and the cache;
//! the open paths find them there. Failures are silent, since prefetching
//! is only an optimization: after one, that kind of prefetch pauses for
//! [`FAILURE_BACKOFF`], or [`RATE_LIMIT_BACKOFF`] when GitHub's rate limit
//! was hit, instead of retrying on the next refresh. Nothing is prefetched
//! while GitHub is not connected, which includes `gh` being missing or
//! logged out. A prefetch whose failure leaves the forge unavailable (see
//! `ForgeError::leaves_forge_unavailable`) turns the connection off as any
//! request would, still without a message; the next pull request feature
//! the user asks for explains why.
//!
//! # ssh and the terminal
//!
//! Nobody asked for a prefetch, so its `git fetch` must never prompt. It
//! runs detached from vigil's terminal, so ssh cannot ask for a passphrase
//! or a host key confirmation over the screen, and with ssh in batch mode
//! unless the user set up their own ssh command (see
//! `git::prefetch_pull_requests`). An ssh agent that confirms every use
//! (`ssh-add -c`, 1Password, a security key waiting for a touch) is outside
//! git's reach and may still ask, once: a fetch that fails because access
//! was refused stops commit prefetching for the rest of the session
//! instead of asking again every few minutes. Detail prefetches go through
//! `gh` over HTTPS and are unaffected.
//!
//! # Lifecycle and concurrency
//!
//! At most one commit prefetch and one detail prefetch run at a time, and
//! dropping the state aborts both. A newer list page supersedes a running
//! detail prefetch, whose `gh` process is killed. A running commit
//! prefetch is not superseded; rows asked for meanwhile run right after
//! it, newest request only. Its fetch is terminated after
//! `git::PREFETCH_FETCH_TIMEOUT`, as when a laptop slept through it and
//! left a dead connection; that counts as a failure and pauses commit
//! prefetching like any other.
//!
//! A reviewer may open a pull request that a running commit prefetch is
//! fetching. Two fetches writing the same ref at once could make the
//! reviewer's fail on the ref lock, so the open waits for the prefetch to
//! land (usually sooner than a fetch of its own would) and then resolves
//! locally, fetching only if that misses. It waits at most
//! [`OPEN_WAITS_FOR_PREFETCH`], so a stuck prefetch never holds an open
//! up; past that the open goes ahead on its own. The other way round, a prefetch
//! never includes a pull request being opened: one whose outdated list row
//! is being looked up, whose commits are being fetched, or that is under
//! review. Its open loads it live anyway.

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use tokio::{sync::watch, task};

use crate::{
    event::Event,
    forge::{
        CheckRollup, ForgeError, GitHub, PullRequest, PullRequestList, PullRequestSummary,
        Snapshot, Timestamp,
    },
    git::{self, PrefetchReport, PullRequestFetch, PullRequestFetchError},
};

use super::{super::App, PullRequestEvent, gateway::ForgeGateway, task::RequestSlot};

/// How many rows at the top of a live list page are prefetched. GitHub
/// sorts them by last update, so these are the likeliest to be opened.
pub(in crate::app) const PREFETCH_ROWS: usize = 10;

/// How long a kind of prefetch pauses after it fails: offline, the remote
/// unreachable, or GitHub erroring.
pub(in crate::app) const FAILURE_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// How long detail prefetches pause after GitHub's rate limit, which
/// resets hourly; interactive loads keep whatever budget is left.
pub(in crate::app) const RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(15 * 60);

/// Results of background prefetches.
#[derive(Debug)]
pub enum PrefetchEvent {
    /// A commit prefetch finished.
    CommitsFetched {
        request_id: u64,
        result: Result<PrefetchReport, PullRequestFetchError>,
    },
    /// The forge cache named the rows whose saved detail is stale; `None`
    /// when it could not answer.
    StaleDetailsFound {
        request_id: u64,
        stale: Option<Vec<u64>>,
    },
    /// A detail prefetch's GitHub load finished. `requested_at` is when the
    /// request started, which the saved details are dated by.
    DetailsLoaded {
        request_id: u64,
        requested_at: Timestamp,
        result: Result<Vec<PullRequest>, ForgeError>,
    },
}

/// What prompted a detail prefetch, which decides whether it may replace
/// one that is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailTrigger {
    /// A live list page; it supersedes a running prefetch.
    ListPage,
    /// A current-branch lookup, which runs often; it waits its turn and is
    /// dropped if a prefetch is running, to be asked again next lookup.
    CurrentBranch,
}

/// How long an open waits for a running commit prefetch of its pull
/// request before resolving or fetching on its own. A healthy prefetch
/// lands within this; a stuck one must not hold the reviewer up.
pub(in crate::app) const OPEN_WAITS_FOR_PREFETCH: Duration = Duration::from_secs(5);

/// Waits until `landing` closes, as when the commit prefetch it came from
/// is done, or `limit` passes. Returns whether the prefetch landed.
pub(in crate::app) async fn wait_for_landing(
    mut landing: watch::Receiver<()>,
    limit: Duration,
) -> bool {
    // `changed` errs once the sender drops, which is how a prefetch lands.
    tokio::time::timeout(limit, async { while landing.changed().await.is_ok() {} })
        .await
        .is_ok()
}

/// Whether a kind of prefetch may start.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Pause {
    #[default]
    Off,
    /// After a failure that may pass: offline, a timeout, GitHub erroring.
    Until(Instant),
    /// After a failure that will repeat until the user acts, such as ssh
    /// refusing the key: retrying would only ask their agent again.
    ForSession,
}

impl Pause {
    fn active(&self, now: Instant) -> bool {
        match self {
            Self::Off => false,
            Self::Until(until) => now < *until,
            Self::ForSession => true,
        }
    }

    fn start(&mut self, now: Instant, pause: Duration) {
        if *self != Self::ForSession {
            *self = Self::Until(now + pause);
        }
    }
}

/// A row's head as GitHub reported it: what a commit prefetch settles.
///
/// The base is left out on purpose. GitHub's base commit moves every time
/// the base branch does, so keying on it would refetch every row after
/// each merge on a busy branch. A row is prefetched again when its head
/// moves, which fetches its current base too if that is missing; an open
/// whose base moved since just fetches it then.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CommitKey {
    number: u64,
    head_oid: String,
}

impl CommitKey {
    fn of(request: &PullRequestFetch) -> Self {
        Self {
            number: request.number,
            head_oid: request.head_oid.clone(),
        }
    }
}

/// A row's detail as GitHub reported it: what a detail prefetch settles.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DetailKey {
    number: u64,
    updated_at: Timestamp,
    head_oid: String,
    /// CI finishing moves this but not `updated_at`.
    checks: Option<CheckRollup>,
}

impl DetailKey {
    fn of(summary: &PullRequestSummary) -> Self {
        Self {
            number: summary.number,
            updated_at: summary.updated_at.clone(),
            head_oid: summary.head_oid.clone(),
            checks: summary.checks,
        }
    }
}

/// The commit prefetch in flight.
#[derive(Debug)]
struct RunningFetch {
    requests: Vec<PullRequestFetch>,
    /// Closes once the fetch's `git` process is done.
    landed: watch::Receiver<()>,
}

#[derive(Debug, Default)]
struct CommitPrefetch {
    request: RequestSlot,
    running: Option<RunningFetch>,
    /// Rows asked for while a fetch ran.
    queued: Option<Vec<PullRequestFetch>>,
    /// Rows found local or fetched this session.
    settled: HashSet<CommitKey>,
    pause: Pause,
}

#[derive(Debug, Default)]
struct DetailPrefetch {
    request: RequestSlot,
    /// The rows the running prefetch covers, narrowed to the stale ones
    /// once the cache answers.
    rows: Vec<PullRequestSummary>,
    /// Rows whose detail was found current or loaded this session.
    settled: HashSet<DetailKey>,
    pause: Pause,
}

/// Prefetch state: what runs, what is done, and what is paused. Every
/// method is a pure state transition; `App` spawns the work.
#[derive(Debug, Default)]
pub(in crate::app) struct Prefetch {
    commits: CommitPrefetch,
    details: DetailPrefetch,
}

/// A commit prefetch ready to spawn.
pub(in crate::app) struct CommitPrefetchStart {
    pub(in crate::app) request_id: u64,
    pub(in crate::app) requests: Vec<PullRequestFetch>,
    /// Dropped when the fetch is done, which releases opens waiting on it.
    pub(in crate::app) landed: watch::Sender<()>,
}

impl Prefetch {
    /// Starts prefetching the commits of `requests` not settled yet.
    /// `None` when there is nothing to do, prefetching is paused, or a
    /// fetch is running (the rows then run after it).
    fn begin_commits(
        &mut self,
        requests: Vec<PullRequestFetch>,
        now: Instant,
    ) -> Option<CommitPrefetchStart> {
        let commits = &mut self.commits;
        if commits.pause.active(now) {
            return None;
        }
        let requests = requests
            .into_iter()
            .filter(|request| !commits.settled.contains(&CommitKey::of(request)))
            .collect::<Vec<_>>();
        if requests.is_empty() {
            return None;
        }
        if commits.running.is_some() {
            commits.queued = Some(requests);
            return None;
        }
        let (landed, receiver) = watch::channel(());
        commits.running = Some(RunningFetch {
            requests: requests.clone(),
            landed: receiver,
        });
        Some(CommitPrefetchStart {
            request_id: commits.request.begin(),
            requests,
            landed,
        })
    }

    fn attach_commits(&mut self, id: u64, handle: task::JoinHandle<()>) {
        self.commits.request.attach(id, handle);
    }

    /// Records a finished commit prefetch: every row it covered is settled,
    /// or on failure prefetching pauses. Returns rows queued meanwhile.
    fn finish_commits(
        &mut self,
        id: u64,
        result: &Result<PrefetchReport, PullRequestFetchError>,
        now: Instant,
    ) -> Option<Vec<PullRequestFetch>> {
        let commits = &mut self.commits;
        if !commits.request.complete(id) {
            return None;
        }
        let running = commits.running.take()?;
        match result {
            Ok(report) => commits.settled.extend(
                running
                    .requests
                    .iter()
                    .filter(|request| report.outcome(request.number).is_some())
                    .map(CommitKey::of),
            ),
            // Refused credentials or an unverified host key repeat until the
            // user acts; retrying would only ask their ssh agent again.
            Err(PullRequestFetchError::Access { .. }) => {
                commits.pause = Pause::ForSession;
                commits.queued = None;
            }
            Err(_) => commits.pause.start(now, FAILURE_BACKOFF),
        }
        commits.queued.take()
    }

    /// A signal that closes once the running commit prefetch lands, if it
    /// is fetching pull request `number`.
    pub(in crate::app) fn commits_landing(&self, number: u64) -> Option<watch::Receiver<()>> {
        let running = self.commits.running.as_ref()?;
        running
            .requests
            .iter()
            .any(|request| request.number == number)
            .then(|| running.landed.clone())
    }

    /// Starts a detail prefetch of `rows` not settled yet. `None` when
    /// there is nothing to do, prefetching is paused, or `trigger` must
    /// not replace the one running.
    fn begin_details(
        &mut self,
        rows: Vec<PullRequestSummary>,
        trigger: DetailTrigger,
        now: Instant,
    ) -> Option<(u64, Vec<PullRequestSummary>)> {
        let details = &mut self.details;
        if details.pause.active(now)
            || (trigger == DetailTrigger::CurrentBranch && details.request.in_flight())
        {
            return None;
        }
        let rows = rows
            .into_iter()
            .filter(|row| !details.settled.contains(&DetailKey::of(row)))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return None;
        }
        details.rows = rows.clone();
        Some((details.request.begin(), rows))
    }

    fn attach_details(&mut self, id: u64, handle: task::JoinHandle<()>) {
        self.details.request.attach(id, handle);
    }

    /// Narrows the running detail prefetch to the rows the cache found
    /// stale, settling the rest. Returns the numbers to load, or `None`
    /// (ending the prefetch) when nothing is stale or the cache could not
    /// answer.
    fn narrow_details(&mut self, id: u64, stale: Option<Vec<u64>>) -> Option<Vec<u64>> {
        let details = &mut self.details;
        if !details.request.is_current(id) {
            return None;
        }
        let Some(stale) = stale else {
            details.request.complete(id);
            return None;
        };
        let (stale_rows, current_rows): (Vec<_>, Vec<_>) = std::mem::take(&mut details.rows)
            .into_iter()
            .partition(|row| stale.contains(&row.number));
        details
            .settled
            .extend(current_rows.iter().map(DetailKey::of));
        if stale_rows.is_empty() {
            details.request.complete(id);
            return None;
        }
        let numbers = stale_rows.iter().map(|row| row.number).collect();
        details.rows = stale_rows;
        Some(numbers)
    }

    /// Records a finished detail load: its rows are settled, or on failure
    /// prefetching pauses. Returns the details to save; `None` for stale
    /// responses and failures.
    fn finish_details(
        &mut self,
        id: u64,
        result: Result<Vec<PullRequest>, ForgeError>,
        now: Instant,
    ) -> Option<Vec<PullRequest>> {
        let details = &mut self.details;
        if !details.request.complete(id) {
            return None;
        }
        let rows = std::mem::take(&mut details.rows);
        match result {
            Ok(loaded) => {
                details.settled.extend(rows.iter().map(DetailKey::of));
                Some(loaded)
            }
            Err(error) => {
                let pause = match error {
                    ForgeError::RateLimited { .. } => RATE_LIMIT_BACKOFF,
                    _ => FAILURE_BACKOFF,
                };
                details.pause.start(now, pause);
                None
            }
        }
    }

    /// Forgets which details are saved, and drops a running detail
    /// prefetch, once the repository turns out to be renamed: saved details
    /// are keyed by its name, so those saved under the old one are not found
    /// under the new one. Commit state is kept, since commits and pull
    /// request numbers survive a rename, and so is any backoff.
    pub(in crate::app) fn forget_saved_details(&mut self) {
        let details = &mut self.details;
        details.request.cancel();
        details.rows.clear();
        details.settled.clear();
    }

    #[cfg(test)]
    pub(in crate::app) fn commit_request_id(&self) -> u64 {
        self.commits.request.current_id()
    }

    #[cfg(test)]
    pub(in crate::app) fn detail_request_id(&self) -> u64 {
        self.details.request.current_id()
    }

    #[cfg(test)]
    pub(in crate::app) fn detail_in_flight(&self) -> bool {
        self.details.request.in_flight()
    }
}

impl App {
    /// Prefetches the top rows of a list page just loaded live from GitHub.
    /// Never call it for a saved page: its heads may be long gone.
    pub(super) fn prefetch_listed_pull_requests(&mut self, list: &PullRequestList) {
        let rows = list
            .pull_requests
            .iter()
            .take(PREFETCH_ROWS)
            .cloned()
            .collect::<Vec<_>>();
        self.prefetch_commits(&rows);
        self.prefetch_details(rows, DetailTrigger::ListPage);
    }

    /// Prefetches the current branch's pull request detail after a lookup
    /// found it, unless it is already saved for what the lookup reports.
    pub(super) fn prefetch_current_branch_detail(&mut self, summary: &PullRequestSummary) {
        self.prefetch_details(vec![summary.clone()], DetailTrigger::CurrentBranch);
    }

    /// The client to prefetch with: connected to GitHub, and in tests only
    /// when prefetches are recorded rather than run.
    fn prefetch_client(&self) -> Option<GitHub> {
        match self.pull_requests.gateway() {
            // A test must never fetch or reach GitHub, even by accident;
            // tests that do not record get no prefetching at all.
            #[cfg(test)]
            ForgeGateway::Live => return None,
            #[cfg(not(test))]
            ForgeGateway::Live => {}
            #[cfg(test)]
            ForgeGateway::Recording(_) => {}
        }
        self.pull_requests.github().cloned()
    }

    /// Whether an open is looking up, fetching, or showing pull request
    /// `number`, which prefetches then leave alone. A list row's lookup
    /// opens it next, so it counts as being opened.
    fn opening_or_open(&self, number: u64) -> bool {
        [
            self.pull_requests.looking_up_number(),
            self.pull_requests.opening_number(),
            self.pull_requests.open_number(),
        ]
        .contains(&Some(number))
    }

    fn prefetch_commits(&mut self, rows: &[PullRequestSummary]) {
        let Some(github) = self.prefetch_client() else {
            return;
        };
        let requests = rows
            .iter()
            .map(|row| PullRequestFetch {
                repository: github.repository().clone(),
                number: row.number,
                head_oid: row.head_oid.clone(),
                base_oid: row.base_oid.clone(),
                base_ref_name: row.base_ref_name.clone(),
            })
            .collect();
        self.start_commit_prefetch(requests);
    }

    /// Starts prefetching `requests`, leaving out the pull request an open
    /// is fetching: two fetches of one ref could fail each other.
    fn start_commit_prefetch(&mut self, mut requests: Vec<PullRequestFetch>) {
        requests.retain(|request| !self.opening_or_open(request.number));
        let Some(start) = self
            .pull_requests
            .prefetch_mut()
            .begin_commits(requests, Instant::now())
        else {
            return;
        };
        let CommitPrefetchStart {
            request_id,
            requests,
            landed,
        } = start;
        match self.pull_requests.gateway_mut() {
            ForgeGateway::Live => {}
            #[cfg(test)]
            ForgeGateway::Recording(log) => {
                log.push(super::gateway::ForgeCall::PrefetchCommits(
                    requests.iter().map(|request| request.number).collect(),
                ));
                return;
            }
        }
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        let handle = task::spawn(async move {
            let result = git::prefetch_pull_requests(&repo_root, &requests).await;
            drop(landed);
            let _ = sender.send(Event::PullRequest(PullRequestEvent::Prefetch(
                PrefetchEvent::CommitsFetched { request_id, result },
            )));
        });
        self.pull_requests
            .prefetch_mut()
            .attach_commits(request_id, handle);
    }

    /// Starts a detail prefetch of `rows`, leaving out the pull request an
    /// open is already loading live.
    fn prefetch_details(&mut self, mut rows: Vec<PullRequestSummary>, trigger: DetailTrigger) {
        if self.prefetch_client().is_none() {
            return;
        }
        let Some(repository) = self.saved_snapshot_repository() else {
            return;
        };
        rows.retain(|row| !self.opening_or_open(row.number));
        let Some((request_id, rows)) =
            self.pull_requests
                .prefetch_mut()
                .begin_details(rows, trigger, Instant::now())
        else {
            return;
        };
        let stale = self
            .pull_requests
            .saved()
            .stale_pull_requests(repository, rows);
        let sender = self.events.sender();
        let handle = task::spawn(async move {
            let stale = stale.await;
            let _ = sender.send(Event::PullRequest(PullRequestEvent::Prefetch(
                PrefetchEvent::StaleDetailsFound { request_id, stale },
            )));
        });
        self.pull_requests
            .prefetch_mut()
            .attach_details(request_id, handle);
    }

    /// Applies a prefetch result. Nothing visible changes, so it never asks
    /// for a redraw.
    pub(super) fn handle_prefetch_event(&mut self, event: PrefetchEvent) -> bool {
        match event {
            PrefetchEvent::CommitsFetched { request_id, result } => {
                let queued = self.pull_requests.prefetch_mut().finish_commits(
                    request_id,
                    &result,
                    Instant::now(),
                );
                if let Some(queued) = queued
                    && self.prefetch_client().is_some()
                {
                    self.start_commit_prefetch(queued);
                }
            }
            PrefetchEvent::StaleDetailsFound { request_id, stale } => {
                self.load_stale_details(request_id, stale);
            }
            PrefetchEvent::DetailsLoaded {
                request_id,
                requested_at,
                result,
            } => {
                // Quietly stop every other request too; the next thing the
                // user asks for explains why.
                if let Err(error) = &result
                    && error.leaves_forge_unavailable()
                {
                    self.pull_requests.mark_unavailable(error.clone());
                }
                let loaded = self.pull_requests.prefetch_mut().finish_details(
                    request_id,
                    result,
                    Instant::now(),
                );
                for detail in loaded.into_iter().flatten() {
                    self.save_pull_request(Snapshot::new(detail, requested_at.clone()));
                }
            }
        }
        false
    }

    fn load_stale_details(&mut self, request_id: u64, stale: Option<Vec<u64>>) {
        let Some(numbers) = self
            .pull_requests
            .prefetch_mut()
            .narrow_details(request_id, stale)
        else {
            return;
        };
        let Some(github) = self.prefetch_client() else {
            return;
        };
        match self.pull_requests.gateway_mut() {
            ForgeGateway::Live => {}
            #[cfg(test)]
            ForgeGateway::Recording(log) => {
                log.push(super::gateway::ForgeCall::PrefetchDetails(numbers));
                return;
            }
        }
        let sender = self.events.sender();
        let handle = task::spawn(async move {
            // GitHub's answer is at least as current as the request's start.
            let requested_at = Timestamp::now();
            let result = github.load_pull_requests(&numbers).await;
            let _ = sender.send(Event::PullRequest(PullRequestEvent::Prefetch(
                PrefetchEvent::DetailsLoaded {
                    request_id,
                    requested_at,
                    result,
                },
            )));
        });
        self.pull_requests
            .prefetch_mut()
            .attach_details(request_id, handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::pull_request::fixtures, forge::RepositoryRef};

    fn fetch(number: u64) -> PullRequestFetch {
        let summary = fixtures::summary(number);
        PullRequestFetch {
            repository: RepositoryRef {
                host: "github.com".to_string(),
                owner: "Scott-fo".to_string(),
                name: "vigil".to_string(),
            },
            number,
            head_oid: summary.head_oid,
            base_oid: summary.base_oid,
            base_ref_name: summary.base_ref_name,
        }
    }

    fn report(numbers: &[u64]) -> Result<PrefetchReport, PullRequestFetchError> {
        Ok(PrefetchReport {
            outcomes: numbers
                .iter()
                .map(|number| (*number, git::PrefetchOutcome::Fetched))
                .collect(),
            fetches: 1,
        })
    }

    #[test]
    fn settled_commits_are_not_prefetched_again_until_they_move() {
        let mut prefetch = Prefetch::default();
        let now = Instant::now();
        let start = prefetch
            .begin_commits(vec![fetch(1), fetch(2)], now)
            .expect("starts");
        prefetch.finish_commits(start.request_id, &report(&[1, 2]), now);

        assert!(
            prefetch
                .begin_commits(vec![fetch(1), fetch(2)], now)
                .is_none()
        );
        // The base branch advancing moves every row's base commit; that
        // alone refetches nothing.
        let mut rebased = fetch(1);
        rebased.base_oid = "d".repeat(40);
        assert!(prefetch.begin_commits(vec![rebased], now).is_none());
        let mut pushed = fetch(2);
        pushed.head_oid = "c".repeat(40);
        let start = prefetch
            .begin_commits(vec![fetch(1), pushed.clone()], now)
            .expect("a moved head is prefetched");
        assert_eq!(start.requests, vec![pushed]);
    }

    #[tokio::test]
    async fn an_open_stops_waiting_for_a_prefetch_that_never_lands() {
        let (landed, landing) = watch::channel(());
        let limit = Duration::from_millis(50);

        let started = std::time::Instant::now();
        assert!(!wait_for_landing(landing.clone(), limit).await);
        assert!(started.elapsed() >= limit);

        drop(landed);
        assert!(wait_for_landing(landing, OPEN_WAITS_FOR_PREFETCH).await);
    }

    #[test]
    fn a_timed_out_fetch_clears_the_running_prefetch_and_pauses() {
        let mut prefetch = Prefetch::default();
        let now = Instant::now();
        let start = prefetch.begin_commits(vec![fetch(1)], now).expect("starts");
        let timed_out = Err(PullRequestFetchError::TimedOut {
            remote: "origin".to_string(),
        });

        prefetch.finish_commits(start.request_id, &timed_out, now);

        assert!(
            prefetch.commits_landing(1).is_none(),
            "opens no longer wait"
        );
        assert!(prefetch.begin_commits(vec![fetch(1)], now).is_none());
        assert!(
            prefetch
                .begin_commits(vec![fetch(1)], now + FAILURE_BACKOFF)
                .is_some()
        );
    }

    #[test]
    fn refused_ssh_access_stops_commit_prefetching_for_the_session() {
        let mut prefetch = Prefetch::default();
        let now = Instant::now();
        let start = prefetch.begin_commits(vec![fetch(1)], now).expect("starts");
        assert!(
            prefetch.begin_commits(vec![fetch(2)], now).is_none(),
            "queued"
        );
        let refused = Err(PullRequestFetchError::Access {
            remote: "origin".to_string(),
            stderr: "git@github.com: Permission denied (publickey).".to_string(),
        });

        let queued = prefetch.finish_commits(start.request_id, &refused, now);

        assert_eq!(queued, None, "nothing queued runs");
        let much_later = now + RATE_LIMIT_BACKOFF * 100;
        assert!(prefetch.begin_commits(vec![fetch(3)], much_later).is_none());
        // Details go over `gh`, which ssh does not affect.
        assert!(
            prefetch
                .begin_details(vec![fixtures::summary(3)], DetailTrigger::ListPage, now)
                .is_some()
        );
    }

    #[tokio::test]
    async fn an_open_waits_only_for_a_fetch_of_its_own_pull_request() {
        let mut prefetch = Prefetch::default();
        let now = Instant::now();
        let start = prefetch.begin_commits(vec![fetch(1)], now).expect("starts");

        assert!(prefetch.commits_landing(2).is_none());
        let mut landing = prefetch.commits_landing(1).expect("fetching #1");
        drop(start.landed);
        assert!(
            landing.changed().await.is_err(),
            "released once the fetch is done"
        );
        prefetch.finish_commits(start.request_id, &report(&[1]), now);
        assert!(prefetch.commits_landing(1).is_none());
    }

    #[test]
    fn rows_asked_for_during_a_fetch_run_after_it() {
        let mut prefetch = Prefetch::default();
        let now = Instant::now();
        let first = prefetch.begin_commits(vec![fetch(1)], now).expect("starts");
        assert!(prefetch.begin_commits(vec![fetch(2)], now).is_none());
        assert!(prefetch.begin_commits(vec![fetch(3)], now).is_none());

        let queued = prefetch.finish_commits(first.request_id, &report(&[1]), now);

        assert_eq!(queued, Some(vec![fetch(3)]), "only the newest request");
    }

    #[test]
    fn a_failed_fetch_pauses_commit_prefetching() {
        let mut prefetch = Prefetch::default();
        let now = Instant::now();
        let start = prefetch.begin_commits(vec![fetch(1)], now).expect("starts");
        let offline = Err(PullRequestFetchError::Fetch {
            remote: "origin".to_string(),
            stderr: "could not resolve host".to_string(),
        });
        prefetch.finish_commits(start.request_id, &offline, now);

        assert!(prefetch.begin_commits(vec![fetch(1)], now).is_none());
        assert!(
            prefetch
                .begin_commits(vec![fetch(1)], now + FAILURE_BACKOFF)
                .is_some(),
            "unsettled rows are tried again after the pause"
        );
    }

    #[test]
    fn detail_prefetches_back_off_longer_after_a_rate_limit() {
        let mut prefetch = Prefetch::default();
        let now = Instant::now();
        let rows = vec![fixtures::summary(1)];
        let (id, _) = prefetch
            .begin_details(rows.clone(), DetailTrigger::ListPage, now)
            .expect("starts");
        assert_eq!(prefetch.narrow_details(id, Some(vec![1])), Some(vec![1]));
        let limited = Err(ForgeError::RateLimited {
            message: "API rate limit exceeded".to_string(),
        });
        assert_eq!(prefetch.finish_details(id, limited, now), None);

        let later = now + FAILURE_BACKOFF;
        assert!(
            prefetch
                .begin_details(rows.clone(), DetailTrigger::ListPage, later)
                .is_none()
        );
        assert!(
            prefetch
                .begin_details(rows, DetailTrigger::ListPage, now + RATE_LIMIT_BACKOFF)
                .is_some()
        );
    }

    #[test]
    fn a_current_branch_prefetch_never_replaces_a_running_one() {
        let mut prefetch = Prefetch::default();
        let now = Instant::now();
        let (id, _) = prefetch
            .begin_details(vec![fixtures::summary(1)], DetailTrigger::ListPage, now)
            .expect("starts");

        assert!(
            prefetch
                .begin_details(
                    vec![fixtures::summary(2)],
                    DetailTrigger::CurrentBranch,
                    now
                )
                .is_none()
        );
        assert_eq!(prefetch.detail_request_id(), id);
        let (newer, _) = prefetch
            .begin_details(vec![fixtures::summary(3)], DetailTrigger::ListPage, now)
            .expect("a newer page supersedes");
        assert_ne!(newer, id);
        assert_eq!(prefetch.narrow_details(id, Some(vec![1])), None);
    }

    #[test]
    fn details_found_current_are_settled_without_a_load() {
        let mut prefetch = Prefetch::default();
        let now = Instant::now();
        let rows = vec![fixtures::summary(1), fixtures::summary(2)];
        let (id, _) = prefetch
            .begin_details(rows.clone(), DetailTrigger::ListPage, now)
            .expect("starts");

        assert_eq!(prefetch.narrow_details(id, Some(Vec::new())), None);
        assert!(!prefetch.detail_in_flight());
        assert!(
            prefetch
                .begin_details(rows, DetailTrigger::ListPage, now)
                .is_none()
        );
    }
}
