use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
    PullRequestEvent, PullRequestListStatus, fixtures,
    gateway::ForgeCall,
    list::{PullRequestListState, QueryInput, RowCurrency},
    state::{
        BranchKey, ConnectOutcome, ConnectReason, ForgeConnection, PollOutcome, PullRequestPage,
        PullRequests, ReviewOrigin,
    },
};
use crate::{
    app::{ActivePane, App, DiffViewMode, ReviewMode, Screen},
    event::Event,
    forge::{
        DiffSide, ForgeError, GitHub, PullRequestList, PullRequestListFilter, PullRequestSummary,
        RepositoryRef, Snapshot, Timestamp,
    },
    git::{self, FetchedPullRequest},
    sidebar::SidebarItem,
};

fn connected() -> PullRequests {
    let mut state = PullRequests::default();
    let id = state
        .begin_connect(ConnectReason::Background)
        .expect("idle state connects");
    state.finish_connect(id, Ok(github()));
    state
}

fn github() -> GitHub {
    GitHub::new(
        "/tmp/vigil-pr-tests",
        RepositoryRef {
            host: "github.com".to_string(),
            owner: "Scott-fo".to_string(),
            name: "vigil".to_string(),
        },
    )
}

fn key(branch: &str) -> BranchKey {
    BranchKey {
        branch: branch.to_string(),
        tip: "abc1234".to_string(),
    }
}

fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn background_connection_failures_stay_quiet_until_the_user_asks() {
    let mut state = PullRequests::default();
    let id = state.begin_connect(ConnectReason::Background).unwrap();
    let outcome = state.finish_connect(id, Err(ForgeError::GhNotInstalled));
    assert!(matches!(
        outcome,
        Some(ConnectOutcome::Failed { report: None })
    ));

    // Nothing retries in the background once GitHub is known unavailable.
    assert_eq!(state.begin_connect(ConnectReason::Background), None);

    let id = state.begin_connect(ConnectReason::UserRequest).unwrap();
    let outcome = state.finish_connect(id, Err(ForgeError::GhNotInstalled));
    assert!(matches!(
        outcome,
        Some(ConnectOutcome::Failed {
            report: Some(ForgeError::GhNotInstalled)
        })
    ));
}

#[test]
fn a_user_request_during_a_background_connect_reports_its_failure() {
    let mut state = PullRequests::default();
    let id = state.begin_connect(ConnectReason::Background).unwrap();
    assert_eq!(state.begin_connect(ConnectReason::UserRequest), None);

    let outcome = state.finish_connect(id, Err(ForgeError::GhNotInstalled));
    assert!(matches!(
        outcome,
        Some(ConnectOutcome::Failed { report: Some(_) })
    ));
}

#[test]
fn disabled_state_never_connects() {
    let mut state = PullRequests::disabled();
    assert_eq!(state.begin_connect(ConnectReason::UserRequest), None);
    assert!(state.github().is_none());
}

#[test]
fn current_branch_pull_request_belongs_to_its_branch() {
    let mut state = connected();
    let now = Instant::now();
    let id = state
        .begin_current_branch_load(key("feature"), false, now)
        .unwrap();
    assert!(state.finish_current_branch_load(id, &Ok(Some(fixtures::summary(17))), now));

    assert_eq!(
        state.current_branch_summary("feature").map(|pr| pr.number),
        Some(17)
    );
    assert!(state.current_branch_summary("main").is_none());
}

#[test]
fn current_branch_lookups_are_throttled_per_branch_tip() {
    let mut state = connected();
    let now = Instant::now();
    let id = state
        .begin_current_branch_load(key("feature"), false, now)
        .unwrap();
    // The same lookup is not started twice while it runs.
    assert_eq!(
        state.begin_current_branch_load(key("feature"), false, now),
        None
    );
    state.finish_current_branch_load(id, &Ok(None), now);

    let soon = now + Duration::from_secs(2);
    assert_eq!(
        state.begin_current_branch_load(key("feature"), false, soon),
        None
    );
    assert!(
        state
            .begin_current_branch_load(key("feature"), true, soon)
            .is_some(),
        "an explicit request always looks up"
    );
}

#[test]
fn stale_current_branch_responses_are_dropped() {
    let mut state = connected();
    let now = Instant::now();
    let stale = state
        .begin_current_branch_load(key("feature"), false, now)
        .unwrap();
    let latest = state
        .begin_current_branch_load(key("other"), false, now)
        .unwrap();

    assert!(!state.finish_current_branch_load(stale, &Ok(Some(fixtures::summary(1))), now));
    assert!(state.finish_current_branch_load(latest, &Ok(Some(fixtures::summary(2))), now));
    assert!(state.current_branch_summary("feature").is_none());
    assert_eq!(
        state.current_branch_summary("other").map(|pr| pr.number),
        Some(2)
    );
}

#[test]
fn detail_that_arrives_before_the_fetch_applies_when_the_review_opens() {
    let mut state = connected();
    let (fetch_id, detail_id) = state.begin_open(fixtures::summary(17), ReviewOrigin::Elsewhere);
    let threads = vec![fixtures::thread(
        "T1",
        "src/lib.rs",
        DiffSide::Right,
        Some(3),
    )];
    assert!(
        state
            .finish_detail(detail_id, Ok(fixtures::pull_request(17, threads)))
            .is_some()
    );
    assert!(state.open().is_none(), "nothing opens before the fetch");

    let summary = state.finish_fetch(fetch_id).expect("fetch is current");
    state.enter(summary, "head".to_string());

    let open = state.open().expect("review is open");
    assert!(open.detail().is_some());
    assert!(open.threads().has_inline_threads("src/lib.rs"));
    assert_eq!(state.page(), Some(PullRequestPage::Overview));
}

