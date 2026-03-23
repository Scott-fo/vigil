use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
};

use crate::app::{App, BranchCompareField};

use super::super::layout::centered_rect;
use super::super::{
    border_active_color, border_color, diff_context_color, element_color, error_color, panel_color,
    primary_color, selected_list_item_text_color, text_color, text_muted_color,
};

pub(super) fn render_branch_compare_modal(frame: &mut Frame, app: &mut App) {
    let area = centered_rect(84, 23, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_active_color()))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            " Branch Compare ",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    let source_active = app.branch_compare_active_field == BranchCompareField::Source;
    let source_display = if app.branch_compare_source_query.is_empty() {
        app.branch_compare_source_ref
            .clone()
            .unwrap_or_else(|| "Source ref".to_string())
    } else {
        app.branch_compare_source_query.clone()
    };
    let source = Paragraph::new(Line::from(Span::styled(
        source_display,
        if app.branch_compare_source_query.is_empty() {
            Style::new().fg(text_muted_color())
        } else {
            Style::new().fg(text_color())
        },
    )))
    .style(Style::new().bg(element_color()))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(if source_active {
                primary_color()
            } else {
                border_color()
            }))
            .title(" Source ")
            .padding(Padding::horizontal(1)),
    );
    frame.render_widget(source, chunks[0]);

    let destination_active = app.branch_compare_active_field == BranchCompareField::Destination;
    let destination_display = if app.branch_compare_destination_query.is_empty() {
        app.branch_compare_destination_ref
            .clone()
            .unwrap_or_else(|| "Destination ref".to_string())
    } else {
        app.branch_compare_destination_query.clone()
    };
    let destination = Paragraph::new(Line::from(Span::styled(
        destination_display,
        if app.branch_compare_destination_query.is_empty() {
            Style::new().fg(text_muted_color())
        } else {
            Style::new().fg(text_color())
        },
    )))
    .style(Style::new().bg(element_color()))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(if destination_active {
                primary_color()
            } else {
                border_color()
            }))
            .title(" Destination ")
            .padding(Padding::horizontal(1)),
    );
    frame.render_widget(destination, chunks[1]);

    let filtered_refs = app.filtered_branch_compare_refs();
    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_color()))
        .style(Style::new().bg(panel_color()));
    let list_inner = list_block.inner(chunks[2]);
    frame.render_widget(list_block, chunks[2]);

    if app.branch_compare_loading {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Loading refs...",
                Style::new().fg(text_muted_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );
    } else if let Some(error) = app.branch_compare_error.as_ref() {
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(Span::styled(
                    "Unable to load refs.",
                    Style::new().fg(error_color()),
                )),
                Line::default(),
                Line::from(Span::styled(
                    error.clone(),
                    Style::new().fg(text_muted_color()),
                )),
            ]))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );
    } else if filtered_refs.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No matching refs.",
                Style::new().fg(text_muted_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );
    } else {
        let viewport_height = list_inner.height as usize;
        let selected_index = match app.branch_compare_active_field {
            BranchCompareField::Source => app.branch_compare_selected_source_index,
            BranchCompareField::Destination => app.branch_compare_selected_destination_index,
        }
        .min(filtered_refs.len().saturating_sub(1));
        let max_scroll = filtered_refs.len().saturating_sub(viewport_height);
        let visible_start = selected_index
            .saturating_sub(viewport_height.saturating_sub(1))
            .min(max_scroll);
        let visible_end = (visible_start + viewport_height).min(filtered_refs.len());

        let lines = filtered_refs[visible_start..visible_end]
            .iter()
            .enumerate()
            .map(|(offset, ref_name)| {
                let display_index = visible_start + offset;
                let selected = display_index == selected_index;
                let style = if selected {
                    Style::new()
                        .bg(primary_color())
                        .fg(selected_list_item_text_color())
                } else {
                    Style::new().fg(text_color())
                };
                Line::from(Span::styled(ref_name.clone(), style)).style(style)
            })
            .collect::<Vec<_>>();

        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .style(Style::new().bg(panel_color()))
                .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );

        if filtered_refs.len() > viewport_height {
            let mut scrollbar_state = ScrollbarState::new(filtered_refs.len())
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
            "Tab switches field. Type to filter. j/k move. Enter compares. Esc closes.",
            Style::new().fg(text_muted_color()),
        )),
        Line::from(Span::styled(
            format!(
                "source: {}  destination: {}",
                app.branch_compare_source_ref.as_deref().unwrap_or("none"),
                app.branch_compare_destination_ref
                    .as_deref()
                    .unwrap_or("none")
            ),
            Style::new().fg(diff_context_color()),
        )),
    ]))
    .style(Style::new().bg(panel_color()))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(footer, chunks[3]);
}
