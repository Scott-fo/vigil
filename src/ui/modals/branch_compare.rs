use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::{App, BranchCompareField};

use super::super::{
    diff_context_color, panel_color, primary_color, selected_list_item_text_color, text_color,
    text_muted_color,
};
use super::frame::render_modal_frame;
use super::list::{
    render_list_error, render_list_frame, render_list_message, render_modal_input,
    render_visible_list,
};

pub(super) fn render_branch_compare_modal(frame: &mut Frame, app: &mut App) {
    let inner = render_modal_frame(frame, 84, 23, "Branch Compare");

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
    render_modal_input(
        frame,
        chunks[0],
        source_display,
        app.branch_compare_source_query.is_empty(),
        source_active,
        Some(" Source "),
    );

    let destination_active = app.branch_compare_active_field == BranchCompareField::Destination;
    let destination_display = if app.branch_compare_destination_query.is_empty() {
        app.branch_compare_destination_ref
            .clone()
            .unwrap_or_else(|| "Destination ref".to_string())
    } else {
        app.branch_compare_destination_query.clone()
    };
    render_modal_input(
        frame,
        chunks[1],
        destination_display,
        app.branch_compare_destination_query.is_empty(),
        destination_active,
        Some(" Destination "),
    );

    let filtered_refs = app.filtered_branch_compare_refs();
    let list_inner = render_list_frame(frame, chunks[2]);

    if app.branch_compare_loading {
        render_list_message(frame, list_inner, "Loading refs...");
    } else if let Some(error) = app.branch_compare_error.as_ref() {
        render_list_error(frame, list_inner, "Unable to load refs.", error);
    } else if filtered_refs.is_empty() {
        render_list_message(frame, list_inner, "No matching refs.");
    } else {
        let selected_index = match app.branch_compare_active_field {
            BranchCompareField::Source => app.branch_compare_selected_source_index,
            BranchCompareField::Destination => app.branch_compare_selected_destination_index,
        }
        .min(filtered_refs.len().saturating_sub(1));

        render_visible_list(
            frame,
            list_inner,
            filtered_refs.len(),
            selected_index,
            |display_index, selected| {
                let ref_name = &filtered_refs[display_index];
                let style = if selected {
                    Style::new()
                        .bg(primary_color())
                        .fg(selected_list_item_text_color())
                } else {
                    Style::new().fg(text_color())
                };
                Line::from(Span::styled(ref_name.clone(), style)).style(style)
            },
        );
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