#[test]
fn stale_fetches_and_details_are_dropped() {
    let mut state = connected();
    let (old_fetch, old_detail) = state.begin_open(fixtures::summary(1), ReviewOrigin::Elsewhere);
    let (new_fetch, _) = state.begin_open(fixtures::summary(2), ReviewOrigin::Elsewhere);

    assert!(state.finish_fetch(old_fetch).is_none());
    assert!(
        state
            .finish_detail(old_detail, Ok(fixtures::pull_request(1, Vec::new())))
            .is_none()
    );
    assert_eq!(state.finish_fetch(new_fetch).map(|pr| pr.number), Some(2));
}

#[test]
fn reloading_the_open_pull_request_keeps_its_page_and_detail() {
    let mut state = connected();
    let (fetch_id, detail_id) = state.begin_open(fixtures::summary(17), ReviewOrigin::Elsewhere);
    state.finish_detail(detail_id, Ok(fixtures::pull_request(17, Vec::new())));
    let summary = state.finish_fetch(fetch_id).unwrap();
    state.enter(summary, "head-1".to_string());
    state.set_page(PullRequestPage::Files);

    let (fetch_id, _) = state.begin_open(fixtures::summary(17), ReviewOrigin::Elsewhere);
    let summary = state.finish_fetch(fetch_id).unwrap();
    state.enter(summary, "head-2".to_string());

    assert_eq!(state.page(), Some(PullRequestPage::Files));
    assert!(state.open().unwrap().detail().is_some());
}

#[test]
fn polling_reports_new_commits_once_and_activity_separately() {
    let mut state = connected();
    let summary = fixtures::summary(17);
    let reviewed_head = summary.head_oid.clone();
    state.enter(summary.clone(), reviewed_head);

    let (id, number) = state.begin_poll().unwrap();
    assert_eq!(number, 17);
    assert_eq!(state.begin_poll(), None, "one poll at a time");
    assert_eq!(
        state.finish_poll(id, Ok(summary.clone())),
        Some(PollOutcome::Unchanged)
    );

    let mut commented = summary.clone();
    commented.updated_at = Timestamp::new("2026-09-29T17:00:00Z");
    let (id, _) = state.begin_poll().unwrap();
    assert_eq!(
        state.finish_poll(id, Ok(commented.clone())),
        Some(PollOutcome::Updated)
    );

    let mut pushed = commented;
    pushed.head_oid = "newhead".to_string();
    let (id, _) = state.begin_poll().unwrap();
    assert_eq!(
        state.finish_poll(id, Ok(pushed.clone())),
        Some(PollOutcome::HeadMoved { first_notice: true })
    );
    assert_eq!(state.open().unwrap().newer_head(), Some("newhead"));
    let (id, _) = state.begin_poll().unwrap();
    assert_eq!(
        state.finish_poll(id, Ok(pushed)),
        Some(PollOutcome::HeadMoved {
            first_notice: false
        })
    );
}

#[test]
fn polling_reloads_detail_while_github_computes_mergeability() {
    let mut state = connected();
    let (fetch_id, detail_id) = state.begin_open(fixtures::summary(17), ReviewOrigin::Elsewhere);
    // The fixture reports mergeability as unknown.
    state.finish_detail(detail_id, Ok(fixtures::pull_request(17, Vec::new())));
    let summary = state.finish_fetch(fetch_id).unwrap();
    let head = summary.head_oid.clone();
    state.enter(summary.clone(), head);

    let (id, _) = state.begin_poll().unwrap();
    assert_eq!(
        state.finish_poll(id, Ok(summary)),
        Some(PollOutcome::Updated)
    );
}

#[test]
fn closing_ends_the_review_and_drops_late_details() {
    let mut state = connected();
    state.enter(fixtures::summary(17), "head".to_string());
    let (id, _) = state.begin_detail_reload().unwrap();
    state.close();

    assert!(state.open().is_none());
    assert!(
        state
            .finish_detail(id, Ok(fixtures::pull_request(17, Vec::new())))
            .is_none()
    );
}

#[test]
fn the_overview_row_leads_the_sidebar_and_switches_pages() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));
    app.open_pull_request_for_test(
        fixtures::pull_request(17, Vec::new()),
        vec![fixtures::file("src/lib.rs"), fixtures::file("src/main.rs")],
    );

    assert_eq!(app.sidebar_items.first(), Some(&SidebarItem::Overview));
    assert_eq!(app.selected_sidebar_row, 0);
    assert!(app.pull_request_overview_visible());
    assert!(app.pull_request_overview().is_some());
}

