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

fn render_to_buffer(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    use ratatui::{Terminal, backend::TestBackend};

    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

/// Column where `needle` starts on `row`, read from the rendered cells.
fn column_of(buffer: &ratatui::buffer::Buffer, row: u16, needle: &str) -> Option<u16> {
    let width = buffer.area.width;
    let cells: Vec<&str> = (0..width)
        .map(|column| buffer[(column, row)].symbol())
        .collect();
    (0..width as usize).find_map(|start| {
        let text: String = cells[start..].concat();
        text.starts_with(needle).then_some(start as u16)
    })
}

/// The view-mode chip shows the current mode (padded, as rendered); the `v`
/// hint names the mode it switches to.
fn mode_labels(app: &App) -> (&'static str, &'static str) {
    match app.diff_view_mode {
        crate::app::DiffViewMode::Unified => (" unified ", "v split"),
        crate::app::DiffViewMode::Split => (" split ", "v unified"),
    }
}

#[test]
fn footer_clicks_land_on_the_rendered_chips_and_hints() {
    let mut app = build_test_app();
    let (width, height) = (160, 20);
    let buffer = render_to_buffer(&mut app, width, height);
    let footer_row = height - 1;

    let (mode_chip, mode_hint) = mode_labels(&app);
    let expectations = [
        (mode_chip, Some(FooterAction::ToggleDiffViewMode)),
        (mode_hint, Some(FooterAction::ToggleDiffViewMode)),
        (" wrap ", Some(FooterAction::ToggleLineWrap)),
        ("? help", Some(FooterAction::OpenHelp)),
        ("tab diff", Some(FooterAction::SwitchPane)),
        ("j/k move", None),
    ];
    for (needle, expected) in expectations {
        let start = column_of(&buffer, footer_row, needle)
            .unwrap_or_else(|| panic!("footer should render {needle:?}"));
        for column in start..start + needle.chars().count() as u16 {
            assert_eq!(
                footer_action_at(&app, column, footer_row, width, height),
                expected,
                "{needle:?} at column {column}"
            );
        }
    }

    // The summary on the left and rows above the footer are not controls.
    assert_eq!(footer_action_at(&app, 1, footer_row, width, height), None);
    let chip = column_of(&buffer, footer_row, mode_chip).unwrap();
    assert_eq!(
        footer_action_at(&app, chip, footer_row - 1, width, height),
        None
    );
}

#[test]
fn footer_hit_testing_follows_hints_that_drop_on_narrow_terminals() {
    let mut app = build_test_app();
    let (width, height) = (70, 20);
    let buffer = render_to_buffer(&mut app, width, height);
    let footer_row = height - 1;

    let footer_text: String = (0..width)
        .map(|column| buffer[(column, footer_row)].symbol())
        .collect();
    let chip = column_of(&buffer, footer_row, mode_labels(&app).0)
        .unwrap_or_else(|| panic!("chips always render: {footer_text:?}"));
    assert_eq!(
        footer_action_at(&app, chip, footer_row, width, height),
        Some(FooterAction::ToggleDiffViewMode)
    );
    assert_eq!(column_of(&buffer, footer_row, "ff find file"), None);
    for column in 0..width {
        assert_ne!(
            footer_action_at(&app, column, footer_row, width, height),
            Some(FooterAction::FindFile),
            "a dropped hint must not stay clickable"
        );
    }
}

#[test]
fn hovered_sidebar_row_is_tinted_and_selection_still_wins() {
    let mut app = build_test_app();
    app.files.push(FileEntry {
        status: "M ".to_string(),
        path: "src/lib.rs".to_string(),
        label: "lib.rs".to_string(),
        filetype: Some("rust"),
    });
    app.sidebar_items =
        crate::sidebar::build_sidebar_items(&app.files, &std::collections::HashSet::new());
    let row_of = |buffer: &ratatui::buffer::Buffer, name: &str| {
        (0..20u16)
            .find(|row| column_of(buffer, *row, name).is_some_and(|column| column < 32))
            .expect("file should render in the sidebar")
    };
    app.selected_sidebar_row = app
        .sidebar_items
        .iter()
        .position(|item| item.path() == "src/lib.rs")
        .unwrap();
    let buffer = render_to_buffer(&mut app, 120, 20);
    let cursor_row = row_of(&buffer, "lib.rs");
    let hover_row = row_of(&buffer, "main.rs");

    app.mouse_position = Some(ratatui::layout::Position::new(10, hover_row));
    let buffer = render_to_buffer(&mut app, 120, 20);
    assert_eq!(buffer[(10, hover_row)].bg, hover_color());

    app.mouse_position = Some(ratatui::layout::Position::new(10, cursor_row));
    let buffer = render_to_buffer(&mut app, 120, 20);
    assert_eq!(
        buffer[(10, cursor_row)].bg,
        selection_color(),
        "the cursor row keeps its selection tint under the mouse"
    );
    assert_ne!(buffer[(10, hover_row)].bg, hover_color());
}
