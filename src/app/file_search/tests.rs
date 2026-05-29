use std::path::PathBuf;

use super::super::{App, FileEntry};

fn build_test_app() -> App {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-app-tests"));
    app.files = vec![
        FileEntry {
            status: "M ".to_string(),
            path: "src/app/mod.rs".to_string(),
            label: "mod.rs".to_string(),
            filetype: Some("rust"),
        },
        FileEntry {
            status: "A ".to_string(),
            path: "src/ui/sidebar.rs".to_string(),
            label: "sidebar.rs".to_string(),
            filetype: Some("rust"),
        },
    ];
    app.rebuild_sidebar_items();
    app
}

#[test]
fn file_search_filters_by_path_fragments() {
    let mut app = build_test_app();
    app.file_search_query = "side".to_string();

    assert_eq!(app.filtered_file_search_indices(), vec![1]);
}

#[tokio::test]
async fn cancelling_file_search_restores_initial_selection() {
    let mut app = build_test_app();
    app.open_file_search_modal()
        .await
        .expect("modal should open");
    app.move_file_search_selection(1)
        .await
        .expect("selection should preview");

    assert_eq!(
        app.selected_file().map(|file| file.path.as_str()),
        Some("src/ui/sidebar.rs")
    );

    app.cancel_file_search_modal()
        .await
        .expect("cancel should restore");

    assert_eq!(
        app.selected_file().map(|file| file.path.as_str()),
        Some("src/app/mod.rs")
    );
    assert!(!app.file_search_modal_open);
}