#[tokio::test]
async fn sidebar_navigation_moves_between_the_overview_and_files() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));
    app.open_pull_request_for_test(
        fixtures::pull_request(17, Vec::new()),
        vec![fixtures::file("src/lib.rs"), fixtures::file("src/main.rs")],
    );
    app.active_pane = ActivePane::Sidebar;

    // Row 1 is the `src/` directory; like any directory row it leaves the
    // page as it was.
    app.handle_key_event(press(KeyCode::Char('j'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(app.sidebar_items[app.selected_sidebar_row].is_directory());
    assert!(app.pull_request_overview_visible());
    app.handle_key_event(press(KeyCode::Char('j'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(!app.pull_request_overview_visible());
    assert!(app.sidebar_items[app.selected_sidebar_row].file().is_some());

    app.handle_key_event(press(KeyCode::Home, KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(app.pull_request_overview_visible());

    // Keys that act on a file do nothing on the overview.
    app.handle_key_event(press(KeyCode::Char('x'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(app.viewed_file_count(), 0);

    app.handle_key_event(press(KeyCode::Char(']'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(!app.pull_request_overview_visible());
    app.cancel_inflight_diff_load();
    app.abort_background_tasks();
}

/// Regression: Enter on the overview row used to move focus onto the
/// overview page with nothing on screen showing it, so the sidebar seemed to
/// stop responding until a file was clicked.
#[tokio::test]
async fn the_overview_never_takes_focus_from_the_sidebar() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));
    app.open_pull_request_for_test(
        fixtures::pull_request(17, Vec::new()),
        vec![fixtures::file("src/lib.rs")],
    );
    app.active_pane = ActivePane::Sidebar;
    assert!(app.pull_request_overview_visible());

    for code in [KeyCode::Enter, KeyCode::Char('o'), KeyCode::Tab] {
        app.handle_key_event(press(code, KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(app.active_pane, ActivePane::Sidebar, "{code:?}");
        assert!(app.pull_request_overview_visible(), "{code:?}");
    }

    // Ctrl-d/Ctrl-u scroll the page from the sidebar, as they scroll a diff.
    app.handle_key_event(press(KeyCode::Char('d'), KeyModifiers::CONTROL))
        .await
        .unwrap();
    assert_eq!(app.pull_requests.open().unwrap().overview_scroll(), 12);
    app.handle_key_event(press(KeyCode::Char('u'), KeyModifiers::CONTROL))
        .await
        .unwrap();
    assert_eq!(app.pull_requests.open().unwrap().overview_scroll(), 0);

    // Movement keys keep driving the sidebar.
    app.handle_key_event(press(KeyCode::Char('j'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert_ne!(app.selected_sidebar_row, 0);

    // With the sidebar hidden the page is all there is, so it takes the keys.
    app.handle_key_event(press(KeyCode::Home, KeyModifiers::NONE))
        .await
        .unwrap();
    app.toggle_sidebar_hidden();
    app.handle_key_event(press(KeyCode::Char('j'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(app.pull_requests.open().unwrap().overview_scroll(), 1);
    app.toggle_sidebar_hidden();
    assert_eq!(app.active_pane, ActivePane::Sidebar);

    app.cancel_inflight_diff_load();
    app.abort_background_tasks();
}

#[tokio::test]
async fn the_branch_in_view_follows_the_screen() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));
    assert_eq!(app.pull_request_branch_in_view(), None);

    app.open_pull_request_for_test(
        fixtures::pull_request(17, Vec::new()),
        vec![fixtures::file("src/lib.rs")],
    );
    let reviewed = app.pull_request_selection().unwrap().head_ref_name.clone();
    assert_eq!(app.pull_request_branch_in_view(), Some(reviewed));

    app.cancel_inflight_diff_load();
    app.abort_background_tasks();
}

#[tokio::test]
async fn k_checks_out_the_reviewed_head_as_a_branch() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));
    app.open_pull_request_for_test(
        fixtures::pull_request(17, Vec::new()),
        vec![fixtures::file("src/lib.rs")],
    );
    let selection = app.pull_request_selection().unwrap().clone();
    app.handle_key_event(press(KeyCode::Char('K'), KeyModifiers::SHIFT))
        .await
        .unwrap();
    assert_eq!(
        app.branch_operation(),
        Some(&git::BranchOperation::CheckoutPullRequest(
            git::PullRequestCheckout {
                number: 17,
                branch: selection.head_ref_name.clone(),
                head_oid: selection.head_oid.clone(),
                upstream: Some(git::RemoteBranch {
                    remote: "origin".to_string(),
                    branch: selection.head_ref_name.clone(),
                }),
            }
        ))
    );

    app.cancel_inflight_diff_load();
    app.abort_background_tasks();
}

#[test]
fn a_fork_head_checks_out_under_its_number_without_an_upstream() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));
    let mut detail = fixtures::pull_request(17, Vec::new());
    detail.summary.is_cross_repository = true;
    detail.summary.head_ref_name = "main".to_string();
    app.open_pull_request_for_test(detail, vec![fixtures::file("src/lib.rs")]);

    let checkout = app.pull_request_selection().unwrap().checkout();
    assert_eq!(checkout.branch, "pr-17");
    assert_eq!(checkout.upstream, None);
    app.abort_background_tasks();
}

#[test]
fn inline_threads_add_rows_to_the_diff_viewport() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));
    app.open_pull_request_for_test(
        fixtures::pull_request(
            17,
            vec![fixtures::thread(
                "T1",
                "src/app.rs",
                DiffSide::Right,
                Some(2),
            )],
        ),
        vec![fixtures::file("src/app.rs")],
    );
    app.show_pull_request_files();
    app.diff_view = git::build_diff_view_from_diff_text(
        "diff --git a/src/app.rs b/src/app.rs\n\
         --- a/src/app.rs\n\
         +++ b/src/app.rs\n\
         @@ -1,0 +1,3 @@\n\
         +fn one() {}\n\
         +fn two() {}\n\
         +fn three() {}\n",
        Some("rust"),
    );

    let raw = app
        .diff_view
        .rendered_lines(DiffViewMode::Unified, 80, app.diff_line_wrap_mode)
        .len();
    let viewport = app
        .prepare_diff_viewport(DiffViewMode::Unified, 80, 40)
        .expect("viewport");
    let thread_rows = (0..raw)
        .map(|index| {
            app.review_thread_rows_at(DiffViewMode::Unified, 80, index)
                .len()
        })
        .sum::<usize>();

    assert!(thread_rows > 0, "the thread anchors to new line 2");
    assert_eq!(viewport.rendered_line_count, raw + thread_rows);
    assert!(viewport.visible_display_indices.contains(&None));

    // Scrolling reaches the last row, past the thread.
    app.diff_scroll = u16::MAX;
    let viewport = app
        .prepare_diff_viewport(DiffViewMode::Unified, 80, 3)
        .expect("viewport");
    assert_eq!(
        app.diff_scroll as usize,
        viewport.rendered_line_count.saturating_sub(3)
    );
}

struct TempRepo {
    root: PathBuf,
}

impl TempRepo {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("vigil-pr-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        let repo = Self { root };
        repo.run(&["init", "--quiet", "--initial-branch=main"]);
        repo.run(&["config", "user.name", "Vigil Tests"]);
        repo.run(&["config", "user.email", "vigil-tests@example.com"]);
        repo.run(&["config", "commit.gpgsign", "false"]);
        repo
    }

    fn write(&self, path: &str, content: &str) {
        std::fs::write(self.root.join(path), content).unwrap();
    }

    fn run(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn commit(&self, message: &str) -> String {
        self.run(&["add", "-A"]);
        self.run(&["commit", "--quiet", "-m", message]);
        self.run(&["rev-parse", "HEAD"])
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn opening_a_fetched_pull_request_reviews_it_and_ctrl_l_leaves() {
    let repo = TempRepo::new("open");
    repo.write("src/lib.rs", "fn a() {}\n");
    let base = repo.commit("base");
    repo.run(&["switch", "--quiet", "-c", "feature"]);
    repo.write("src/lib.rs", "fn a() {}\nfn b() {}\n");
    repo.write("src/new.rs", "fn new() {}\n");
    let head = repo.commit("feature");
    repo.run(&["switch", "--quiet", "main"]);

    let mut app = App::new_for_benchmarks(repo.path().to_path_buf());
    let mut summary = fixtures::summary(7);
    summary.head_oid = head.clone();
    summary.base_oid = base.clone();
    let fetched = FetchedPullRequest {
        remote: "origin".to_string(),
        head_ref: git::pull_request_head_ref(7),
        head_oid: head.clone(),
        base_oid: base,
    };
    app.enter_pull_request_review(summary, fetched)
        .await
        .unwrap();

    let ReviewMode::PullRequest(selection) = &app.review_mode else {
        panic!("review should show the pull request");
    };
    assert_eq!(selection.number, 7);
    assert_eq!(selection.compare.source_ref, head);
    assert_eq!(app.sidebar_items.first(), Some(&SidebarItem::Overview));
    assert!(app.pull_request_overview_visible());
    let paths = app
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, vec!["src/lib.rs", "src/new.rs"]);

    // Branch merge never applies to a pull request.
    app.handle_key_event(press(KeyCode::Char('m'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(app.branch_merge_target.is_none());

    app.handle_key_event(press(KeyCode::Char('l'), KeyModifiers::CONTROL))
        .await
        .unwrap();
    assert!(matches!(app.review_mode, ReviewMode::WorkingTree));
    assert!(app.pull_requests.open().is_none());
    assert!(!app.sidebar_items.iter().any(SidebarItem::is_overview));
    assert!(!app.pull_request_overview_visible());

    app.quit();
}

/// A repository with `main` at a base commit and `feature` one commit
/// ahead, plus an `origin` that does not exist, so any fetch fails fast
/// without touching the network. Returns the repository, base, and head.
fn repo_with_local_pull_request(name: &str) -> (TempRepo, String, String) {
    let repo = TempRepo::new(name);
    repo.write("src/lib.rs", "fn a() {}\n");
    let base = repo.commit("base");
    repo.run(&["switch", "--quiet", "-c", "feature"]);
    repo.write("src/lib.rs", "fn a() {}\nfn b() {}\n");
    let head = repo.commit("feature");
    repo.run(&["switch", "--quiet", "main"]);
    repo.run(&["remote", "add", "origin", "/nonexistent/vigil-origin.git"]);
    (repo, base, head)
}

/// An app connected to GitHub for `repo` without asking it.
fn connected_app(repo: &TempRepo) -> App {
    let mut app = App::new_for_benchmarks(repo.path().to_path_buf());
    app.pull_requests.connect_for_test(GitHub::new(
        repo.path(),
        RepositoryRef {
            host: "github.com".to_string(),
            owner: "Scott-fo".to_string(),
            name: "vigil".to_string(),
        },
    ));
    app
}

/// Opens `summary` and waits for its commits, answering the fetch events,
/// and returns what happened in order. The detail load is dropped before it
/// runs: tests never reach GitHub.
async fn open_and_wait_for_commits(
    app: &mut App,
    summary: PullRequestSummary,
) -> Vec<&'static str> {
    app.open_pull_request(summary, ReviewOrigin::PullRequestList);
    wait_for_commits(app).await
}

/// Answers the events of an open already started until its commits are
/// ready or failed, and returns what happened in order. The detail load is
/// dropped before it runs.
async fn wait_for_commits(app: &mut App) -> Vec<&'static str> {
    app.pull_requests.cancel_detail_for_test();
    let mut seen = Vec::new();
    loop {
        let Event::PullRequest(event) = app.events.next().await.unwrap() else {
            continue;
        };
        let step = match &event {
            PullRequestEvent::FetchStarted { .. } => "fetch started",
            PullRequestEvent::Fetched { result: Ok(_), .. } => "commits ready",
            PullRequestEvent::Fetched { result: Err(_), .. } => "commits failed",
            _ => "other",
        };
        seen.push(step);
        app.handle_pull_request_event(event).await.unwrap();
        if step.starts_with("commits") {
            return seen;
        }
    }
}

/// A saved list page may name a head GitHub has since moved past, whose
/// commits are likely still local. Opening its row looks the pull request
/// up and reviews the live head; a live page's row opens at once.
#[tokio::test]
async fn a_row_from_a_saved_page_is_looked_up_and_opens_the_live_head() {
    let (repo, base, head) = repo_with_local_pull_request("open-saved-row");
    let mut live = fixtures::summary(7);
    live.head_oid = head.clone();
    live.base_oid = base.clone();
    let mut outdated = live.clone();
    outdated.head_oid = base.clone();
    let page = |summary: &PullRequestSummary| PullRequestList {
        pull_requests: vec![summary.clone()],
        total_count: 1,
    };

    let mut app = connected_app(&repo);
    app.record_forge_calls_for_test();
    app.screen = Screen::PullRequestList;
    let list = app.pull_requests.list_mut();
    let (_, filter) = list.begin_load(Instant::now());
    list.show_saved(
        filter,
        Snapshot::new(page(&outdated), Timestamp::new("2026-09-29T15:00:00Z")),
    );
    app.open_selected_pull_request();
    assert_eq!(app.recorded_forge_calls(), vec![ForgeCall::LookUp(7)]);
    assert_eq!(
        app.pull_requests.opening_number(),
        None,
        "nothing opens yet"
    );

    let lookup = app.pull_requests.lookup_request_id();
    app.handle_pull_request_looked_up(lookup, Ok(live.clone()));
    let seen = wait_for_commits(&mut app).await;
    assert_eq!(seen, vec!["commits ready"]);
    let ReviewMode::PullRequest(selection) = &app.review_mode else {
        panic!("review should show the pull request: {seen:?}");
    };
    assert_eq!(selection.head_oid, head, "the live head, not the saved one");

    let mut app = connected_app(&repo);
    app.record_forge_calls_for_test();
    app.screen = Screen::PullRequestList;
    let list = app.pull_requests.list_mut();
    let (id, filter) = list.begin_load(Instant::now());
    list.finish_load(id, filter, Ok(page(&live)));
    app.open_selected_pull_request();
    let seen = wait_for_commits(&mut app).await;
    assert_eq!(seen, vec!["commits ready"]);
    assert!(app.recorded_forge_calls().is_empty(), "no lookup");
}

/// An app on the list screen whose selected row, #7, is outdated.
fn app_with_outdated_row() -> App {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));
    app.record_forge_calls_for_test();
    app.pull_requests.connect_for_test(github());
    app.screen = Screen::PullRequestList;
    let list = app.pull_requests.list_mut();
    let (_, filter) = list.begin_load(Instant::now());
    list.show_saved(
        filter,
        Snapshot::new(
            fixtures::pull_request_list(&[7, 8], 2),
            Timestamp::new("2026-09-29T15:00:00Z"),
        ),
    );
    app
}

/// Enter on outdated #7 starts a lookup. Opening #8 before it answers, or
/// leaving the list, means #7 must not open when it does.
#[test]
fn a_later_open_or_leaving_the_list_drops_a_running_lookup() {
    let mut app = app_with_outdated_row();
    app.open_selected_pull_request();
    assert_eq!(app.recorded_forge_calls(), vec![ForgeCall::LookUp(7)]);
    let lookup = app.pull_requests.lookup_request_id();
    app.pull_requests
        .begin_open(fixtures::summary(8), ReviewOrigin::PullRequestList);
    assert!(!app.handle_pull_request_looked_up(lookup, Ok(fixtures::summary(7))));
    assert_eq!(app.pull_requests.opening_number(), Some(8));

    let mut app = app_with_outdated_row();
    app.open_selected_pull_request();
    let lookup = app.pull_requests.lookup_request_id();
    app.handle_pull_request_list_key(press(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.screen, Screen::Review);
    assert!(!app.handle_pull_request_looked_up(lookup, Ok(fixtures::summary(7))));
    assert_eq!(app.pull_requests.opening_number(), None);
    assert_ne!(
        app.status_message.as_deref(),
        Some("looking up pull request #7…"),
        "the dropped lookup's status goes with it"
    );
}

/// A list with `row` on its first tab, loaded by a request that started
/// `age` ago.
fn state_with_live_row(row: PullRequestSummary, age: Duration) -> PullRequests {
    let mut state = connected();
    let list = state.list_mut();
    let (id, filter) = list.begin_load(Instant::now().checked_sub(age).unwrap());
    list.finish_load(
        id,
        filter,
        Ok(PullRequestList {
            pull_requests: vec![row],
            total_count: 1,
        }),
    );
    state
}

fn selected_currency(state: &PullRequests) -> RowCurrency {
    state.selected_list_row(Instant::now()).unwrap().1
}

#[test]
fn list_rows_are_current_only_within_a_list_refresh_and_after_a_good_load() {
    let row = fixtures::summary(7);
    let state = state_with_live_row(row.clone(), Duration::ZERO);
    assert_eq!(selected_currency(&state), RowCurrency::Current);

    let state = state_with_live_row(row.clone(), Duration::from_secs(61));
    assert_eq!(
        selected_currency(&state),
        RowCurrency::Outdated,
        "older than a list refresh, as after the list was left and reopened"
    );

    let mut state = state_with_live_row(row, Duration::ZERO);
    let list = state.list_mut();
    let (id, filter) = list.begin_load(Instant::now());
    list.finish_load(id, filter, Err(ForgeError::GhNotInstalled));
    assert_eq!(
        selected_currency(&state),
        RowCurrency::Outdated,
        "a failed reload leaves the rows unconfirmed"
    );
}

/// Open #7 at H1, `r` fetches H2, Esc back to the list and Enter before
/// the list reloads: the row still names H1, which is local.
#[test]
fn a_list_row_behind_this_sessions_review_is_outdated() {
    let row = fixtures::summary(7);
    let mut state = state_with_live_row(row.clone(), Duration::ZERO);
    let (fetch_id, _) = state.begin_open(row.clone(), ReviewOrigin::PullRequestList);
    let summary = state.finish_fetch(fetch_id).unwrap();
    state.enter(summary, "2".repeat(40));
    assert_eq!(
        selected_currency(&state),
        RowCurrency::Outdated,
        "while open"
    );

    state.close();
    assert_eq!(
        selected_currency(&state),
        RowCurrency::Outdated,
        "after the review ended"
    );

    let mut newer = row;
    newer.head_oid = "3".repeat(40);
    newer.updated_at = Timestamp::new("2026-09-30T09:00:00Z");
    let mut state = state_with_live_row(newer, Duration::ZERO);
    let (fetch_id, _) = state.begin_open(fixtures::summary(7), ReviewOrigin::PullRequestList);
    let summary = state.finish_fetch(fetch_id).unwrap();
    state.enter(summary, "2".repeat(40));
    state.close();
    assert_eq!(
        selected_currency(&state),
        RowCurrency::Current,
        "a row updated after the review's summary is newer than it"
    );
}

#[tokio::test]
async fn opening_a_pull_request_with_local_commits_reviews_it_without_fetching() {
    let (repo, base, head) = repo_with_local_pull_request("open-local");
    let mut app = connected_app(&repo);
    let mut summary = fixtures::summary(7);
    summary.head_oid = head.clone();
    summary.base_oid = base.clone();

    let seen = open_and_wait_for_commits(&mut app, summary).await;

    assert_eq!(seen, vec!["commits ready"]);
    let ReviewMode::PullRequest(selection) = &app.review_mode else {
        panic!("review should show the pull request: {seen:?}");
    };
    assert_eq!(selection.number, 7);
    assert_eq!(selection.head_oid, head);
    assert_eq!(selection.base_oid, base);
    assert_eq!(selection.remote, "origin");
    assert_eq!(repo.run(&["rev-parse", "refs/vigil/pr/7/head"]), head);
    assert!(
        !app.status_message
            .as_deref()
            .unwrap_or_default()
            .contains("fetching"),
        "{:?}",
        app.status_message
    );
    app.quit();
}

#[tokio::test]
async fn opening_a_pull_request_without_its_commits_fetches_them() {
    let (repo, base, _) = repo_with_local_pull_request("open-remote");
    let mut app = connected_app(&repo);
    let mut summary = fixtures::summary(8);
    summary.head_oid = "0123456789abcdef0123456789abcdef01234567".to_string();
    summary.base_oid = base;

    let seen = open_and_wait_for_commits(&mut app, summary).await;

    // The fetch ran (and failed: `origin` does not exist).
    assert_eq!(seen, vec!["fetch started", "commits failed"]);
    assert!(matches!(app.review_mode, ReviewMode::WorkingTree));
    assert!(app.pull_requests.open().is_none());
    app.quit();
}

#[test]
fn only_the_current_open_reports_its_fetch() {
    let mut state = connected();
    let (first, _) = state.begin_open(fixtures::summary(1), ReviewOrigin::Elsewhere);
    assert_eq!(state.fetching_number(first), Some(1));

    let (second, _) = state.begin_open(fixtures::summary(2), ReviewOrigin::Elsewhere);
    assert_eq!(state.fetching_number(first), None);
    assert_eq!(state.fetching_number(second), Some(2));

    state.finish_fetch(second);
    assert_eq!(state.fetching_number(second), None);
}

#[test]
fn a_request_that_finds_gh_unusable_turns_the_connection_off() {
    let mut state = connected();
    state.mark_unavailable(ForgeError::GhNotInstalled);
    assert!(state.github().is_none());
    assert!(matches!(
        state.connection(),
        ForgeConnection::Unavailable(ForgeError::GhNotInstalled)
    ));
    // Background work stays off; the user asking reconnects.
    assert_eq!(state.begin_connect(ConnectReason::Background), None);
    assert!(state.begin_connect(ConnectReason::UserRequest).is_some());

    // Only a live connection is turned off.
    let mut disabled = PullRequests::disabled();
    disabled.mark_unavailable(ForgeError::GhNotInstalled);
    assert!(matches!(disabled.connection(), ForgeConnection::Disabled));
}

#[tokio::test]
async fn a_lookup_that_cannot_see_the_repository_stops_background_lookups() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));
    app.set_branch_snapshot(fixtures::branch_snapshot("feature"));
    app.pull_requests.connect_for_test(github());
    let request_id = app
        .pull_requests
        .begin_current_branch_load(key("feature"), true, Instant::now())
        .unwrap();
    app.handle_pull_request_event(PullRequestEvent::CurrentBranchLoaded {
        request_id,
        result: Err(ForgeError::NotFound {
            message: "Could not resolve to a Repository with the name 'Scott-fo/vigil'."
                .to_string(),
        }),
    })
    .await
    .unwrap();

    assert!(matches!(
        app.pull_requests.connection(),
        ForgeConnection::Unavailable(ForgeError::NotFound { .. })
    ));
    // The next branch snapshot neither looks up nor reconnects.
    app.refresh_current_branch_pull_request(true);
    assert!(matches!(
        app.pull_requests.connection(),
        ForgeConnection::Unavailable(_)
    ));
    assert!(app.pull_requests.github().is_none());
    app.quit();
}

fn repository(owner: &str, name: &str) -> RepositoryRef {
    RepositoryRef {
        host: "github.com".to_string(),
        owner: owner.to_string(),
        name: name.to_string(),
    }
}

/// State connected to `scott-fo/VIGIL` as a remote URL spells it.
fn connected_locally() -> PullRequests {
    let mut state = PullRequests::default();
    let id = state.begin_connect(ConnectReason::Background).unwrap();
    state.finish_connect(
        id,
        Ok(GitHub::unconfirmed(
            "/tmp/vigil-pr-tests",
            repository("scott-fo", "VIGIL"),
        )),
    );
    state
}

#[test]
fn only_locally_resolved_repositories_are_confirmed() {
    assert!(connected().begin_confirm().is_none());

    let mut state = connected_locally();
    let (id, github) = state.begin_confirm().expect("a local connect is confirmed");
    assert_eq!(github.repository(), &repository("scott-fo", "VIGIL"));
    // A spelling difference adopts GitHub's name without calling the
    // repository renamed.
    let canonical = GitHub::new("/elsewhere", repository("Scott-fo", "vigil"));
    assert!(!state.finish_confirm(id, Ok(canonical)));
    let github = state.github().unwrap();
    assert_eq!(github.repository(), &repository("Scott-fo", "vigil"));
    assert_eq!(github.repo_root(), Path::new("/tmp/vigil-pr-tests"));
    assert!(github.is_repository_confirmed());
    assert!(state.begin_confirm().is_none());
}

#[test]
fn a_renamed_repository_switches_the_client_once_confirmed() {
    let mut state = connected_locally();
    let (stale, _) = state.begin_confirm().unwrap();
    let (id, _) = state.begin_confirm().unwrap();
    let renamed = GitHub::new("/tmp/vigil-pr-tests", repository("Scott-fo", "vigil-next"));
    assert!(!state.finish_confirm(stale, Ok(renamed.clone())));
    assert!(state.finish_confirm(id, Ok(renamed)));
    assert_eq!(
        state.github().unwrap().repository(),
        &repository("Scott-fo", "vigil-next")
    );

    // A failed confirmation keeps the name the remote gave.
    let mut state = connected_locally();
    let (id, _) = state.begin_confirm().unwrap();
    assert!(!state.finish_confirm(id, Err(ForgeError::GhNotInstalled)));
    assert_eq!(
        state.github().unwrap().repository(),
        &repository("scott-fo", "VIGIL")
    );
}

/// Every tab loaded under the old name found nothing, since GitHub search
/// does not follow renames; none of them may keep showing that.
#[tokio::test]
async fn a_confirmed_rename_forgets_every_tab_loaded_under_the_old_name() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));
    app.pull_requests = connected_locally();
    for filter in [
        PullRequestListFilter::NeedsMyReview,
        PullRequestListFilter::Mine,
    ] {
        let list = app.pull_requests.list_mut();
        list.set_filter(filter);
        let (id, filter) = list.begin_load(Instant::now());
        list.finish_load(id, filter, Ok(fixtures::pull_request_list(&[], 0)));
    }
    let (request_id, _) = app.pull_requests.begin_confirm().unwrap();

    app.handle_pull_request_event(PullRequestEvent::RepositoryConfirmed {
        request_id,
        result: Ok(GitHub::new(
            "/tmp/vigil-pr-tests",
            repository("Scott-fo", "vigil-next"),
        )),
    })
    .await
    .unwrap();

    let list = app.pull_requests.list();
    assert!(!list.has_page(PullRequestListFilter::NeedsMyReview));
    assert!(!list.has_page(PullRequestListFilter::Mine));
    app.quit();
}

#[tokio::test]
async fn confirming_a_repository_gh_cannot_see_turns_the_connection_off() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));
    app.pull_requests = connected_locally();
    let (request_id, _) = app.pull_requests.begin_confirm().unwrap();
    app.handle_pull_request_event(PullRequestEvent::RepositoryConfirmed {
        request_id,
        result: Err(ForgeError::NotFound {
            message: "Could not resolve to a Repository with the name 'scott-fo/VIGIL'."
                .to_string(),
        }),
    })
    .await
    .unwrap();
    assert!(matches!(
        app.pull_requests.connection(),
        ForgeConnection::Unavailable(ForgeError::NotFound { .. })
    ));
    app.quit();
}

