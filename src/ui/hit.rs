mod diff;
mod sidebar;

use ratatui::layout::Rect;

use crate::app::{App, Screen};

use super::{
    layout::ScreenLayout,
    pull_request::{PullRequestListTarget, list_target_at},
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
    PullRequestList(PullRequestListTarget),
}

/// Whether the review screen's panes are on screen and take the mouse.
pub(super) fn review_panes_visible(app: &App) -> bool {
    app.screen() == Screen::Review && !app.show_splash()
}

/// The tab or row of the pull request list under a terminal cell.
pub fn pull_request_list_target_at(
    app: &App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<PullRequestListTarget> {
    if app.screen() != Screen::PullRequestList {
        return None;
    }
    list_target_at(
        app,
        Rect::new(0, 0, terminal_width, terminal_height),
        mouse_column,
        mouse_row,
    )
}

pub fn footer_action_at(
    app: &App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<FooterAction> {
    if !review_panes_visible(app) {
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
    if let Some(target) = pull_request_list_target_at(
        app,
        mouse_column,
        mouse_row,
        terminal_width,
        terminal_height,
    ) {
        return Some(HoverTarget::PullRequestList(target));
    }
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
