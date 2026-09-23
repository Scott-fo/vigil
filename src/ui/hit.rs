mod diff;
mod sidebar;

use ratatui::layout::Rect;

use crate::app::App;

use super::{
    layout::ScreenLayout,
    status::{FooterAction, footer_action_at as footer_action_in},
};

pub use self::{
    diff::{
        diff_gap_click_at, diff_selection_drag_point_at, diff_selection_point_at,
        prepare_diff_viewport_for_terminal,
    },
    sidebar::{hovered_pane_at, sidebar_file_at, sidebar_item_index_at},
};

/// The interactive element under the mouse. The app compares targets between
/// mouse moves and redraws only when the target changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverTarget {
    SidebarRow(usize),
    Footer(FooterAction),
}

pub fn footer_action_at(
    app: &App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<FooterAction> {
    if app.show_splash() {
        return None;
    }
    let footer = ScreenLayout::new(
        Rect::new(0, 0, terminal_width, terminal_height),
        app.sidebar_hidden,
    )
    .footer;
    footer_action_in(app, footer, mouse_column, mouse_row)
}

pub fn hover_target_at(
    app: &App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<HoverTarget> {
    sidebar_item_index_at(
        app,
        mouse_column,
        mouse_row,
        terminal_width,
        terminal_height,
    )
    .map(HoverTarget::SidebarRow)
    .or_else(|| {
        footer_action_at(
            app,
            mouse_column,
            mouse_row,
            terminal_width,
            terminal_height,
        )
        .map(HoverTarget::Footer)
    })
}