#[test]
fn failed_current_branch_lookups_wait_out_the_interval_too() {
    let mut state = connected();
    let now = Instant::now();
    let id = state
        .begin_current_branch_load(key("feature"), false, now)
        .unwrap();
    state.finish_current_branch_load(id, &Err(ForgeError::GhNotInstalled), now);

    let soon = now + Duration::from_secs(2);
    assert_eq!(
        state.begin_current_branch_load(key("feature"), false, soon),
        None
    );
    assert!(
        state
            .begin_current_branch_load(key("feature"), true, soon)
            .is_some(),
        "an explicit request still looks up"
    );
}

#[tokio::test]
async fn a_list_load_that_finds_gh_logged_out_explains_it_on_the_list() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));
    app.show_pull_request_list_for_test(fixtures::pull_request_list(&[1], 1));
    let (request_id, filter) = app
        .pull_requests
        .list_mut()
        .begin_load(std::time::Instant::now());
    let error = ForgeError::NotAuthenticated {
        message: "no token".to_string(),
    };
    app.handle_pull_request_event(PullRequestEvent::ListLoaded {
        request_id,
        filter,
        result: Err(error.clone()),
    })
    .await
    .unwrap();

    assert_eq!(
        app.pull_request_list_view().status,
        PullRequestListStatus::Unavailable(Some(&error))
    );
    app.quit();
}

