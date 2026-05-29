use std::path::PathBuf;

use super::*;
use crate::git::FileEntry;

fn build_test_app() -> App {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-ui-tests"));
    app.files.push(FileEntry {
        status: "M ".to_string(),
        path: "src/main.rs".to_string(),
        label: "main.rs".to_string(),
        filetype: Some("rust"),
    });
    app
}

#[test]
fn hovered_pane_uses_full_width_diff_when_sidebar_is_hidden() {
    let mut app = build_test_app();
    app.sidebar_hidden = true;

    assert_eq!(hovered_pane_at(&app, 2, 2, 120, 40), Some(ActivePane::Diff));
}

#[test]
fn sidebar_hit_testing_is_disabled_when_sidebar_is_hidden() {
    let mut app = build_test_app();
    app.sidebar_hidden = true;

    assert_eq!(sidebar_file_at(&app, 2, 2, 120, 40), None);
}
