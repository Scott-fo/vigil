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

#[test]
fn sidebar_click_targets_the_row_that_rendered_the_file() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut app = build_test_app();
    app.sidebar_items =
        crate::sidebar::build_sidebar_items(&app.files, &std::collections::HashSet::new());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| render(frame, &mut app)).unwrap();

    let buffer = terminal.backend().buffer();
    let (column, row) = (0..40u16)
        .find_map(|row| {
            let text: String = (0..40u16)
                .map(|column| buffer[(column, row)].symbol())
                .collect();
            text.find("main.rs")
                .map(|byte_index| (text[..byte_index].chars().count() as u16, row))
        })
        .expect("sidebar should render the changed file");

    assert_eq!(
        sidebar_file_at(&app, column, row, 120, 40),
        Some("src/main.rs".to_string())
    );
    assert_eq!(
        hovered_pane_at(&app, column, row, 120, 40),
        Some(ActivePane::Sidebar)
    );
}