fn row_numbers(list: &PullRequestListState) -> Vec<u64> {
    list.visible_rows().iter().map(|row| row.number).collect()
}

fn loaded_list(numbers: &[u64]) -> PullRequestListState {
    let mut list = PullRequestListState::default();
    let (id, filter) = list.begin_load(std::time::Instant::now());
    list.finish_load(
        id,
        filter,
        Ok(fixtures::pull_request_list(numbers, numbers.len() as u64)),
    );
    list
}

#[test]
fn list_tabs_keep_their_pages_and_drop_stale_loads() {
    let mut list = PullRequestListState::default();
    let (needs_review, filter) = list.begin_load(std::time::Instant::now());
    assert_eq!(filter, PullRequestListFilter::NeedsMyReview);

    assert!(list.set_filter(PullRequestListFilter::Mine));
    let (mine, filter) = list.begin_load(std::time::Instant::now());
    assert_eq!(filter, PullRequestListFilter::Mine);
    assert!(
        !list.finish_load(
            needs_review,
            PullRequestListFilter::NeedsMyReview,
            Ok(fixtures::pull_request_list(&[1], 1)),
        ),
        "the superseded load for the old tab is dropped"
    );
    assert!(list.finish_load(
        mine,
        PullRequestListFilter::Mine,
        Ok(fixtures::pull_request_list(&[2, 3], 2)),
    ));
    assert_eq!(row_numbers(&list), vec![2, 3]);

    assert!(list.set_filter(PullRequestListFilter::NeedsMyReview));
    assert!(row_numbers(&list).is_empty(), "that tab never loaded");
    assert!(!list.set_filter(PullRequestListFilter::NeedsMyReview));
    assert!(list.cycle_filter(1));
    assert_eq!(list.filter(), PullRequestListFilter::Mine);
    assert_eq!(row_numbers(&list), vec![2, 3], "cached rows show at once");
    assert!(list.cycle_filter(-2));
    assert_eq!(list.filter(), PullRequestListFilter::AllOpen);
}

