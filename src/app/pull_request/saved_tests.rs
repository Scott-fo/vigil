//! Tests for saved snapshots: the list and the review show what the forge
//! cache holds until live loads replace it, a late cache read never
//! replaces live data, and nothing is written to GitHub from saved data.
//! Only the round-trip tests at the end use a cache file, a temporary one;
//! nothing here reaches GitHub or the user's cache.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
    MergeBlocker, PullRequestEvent, PullRequestListStatus, PullRequestModalView, fixtures,
    gateway::{ForgeCall, ForgeMutation},
    modal::PullRequestModal,
};
use crate::{
    app::{App, Screen},
    event::Event,
    forge::{
        ForgeCache, ForgeError, GitHub, MergeStateStatus, Mergeability, PullRequest,
        PullRequestList, PullRequestListFilter, RepositoryRef, Snapshot, Timestamp,
    },
};

const SAVED_AT: &str = "2026-09-29T15:00:00Z";

fn repository() -> RepositoryRef {
    RepositoryRef {
        host: "github.com".to_string(),
        owner: "Scott-fo".to_string(),
        name: "vigil".to_string(),
    }
}

fn saved<T>(value: T) -> Snapshot<T> {
    Snapshot::new(value, Timestamp::new(SAVED_AT))
}

/// An app connected to GitHub with the list on screen and the first tab's
/// live load started but not answered. Nothing is spawned.
fn list_app() -> (App, u64, PullRequestListFilter) {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-saved-tests"));
    app.pull_requests
        .connect_for_test(GitHub::new(app.repo_root.clone(), repository()));
    app.screen = Screen::PullRequestList;
    let (request_id, filter) = app.pull_requests.list_mut().begin_load();
    (app, request_id, filter)
}

fn shown_numbers(app: &App) -> Vec<u64> {
    app.pull_request_list_view()
        .rows
        .iter()
        .map(|summary| summary.number)
        .collect()
}

#[test]
fn a_saved_page_shows_at_once_marked_with_its_age() {
    let (mut app, _, filter) = list_app();
    assert_eq!(
        app.pull_request_list_view().status,
        PullRequestListStatus::Loading
    );

    let shown = app.handle_saved_pull_request_list(
        &repository(),
        filter,
        Some(saved(fixtures::pull_request_list(&[4, 5], 2))),
    );

    assert!(shown);
    let view = app.pull_request_list_view();
    assert_eq!(view.status, PullRequestListStatus::Ready);
    assert_eq!(view.saved_at, Some(&Timestamp::new(SAVED_AT)));
    assert!(view.refreshing, "the live load is still running");
    assert_eq!(shown_numbers(&app), vec![4, 5]);
}

#[test]
fn the_live_page_replaces_a_saved_one_and_keeps_the_selection() {
    let (mut app, request_id, filter) = list_app();
    app.handle_saved_pull_request_list(
        &repository(),
        filter,
        Some(saved(fixtures::pull_request_list(&[4, 5], 2))),
    );
    app.pull_requests.list_mut().move_selection(1);

    app.handle_pull_request_list_loaded(
        request_id,
        filter,
        Ok(fixtures::pull_request_list(&[6, 5, 4], 3)),
    );

    let view = app.pull_request_list_view();
    assert_eq!(view.saved_at, None);
    assert!(!view.refreshing);
    assert_eq!(shown_numbers(&app), vec![6, 5, 4]);
    assert_eq!(
        app.pull_requests
            .list()
            .selected_summary()
            .map(|pr| pr.number),
        Some(5)
    );
}

#[test]
fn a_saved_page_that_arrives_after_the_live_one_is_dropped() {
    let (mut app, request_id, filter) = list_app();
    app.handle_pull_request_list_loaded(
        request_id,
        filter,
        Ok(fixtures::pull_request_list(&[6], 1)),
    );

    let shown = app.handle_saved_pull_request_list(
        &repository(),
        filter,
        Some(saved(fixtures::pull_request_list(&[4, 5], 2))),
    );

    assert!(!shown);
    assert_eq!(shown_numbers(&app), vec![6]);
    assert_eq!(app.pull_request_list_view().saved_at, None);
}

