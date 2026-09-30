//! The last known state of pull requests, saved on disk between sessions.
//!
//! Every [`GitHub`](super::GitHub) read waits on the network. [`ForgeCache`]
//! keeps the latest answer to each list and detail load so a screen can show
//! it at once, marked with when it was fetched, while a live load replaces
//! it. It is a cache, not a record: the file (`forge-cache.sqlite3`, next
//! to the review database) holds only pull request metadata, never diffs or
//! file contents, and deleting it loses nothing.
//!
//! # Staleness contract
//!
//! A [`Snapshot`] is what GitHub said at [`Snapshot::fetched_at`], nothing
//! more. Callers must treat it as display-only until a live load confirms
//! it: mergeability, checks, permissions, and thread state may all have
//! changed since, so no write to GitHub should be decided from a snapshot.
//! Saving never replaces a snapshot with an older one, so writers that race
//! (a live load and a background prefetch) keep the newest.
//!
//! # Write policy
//!
//! Callers save every live result; the cache decides what reaches the disk,
//! so periodic reloads do not churn it. Each row stores a hash of its
//! encoded value next to the value:
//!
//! - Changed content rewrites the row.
//! - Unchanged content writes nothing, except that a row whose fetch time
//!   is more than 15 minutes behind gets that one integer moved forward. The
//!   value is never rewritten just to record a confirmation, so a read may
//!   report a fetch time up to 15 minutes older than the last confirmation;
//!   it never overstates freshness.
//!
//! Snapshots are dated by when their request started, so "newest" means
//! the answer GitHub gave last. A row dated in the future (a clock that ran
//! ahead) is not trusted to be newer: any save replaces it.
//!
//! Growth is bounded when the cache opens, never on a save: rows not
//! refreshed for 14 days are deleted (so repositories no longer used drop
//! out), as are rows dated more than a day ahead, and only the 300 most
//! recently refreshed pull request details are kept. Opening a cache of
//! another format version empties it. Nothing vacuums the file; later rows
//! reuse freed pages.
//!
//! # Cost and failure
//!
//! Every call is blocking SQLite I/O; interactive callers run them off the
//! UI thread. Reads never fail: a missing, unreadable, or out-of-date row
//! is a miss (`None`). Saves report errors, which callers may ignore: the
//! cache is best-effort.

use std::{
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use color_eyre::eyre::WrapErr;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};

use super::types::{PullRequest, PullRequestList, PullRequestListFilter, RepositoryRef, Timestamp};

/// The encoding of saved rows. Bump it whenever a cached domain type
/// changes shape or meaning; opening a cache of another version empties it.
const CACHE_VERSION: i64 = 1;

/// Confirming unchanged content moves its fetch time only once the saved
/// one is this far behind.
const REFRESH_GRANULARITY: Duration = Duration::from_secs(15 * 60);

/// Rows dated further ahead than this are deleted on open.
const CLOCK_SKEW_ALLOWANCE: Duration = Duration::from_secs(24 * 60 * 60);

/// How long a call waits for another vigil's write to finish.
const BUSY_TIMEOUT: Duration = Duration::from_millis(500);

/// A value as GitHub reported it at `fetched_at`.
///
/// `fetched_at` is when the request that returned `value` started, taken
/// with [`Timestamp::now`] before sending it: GitHub's answer is at least
/// that current, so ordering snapshots by it keeps the newest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot<T> {
    pub value: T,
    pub fetched_at: Timestamp,
}

impl<T> Snapshot<T> {
    pub fn new(value: T, fetched_at: Timestamp) -> Self {
        Self { value, fetched_at }
    }
}

/// What a save wrote. See the module's write policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveOutcome {
    /// New or changed content was written.
    Stored,
    /// Same content; only its fetch time moved forward.
    Refreshed,
    /// Nothing was written: the content is unchanged and was confirmed
    /// recently, or a newer snapshot is already saved.
    Unchanged,
}

/// When rows are deleted.
#[derive(Debug, Clone, Copy)]
struct Retention {
    /// Rows not refreshed for this long are deleted.
    max_age: Duration,
    /// Pull request details kept, most recently refreshed first.
    max_pull_requests: u32,
}

impl Retention {
    const DEFAULT: Self = Self {
        max_age: Duration::from_secs(14 * 24 * 60 * 60),
        max_pull_requests: 300,
    };
}

