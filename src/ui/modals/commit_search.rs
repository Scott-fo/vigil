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
    render_list_error, render_list_frame, render_list_message, render_modal_input,
    render_visible_list,
};

pub(super) fn render_commit_search_modal(frame: &mut Frame, app: &mut App) {
    let inner = render_modal_frame(frame, 92, 22, "Commit Search");

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    if app.commit_search_query.is_empty() {
        render_modal_input(
            frame,
            chunks[0],
            "Search by hash or subject...",
            true,
            false,
            None,
        );
    } else {
        render_modal_input(
            frame,
            chunks[0],
            app.commit_search_query.clone(),
            false,
            false,
            None,
        );
    }

    let filtered_indices = app.filtered_commit_search_indices();
    let list_inner = render_list_frame(frame, chunks[1]);

    if app.commit_search_loading {
        render_list_message(frame, list_inner, "Loading commits...");
    } else if let Some(error) = app.commit_search_error.as_ref() {
        render_list_error(frame, list_inner, "Unable to load commits.", error);
    } else if filtered_indices.is_empty() {
        render_list_message(frame, list_inner, "No matching commits.");
    } else {
        let selected_index = app
            .commit_search_selected_index
            .min(filtered_indices.len().saturating_sub(1));

        render_visible_list(
            frame,
            list_inner,
            filtered_indices.len(),
            selected_index,
            |display_index, selected| {
                let entry_index = filtered_indices[display_index];
                let commit = &app.commit_search_entries[entry_index];
                let base_style = if selected {
                    Style::new()
                        .bg(primary_color())
                        .fg(selected_list_item_text_color())
                } else {
                    Style::new().fg(text_color())
                };
                let hash_style = if selected {
                    Style::new()
                        .bg(primary_color())
                        .fg(selected_list_item_text_color())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                        .fg(primary_color())
                        .add_modifier(Modifier::BOLD)
                };

                Line::from(vec![
                    Span::styled(format!("{:<10}", commit.short_hash), hash_style),
                    Span::styled(" ", base_style),
                    Span::styled(commit.subject.clone(), base_style),
                ])
                .style(base_style)
            },
        );
    }

    let selected_label = filtered_indices
        .get(app.commit_search_selected_index)
        .and_then(|index| app.commit_search_entries.get(*index))
        .map(|commit| format!("selected {}", commit.short_hash))
        .unwrap_or_else(|| "no selection".to_string());
    let footer = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "Type to filter. j/k move. Enter selects. Esc closes.",
            Style::new().fg(text_muted_color()),
        )),
        Line::from(Span::styled(
            selected_label,
            Style::new().fg(diff_context_color()),
        )),
    ]))
    .style(Style::new().bg(panel_color()))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(footer, chunks[2]);
}