#[test]
fn a_failed_live_load_keeps_the_saved_page_and_reports_the_failure() {
    let (mut app, request_id, filter) = list_app();
    app.handle_saved_pull_request_list(
        &repository(),
        filter,
        Some(saved(fixtures::pull_request_list(&[4], 1))),
    );

    app.handle_pull_request_list_loaded(request_id, filter, Err(ForgeError::GhNotInstalled));

    let view = app.pull_request_list_view();
    assert_eq!(shown_numbers(&app), vec![4]);
    assert!(view.saved_at.is_some());
    assert!(view.stale_error.is_some());
}

/// A logged-out `gh` is often found by the first request, which turns the
/// connection off. The list then explains that, as it does over live rows,
/// rather than showing saved rows it cannot refresh; a saved page read
/// after that is dropped.
#[tokio::test]
async fn a_live_load_that_finds_gh_logged_out_explains_it_over_a_saved_page() {
    let (mut app, request_id, filter) = list_app();
    app.handle_saved_pull_request_list(
        &repository(),
        filter,
        Some(saved(fixtures::pull_request_list(&[4], 1))),
    );
    let error = ForgeError::NotAuthenticated {
        message: "not logged in".to_string(),
    };

    app.handle_pull_request_event(PullRequestEvent::ListLoaded {
        request_id,
        filter,
        result: Err(error.clone()),
    })
    .await
    .unwrap();

    let view = app.pull_request_list_view();
    assert_eq!(
        view.status,
        PullRequestListStatus::Unavailable(Some(&error))
    );
    assert_eq!(view.saved_at, None, "no age for rows not drawn");
    assert!(!app.handle_saved_pull_request_list(
        &repository(),
        PullRequestListFilter::AllOpen,
        Some(saved(fixtures::pull_request_list(&[5], 1))),
    ));
    app.quit();
}

#[test]
fn saved_pages_of_another_repository_or_a_miss_show_nothing() {
    let (mut app, _, filter) = list_app();
    let other = RepositoryRef {
        owner: "someone".to_string(),
        ..repository()
    };

    assert!(!app.handle_saved_pull_request_list(
        &other,
        filter,
        Some(saved(fixtures::pull_request_list(&[4], 1))),
    ));
    assert!(!app.handle_saved_pull_request_list(&repository(), filter, None));
    assert_eq!(
        app.pull_request_list_view().status,
        PullRequestListStatus::Loading
    );
}

#[test]
fn a_saved_page_fills_a_tab_that_is_not_shown_without_disturbing_this_one() {
    let (mut app, request_id, filter) = list_app();
    app.handle_pull_request_list_loaded(
        request_id,
        filter,
        Ok(fixtures::pull_request_list(&[6], 1)),
    );

    app.handle_saved_pull_request_list(
        &repository(),
        PullRequestListFilter::AllOpen,
        Some(saved(fixtures::pull_request_list(&[1, 2, 3], 3))),
    );

    assert_eq!(shown_numbers(&app), vec![6]);
    app.pull_requests
        .list_mut()
        .set_filter(PullRequestListFilter::AllOpen);
    assert_eq!(shown_numbers(&app), vec![1, 2, 3]);
    assert!(app.pull_request_list_view().saved_at.is_some());
}

// The review -------------------------------------------------------------

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn mergeable(mut detail: PullRequest) -> PullRequest {
    detail.mergeable = Mergeability::Mergeable;
    detail.merge_state = MergeStateStatus::Clean;
    detail
}

/// A review of #18 showing a saved detail, the live one still loading.
/// Returns the app and the live detail's request id.
fn review_on_saved_detail() -> (App, u64) {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-saved-tests"));
    app.record_forge_calls_for_test();
    let mut saved_detail = mergeable(fixtures::pull_request(18, Vec::new()));
    saved_detail.body = "Saved body.".to_string();
    let detail_id = app.open_pull_request_with_saved_detail_for_test(
        fixtures::summary(18),
        saved(saved_detail),
        vec![fixtures::file("src/app.rs")],
    );
    (app, detail_id)
}

fn mutations(app: &App) -> Vec<ForgeMutation> {
    app.recorded_forge_calls()
        .into_iter()
        .filter_map(|call| match call {
            ForgeCall::Mutation(mutation) => Some(mutation),
            _ => None,
        })
        .collect()
}

fn shown_body(app: &App) -> Option<String> {
    app.pull_request_overview()
        .and_then(|overview| overview.detail.map(|detail| detail.body.clone()))
}

