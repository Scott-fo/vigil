//! Saved snapshots: the last known list pages and pull request details,
//! shown at once while live loads run.
//!
//! Every GitHub read takes half a second or more. When the list shows a tab
//! it has nothing for, or a pull request opens without its detail, the app
//! reads the forge cache ([`ForgeCache`]) alongside the live load and shows
//! what it finds, marked [`Freshness::Saved`] with its fetch time. The live
//! result replaces it when it lands, and is saved in turn.
//!
//! Ordering is by content, not request ids: a saved snapshot only fills a
//! gap. It never replaces anything already shown, so a cache read that
//! finishes after the live load is dropped. Saved data is display-only;
//! features that write to GitHub from what they show (merging, state
//! changes) wait for [`Freshness::Live`].
//!
//! Reads run on the blocking pool; saves go through one background task in
//! order. The cache is best-effort: a failure to open, read, or save is a
//! miss and is never reported. Tests get [`SavedSnapshots::off`] through
//! `PullRequests::disabled`, and never touch the user's cache file.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use tokio::{sync::mpsc, task};

use crate::{
    event::Event,
    forge::{
        ForgeCache, PullRequest, PullRequestList, PullRequestListFilter, PullRequestSummary,
        RepositoryRef, Snapshot, Timestamp,
    },
};

use super::{super::App, PullRequestEvent};

/// How current pull request data on screen is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) enum Freshness {
    /// Loaded from GitHub in this session, by a request that started at
    /// `requested_at`.
    Live { requested_at: Instant },
    /// Read from the forge cache: what GitHub said at `fetched_at`, not yet
    /// confirmed by a live load.
    Saved { fetched_at: Timestamp },
}

impl Freshness {
    pub(in crate::app) fn saved_at(&self) -> Option<&Timestamp> {
        match self {
            Self::Live { .. } => None,
            Self::Saved { fetched_at } => Some(fetched_at),
        }
    }

    pub(in crate::app) fn is_live(&self) -> bool {
        matches!(self, Self::Live { .. })
    }
}

/// The wall-clock time of `requested_at`, to date a live result by when its
/// request started.
pub(in crate::app) fn request_time(requested_at: Instant) -> Timestamp {
    let started = SystemTime::now()
        .checked_sub(requested_at.elapsed())
        .unwrap_or(UNIX_EPOCH);
    Timestamp::from_unix_seconds(
        started
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64),
    )
}

/// After a failed open, how long reads and saves miss before the next try.
const REOPEN_INTERVAL: Duration = Duration::from_secs(60);

/// A cache file opened on first use, off the UI thread. A failed open, such
/// as a busy timeout while another vigil migrates the file, is retried at
/// most once per [`REOPEN_INTERVAL`] rather than giving up for the session.
#[derive(Debug)]
struct LazyCache {
    path: PathBuf,
    state: Mutex<LazyState>,
}

#[derive(Debug)]
enum LazyState {
    Unopened,
    Open(ForgeCache),
    Failed { at: Instant },
}

impl LazyCache {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: Mutex::new(LazyState::Unopened),
        }
    }

    /// The cache, opening it unless an open failed within the interval
    /// before `now`. Blocking.
    fn resolve(&self, now: Instant) -> Option<ForgeCache> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match &*state {
            LazyState::Open(cache) => return Some(cache.clone()),
            LazyState::Failed { at } if now.saturating_duration_since(*at) < REOPEN_INTERVAL => {
                return None;
            }
            LazyState::Unopened | LazyState::Failed { .. } => {}
        }
        match ForgeCache::open(self.path.clone()) {
            Ok(cache) => {
                *state = LazyState::Open(cache.clone());
                Some(cache)
            }
            Err(_) => {
                *state = LazyState::Failed { at: now };
                None
            }
        }
    }
}

/// Which cache snapshots come from.
#[derive(Debug, Clone)]
enum CacheSource {
    /// No cache: reads miss and saves are dropped.
    Off,
    /// The per-user cache file.
    #[cfg_attr(test, allow(dead_code))]
    UserCache(Arc<LazyCache>),
    /// A cache the caller opened, such as a temporary file in tests.
    #[cfg(test)]
    Given(ForgeCache),
}

impl CacheSource {
    /// The cache, opening it on first use. Blocking.
    fn resolve(&self) -> Option<ForgeCache> {
        match self {
            Self::Off => None,
            #[cfg(not(test))]
            Self::UserCache(cache) => cache.resolve(Instant::now()),
            // Tests must never touch the user's cache file, even by accident.
            #[cfg(test)]
            Self::UserCache(_) => {
                panic!("tests must not open the user's forge cache (PullRequests::disabled)")
            }
            #[cfg(test)]
            Self::Given(cache) => Some(cache.clone()),
        }
    }
}

