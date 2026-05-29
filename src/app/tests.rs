use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn build_test_app() -> App {
    App::new_for_benchmarks(PathBuf::from("/tmp/vigil-app-tests"))
}

#[tokio::test]
async fn global_shortcuts_use_f_for_file_search_and_p_for_pull() {
    let mut app = build_test_app();

    app.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(app.file_search_modal_open);
    assert!(app.remote_sync.is_none());

    app.file_search_modal_open = false;
    app.handle_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(!app.file_search_modal_open);
    assert_eq!(app.remote_sync, Some(RemoteSyncDirection::Pull));

    app.abort_background_tasks();
}

#[tokio::test]
async fn global_merge_shortcut_opens_branch_merge_confirmation() {
    let mut app = build_test_app();
    app.review_mode = ReviewMode::BranchCompare(BranchCompareSelection {
        source_ref: "feature/login".to_string(),
        destination_ref: "main".to_string(),
    });

    app.handle_key_event(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE))
        .await
        .unwrap();

    let target = app
        .branch_merge_target
        .as_ref()
        .expect("merge target should be selected from branch compare mode");
    assert_eq!(target.source_ref, "feature/login");
    assert_eq!(target.destination_ref, "main");

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(app.branch_merge_target.is_none());
}

#[tokio::test]
async fn launch_returns_with_empty_first_paint_state() {
    let mut app = App::new(AppLaunchOptions {
        repo_root: Some(PathBuf::from("/tmp/vigil-app-tests")),
        ..AppLaunchOptions::default()
    })
    .await
    .expect("launch should build app state before repo status loads");

    assert!(app.files.is_empty());
    assert_eq!(app.repo_request_id, 1);
    assert!(app.repo_loading);
    assert_eq!(app.status_message.as_deref(), Some("Loading repository..."));
    assert!(app.highlight_registry.is_none());
    assert!(!app.highlight_registry_loading);

    app.quit();
}

#[tokio::test]
async fn working_tree_status_event_applies_files_and_queues_plain_diff_load() {
    let mut app = build_test_app();
    app.repo_request_id = 7;

    let loaded = app.handle_working_tree_status_loaded(
        7,
        Ok(git::WorkingTreeStatus {
            repo_root: PathBuf::from("/tmp/vigil-app-tests"),
            files: vec![FileEntry {
                status: " M".to_string(),
                path: "src/main.rs".to_string(),
                label: "main.rs".to_string(),
                filetype: Some("rust"),
            }],
        }),
    );

    assert!(loaded);
    assert!(!app.repo_loading);
    assert_eq!(app.files.len(), 1);
    assert_eq!(
        app.selected_file().map(|file| file.path.as_str()),
        Some("src/main.rs")
    );
    assert_eq!(app.status_message.as_deref(), Some("1 changed file"));
    assert!(app.diff_load_task.is_some());
    assert!(app.highlight_registry_loading);

    app.cancel_inflight_diff_load();
    app.abort_background_tasks();
}

#[test]
fn stale_working_tree_status_event_is_ignored() {
    let mut app = build_test_app();
    app.repo_request_id = 7;

    let loaded = app.handle_working_tree_status_loaded(
        6,
        Ok(git::WorkingTreeStatus {
            repo_root: PathBuf::from("/tmp/vigil-app-tests"),
            files: vec![FileEntry {
                status: " M".to_string(),
                path: "src/main.rs".to_string(),
                label: "main.rs".to_string(),
                filetype: Some("rust"),
            }],
        }),
    );

    assert!(!loaded);
    assert!(app.files.is_empty());
    assert!(app.diff_load_task.is_none());
}

#[test]
fn toggling_sidebar_hidden_moves_focus_to_diff() {
    let mut app = build_test_app();
    app.active_pane = ActivePane::Sidebar;

    app.toggle_sidebar_hidden();

    assert!(app.sidebar_hidden);
    assert_eq!(app.active_pane, ActivePane::Diff);

    app.toggle_sidebar_hidden();

    assert!(!app.sidebar_hidden);
    assert_eq!(app.active_pane, ActivePane::Diff);
}

#[tokio::test]
async fn sidebar_focus_can_toggle_flattened_directories_and_select_files() {
    let mut app = build_test_app();
    app.files = vec![
        FileEntry {
            status: "M ".to_string(),
            path: "src/components/Button.tsx".to_string(),
            label: "Button.tsx".to_string(),
            filetype: Some("tsx"),
        },
        FileEntry {
            status: "M ".to_string(),
            path: "src/index.ts".to_string(),
            label: "index.ts".to_string(),
            filetype: Some("typescript"),
        },
        FileEntry {
            status: "M ".to_string(),
            path: "README.md".to_string(),
            label: "README.md".to_string(),
            filetype: Some("markdown"),
        },
    ];
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();

    assert_eq!(app.sidebar_items[0].path(), "src/");
    app.select_sidebar_row(0).await.unwrap();
    assert!(app.toggle_focused_sidebar_directory());
    assert!(app.collapsed_directories.contains("src/"));
    assert_eq!(
        app.sidebar_items
            .iter()
            .map(SidebarItem::path)
            .collect::<Vec<_>>(),
        vec!["src/", "README.md"]
    );
    assert_eq!(app.selected_sidebar_row, 0);

    assert!(app.toggle_focused_sidebar_directory());
    assert!(!app.collapsed_directories.contains("src/"));
    assert!(
        app.sidebar_items
            .iter()
            .any(|item| item.path() == "src/components/Button.tsx")
    );

    let readme_row = app
        .sidebar_items
        .iter()
        .position(|item| item.path() == "README.md")
        .unwrap();
    app.select_sidebar_row(readme_row).await.unwrap();
    assert_eq!(
        app.selected_file().map(|file| file.path.as_str()),
        Some("README.md")
    );
}
