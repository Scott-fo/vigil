//! Tests for prefetching: a live list page prefetches its top rows'
//! commits and stale details, a saved page or an unavailable `gh` prefetch
//! nothing, unchanged refreshes cost no GitHub request, failures back off,
//! and a prefetched detail is what an open shows at once. Every `git` and
//! `gh` call is recorded, never run; the only file touched is a temporary
//! forge cache.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

use super::{
    PullRequestEvent, fixtures,
    gateway::ForgeCall,
    prefetch::{PREFETCH_ROWS, PrefetchEvent},
    state::{BranchKey, ReviewOrigin},
};
use crate::{
    app::{App, Screen},
    event::Event,
    forge::{
        ForgeCache, ForgeError, GitHub, PullRequest, PullRequestList, PullRequestListFilter,
        RepositoryRef, Snapshot, Timestamp,
    },
    git::{PrefetchOutcome, PrefetchReport, PullRequestFetchError},
};

/// When recorded detail prefetches "started"; saves are dated by it.
const REQUESTED_AT: &str = "2026-09-30T22:00:00Z";

fn repository() -> RepositoryRef {
    RepositoryRef {
        host: "github.com".to_string(),
        owner: "Scott-fo".to_string(),
        name: "vigil".to_string(),
    }
}

fn temp_cache() -> (ForgeCache, PathBuf) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir()
        .join("vigil-prefetch-tests")
        .join(format!(
            "{}-{}.sqlite3",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    (ForgeCache::open(path.clone()).expect("cache opens"), path)
}

fn remove(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

/// An app connected to GitHub with prefetches recorded, the list on screen,
/// and `cache` as its forge cache (none when `None`).
fn recording_app(cache: Option<&ForgeCache>) -> App {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-prefetch-tests"));
    app.pull_requests
        .connect_for_test(GitHub::new(app.repo_root.clone(), repository()));
    app.record_forge_calls_for_test();
    if let Some(cache) = cache {
        app.use_forge_cache_for_test(cache.clone());
    }
    app.screen = Screen::PullRequestList;
    app
}

/// Delivers `page` as the live answer to a fresh load of the shown tab.
fn load_live(app: &mut App, page: PullRequestList) -> PullRequestListFilter {
    let (request_id, filter) = app.pull_requests.list_mut().begin_load(Instant::now());
    assert!(app.handle_pull_request_list_loaded(request_id, filter, Ok(page)));
    filter
}

/// Handles the next background event, which must arrive promptly.
async fn pump(app: &mut App) -> bool {
    let event = tokio::time::timeout(Duration::from_secs(5), app.events.next())
        .await
        .expect("an event arrives")
        .unwrap();
    let Event::PullRequest(event) = event else {
        panic!("a pull request event");
    };
    app.handle_pull_request_event(event).await.unwrap()
}

/// Whether a background event is waiting, giving tasks a moment to run.
async fn event_pending(app: &mut App) -> bool {
    tokio::time::timeout(Duration::from_millis(100), app.events.next())
        .await
        .is_ok()
}

fn prefetch_calls(app: &App) -> Vec<ForgeCall> {
    app.recorded_forge_calls()
        .into_iter()
        .filter(|call| {
            matches!(
                call,
                ForgeCall::PrefetchCommits(_) | ForgeCall::PrefetchDetails(_)
            )
        })
        .collect()
}

fn fetched(numbers: &[u64]) -> PrefetchReport {
    PrefetchReport {
        outcomes: numbers
            .iter()
            .map(|number| (*number, PrefetchOutcome::Fetched))
            .collect(),
        fetches: 1,
    }
}

async fn finish_commits(app: &mut App, numbers: &[u64]) {
    finish_commits_with(app, Ok(fetched(numbers))).await;
}

async fn finish_commits_with(app: &mut App, result: Result<PrefetchReport, PullRequestFetchError>) {
    let request_id = app.pull_requests.prefetch().commit_request_id();
    let event = PullRequestEvent::Prefetch(PrefetchEvent::CommitsFetched { request_id, result });
    assert!(!app.handle_pull_request_event(event).await.unwrap());
}

async fn finish_details(app: &mut App, result: Result<Vec<PullRequest>, ForgeError>) -> bool {
    let request_id = app.pull_requests.prefetch().detail_request_id();
    let event = PullRequestEvent::Prefetch(PrefetchEvent::DetailsLoaded {
        request_id,
        requested_at: Timestamp::new(REQUESTED_AT),
        result,
    });
    app.handle_pull_request_event(event).await.unwrap()
}

/// Polls `read` until it finds something; saves land in the background.
async fn eventually<T>(mut read: impl FnMut() -> Option<T>) -> T {
    for _ in 0..200 {
        if let Some(value) = read() {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("nothing was saved");
}

#[tokio::test]
async fn a_live_page_prefetches_its_top_rows() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    let numbers = (1..=12).collect::<Vec<u64>>();
    let top = numbers[..PREFETCH_ROWS].to_vec();

    load_live(&mut app, fixtures::pull_request_list(&numbers, 12));
    assert_eq!(
        prefetch_calls(&app),
        vec![ForgeCall::PrefetchCommits(top.clone())],
        "commits first; details wait for the cache to name the stale rows"
    );
    assert!(!pump(&mut app).await, "prefetching never redraws");

    assert_eq!(
        prefetch_calls(&app),
        vec![
            ForgeCall::PrefetchCommits(top.clone()),
            ForgeCall::PrefetchDetails(top),
        ]
    );
    app.quit();
    remove(&path);
}

/// An open fetching #4 writes its ref; a prefetch writing the same ref at
/// once could fail it, so the prefetch leaves #4 to the open.
#[tokio::test]
async fn a_pull_request_being_opened_is_left_to_the_open() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    app.pull_requests
        .begin_open(fixtures::summary(4), ReviewOrigin::PullRequestList);
    app.pull_requests.cancel_detail_for_test();

    load_live(&mut app, fixtures::pull_request_list(&[4, 5], 2));
    pump(&mut app).await;

    assert_eq!(
        prefetch_calls(&app),
        vec![
            ForgeCall::PrefetchCommits(vec![5]),
            ForgeCall::PrefetchDetails(vec![5]),
        ]
    );
    app.quit();
    remove(&path);
}

/// An outdated row is looked up by number and then opened, so a running
/// lookup counts as being opened; once it is cancelled, it no longer does.
#[tokio::test]
async fn a_pull_request_being_looked_up_is_left_to_the_open() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    app.pull_requests.begin_lookup(4);

    load_live(&mut app, fixtures::pull_request_list(&[4, 5], 2));
    pump(&mut app).await;
    assert_eq!(
        prefetch_calls(&app),
        vec![
            ForgeCall::PrefetchCommits(vec![5]),
            ForgeCall::PrefetchDetails(vec![5]),
        ]
    );

    let lookup = app.pull_requests.lookup_request_id();
    assert!(app.pull_requests.finish_lookup(lookup));
    assert_eq!(app.pull_requests.looking_up_number(), None);

    // Leaving the list drops a running lookup; its row is fair game again.
    app.pull_requests.begin_lookup(6);
    assert_eq!(app.pull_requests.looking_up_number(), Some(6));
    app.close_pull_request_list();
    assert_eq!(app.pull_requests.looking_up_number(), None);
    app.quit();
    remove(&path);
}

/// Saved details are keyed by the repository's name, so after a rename
/// the ones saved under the old name are not found: the rows' details are
/// prefetched again, while their commits, which a rename does not touch,
/// are not.
#[tokio::test]
async fn a_confirmed_rename_prefetches_details_again_but_not_commits() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    app.pull_requests
        .connect_for_test(GitHub::unconfirmed(app.repo_root.clone(), repository()));
    let page = fixtures::pull_request_list(&[4], 1);
    load_live(&mut app, page.clone());
    pump(&mut app).await;
    finish_commits(&mut app, &[4]).await;
    finish_details(&mut app, Ok(vec![fixtures::pull_request(4, Vec::new())])).await;
    let calls = prefetch_calls(&app).len();

    // Off the list, so the rename reloads nothing through `gh`.
    app.screen = Screen::Review;
    let (request_id, _) = app.pull_requests.begin_confirm().expect("unconfirmed");
    app.handle_pull_request_event(PullRequestEvent::RepositoryConfirmed {
        request_id,
        result: Ok(GitHub::new(
            app.repo_root.clone(),
            RepositoryRef {
                name: "vigil-next".to_string(),
                ..repository()
            },
        )),
    })
    .await
    .unwrap();
    app.screen = Screen::PullRequestList;

    load_live(&mut app, page);
    pump(&mut app).await;
    assert_eq!(
        prefetch_calls(&app)[calls..],
        [ForgeCall::PrefetchDetails(vec![4])]
    );
    app.quit();
    remove(&path);
}

