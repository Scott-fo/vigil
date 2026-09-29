use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::{
    app::{ActionsMenu, PullRequestAction},
    ui::{error_color, text_color, text_faint_color, warning_color},
};

use super::super::{
    frame::{render_danger_modal_frame, render_modal_frame},
    hints::render_hint_footer,
    list::render_visible_list,
};

pub(super) fn render_actions(frame: &mut Frame, menu: &ActionsMenu, number: u64, updating: bool) {
    let title = format!("Pull request #{number}");
    let height = menu.actions().len() as u16 + 8;
    let inner = if menu.confirming().is_some() {
        render_danger_modal_frame(frame, 60, height, title)
    } else {
        render_modal_frame(frame, 60, height, title)
    };
    let [list, _, status, _, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);

    render_visible_list(
        frame,
        list,
        menu.actions().len(),
        menu.selected(),
        |index, _| {
            let action = menu.actions()[index];
            let style = match action {
                PullRequestAction::Close => Style::new().fg(error_color()),
                _ => Style::new().fg(text_color()),
            };
            vec![Span::styled(action.label(), style)]
        },
    );

    let status_span = if updating {
        Some(Span::styled("updating…", Style::new().fg(warning_color())))
    } else if let Some(action) = menu.confirming() {
        Some(Span::styled(
            format!("⏎ again: {} #{number}", action.label().to_lowercase()),
            Style::new().fg(error_color()).add_modifier(Modifier::BOLD),
        ))
    } else {
        menu.error()
            .map(|error| Span::styled(error.to_string(), Style::new().fg(error_color())))
    };
    if let Some(span) = status_span {
        frame.render_widget(
            Paragraph::new(Line::from(span)).block(Block::new().padding(Padding::horizontal(1))),
            status,
        );
    } else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Merging lives on M.",
                Style::new().fg(text_faint_color()),
            )))
            .block(Block::new().padding(Padding::horizontal(1))),
            status,
        );
    }

    let hints = if menu.confirming().is_some() {
        vec![("⏎", "confirm"), ("esc", "back")]
    } else {
        vec![("j/k", "move"), ("⏎", "run"), ("esc", "close")]
    };
    render_hint_footer(frame, footer, &hints, None);
}
