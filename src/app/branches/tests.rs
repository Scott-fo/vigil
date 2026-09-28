use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{BranchPanelMode, BranchPanelRow, BranchSection};
use crate::{
    app::App,
    git::{
        BranchEntry, BranchLocation, BranchOperation, BranchOperationError, BranchSnapshot,
        BranchTip, Divergence, HeadState, Upstream,
    },
};

fn local(name: &str, upstream: Option<&str>) -> BranchEntry {
    BranchEntry {
        name: name.to_string(),
        location: BranchLocation::Local,
        is_head: false,
        upstream: upstream.map(|upstream| Upstream {
            name: upstream.to_string(),
            divergence: Divergence::Tracking {
                ahead: 0,
                behind: 0,
            },
        }),
        tip: BranchTip {
            short_hash: "abc1234".to_string(),
            subject: format!("work on {name}"),
            committed_at: 1_700_000_000,
        },
    }
}

fn remote(name: &str) -> BranchEntry {
    BranchEntry {
        location: BranchLocation::Remote {
            remote: "origin".to_string(),
        },
        ..local(name, None)
    }
}

/// `main` is checked out and tracks `origin/main`; `spike` is local only;
/// `origin/review` exists only on the remote.
fn snapshot() -> BranchSnapshot {
    let mut main = local("main", Some("origin/main"));
    main.is_head = true;
    BranchSnapshot {
        head: HeadState::Branch("main".to_string()),
        branches: vec![
            local("spike", None),
            main,
            local("docs", None),
            remote("origin/main"),
            remote("origin/review"),
        ],
        previous_branch: Some("docs".to_string()),
        remotes: vec!["origin".to_string()],
        last_fetch: None,
        operation: None,
    }
}

fn app_with_open_panel() -> App {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-branch-tests"));
    app.branch_status.set_snapshot(snapshot());
    app.open_branch_panel();
    // Opening queues a reload; keep the fixture instead.
    app.abort_background_tasks();
    app.branch_status.set_snapshot(snapshot());
    app
}

fn press(app: &mut App, code: KeyCode) {
    assert!(app.handle_branch_panel_key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        press(app, KeyCode::Char(ch));
    }
}

fn selected_name(app: &mut App) -> String {
    let snapshot = app.branch_status.snapshot().cloned().unwrap();
    let panel = app.branch_panel.as_mut().unwrap();
    panel.selected_branch(&snapshot).unwrap().name.clone()
}

fn mode(app: &App) -> BranchPanelMode {
    app.branch_panel.as_ref().unwrap().mode().clone()
}

fn finish_operation(app: &mut App) {
    app.branch_operation = None;
    app.abort_background_tasks();
}

#[tokio::test]
async fn rows_put_the_checked_out_branch_first_and_hide_tracked_remotes() {
    let snapshot = snapshot();
    let mut app = app_with_open_panel();
    let rows = app.branch_panel.as_mut().unwrap().rows(&snapshot);

    let names = rows
        .iter()
        .map(|row| match row {
            BranchPanelRow::Section(BranchSection::Local) => "LOCAL",
            BranchPanelRow::Section(BranchSection::Remote) => "REMOTE",
            BranchPanelRow::Branch(index) => snapshot.branches[*index].name.as_str(),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        ["LOCAL", "main", "spike", "docs", "REMOTE", "origin/review"]
    );
}

#[tokio::test]
async fn opening_the_panel_selects_the_previous_branch() {
    let mut app = app_with_open_panel();

    assert_eq!(selected_name(&mut app), "docs");
}

#[tokio::test]
async fn filtering_drops_empty_sections_and_enter_switches() {
    let mut app = app_with_open_panel();

    press(&mut app, KeyCode::Char('/'));
    type_text(&mut app, "rev");
    let view = app.branch_panel_view().unwrap();
    assert_eq!(view.rows.len(), 2);
    assert_eq!(view.rows[0], BranchPanelRow::Section(BranchSection::Remote));

    press(&mut app, KeyCode::Enter);

    assert_eq!(
        app.branch_operation(),
        Some(&BranchOperation::Track {
            remote_branch: "origin/review".to_string(),
            local_name: "review".to_string(),
        })
    );
    finish_operation(&mut app);
}

#[tokio::test]
async fn escape_leaves_filter_then_clears_query_then_closes() {
    let mut app = app_with_open_panel();
    press(&mut app, KeyCode::Char('/'));
    type_text(&mut app, "sp");

    press(&mut app, KeyCode::Esc);
    assert_eq!(mode(&app), BranchPanelMode::Browse);
    assert_eq!(app.branch_panel.as_ref().unwrap().query(), "sp");

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.branch_panel.as_ref().unwrap().query(), "");

    press(&mut app, KeyCode::Esc);
    assert!(!app.branch_panel_open());
}