#[tokio::test]
async fn a_saved_page_prefetches_nothing() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    let (_, filter) = app.pull_requests.list_mut().begin_load(Instant::now());

    assert!(app.handle_saved_pull_request_list(
        &repository(),
        filter,
        Some(Snapshot::new(
            fixtures::pull_request_list(&[4, 5], 2),
            Timestamp::new("2026-09-29T15:00:00Z"),
        )),
    ));

    assert!(prefetch_calls(&app).is_empty());
    assert!(!event_pending(&mut app).await);
    app.quit();
    remove(&path);
}

#[tokio::test]
async fn nothing_is_prefetched_while_gh_is_unavailable() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    let (request_id, filter) = app.pull_requests.list_mut().begin_load(Instant::now());
    app.pull_requests
        .mark_unavailable(ForgeError::NotAuthenticated {
            message: "not logged in".to_string(),
        });

    app.handle_pull_request_list_loaded(
        request_id,
        filter,
        Ok(fixtures::pull_request_list(&[4, 5], 2)),
    );

    assert!(prefetch_calls(&app).is_empty());
    assert!(!event_pending(&mut app).await);
    app.quit();
    remove(&path);
}

#[tokio::test]
async fn details_already_saved_for_the_rows_cost_no_github_request() {
    let (cache, path) = temp_cache();
    for number in [4, 5] {
        cache
            .store_pull_request(
                &repository(),
                &Snapshot::new(fixtures::pull_request(number, Vec::new()), Timestamp::now()),
            )
            .unwrap();
    }
    let mut app = recording_app(Some(&cache));

    load_live(&mut app, fixtures::pull_request_list(&[4, 5], 2));
    pump(&mut app).await;

    assert_eq!(
        prefetch_calls(&app),
        vec![ForgeCall::PrefetchCommits(vec![4, 5])]
    );
    app.quit();
    remove(&path);
}

