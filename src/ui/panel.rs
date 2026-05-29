use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders},
};

use crate::app::{ActivePane, App, DiffViewMode};

use super::{border_active_color, border_color, panel_color, text_color, text_muted_color};

pub(super) fn diff_pane_label(app: &App) -> String {
    if app.sidebar_hidden {
        return format!(
            "{}  diff  sidebar hidden",
            diff_mode_label(app.diff_view_mode)
        );
    }

    match app.active_pane {
        ActivePane::Sidebar => format!("{}  sidebar", diff_mode_label(app.diff_view_mode)),
        ActivePane::Diff => format!("{}  diff", diff_mode_label(app.diff_view_mode)),
    }
}

pub(super) fn bordered_panel(
    title: &str,
    active: bool,
    right_title: Option<String>,
) -> Block<'static> {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(if active {
            border_active_color()
        } else {
            border_color()
        }))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            format!(" {} ", title),
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )));

    if let Some(right_title) = right_title {
        block = block.title_bottom(
            Line::from(Span::styled(
                format!(" {} ", right_title),
                Style::new().fg(text_muted_color()),
            ))
            .right_aligned(),
        );
    }

    block
}

fn diff_mode_label(mode: DiffViewMode) -> &'static str {
    match mode {
        DiffViewMode::Unified => "unified",
        DiffViewMode::Split => "split",
    }
}
