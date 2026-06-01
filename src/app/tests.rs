use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn build_test_app() -> App {
    App::new_for_benchmarks(PathBuf::from("/tmp/vigil-app-tests"))
}

#[tokio::test]
async fn f2_toggles_diff_stats_modal() {
    let mut app = build_test_app();

    app.handle_key_event(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(app.diff_stats_modal_open);

    app.handle_key_event(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(!app.diff_stats_modal_open);
}

#[tokio::test]
async fn global_shortcuts_use_ff_for_file_search_fg_for_diff_search_and_p_for_pull() {
    let mut app = build_test_app();

    app.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(!app.file_search_modal_open);
    assert!(!app.diff_search_modal_open);
    assert_eq!(
        app.status_message.as_deref(),
        Some("f: f files, g diff search")
    );

    app.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(app.file_search_modal_open);
    assert!(app.remote_sync.is_none());

    app.file_search_modal_open = false;
    app.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
        .await
        .unwrap();
    app.handle_key_event(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(app.diff_search_modal_open);
    assert!(!app.file_search_modal_open);
    assert!(app.diff_search_loading);

    app.close_diff_search_modal();
    app.handle_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(!app.file_search_modal_open);
    assert_eq!(app.remote_sync, Some(RemoteSyncDirection::Pull));

    app.cancel_diff_search_tasks();
    app.abort_background_tasks();
}

#[tokio::test]
async fn diff_search_index_loaded_in_background_is_reused_on_open() {
    let mut app = build_test_app();

    app.queue_diff_search_index_load();
    let request_id = app.diff_search_index_request_id;
    app.cancel_diff_search_tasks();
    let index = git::DiffSearchIndex::from_diff_text(concat!(
        "diff --git a/src/lib.rs b/src/lib.rs\n",
        "--- a/src/lib.rs\n",
        "+++ b/src/lib.rs\n",
        "@@ -1,1 +1,1 @@\n",
        "-fn before() {}\n",
        "+fn after() {}\n",
    ))
    .expect("diff search index should build");

    assert!(!app.handle_diff_search_index_loaded(request_id, Ok(index)));
    assert!(app.diff_search_index.is_some());

    app.open_diff_search_modal();

    assert!(app.diff_search_modal_open);
    assert!(!app.diff_search_loading);
    assert!(app.diff_search_error.is_none());
    assert!(app.diff_search_load_task.is_none());

    app.close_diff_search_modal();
}

#[tokio::test]
async fn diff_search_tab_toggles_literal_and_fuzzy_modes() {
    let mut app = build_test_app();
    app.diff_search_modal_open = true;

    assert_eq!(app.diff_search_mode, git::DiffSearchMode::Literal);

    assert!(
        app.handle_diff_search_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap()
    );
    assert_eq!(app.diff_search_mode, git::DiffSearchMode::Fuzzy);

    assert!(
        app.handle_diff_search_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap()
    );
    assert_eq!(app.diff_search_mode, git::DiffSearchMode::Literal);
}

#[tokio::test]
async fn diff_search_index_survives_modal_close() {
    let mut app = build_test_app();

    app.open_diff_search_modal();
    let request_id = app.diff_search_index_request_id;
    app.cancel_diff_search_tasks();
    let index = git::DiffSearchIndex::from_diff_text(concat!(
        "diff --git a/src/main.rs b/src/main.rs\n",
        "--- a/src/main.rs\n",
        "+++ b/src/main.rs\n",
        "@@ -1,1 +1,1 @@\n",
        "-let old_value = 1;\n",
        "+let new_value = 2;\n",
    ))
    .expect("diff search index should build");

    assert!(app.handle_diff_search_index_loaded(request_id, Ok(index)));
    assert!(app.diff_search_index.is_some());

    app.close_diff_search_modal();

    assert!(!app.diff_search_modal_open);
    assert!(app.diff_search_index.is_some());

    app.open_diff_search_modal();

    assert!(!app.diff_search_loading);
    assert!(app.diff_search_error.is_none());
    assert!(app.diff_search_load_task.is_none());

    app.close_diff_search_modal();
}

#[tokio::test]
async fn diff_search_jump_waits_for_selected_file_diff_from_sidebar() {
    let mut app = build_test_app();
    app.files = vec![
        FileEntry {
            status: " M".to_string(),
            path: "src/a.rs".to_string(),
            label: "a.rs".to_string(),
            filetype: Some("rust"),
        },
        FileEntry {
            status: " M".to_string(),
            path: "src/b.rs".to_string(),
            label: "b.rs".to_string(),
            filetype: Some("rust"),
        },
    ];
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.update_diff_viewport(app.diff_view_mode, 120, 0, 20);
    app.active_pane = ActivePane::Sidebar;
    app.diff_search_modal_open = true;
    app.diff_search_results = git::DiffSearchResults {
        total_matched: 1,
        total_matched_exact: true,
        items: vec![git::DiffSearchResult {
            file_path: "src/b.rs".to_string(),
            filetype: Some("rust"),
            hunk_index: 0,
            hunk_old_start: 1,
            hunk_new_start: 1,
            kind: git::DiffSearchLineKind::Addition,
            old_line: None,
            new_line: Some(2),
            line: "fn target() {}".to_string(),
            match_ranges: Vec::new(),
            syntax_ranges: Vec::new(),
            preview_lines: Vec::new(),
            score: 1,
        }],
    };

    app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();

    assert_eq!(
        app.selected_file().map(|file| file.path.as_str()),
        Some("src/b.rs")
    );
    assert!(app.pending_diff_search_target.is_some());

    let request_id = app.diff_request_id;
    app.cancel_inflight_diff_load();
    let diff_view = git::build_diff_view_from_diff_text(
        concat!(
            "diff --git a/src/b.rs b/src/b.rs\n",
            "--- a/src/b.rs\n",
            "+++ b/src/b.rs\n",
            "@@ -1,1 +1,2 @@\n",
            " fn existing() {}\n",
            "+fn target() {}\n",
        ),
        Some("rust"),
    );

    assert!(app.handle_diff_loaded(request_id, Ok(diff_view)));
    assert_eq!(app.active_pane, ActivePane::Diff);
    assert_eq!(
        app.diff_view.selected_new_line_number(
            app.diff_view_mode,
            app.current_diff_display_width(),
            app.diff_line_wrap_mode,
            app.selected_diff_line_index,
        ),
        Some(2)
    );
    assert!(app.pending_diff_search_target.is_none());

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
async fn review_context_modal_captures_multiline_context() {
    let mut app = build_test_app();

    app.handle_key_event(KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT))
        .await
        .unwrap();
    app.handle_key_event(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT))
        .await
        .unwrap();
    app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    app.handle_key_event(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(app.review_context_modal_open);
    assert_eq!(app.review_extra_context, "A\nb");

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(!app.review_context_modal_open);
    assert_eq!(app.review_extra_context, "A\nb");
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
    assert!(app.diff_load_task.is_none());
    assert!(app.review_diff_snapshot_task.is_some());
    assert!(!app.diff_view.has_diff_rows());
    assert!(app.highlight_registry_loading);

    app.cancel_inflight_diff_load();
    app.cancel_inflight_review_diff_snapshot();
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