#[tokio::test]
async fn an_unchanged_refresh_prefetches_nothing_and_a_moved_row_only_itself() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    let page = fixtures::pull_request_list(&[4, 5], 2);
    load_live(&mut app, page.clone());
    pump(&mut app).await;
    finish_commits(&mut app, &[4, 5]).await;
    let details = [4, 5]
        .map(|number| fixtures::pull_request(number, Vec::new()))
        .to_vec();
    assert!(!finish_details(&mut app, Ok(details)).await);
    let calls = prefetch_calls(&app).len();

    // The 60-second refresh returns the same rows: no process, no request.
    load_live(&mut app, page.clone());
    assert!(!event_pending(&mut app).await);
    assert_eq!(prefetch_calls(&app).len(), calls);

    // One row gets a comment and a push: only it is prefetched again.
    let mut moved = page;
    moved.pull_requests[1].updated_at = Timestamp::new("2026-09-30T09:00:00Z");
    moved.pull_requests[1].head_oid = "c".repeat(40);
    load_live(&mut app, moved);
    pump(&mut app).await;
    assert_eq!(
        prefetch_calls(&app)[calls..],
        [
            ForgeCall::PrefetchCommits(vec![5]),
            ForgeCall::PrefetchDetails(vec![5]),
        ]
    );
    app.quit();
    remove(&path);
}