#[test]
fn a_saved_detail_shows_on_open_until_the_live_one_arrives() {
    let (mut app, detail_id) = review_on_saved_detail();

    let overview = app.pull_request_overview().expect("overview shows");
    assert_eq!(overview.saved_at, Some(&Timestamp::new(SAVED_AT)));
    assert!(overview.refreshing);
    assert_eq!(shown_body(&app).as_deref(), Some("Saved body."));

    let mut live = fixtures::pull_request(18, Vec::new());
    live.body = "Live body.".to_string();
    assert!(app.handle_pull_request_detail_loaded(detail_id, Ok(live)));

    let overview = app.pull_request_overview().expect("overview shows");
    assert_eq!(overview.saved_at, None);
    assert!(!overview.refreshing);
    assert_eq!(shown_body(&app).as_deref(), Some("Live body."));
}

#[test]
fn a_saved_detail_read_after_the_live_one_is_dropped() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-saved-tests"));
    let mut live = fixtures::pull_request(18, Vec::new());
    live.body = "Live body.".to_string();
    app.open_pull_request_for_test(live, vec![fixtures::file("src/app.rs")]);

    let mut stale = fixtures::pull_request(18, Vec::new());
    stale.body = "Saved body.".to_string();
    assert!(!app.pull_requests.wants_saved_detail(18));
    assert!(!app.pull_requests.show_saved_detail(18, saved(stale)));

    assert_eq!(shown_body(&app).as_deref(), Some("Live body."));
    assert_eq!(app.pull_request_overview().unwrap().saved_at, None);
}

#[test]
fn a_live_detail_during_the_fetch_wins_over_a_saved_one_read_later() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-saved-tests"));
    let summary = fixtures::summary(18);
    let (_, detail_id) = app
        .pull_requests
        .begin_open(summary.clone(), super::state::ReviewOrigin::Elsewhere);
    let mut live = fixtures::pull_request(18, Vec::new());
    live.body = "Live body.".to_string();
    app.pull_requests.finish_detail(detail_id, Ok(live));
    let mut stale = fixtures::pull_request(18, Vec::new());
    stale.body = "Saved body.".to_string();
    app.pull_requests.show_saved_detail(18, saved(stale));

    app.pull_requests
        .enter(summary.clone(), summary.head_oid.clone());

    let open = app.pull_requests.open().unwrap();
    assert_eq!(
        open.detail().map(|detail| detail.body.as_str()),
        Some("Live body.")
    );
    assert!(open.live_detail().is_some());
}

#[test]
fn a_saved_detail_of_another_head_is_not_shown() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-saved-tests"));
    let mut old = fixtures::pull_request(18, Vec::new());
    old.summary.head_oid = "0".repeat(40);

    app.open_pull_request_with_saved_detail_for_test(
        fixtures::summary(18),
        saved(old),
        vec![fixtures::file("src/app.rs")],
    );

    let overview = app.pull_request_overview().unwrap();
    assert!(overview.detail.is_none(), "its threads would not match");
    assert_eq!(overview.saved_at, None);
}

#[tokio::test]
async fn merging_waits_for_the_live_detail() {
    let (mut app, detail_id) = review_on_saved_detail();

    app.handle_key_event(press(KeyCode::Char('M')))
        .await
        .unwrap();
    let Some(PullRequestModalView::Merge { blockers, .. }) = app.pull_request_modal() else {
        panic!("the form opens on the saved detail");
    };
    assert_eq!(blockers, vec![MergeBlocker::Refreshing]);

    app.handle_key_event(press(KeyCode::Enter)).await.unwrap();
    app.handle_key_event(press(KeyCode::Enter)).await.unwrap();
    assert!(mutations(&app).is_empty(), "nothing is sent on saved data");
    let Some(PullRequestModal::Merge(form)) = app.pull_requests.modal() else {
        panic!("the form stays open");
    };
    assert_eq!(
        form.error(),
        Some(MergeBlocker::Refreshing.to_string().as_str())
    );

    app.handle_pull_request_detail_loaded(
        detail_id,
        Ok(mergeable(fixtures::pull_request(18, Vec::new()))),
    );
    let Some(PullRequestModalView::Merge { blockers, .. }) = app.pull_request_modal() else {
        panic!("the form is still open");
    };
    assert!(blockers.is_empty());
    app.handle_key_event(press(KeyCode::Enter)).await.unwrap();
    app.handle_key_event(press(KeyCode::Enter)).await.unwrap();
    assert!(matches!(
        mutations(&app).as_slice(),
        [ForgeMutation::Merge { number: 18, .. }]
    ));
}

