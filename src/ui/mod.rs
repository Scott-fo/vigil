mod diff;
mod hit;
mod layout;
mod modals;
mod panel;
mod selection;
mod sidebar;
pub mod splash;
mod status;
mod style;

use ratatui::{
    Frame,
    style::Style,
    widgets::{Block, Clear},
};

#[cfg(test)]
use crate::app::ActivePane;
use crate::app::App;

use self::{
    diff::render_diff,
    layout::main_layout,
    modals::render_modals,
    panel::{bordered_panel, diff_pane_label},
    selection::{highlight_line, highlight_line_range},
    sidebar::render_sidebar,
    splash::Splash,
    status::render_notifications,
    style::{
        add_bg_color, background_color, border_active_color, border_color, diff_context_color,
        element_color, error_color, panel_color, primary_color, selected_list_item_text_color,
        success_color, text_color, text_muted_color, warning_color,
    },
};

pub use self::hit::{
    diff_gap_click_at, diff_selection_drag_point_at, diff_selection_point_at, hovered_pane_at,
    prepare_diff_viewport_for_terminal, sidebar_file_at, sidebar_item_index_at,
};
pub use self::style::{
    added_sign_style, context_sign_style, diff_added_style, diff_context_style, diff_hunk_style,
    diff_meta_style, diff_removed_style, line_number_style, removed_sign_style, syntax_style,
};

const NOTICE_WIDTH: u16 = 36;

pub fn render(frame: &mut Frame, app: &mut App) {
    frame.render_widget(Clear, frame.area());
    frame.render_widget(
        Block::new().style(Style::new().bg(background_color())),
        frame.area(),
    );

    if app.show_splash() {
        frame.render_widget(
            Splash::new(
                app.splash_error(),
                app.repo_loading,
                Style::new().fg(text_color()),
                Style::new().fg(text_muted_color()),
            ),
            frame.area(),
        );
    } else {
        let [sidebar_area, diff_area] = main_layout(frame.area(), app.sidebar_hidden);
        if !app.sidebar_hidden {
            render_sidebar(frame, app, sidebar_area);
        }
        render_diff(frame, app, diff_area);
    }

    render_modals(frame, app);
    render_notifications(frame, app);
}

#[cfg(test)]
mod tests;