#[tokio::test]
async fn switching_to_the_checked_out_branch_is_refused_in_the_panel() {
    let mut app = app_with_open_panel();
    press(&mut app, KeyCode::Home);
    assert_eq!(selected_name(&mut app), "main");

    press(&mut app, KeyCode::Enter);

    assert!(app.branch_operation().is_none());
    assert_eq!(
        app.branch_panel.as_ref().unwrap().error(),
        Some("already on main")
    );
}

#[tokio::test]
async fn new_branch_starts_from_the_selection_and_dashes_spaces() {
    let mut app = app_with_open_panel();

    press(&mut app, KeyCode::Char('n'));
    type_text(&mut app, "fix login");
    assert_eq!(
        mode(&app),
        BranchPanelMode::Create {
            start_point: "docs".to_string(),
            name: "fix-login".to_string(),
        }
    );

    press(&mut app, KeyCode::Enter);

    assert_eq!(mode(&app), BranchPanelMode::Browse);
    assert_eq!(
        app.branch_operation(),
        Some(&BranchOperation::Create {
            name: "fix-login".to_string(),
            start_point: "docs".to_string(),
        })
    );
    finish_operation(&mut app);
}

#[tokio::test]
async fn delete_refuses_head_and_remote_branches() {
    let mut app = app_with_open_panel();

    press(&mut app, KeyCode::Home);
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(mode(&app), BranchPanelMode::Browse);
    assert!(app.branch_panel.as_ref().unwrap().error().is_some());

    press(&mut app, KeyCode::End);
    assert_eq!(selected_name(&mut app), "origin/review");
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(mode(&app), BranchPanelMode::Browse);
    assert!(app.branch_panel.as_ref().unwrap().error().is_some());
}

#[tokio::test]
async fn unmerged_delete_asks_again_to_force() {
    let mut app = app_with_open_panel();
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(
        mode(&app),
        BranchPanelMode::Delete {
            branch: "docs".to_string(),
            force: false,
        }
    );
    press(&mut app, KeyCode::Enter);
    assert!(app.branch_operation().is_some());

    app.handle_branch_operation_finished(Err(BranchOperationError::NotFullyMerged {
        branch: "docs".to_string(),
    }))
    .await
    .unwrap();

    assert!(app.branch_operation().is_none());
    assert_eq!(
        mode(&app),
        BranchPanelMode::Delete {
            branch: "docs".to_string(),
            force: true,
        }
    );
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.branch_operation(),
        Some(&BranchOperation::Delete {
            branch: "docs".to_string(),
            force: true,
        })
    );
    finish_operation(&mut app);
}

#[tokio::test]
async fn a_second_operation_waits_for_the_first() {
    let mut app = app_with_open_panel();
    press(&mut app, KeyCode::Char('f'));
    assert_eq!(app.branch_operation(), Some(&BranchOperation::Fetch));

    press(&mut app, KeyCode::Char('p'));

    assert_eq!(app.branch_operation(), Some(&BranchOperation::Fetch));
    assert!(app.branch_panel.as_ref().unwrap().error().is_some());
    finish_operation(&mut app);
}

#[tokio::test]
async fn stale_branch_loads_are_ignored() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-branch-tests"));
    app.queue_branch_status_load();
    app.queue_branch_status_load();
    app.abort_background_tasks();
    let current = app.branch_status.request_id;

    assert!(!app.handle_branch_status_loaded(current - 1, Ok(snapshot())));
    assert!(app.branch_snapshot().is_none());
    assert!(app.handle_branch_status_loaded(current, Ok(snapshot())));
    assert!(app.branch_snapshot().is_some());
}