#[tokio::test]
async fn a_failed_refresh_leaves_saved_details_unable_to_merge() {
    let (mut app, detail_id) = review_on_saved_detail();
    app.handle_pull_request_detail_loaded(detail_id, Err(ForgeError::GhNotInstalled));

    app.handle_key_event(press(KeyCode::Char('M')))
        .await
        .unwrap();
    app.handle_key_event(press(KeyCode::Enter)).await.unwrap();

    assert!(mutations(&app).is_empty());
    let Some(PullRequestModalView::Merge { blockers, .. }) = app.pull_request_modal() else {
        panic!("the form is open");
    };
    assert_eq!(blockers, vec![MergeBlocker::Unconfirmed]);
}

#[tokio::test]
async fn state_changes_wait_for_the_live_detail() {
    let (mut app, _) = review_on_saved_detail();

    app.handle_key_event(press(KeyCode::Char('A')))
        .await
        .unwrap();

    assert!(app.pull_requests.modal().is_none());
    let notice = app
        .snackbar_notice
        .as_ref()
        .map(|notice| notice.message.clone());
    assert_eq!(notice, Some(MergeBlocker::Refreshing.to_string()));
}

#[tokio::test]
async fn a_live_detail_with_a_newer_head_says_the_review_is_behind() {
    let (mut app, detail_id) = review_on_saved_detail();
    let mut live = fixtures::pull_request(18, Vec::new());
    live.summary.head_oid = "1".repeat(40);

    app.handle_pull_request_detail_loaded(detail_id, Ok(live));

    assert!(app.pull_request_has_newer_head());
    let notice = app
        .snackbar_notice
        .as_ref()
        .map(|notice| notice.message.clone());
    assert_eq!(
        notice.as_deref(),
        Some("pull request #18 has new commits · r to reload")
    );
}

// Round trips through a cache file -----------------------------------------

fn temp_cache() -> (ForgeCache, PathBuf) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join("vigil-saved-tests").join(format!(
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
async fn live_results_are_saved_and_the_next_session_shows_them() {
    let (cache, path) = temp_cache();
    let page = fixtures::pull_request_list(&[4, 5], 2);
    let detail = fixtures::pull_request(18, Vec::new());
    {
        let (mut app, request_id, filter) = list_app();
        app.use_forge_cache_for_test(cache.clone());
        app.handle_pull_request_list_loaded(request_id, filter, Ok(page.clone()));
        let (_, detail_id) = app.pull_requests.begin_open(
            detail.summary.clone(),
            super::state::ReviewOrigin::Elsewhere,
        );
        app.handle_pull_request_detail_loaded(detail_id, Ok(detail.clone()));

        let saved_page = eventually(|| cache.pull_request_list(&repository(), filter)).await;
        assert_eq!(saved_page.value, page);
        let saved_detail = eventually(|| cache.pull_request(&repository(), 18)).await;
        assert_eq!(saved_detail.value, detail);
    }

    let (mut app, _, filter) = list_app();
    app.use_forge_cache_for_test(cache);
    app.read_saved_pull_request_list(filter);
    let event = tokio::time::timeout(Duration::from_secs(5), app.events.next())
        .await
        .expect("the read answers")
        .unwrap();
    let Event::PullRequest(event) = event else {
        panic!("a pull request event");
    };
    assert!(app.handle_pull_request_event(event).await.unwrap());

    assert_eq!(shown_numbers(&app), vec![4, 5]);
    assert!(app.pull_request_list_view().saved_at.is_some());
    remove(&path);
}

#[test]
fn saves_are_dropped_without_a_cache() {
    let (mut app, request_id, filter) = list_app();

    // No runtime here: a queued save would panic spawning its task.
    app.handle_pull_request_list_loaded(
        request_id,
        filter,
        Ok(PullRequestList {
            pull_requests: Vec::new(),
            total_count: 0,
        }),
    );
    app.save_pull_request(fixtures::pull_request(18, Vec::new()));
}