/// A queued save.
#[derive(Debug)]
enum SnapshotSave {
    List {
        repository: RepositoryRef,
        filter: PullRequestListFilter,
        snapshot: Snapshot<PullRequestList>,
    },
    PullRequest {
        repository: RepositoryRef,
        snapshot: Box<Snapshot<PullRequest>>,
    },
}

impl SnapshotSave {
    fn apply(self, cache: &ForgeCache) {
        // Best-effort: a failed save only means a colder start next time.
        let _ = match self {
            Self::List {
                repository,
                filter,
                snapshot,
            } => cache.store_pull_request_list(&repository, filter, &snapshot),
            Self::PullRequest {
                repository,
                snapshot,
            } => cache.store_pull_request(&repository, &snapshot),
        };
    }
}

/// The app's handle to the forge cache: where snapshots are read from, and
/// the queue saves go through.
#[derive(Debug)]
pub(in crate::app) struct SavedSnapshots {
    source: CacheSource,
    saves: Option<mpsc::UnboundedSender<SnapshotSave>>,
}

impl SavedSnapshots {
    /// The per-user cache, opened lazily.
    pub(in crate::app) fn user_cache() -> Self {
        Self::from_source(CacheSource::UserCache(Arc::new(LazyCache::new(
            ForgeCache::default_path(),
        ))))
    }

    /// No cache at all.
    pub(in crate::app) fn off() -> Self {
        Self::from_source(CacheSource::Off)
    }

    #[cfg(test)]
    pub(in crate::app) fn given(cache: ForgeCache) -> Self {
        Self::from_source(CacheSource::Given(cache))
    }

    fn from_source(source: CacheSource) -> Self {
        Self {
            source,
            saves: None,
        }
    }

    fn enabled(&self) -> bool {
        !matches!(self.source, CacheSource::Off)
    }

    /// Which of `rows` have no saved detail for the `updated_at` and head
    /// they report (see [`ForgeCache::stale_pull_requests`]), checked on
    /// the blocking pool when awaited. `None` when there is no cache or it
    /// cannot answer.
    pub(in crate::app) fn stale_pull_requests(
        &self,
        repository: RepositoryRef,
        rows: Vec<PullRequestSummary>,
    ) -> impl Future<Output = Option<Vec<u64>>> + Send + 'static {
        let source = self.source.clone();
        async move {
            task::spawn_blocking(move || {
                source
                    .resolve()?
                    .stale_pull_requests(&repository, &rows)
                    .ok()
            })
            .await
            .ok()
            .flatten()
        }
    }

    /// Queues `save` behind earlier ones, starting the save task on first
    /// use. Saves still queued when the app quits are dropped.
    fn queue(&mut self, save: SnapshotSave) {
        if !self.enabled() {
            return;
        }
        let source = &self.source;
        let saves = self.saves.get_or_insert_with(|| {
            let (sender, mut receiver) = mpsc::unbounded_channel::<SnapshotSave>();
            let source = source.clone();
            task::spawn(async move {
                while let Some(save) = receiver.recv().await {
                    let source = source.clone();
                    let _ = task::spawn_blocking(move || {
                        if let Some(cache) = source.resolve() {
                            save.apply(&cache);
                        }
                    })
                    .await;
                }
            });
            sender
        });
        let _ = saves.send(save);
    }
}

impl App {
    /// The repository to key snapshots by, while GitHub is connected and
    /// a cache is in use.
    pub(super) fn saved_snapshot_repository(&self) -> Option<RepositoryRef> {
        let saved = self.pull_requests.saved();
        if !saved.enabled() {
            return None;
        }
        // Checked here, on the caller's thread: a panic inside the read or
        // save task would go unnoticed.
        #[cfg(test)]
        assert!(
            !matches!(saved.source, CacheSource::UserCache(_)),
            "tests must not use the user's forge cache (PullRequests::disabled)"
        );
        Some(self.pull_requests.github()?.repository().clone())
    }

    /// Whether a read issued for `repository` still belongs to the
    /// connected one. A rename found after connecting switches the client,
    /// and a read under the old name that lands afterwards is dropped.
    /// Spelling is compared as the cache keys it, ignoring case, since
    /// adopting GitHub's canonical casing is not a different repository.
    fn is_saved_snapshot_repository(&self, repository: &RepositoryRef) -> bool {
        self.pull_requests.github().is_some_and(|github| {
            let current = github.repository();
            current.host.eq_ignore_ascii_case(&repository.host)
                && current.owner.eq_ignore_ascii_case(&repository.owner)
                && current.name.eq_ignore_ascii_case(&repository.name)
        })
    }

