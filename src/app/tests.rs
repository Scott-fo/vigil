use super::*;

fn build_test_app() -> App {
    App::new_for_benchmarks(PathBuf::from("/tmp/vigil-app-tests"))
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