/// What a row holds.
#[derive(Debug, Clone, Copy)]
enum Entry {
    List(PullRequestListFilter),
    PullRequest(u64),
}

/// A row's primary key.
///
/// GitHub names are case-insensitive, and a repository resolved from a
/// remote URL keeps the URL's spelling while one resolved through GitHub
/// has its canonical casing, so the repository is stored lowercase: every
/// spelling finds the same row. Filter names are fixed strings, so renaming
/// a variant does not orphan saved pages.
#[derive(Debug)]
struct RowKey {
    host: String,
    owner: String,
    name: String,
    kind: &'static str,
    key: String,
}

impl RowKey {
    fn new(repository: &RepositoryRef, entry: Entry) -> Self {
        let (kind, key) = match entry {
            Entry::List(filter) => (
                "list",
                match filter {
                    PullRequestListFilter::NeedsMyReview => "needs-my-review",
                    PullRequestListFilter::Mine => "mine",
                    PullRequestListFilter::AllOpen => "all-open",
                }
                .to_string(),
            ),
            Entry::PullRequest(number) => ("pull_request", number.to_string()),
        };
        Self {
            host: repository.host.to_lowercase(),
            owner: repository.owner.to_lowercase(),
            name: repository.name.to_lowercase(),
            kind,
            key,
        }
    }
}

/// Handle to the forge cache file. Cheap to clone; each call opens its own
/// blocking SQLite connection.
#[derive(Debug, Clone)]
pub struct ForgeCache {
    path: PathBuf,
}

impl ForgeCache {
    /// Opens the cache at the default per-user location.
    pub fn open_default() -> color_eyre::Result<Self> {
        Self::open(crate::user_data::data_dir().join("forge-cache.sqlite3"))
    }

    /// Opens the cache at `path`, creating it as needed and deleting rows
    /// of another cache version or past retention.
    pub fn open(path: PathBuf) -> color_eyre::Result<Self> {
        Self::open_with(path, Retention::DEFAULT)
    }