    /// Reads `filter`'s saved page in the background, for a tab with
    /// nothing to show yet.
    pub(super) fn read_saved_pull_request_list(&mut self, filter: PullRequestListFilter) {
        if self.pull_requests.list().has_page(filter) {
            return;
        }
        let Some(repository) = self.saved_snapshot_repository() else {
            return;
        };
        let source = self.pull_requests.saved().source.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let lookup = repository.clone();
            let snapshot =
                task::spawn_blocking(move || source.resolve()?.pull_request_list(&lookup, filter))
                    .await
                    .ok()
                    .flatten();
            let _ = sender.send(Event::PullRequest(PullRequestEvent::SavedListRead {
                repository,
                filter,
                snapshot,
            }));
        }));
    }

    /// Shows a saved page if its tab still has nothing to show.
    pub(super) fn handle_saved_pull_request_list(
        &mut self,
        repository: &RepositoryRef,
        filter: PullRequestListFilter,
        snapshot: Option<Snapshot<PullRequestList>>,
    ) -> bool {
        let Some(snapshot) = snapshot else {
            return false;
        };
        if !self.is_saved_snapshot_repository(repository) {
            return false;
        }
        self.pull_requests.list_mut().show_saved(filter, snapshot)
    }

    /// Saves a live list page, dated by when its request started.
    pub(super) fn save_pull_request_list(
        &mut self,
        filter: PullRequestListFilter,
        snapshot: Snapshot<PullRequestList>,
    ) {
        let Some(repository) = self.saved_snapshot_repository() else {
            return;
        };
        self.pull_requests.saved_mut().queue(SnapshotSave::List {
            repository,
            filter,
            snapshot,
        });
    }

    /// Reads pull request `number`'s saved detail in the background, unless
    /// the review already shows one.
    pub(super) fn read_saved_pull_request(&mut self, number: u64) {
        if !self.pull_requests.wants_saved_detail(number) {
            return;
        }
        let Some(repository) = self.saved_snapshot_repository() else {
            return;
        };
        let source = self.pull_requests.saved().source.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let lookup = repository.clone();
            let snapshot =
                task::spawn_blocking(move || source.resolve()?.pull_request(&lookup, number))
                    .await
                    .ok()
                    .flatten()
                    .map(Box::new);
            let _ = sender.send(Event::PullRequest(PullRequestEvent::SavedDetailRead {
                repository,
                number,
                snapshot,
            }));
        }));
    }

    /// Shows a saved detail if its pull request still has none.
    pub(super) fn handle_saved_pull_request(
        &mut self,
        repository: &RepositoryRef,
        number: u64,
        snapshot: Option<Box<Snapshot<PullRequest>>>,
    ) -> bool {
        let Some(snapshot) = snapshot else {
            return false;
        };
        if !self.is_saved_snapshot_repository(repository) {
            return false;
        }
        self.pull_requests.show_saved_detail(number, *snapshot)
    }

    /// Saves a pull request's detail from GitHub, dated by when its request
    /// started (take [`Timestamp::now`] before sending it), so the cache
    /// keeps the newest of racing saves. Details loaded outside the review,
    /// such as by a prefetch, go through here too.
    pub(in crate::app) fn save_pull_request(&mut self, snapshot: Snapshot<PullRequest>) {
        let Some(repository) = self.saved_snapshot_repository() else {
            return;
        };
        self.pull_requests
            .saved_mut()
            .queue(SnapshotSave::PullRequest {
                repository,
                snapshot: Box::new(snapshot),
            });
    }

    /// Reads and saves snapshots in `cache` instead of none, for tests.
    #[cfg(test)]
    pub(in crate::app) fn use_forge_cache_for_test(&mut self, cache: ForgeCache) {
        *self.pull_requests.saved_mut() = SavedSnapshots::given(cache);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_open_is_retried_after_the_interval_not_before() {
        let dir = std::env::temp_dir().join(format!(
            "vigil-lazy-cache-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&dir);
        // A file where the cache's directory should be makes opening fail.
        std::fs::write(&dir, "not a directory").unwrap();
        let cache = LazyCache::new(dir.join("forge-cache.sqlite3"));
        let start = Instant::now();

        assert!(cache.resolve(start).is_none());
        std::fs::remove_file(&dir).unwrap();
        assert!(
            cache.resolve(start + Duration::from_secs(10)).is_none(),
            "no retry within the interval"
        );
        assert!(
            cache.resolve(start + REOPEN_INTERVAL).is_some(),
            "retried once the interval passed"
        );
        assert!(
            cache.resolve(start + REOPEN_INTERVAL).is_some(),
            "stays open"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
