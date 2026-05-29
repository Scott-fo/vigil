use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::App;

use super::super::{
    diff_context_color, panel_color, primary_color, selected_list_item_text_color, text_color,
    text_muted_color,
};
use super::frame::render_modal_frame;
use super::list::{
    render_list_frame, render_list_message, render_modal_input, render_visible_list,
};

pub(super) fn render_theme_modal(frame: &mut Frame, app: &mut App) {
    let inner = render_modal_frame(frame, 76, 22, "Theme Picker");

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    if app.theme_modal_query.is_empty() {
        render_modal_input(frame, chunks[0], "Search themes...", true, false, None);
    } else {
        render_modal_input(
            frame,
            chunks[0],
            app.theme_modal_query.clone(),
            false,
            false,
            None,
        );
    }

    let mode_line = Paragraph::new(Line::from(vec![
        Span::styled(
            "mode  ",
            Style::new()
                .fg(primary_color())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(app.theme_mode.as_str(), Style::new().fg(text_color())),
        Span::styled(
            "  m toggles light/dark preview",
            Style::new().fg(text_muted_color()),
        ),
    ]))
    .style(Style::new().bg(panel_color()))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(mode_line, chunks[1]);

    let filtered_theme_names = app.filtered_theme_names();
    let list_inner = render_list_frame(frame, chunks[2]);

    if filtered_theme_names.is_empty() {
        render_list_message(frame, list_inner, "No matching themes.");
    } else {
        let selected_index = app
            .theme_modal_selected_index
            .min(filtered_theme_names.len().saturating_sub(1));

        render_visible_list(
            frame,
            list_inner,
            filtered_theme_names.len(),
            selected_index,
            |display_index, selected| {
                let theme_name = filtered_theme_names[display_index];
                let style = if selected {
                    Style::new()
                        .bg(primary_color())
                        .fg(selected_list_item_text_color())
                } else {
                    Style::new().fg(text_color())
                };
                Line::from(Span::styled(theme_name.to_string(), style)).style(style)
            },
        );
    }

    let footer = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "Type to filter. j/k move. Enter saves. Esc restores previous theme.",
            Style::new().fg(text_muted_color()),
        )),
        Line::from(Span::styled(
            format!(
                "previewing {} ({})",
                app.theme_name,
                app.theme_mode.as_str()
            ),
            Style::new().fg(diff_context_color()),
        )),
    ]))
    .style(Style::new().bg(panel_color()))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(footer, chunks[3]);
}
