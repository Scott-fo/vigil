use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
};

use crate::app::App;

use super::super::layout::centered_rect;
use super::super::{
    border_active_color, border_color, diff_context_color, element_color, panel_color,
    primary_color, selected_list_item_text_color, text_color, text_muted_color,
};

pub(super) fn render_theme_modal(frame: &mut Frame, app: &mut App) {
    let area = centered_rect(76, 22, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_active_color()))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            " Theme Picker ",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    let query_display = if app.theme_modal_query.is_empty() {
        Span::styled("Search themes...", Style::new().fg(text_muted_color()))
    } else {
        Span::styled(app.theme_modal_query.clone(), Style::new().fg(text_color()))
    };
    let query = Paragraph::new(Line::from(query_display))
        .style(Style::new().bg(element_color()))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(border_color()))
                .padding(Padding::horizontal(1)),
        );
    frame.render_widget(query, chunks[0]);

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
    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_color()))
        .style(Style::new().bg(panel_color()));
    let list_inner = list_block.inner(chunks[2]);
    frame.render_widget(list_block, chunks[2]);

    if filtered_theme_names.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No matching themes.",
                Style::new().fg(text_muted_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );
    } else {
        let viewport_height = list_inner.height as usize;
        let selected_index = app
            .theme_modal_selected_index
            .min(filtered_theme_names.len().saturating_sub(1));
        let max_scroll = filtered_theme_names.len().saturating_sub(viewport_height);
        let visible_start = selected_index
            .saturating_sub(viewport_height.saturating_sub(1))
            .min(max_scroll);
        let visible_end = (visible_start + viewport_height).min(filtered_theme_names.len());

        let lines = filtered_theme_names[visible_start..visible_end]
            .iter()
            .enumerate()
            .map(|(offset, theme_name)| {
                let display_index = visible_start + offset;
                let selected = display_index == selected_index;
                let style = if selected {
                    Style::new()
                        .bg(primary_color())
                        .fg(selected_list_item_text_color())
                } else {
                    Style::new().fg(text_color())
                };
                Line::from(Span::styled((*theme_name).to_string(), style)).style(style)
            })
            .collect::<Vec<_>>();

        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .style(Style::new().bg(panel_color()))
                .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );

        if filtered_theme_names.len() > viewport_height {
            let mut scrollbar_state = ScrollbarState::new(filtered_theme_names.len())
                .position(visible_start)
                .viewport_content_length(viewport_height);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::new().fg(border_active_color()))
                .track_style(Style::new().fg(border_color()));
            frame.render_stateful_widget(scrollbar, list_inner, &mut scrollbar_state);
        }
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
