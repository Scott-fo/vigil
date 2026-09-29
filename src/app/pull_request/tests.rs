use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
    fixtures,
    list::{PullRequestListState, QueryInput},
    state::{BranchKey, ConnectOutcome, ConnectReason, PollOutcome, PullRequestPage, PullRequests},
};
use crate::{
    app::{ActivePane, App, DiffViewMode, ReviewMode, Screen},
    forge::{DiffSide, ForgeError, GitHub, PullRequestListFilter, RepositoryRef, Timestamp},
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
    let (fetch_id, detail_id) = state.begin_open(fixtures::summary(17));
    let threads = vec![fixtures::thread(
        "T1",
        "src/lib.rs",
        DiffSide::Right,
        Some(3),
    )];
    assert!(state.finish_detail(detail_id, Ok(fixtures::pull_request(17, threads))));
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
    let (old_fetch, old_detail) = state.begin_open(fixtures::summary(1));
    let (new_fetch, _) = state.begin_open(fixtures::summary(2));

    assert!(state.finish_fetch(old_fetch).is_none());
    assert!(!state.finish_detail(old_detail, Ok(fixtures::pull_request(1, Vec::new()))));
    assert_eq!(state.finish_fetch(new_fetch).map(|pr| pr.number), Some(2));
}

#[test]
fn reloading_the_open_pull_request_keeps_its_page_and_detail() {
    let mut state = connected();
    let (fetch_id, detail_id) = state.begin_open(fixtures::summary(17));
    state.finish_detail(detail_id, Ok(fixtures::pull_request(17, Vec::new())));
    let summary = state.finish_fetch(fetch_id).unwrap();
    state.enter(summary, "head-1".to_string());
    state.set_page(PullRequestPage::Files);

    let (fetch_id, _) = state.begin_open(fixtures::summary(17));
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
    let (fetch_id, detail_id) = state.begin_open(fixtures::summary(17));
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
    assert!(!state.finish_detail(id, Ok(fixtures::pull_request(17, Vec::new()))));
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

    app.handle_key_event(press(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(app.active_pane, ActivePane::Diff);
    app.handle_key_event(press(KeyCode::Char('j'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(app.pull_requests.open().unwrap().overview_scroll(), 1);

    app.handle_key_event(press(KeyCode::Char(']'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(!app.pull_request_overview_visible());
    app.cancel_inflight_diff_load();
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

fn row_numbers(list: &PullRequestListState) -> Vec<u64> {
    list.visible_rows().iter().map(|row| row.number).collect()
}

fn loaded_list(numbers: &[u64]) -> PullRequestListState {
    let mut list = PullRequestListState::default();
    let (id, filter) = list.begin_load();
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
    let (needs_review, filter) = list.begin_load();
    assert_eq!(filter, PullRequestListFilter::NeedsMyReview);

    assert!(list.set_filter(PullRequestListFilter::Mine));
    let (mine, filter) = list.begin_load();
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

    let (id, filter) = list.begin_load();
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