#[test]
fn list_query_filters_rows_and_names_pull_request_numbers() {
    let mut list = loaded_list(&[5, 17, 42]);
    list.start_query();
    assert_eq!(list.query_input(), QueryInput::Editing);
    for ch in "#42".chars() {
        list.push_query(ch);
    }
    assert_eq!(row_numbers(&list), vec![42]);
    assert_eq!(list.query_number(), Some(42));

    list.clear_query();
    assert_eq!(list.query_input(), QueryInput::Off);
    assert_eq!(row_numbers(&list), vec![5, 17, 42]);

    for ch in "#99".chars() {
        list.push_query(ch);
    }
    assert!(row_numbers(&list).is_empty());
    assert!(list.selected_summary().is_none());
    assert_eq!(
        list.query_number(),
        Some(99),
        "Enter looks #99 up by number"
    );
}

#[test]
fn list_selection_follows_its_pull_request_across_reloads() {
    let mut list = loaded_list(&[5, 17, 42]);
    list.move_selection(1);
    assert_eq!(list.selected_summary().map(|row| row.number), Some(17));

    let (id, filter) = list.begin_load(std::time::Instant::now());
    list.finish_load(id, filter, Ok(fixtures::pull_request_list(&[99, 17], 2)));
    assert_eq!(list.selected_summary().map(|row| row.number), Some(17));
    assert_eq!(list.selected(), 1);
}

#[tokio::test]
async fn the_list_screen_takes_every_key_and_esc_returns_to_the_review() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-pr-tests"));

    app.handle_key_event(press(KeyCode::Char('L'), KeyModifiers::SHIFT))
        .await
        .unwrap();
    assert_eq!(app.screen(), Screen::PullRequestList);

    // Review shortcuts do nothing behind the list.
    app.handle_key_event(press(KeyCode::Char('c'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(!app.commit_modal_open);

    app.handle_key_event(press(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(
        app.pull_request_list_view().filter,
        PullRequestListFilter::Mine
    );
    app.handle_key_event(press(KeyCode::Char('3'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(
        app.pull_request_list_view().filter,
        PullRequestListFilter::AllOpen
    );

    app.handle_key_event(press(KeyCode::Char('/'), KeyModifiers::NONE))
        .await
        .unwrap();
    app.handle_key_event(press(KeyCode::Char('q'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(app.running, "q is text while filtering");
    assert_eq!(app.pull_request_list_view().query, "q");
    app.handle_key_event(press(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(app.pull_request_list_view().query, "");
    assert_eq!(app.screen(), Screen::PullRequestList);

    app.handle_key_event(press(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(app.screen(), Screen::Review);
    assert!(app.running);
}