/// On a busy repository the base branch advances between refreshes, which
/// moves every row's base commit. That alone must not refetch anything;
/// a CI run finishing, which does not move `updated_at`, refreshes details.
#[tokio::test]
async fn a_moved_base_refetches_nothing_and_finished_checks_only_details() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    let page = fixtures::pull_request_list(&[4, 5], 2);
    load_live(&mut app, page.clone());
    pump(&mut app).await;
    finish_commits(&mut app, &[4, 5]).await;
    let details = [4, 5]
        .map(|number| fixtures::pull_request(number, Vec::new()))
        .to_vec();
    finish_details(&mut app, Ok(details)).await;
    let calls = prefetch_calls(&app).len();

    let mut main_moved = page.clone();
    for row in &mut main_moved.pull_requests {
        row.base_oid = "d".repeat(40);
    }
    load_live(&mut app, main_moved.clone());
    assert!(!event_pending(&mut app).await);
    assert_eq!(prefetch_calls(&app).len(), calls);

    let mut checks_finished = main_moved;
    checks_finished.pull_requests[0].checks = None;
    load_live(&mut app, checks_finished);
    pump(&mut app).await;
    assert_eq!(
        prefetch_calls(&app)[calls..],
        [ForgeCall::PrefetchDetails(vec![4])]
    );
    app.quit();
    remove(&path);
}

/// ssh refusing the key would fail the same way on every retry, and each
/// retry could ask the user's agent again: commit prefetching stops for the
/// session, silently. Details, fetched through `gh`, carry on.
#[tokio::test]
async fn refused_ssh_access_stops_commit_prefetching_for_the_session() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    load_live(&mut app, fixtures::pull_request_list(&[1], 1));
    pump(&mut app).await;
    finish_commits_with(
        &mut app,
        Err(PullRequestFetchError::Access {
            remote: "origin".to_string(),
            stderr: "git@github.com: Permission denied (publickey).".to_string(),
        }),
    )
    .await;
    assert!(app.snackbar_notice.is_none());
    let calls = prefetch_calls(&app).len();

    load_live(&mut app, fixtures::pull_request_list(&[2], 1));
    pump(&mut app).await;

    assert_eq!(
        prefetch_calls(&app)[calls..],
        [ForgeCall::PrefetchDetails(vec![2])]
    );
    app.quit();
    remove(&path);
}

#[tokio::test]
async fn a_rate_limit_pauses_detail_prefetching_but_not_commits() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    load_live(&mut app, fixtures::pull_request_list(&[1, 2], 2));
    pump(&mut app).await;
    finish_commits(&mut app, &[1, 2]).await;

    let limited = ForgeError::RateLimited {
        message: "API rate limit exceeded".to_string(),
    };
    assert!(!finish_details(&mut app, Err(limited.clone())).await);
    assert!(
        app.snackbar_notice.is_none(),
        "prefetch failures are silent"
    );
    assert!(matches!(
        app.pull_requests.connection(),
        super::state::ForgeConnection::Connected(_)
    ));

    load_live(&mut app, fixtures::pull_request_list(&[3], 1));
    assert!(!event_pending(&mut app).await, "no cache check, no request");
    assert_eq!(
        prefetch_calls(&app),
        vec![
            ForgeCall::PrefetchCommits(vec![1, 2]),
            ForgeCall::PrefetchDetails(vec![1, 2]),
            ForgeCall::PrefetchCommits(vec![3]),
        ]
    );
    app.quit();
    remove(&path);
}

/// `gh` logging out mid-session is found by whichever request runs next.
/// A prefetch turns requests off as any request would, but says nothing:
/// the next pull request feature the user asks for explains why.
#[tokio::test]
async fn a_prefetch_that_finds_gh_logged_out_quietly_stops_requests() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    load_live(&mut app, fixtures::pull_request_list(&[1, 2], 2));
    pump(&mut app).await;
    finish_commits(&mut app, &[1, 2]).await;
    let logged_out = ForgeError::NotAuthenticated {
        message: "not logged in".to_string(),
    };

    assert!(!finish_details(&mut app, Err(logged_out.clone())).await);

    assert!(matches!(
        app.pull_requests.connection(),
        super::state::ForgeConnection::Unavailable(error) if *error == logged_out
    ));
    assert!(app.snackbar_notice.is_none());
    let calls = prefetch_calls(&app).len();
    load_live(&mut app, fixtures::pull_request_list(&[3], 1));
    assert!(!event_pending(&mut app).await);
    assert_eq!(prefetch_calls(&app).len(), calls, "nothing more is asked");
    app.quit();
    remove(&path);
}