    fn open_with(path: PathBuf, retention: Retention) -> color_eyre::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).wrap_err_with(|| {
                format!(
                    "failed to create forge cache directory {}",
                    parent.display()
                )
            })?;
        }
        let cache = Self { path };
        cache.initialize(retention)?;
        Ok(cache)
    }

    /// The last saved page of `repository`'s pull requests under `filter`.
    pub fn pull_request_list(
        &self,
        repository: &RepositoryRef,
        filter: PullRequestListFilter,
    ) -> Option<Snapshot<PullRequestList>> {
        self.read(RowKey::new(repository, Entry::List(filter)))
    }

    /// Saves `snapshot` as `repository`'s page under `filter`, following
    /// the write policy.
    pub fn store_pull_request_list(
        &self,
        repository: &RepositoryRef,
        filter: PullRequestListFilter,
        snapshot: &Snapshot<PullRequestList>,
    ) -> color_eyre::Result<()> {
        self.save_list(repository, filter, snapshot).map(drop)
    }

    fn save_list(
        &self,
        repository: &RepositoryRef,
        filter: PullRequestListFilter,
        snapshot: &Snapshot<PullRequestList>,
    ) -> color_eyre::Result<SaveOutcome> {
        self.store(RowKey::new(repository, Entry::List(filter)), snapshot)
    }

    /// The last saved detail of `repository`'s pull request `number`.
    pub fn pull_request(
        &self,
        repository: &RepositoryRef,
        number: u64,
    ) -> Option<Snapshot<PullRequest>> {
        self.read(RowKey::new(repository, Entry::PullRequest(number)))
    }

    /// Saves `snapshot` under its pull request's number, following the
    /// write policy.
    pub fn store_pull_request(
        &self,
        repository: &RepositoryRef,
        snapshot: &Snapshot<PullRequest>,
    ) -> color_eyre::Result<()> {
        self.save_detail(repository, snapshot).map(drop)
    }

    fn save_detail(
        &self,
        repository: &RepositoryRef,
        snapshot: &Snapshot<PullRequest>,
    ) -> color_eyre::Result<SaveOutcome> {
        let entry = Entry::PullRequest(snapshot.value.summary.number);
        self.store(RowKey::new(repository, entry), snapshot)
    }

    fn initialize(&self, retention: Retention) -> color_eyre::Result<()> {
        let connection = self.connection()?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        // Only a new or outdated file is written to here, so a routine
        // open touches nothing unless rows expire.
        if version != CACHE_VERSION {
            connection.execute_batch(
                "
                pragma journal_mode = wal;
                drop table if exists snapshots;
                create table snapshots (
                    host text not null,
                    owner text not null,
                    name text not null,
                    kind text not null,
                    key text not null,
                    fetched_at integer not null,
                    content_hash integer not null,
                    body text not null,
                    primary key (host, owner, name, kind, key)
                );
                ",
            )?;
            connection.pragma_update(None, "user_version", CACHE_VERSION)?;
        }

        // Rows dated more than a day ahead were written under a skewed
        // clock; they would otherwise outlive every age cutoff.
        let now = now_unix_seconds();
        let cutoff = now.saturating_sub(retention.max_age.as_secs() as i64);
        connection.execute(
            "delete from snapshots where fetched_at < ?1 or fetched_at > ?2",
            [
                cutoff,
                now.saturating_add(CLOCK_SKEW_ALLOWANCE.as_secs() as i64),
            ],
        )?;
        connection.execute(
            "delete from snapshots
             where kind = 'pull_request' and rowid not in (
                 select rowid from snapshots where kind = 'pull_request'
                 order by fetched_at desc limit ?1
             )",
            [retention.max_pull_requests],
        )?;
        Ok(())
    }

    fn connection(&self) -> color_eyre::Result<Connection> {
        let connection = Connection::open(&self.path)
            .wrap_err_with(|| format!("failed to open forge cache {}", self.path.display()))?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        Ok(connection)
    }

    /// The saved snapshot at `key`; `None` when it is missing or cannot be
    /// read.
    fn read<T: DeserializeOwned>(&self, key: RowKey) -> Option<Snapshot<T>> {
        let (fetched_at, body) = self
            .connection()
            .ok()?
            .query_row(
                "select fetched_at, body from snapshots
                 where host = ?1 and owner = ?2 and name = ?3 and kind = ?4 and key = ?5",
                params![key.host, key.owner, key.name, key.kind, key.key],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .ok()??;
        let value = serde_json::from_str(&body).ok()?;
        Some(Snapshot::new(
            value,
            Timestamp::from_unix_seconds(fetched_at),
        ))
    }

    fn store<T: Serialize>(
        &self,
        key: RowKey,
        snapshot: &Snapshot<T>,
    ) -> color_eyre::Result<SaveOutcome> {
        let fetched_at = snapshot
            .fetched_at
            .unix_seconds()
            .ok_or_else(|| color_eyre::eyre::eyre!("invalid fetch time {}", snapshot.fetched_at))?;
        let body = serde_json::to_string(&snapshot.value)?;
        let content_hash = content_hash(&body);
        let connection = self.connection()?;
        let saved = connection
            .query_row(
                "select fetched_at, content_hash from snapshots
                 where host = ?1 and owner = ?2 and name = ?3 and kind = ?4 and key = ?5",
                params![key.host, key.owner, key.name, key.kind, key.key],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        // A row dated in the future came from a skewed clock and is not
        // trusted to be newer than anything.
        let now = now_unix_seconds();
        let outcome = match saved {
            Some((saved_at, saved_hash)) if saved_at > now => {
                if saved_hash == content_hash {
                    SaveOutcome::Refreshed
                } else {
                    SaveOutcome::Stored
                }
            }
            Some((saved_at, _)) if saved_at > fetched_at => SaveOutcome::Unchanged,
            Some((saved_at, saved_hash)) if saved_hash == content_hash => {
                if fetched_at - saved_at < REFRESH_GRANULARITY.as_secs() as i64 {
                    SaveOutcome::Unchanged
                } else {
                    SaveOutcome::Refreshed
                }
            }
            _ => SaveOutcome::Stored,
        };
        match outcome {
            SaveOutcome::Unchanged => {}
            SaveOutcome::Refreshed => {
                connection.execute(
                    "update snapshots set fetched_at = ?6
                     where host = ?1 and owner = ?2 and name = ?3 and kind = ?4 and key = ?5
                       and (fetched_at < ?6 or fetched_at > ?7)",
                    params![
                        key.host, key.owner, key.name, key.kind, key.key, fetched_at, now
                    ],
                )?;
            }
            SaveOutcome::Stored => {
                connection.execute(
                    "insert into snapshots
                         (host, owner, name, kind, key, fetched_at, content_hash, body)
                     values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     on conflict (host, owner, name, kind, key) do update
                     set fetched_at = excluded.fetched_at,
                         content_hash = excluded.content_hash,
                         body = excluded.body
                     where excluded.fetched_at >= snapshots.fetched_at
                        or snapshots.fetched_at > ?9",
                    params![
                        key.host,
                        key.owner,
                        key.name,
                        key.kind,
                        key.key,
                        fetched_at,
                        content_hash,
                        body,
                        now
                    ],
                )?;
            }
        }
        Ok(outcome)
    }
}

/// FNV-1a over the encoded value. Stable across builds and Rust versions,
/// unlike `std`'s hasher, so an upgrade does not rewrite every row.
fn content_hash(body: &str) -> i64 {
    let hash = body.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    i64::from_ne_bytes(hash.to_ne_bytes())
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::forge::{
        CheckCounts, CheckRollup, CheckState, CommentState, DiffSide, MergeMethod, MergeSettings,
        MergeStateStatus, Mergeability, PullRequestState, PullRequestSummary, ReviewDecision,
        ReviewThread, ThreadComment, ThreadId, ThreadSubject, Viewer,
    };

    /// A fresh file per test, so tests never share rows or touch the user's
    /// cache.
    fn temp_path(name: &str) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir()
            .join("vigil-forge-cache-tests")
            .join(format!(
                "{name}-{}-{}-{}.sqlite3",
                std::process::id(),
                now_unix_seconds(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
    }

    fn temp_cache(name: &str) -> (ForgeCache, PathBuf) {
        let path = temp_path(name);
        (ForgeCache::open(path.clone()).expect("cache opens"), path)
    }

    fn remove(path: &std::path::Path) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    fn repository(owner: &str, name: &str) -> RepositoryRef {
        RepositoryRef {
            host: "github.com".to_string(),
            owner: owner.to_string(),
            name: name.to_string(),
        }
    }

    fn summary(number: u64) -> PullRequestSummary {
        PullRequestSummary {
            number,
            title: format!("Pull request {number}"),
            author: "Scott-fo".to_string(),
            url: format!("https://github.com/Scott-fo/vigil/pull/{number}"),
            head_ref_name: "feature".to_string(),
            base_ref_name: "master".to_string(),
            head_oid: "a".repeat(40),
            base_oid: "b".repeat(40),
            is_draft: false,
            state: PullRequestState::Open,
            review_decision: Some(ReviewDecision::ReviewRequired),
            checks: Some(CheckRollup {
                state: CheckState::Pending,
                counts: CheckCounts {
                    pending: 1,
                    success: 2,
                    ..CheckCounts::default()
                },
            }),
            additions: 12,
            deletions: 3,
            changed_files: 2,
            updated_at: Timestamp::new("2026-09-29T16:00:00Z"),
            is_cross_repository: false,
        }
    }

    fn list(numbers: &[u64]) -> PullRequestList {
        PullRequestList {
            pull_requests: numbers.iter().copied().map(summary).collect(),
            total_count: numbers.len() as u64 + 10,
        }
    }

    fn detail(number: u64) -> PullRequest {
        PullRequest {
            summary: summary(number),
            body: "Body".to_string(),
            created_at: Timestamp::new("2026-09-28T10:00:00Z"),
            commit_count: 2,
            mergeable: Mergeability::Mergeable,
            merge_state: MergeStateStatus::Blocked,
            auto_merge: None,
            requested_reviewers: Vec::new(),
            latest_reviews: Vec::new(),
            conversation: Vec::new(),
            checks: Vec::new(),
            review_threads: vec![ReviewThread {
                id: ThreadId::new("T1"),
                path: "src/lib.rs".to_string(),
                subject: ThreadSubject::Line,
                side: DiffSide::Right,
                line: Some(3),
                start: None,
                original_line: Some(3),
                original_start_line: None,
                is_resolved: false,
                resolved_by: None,
                is_outdated: false,
                viewer_can_reply: true,
                viewer_can_resolve: true,
                viewer_can_unresolve: false,
                diff_hunk: "@@ -1 +1 @@".to_string(),
                comments: vec![ThreadComment {
                    id: "C1".to_string(),
                    author: "reviewer".to_string(),
                    body: "Why?".to_string(),
                    created_at: Timestamp::new("2026-09-29T12:00:00Z"),
                    url: String::new(),
                    state: CommentState::Submitted,
                }],
            }],
            viewer: Viewer {
                login: "reviewer".to_string(),
                is_author: false,
                pending_review: None,
                can_update: true,
                can_close: true,
                can_reopen: false,
                can_enable_auto_merge: true,
                can_disable_auto_merge: false,
                can_delete_head_ref: false,
            },
            merge_settings: MergeSettings {
                allowed_methods: vec![MergeMethod::Squash, MergeMethod::Rebase],
                viewer_default_method: MergeMethod::Squash,
                auto_merge_allowed: true,
                delete_branch_on_merge: true,
            },
        }
    }

    fn at(seconds_ago: i64) -> Timestamp {
        Timestamp::from_unix_seconds(now_unix_seconds() - seconds_ago)
    }

    fn row_count(path: &std::path::Path) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row("select count(*) from snapshots", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn lists_round_trip_per_repository_and_filter() {
        let (cache, path) = temp_cache("list");
        let vigil = repository("Scott-fo", "vigil");
        let snapshot = Snapshot::new(list(&[17, 18]), at(60));

        assert_eq!(
            cache.pull_request_list(&vigil, PullRequestListFilter::Mine),
            None
        );
        let outcome = cache
            .save_list(&vigil, PullRequestListFilter::Mine, &snapshot)
            .expect("store");

        assert_eq!(outcome, SaveOutcome::Stored);
        assert_eq!(
            cache.pull_request_list(&vigil, PullRequestListFilter::Mine),
            Some(snapshot)
        );
        remove(&path);
    }

    #[test]
    fn details_round_trip_per_repository_and_number() {
        let (cache, path) = temp_cache("detail");
        let vigil = repository("Scott-fo", "vigil");
        let snapshot = Snapshot::new(detail(17), at(60));

        cache.store_pull_request(&vigil, &snapshot).expect("store");

        assert_eq!(cache.pull_request(&vigil, 17), Some(snapshot));
        assert_eq!(cache.pull_request(&vigil, 18), None);
        remove(&path);
    }

    #[test]
    fn keys_do_not_collide_across_repositories_hosts_filters_or_numbers() {
        let (cache, path) = temp_cache("keys");
        let vigil = repository("Scott-fo", "vigil");
        let fork = repository("someone", "vigil");
        let enterprise = RepositoryRef {
            host: "github.example.com".to_string(),
            ..vigil.clone()
        };
        let store_list = |repository: &RepositoryRef, filter, numbers: &[u64]| {
            cache
                .store_pull_request_list(repository, filter, &Snapshot::new(list(numbers), at(10)))
                .unwrap();
        };
        store_list(&vigil, PullRequestListFilter::Mine, &[1]);
        store_list(&vigil, PullRequestListFilter::AllOpen, &[2, 3]);
        store_list(&fork, PullRequestListFilter::Mine, &[4]);
        cache
            .store_pull_request(&vigil, &Snapshot::new(detail(1), at(10)))
            .unwrap();

        let numbers = |repository: &RepositoryRef, filter| {
            cache.pull_request_list(repository, filter).map(|snapshot| {
                snapshot
                    .value
                    .pull_requests
                    .iter()
                    .map(|summary| summary.number)
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(numbers(&vigil, PullRequestListFilter::Mine), Some(vec![1]));
        assert_eq!(
            numbers(&vigil, PullRequestListFilter::AllOpen),
            Some(vec![2, 3])
        );
        assert_eq!(numbers(&vigil, PullRequestListFilter::NeedsMyReview), None);
        assert_eq!(numbers(&fork, PullRequestListFilter::Mine), Some(vec![4]));
        assert_eq!(numbers(&enterprise, PullRequestListFilter::Mine), None);
        assert!(cache.pull_request(&fork, 1).is_none());
        assert!(cache.pull_request(&enterprise, 1).is_none());
        remove(&path);
    }

    /// A repository resolved from a remote keeps the URL's spelling; one
    /// resolved through GitHub has canonical casing. Both are one repository.
    #[test]
    fn repository_spellings_that_differ_only_in_case_share_rows() {
        let (cache, path) = temp_cache("case");
        let canonical = repository("Scott-fo", "vigil");
        let from_remote = RepositoryRef {
            host: "GitHub.com".to_string(),
            owner: "scott-fo".to_string(),
            name: "VIGIL".to_string(),
        };
        let snapshot = Snapshot::new(list(&[17]), at(60));
        cache
            .store_pull_request_list(&from_remote, PullRequestListFilter::Mine, &snapshot)
            .unwrap();
        cache
            .store_pull_request(&canonical, &Snapshot::new(detail(17), at(60)))
            .unwrap();

        assert_eq!(
            cache.pull_request_list(&canonical, PullRequestListFilter::Mine),
            Some(snapshot)
        );
        assert!(cache.pull_request(&from_remote, 17).is_some());
        assert_eq!(row_count(&path), 2, "one row per entry, not per spelling");
        remove(&path);
    }

    #[test]
    fn an_older_snapshot_never_replaces_a_newer_one() {
        let (cache, path) = temp_cache("newest");
        let vigil = repository("Scott-fo", "vigil");
        let newer = Snapshot::new(detail(17), at(10));
        let mut older_detail = detail(17);
        older_detail.body = "older".to_string();

        cache.store_pull_request(&vigil, &newer).unwrap();
        let outcome = cache
            .save_detail(&vigil, &Snapshot::new(older_detail, at(600)))
            .unwrap();

        assert_eq!(outcome, SaveOutcome::Unchanged);
        assert_eq!(cache.pull_request(&vigil, 17), Some(newer));
        remove(&path);
    }

    /// A reload every minute with nothing new must not write to disk.
    #[test]
    fn saving_unchanged_content_leaves_the_row_alone() {
        let (cache, path) = temp_cache("unchanged");
        let vigil = repository("Scott-fo", "vigil");
        let first = Snapshot::new(list(&[17]), at(120));
        cache
            .store_pull_request_list(&vigil, PullRequestListFilter::Mine, &first)
            .unwrap();
        let before = Connection::open(&path)
            .unwrap()
            .query_row("select rowid, fetched_at from snapshots", [], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap();

        let outcome = cache
            .save_list(
                &vigil,
                PullRequestListFilter::Mine,
                &Snapshot::new(list(&[17]), at(60)),
            )
            .unwrap();

        assert_eq!(outcome, SaveOutcome::Unchanged);
        let after = Connection::open(&path)
            .unwrap()
            .query_row("select rowid, fetched_at from snapshots", [], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap();
        assert_eq!(after, before, "same row, same fetch time");
        assert_eq!(
            cache.pull_request_list(&vigil, PullRequestListFilter::Mine),
            Some(first)
        );
        remove(&path);
    }

    #[test]
    fn unchanged_content_moves_only_its_fetch_time_once_it_lags() {
        let (cache, path) = temp_cache("refresh");
        let vigil = repository("Scott-fo", "vigil");
        cache
            .store_pull_request(&vigil, &Snapshot::new(detail(17), at(3600)))
            .unwrap();
        let confirmed = at(0);

        let outcome = cache
            .save_detail(&vigil, &Snapshot::new(detail(17), confirmed.clone()))
            .unwrap();

        assert_eq!(outcome, SaveOutcome::Refreshed);
        assert_eq!(
            cache.pull_request(&vigil, 17),
            Some(Snapshot::new(detail(17), confirmed))
        );

        let mut changed = detail(17);
        changed.body = "edited".to_string();
        assert_eq!(
            cache
                .save_detail(&vigil, &Snapshot::new(changed.clone(), at(0)))
                .unwrap(),
            SaveOutcome::Stored,
            "changed content is written at once"
        );
        assert_eq!(cache.pull_request(&vigil, 17).unwrap().value, changed);
        remove(&path);
    }

    /// A row stamped under a clock that ran ahead must not block every
    /// later save, nor outlive pruning.
    #[test]
    fn a_row_dated_in_the_future_is_replaced_and_pruned() {
        let (cache, path) = temp_cache("future");
        let vigil = repository("Scott-fo", "vigil");
        let mut skewed = detail(17);
        skewed.body = "from a fast clock".to_string();
        cache
            .save_detail(&vigil, &Snapshot::new(skewed, at(-3600)))
            .unwrap();

        let current = Snapshot::new(detail(17), at(5));
        assert_eq!(
            cache.save_detail(&vigil, &current).unwrap(),
            SaveOutcome::Stored
        );
        assert_eq!(cache.pull_request(&vigil, 17), Some(current));

        cache
            .save_detail(&vigil, &Snapshot::new(detail(18), at(-3 * 86_400)))
            .unwrap();
        let reopened = ForgeCache::open(path.clone()).unwrap();
        assert_eq!(reopened.pull_request(&vigil, 18), None);
        assert!(reopened.pull_request(&vigil, 17).is_some());
        remove(&path);
    }

    #[test]
    fn a_cache_of_another_version_is_emptied_on_open() {
        let (cache, path) = temp_cache("version");
        let vigil = repository("Scott-fo", "vigil");
        cache
            .store_pull_request(&vigil, &Snapshot::new(detail(17), at(10)))
            .unwrap();
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", CACHE_VERSION + 1)
            .unwrap();

        let reopened = ForgeCache::open(path.clone()).expect("reopens");

        assert_eq!(reopened.pull_request(&vigil, 17), None);
        let version: i64 = Connection::open(&path)
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CACHE_VERSION);
        remove(&path);
    }

    #[test]
    fn a_row_that_no_longer_decodes_is_a_miss() {
        let (cache, path) = temp_cache("corrupt");
        let vigil = repository("Scott-fo", "vigil");
        cache
            .store_pull_request(&vigil, &Snapshot::new(detail(17), at(10)))
            .unwrap();
        cache
            .store_pull_request_list(
                &vigil,
                PullRequestListFilter::Mine,
                &Snapshot::new(list(&[17]), at(10)),
            )
            .unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "update snapshots set body = '{\"summary\": 3}', content_hash = 0
                 where kind = 'pull_request'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "update snapshots set body = 'not json', content_hash = 0 where kind = 'list'",
                [],
            )
            .unwrap();

        assert_eq!(cache.pull_request(&vigil, 17), None);
        assert_eq!(
            cache.pull_request_list(&vigil, PullRequestListFilter::Mine),
            None
        );

        cache
            .store_pull_request(&vigil, &Snapshot::new(detail(17), at(5)))
            .unwrap();
        assert!(
            cache.pull_request(&vigil, 17).is_some(),
            "the next save replaces it"
        );
        remove(&path);
    }

    #[test]
    fn opening_prunes_rows_past_retention_by_age_and_by_count() {
        let path = temp_path("retention");
        let retention = Retention {
            max_age: Duration::from_secs(3600),
            max_pull_requests: 2,
        };
        let cache = ForgeCache::open_with(path.clone(), retention).unwrap();
        let vigil = repository("Scott-fo", "vigil");
        let unused = repository("Scott-fo", "old-project");
        cache
            .store_pull_request_list(
                &unused,
                PullRequestListFilter::Mine,
                &Snapshot::new(list(&[1]), at(7200)),
            )
            .unwrap();
        cache
            .store_pull_request_list(
                &vigil,
                PullRequestListFilter::Mine,
                &Snapshot::new(list(&[1]), at(60)),
            )
            .unwrap();
        for (number, age) in [(1, 7200), (2, 300), (3, 200), (4, 100)] {
            cache
                .store_pull_request(&vigil, &Snapshot::new(detail(number), at(age)))
                .unwrap();
        }
        assert_eq!(row_count(&path), 6);

        let reopened = ForgeCache::open_with(path.clone(), retention).unwrap();

        assert_eq!(
            reopened.pull_request_list(&unused, PullRequestListFilter::Mine),
            None,
            "a repository not refreshed within retention drops out"
        );
        assert!(
            reopened
                .pull_request_list(&vigil, PullRequestListFilter::Mine)
                .is_some()
        );
        assert_eq!(reopened.pull_request(&vigil, 1), None, "expired");
        assert_eq!(
            reopened.pull_request(&vigil, 2),
            None,
            "the oldest beyond the cap is evicted"
        );
        assert!(reopened.pull_request(&vigil, 3).is_some());
        assert!(reopened.pull_request(&vigil, 4).is_some());
        assert_eq!(row_count(&path), 3);
        remove(&path);
    }

    #[test]
    fn a_missing_file_is_created_empty() {
        let (cache, path) = temp_cache("fresh");

        assert!(path.exists());
        assert_eq!(
            cache.pull_request_list(&repository("a", "b"), PullRequestListFilter::AllOpen),
            None
        );
        remove(&path);
    }
}