#[tokio::test]
async fn a_prefetched_detail_shows_at_once_when_the_row_opens() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    load_live(&mut app, fixtures::pull_request_list(&[18], 1));
    pump(&mut app).await;
    let detail = fixtures::pull_request(
        18,
        vec![fixtures::thread(
            "T1",
            "src/lib.rs",
            crate::forge::DiffSide::Right,
            Some(3),
        )],
    );

    assert!(!finish_details(&mut app, Ok(vec![detail.clone()])).await);
    assert!(
        app.pull_requests.open().is_none(),
        "a prefetch changes nothing on screen"
    );
    let saved = eventually(|| cache.pull_request(&repository(), 18)).await;
    assert_eq!(
        saved.fetched_at,
        Timestamp::new(REQUESTED_AT),
        "dated by when the prefetch request started"
    );

    // Opening the row reads the saved detail while the live one loads, so
    // the review shows it as soon as the commits are there.
    let summary = fixtures::summary(18);
    let (fetch_id, _) = app
        .pull_requests
        .begin_open(summary.clone(), ReviewOrigin::PullRequestList);
    app.pull_requests.cancel_detail_for_test();
    app.read_saved_pull_request(18);
    pump(&mut app).await;
    let summary = app.pull_requests.finish_fetch(fetch_id).expect("current");
    app.pull_requests
        .enter(summary.clone(), summary.head_oid.clone());

    let open = app.pull_requests.open().expect("under review");
    assert_eq!(open.detail(), Some(&detail));
    assert!(open.detail_saved_at().is_some());
    app.quit();
    remove(&path);
}

#[tokio::test]
async fn the_current_branch_detail_is_prefetched_when_it_changes() {
    let (cache, path) = temp_cache();
    let mut app = recording_app(Some(&cache));
    app.screen = Screen::Review;
    app.set_branch_snapshot(fixtures::branch_snapshot("feature"));
    let look_up = |app: &mut App, summary| {
        let key = BranchKey {
            branch: "feature".to_string(),
            tip: "f076e8b".to_string(),
        };
        let id = app
            .pull_requests
            .begin_current_branch_load(key, true, Instant::now())
            .expect("lookup starts");
        app.handle_current_branch_pull_request_loaded(id, Ok(Some(summary)));
    };

    look_up(&mut app, fixtures::summary(7));
    pump(&mut app).await;
    assert_eq!(
        prefetch_calls(&app),
        vec![ForgeCall::PrefetchDetails(vec![7])]
    );
    assert!(!finish_details(&mut app, Ok(vec![fixtures::pull_request(7, Vec::new())])).await);

    // The next lookup finds nothing new: no cache check, no request.
    look_up(&mut app, fixtures::summary(7));
    assert!(!event_pending(&mut app).await);

    let mut updated = fixtures::summary(7);
    updated.updated_at = Timestamp::new("2026-09-30T09:00:00Z");
    look_up(&mut app, updated);
    pump(&mut app).await;
    assert_eq!(
        prefetch_calls(&app),
        vec![
            ForgeCall::PrefetchDetails(vec![7]),
            ForgeCall::PrefetchDetails(vec![7]),
        ]
    );
    app.quit();
    remove(&path);
}

#[tokio::test]
async fn tests_that_do_not_record_never_prefetch() {
    let (cache, path) = temp_cache();
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-prefetch-tests"));
    app.pull_requests
        .connect_for_test(GitHub::new(app.repo_root.clone(), repository()));
    app.use_forge_cache_for_test(cache);
    app.screen = Screen::PullRequestList;

    load_live(&mut app, fixtures::pull_request_list(&[4, 5], 2));

    assert!(!app.pull_requests.prefetch().detail_in_flight());
    assert!(app.pull_requests.prefetch().commits_landing(4).is_none());
    app.quit();
    remove(&path);
}
