use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::{app::App, git::WorktreeEntry};

use super::super::{
    diff_context_color, panel_color, primary_color, selected_list_item_text_color, success_color,
    text_color, text_muted_color, warning_color,
};
use super::frame::render_modal_frame;
use super::list::{
    render_list_error, render_list_frame, render_list_message, render_modal_input,
    render_visible_list,
};

pub(super) fn render_worktree_modal(frame: &mut Frame, app: &mut App) {
    let inner = render_modal_frame(frame, 96, 22, "Worktrees");

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    if app.worktree_query.is_empty() {
        render_modal_input(
            frame,
            chunks[0],
            "Search by branch, path, dirty, or clean...",
            true,
            false,
            None,
        );
    } else {
        render_modal_input(
            frame,
            chunks[0],
            app.worktree_query.clone(),
            false,
            false,
            None,
        );
    }

    let filtered_indices = app.filtered_worktree_indices();
    let list_inner = render_list_frame(frame, chunks[1]);

    if app.worktree_loading {
        render_list_message(frame, list_inner, "Loading worktrees...");
    } else if let Some(error) = app.worktree_error.as_ref() {
        render_list_error(frame, list_inner, "Unable to load worktrees.", error);
    } else if filtered_indices.is_empty() {
        render_list_message(frame, list_inner, "No matching worktrees.");
    } else {
        let selected_index = app
            .worktree_selected_index
            .min(filtered_indices.len().saturating_sub(1));

        render_visible_list(
            frame,
            list_inner,
            filtered_indices.len(),
            selected_index,
            |display_index, selected| {
                let entry_index = filtered_indices[display_index];
                let entry = &app.worktree_entries[entry_index];
                worktree_line(entry, selected, entry.path == app.repo_root)
            },
        );
    }

    let selected_label = filtered_indices
        .get(app.worktree_selected_index)
        .and_then(|index| app.worktree_entries.get(*index))
        .map(|entry| entry.path.display().to_string())
        .unwrap_or_else(|| "no selection".to_string());
    let footer = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "Type to filter. j/k move. Enter watches selection. Esc closes.",
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

fn worktree_line(entry: &WorktreeEntry, selected: bool, current: bool) -> Line<'static> {
    let base_style = if selected {
        Style::new()
            .bg(primary_color())
            .fg(selected_list_item_text_color())
    } else {
        Style::new().fg(text_color())
    };
    let state_style = if selected {
        base_style.add_modifier(Modifier::BOLD)
    } else if entry.dirty {
        Style::new()
            .fg(warning_color())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new()
            .fg(success_color())
            .add_modifier(Modifier::BOLD)
    };
    let muted_style = if selected {
        base_style
    } else {
        Style::new().fg(text_muted_color())
    };
    let name_style = if selected {
        base_style.add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(text_color()).add_modifier(Modifier::BOLD)
    };
    let state = if entry.dirty { "dirty" } else { "clean" };
    let marker = if current { "*" } else { " " };

    Line::from(vec![
        Span::styled(format!("{marker} "), muted_style),
        Span::styled(format!("{:<7}", state), state_style),
        Span::styled(format!(" {:<24}", worktree_name(entry)), name_style),
        Span::styled(entry.path.display().to_string(), base_style),
    ])
    .style(base_style)
}

fn worktree_name(entry: &WorktreeEntry) -> String {
    if let Some(branch) = entry.branch.as_ref() {
        return branch.clone();
    }
    if entry.detached {
        return entry
            .head
            .as_deref()
            .map(|head| format!("detached {}", head.chars().take(7).collect::<String>()))
            .unwrap_or_else(|| "detached".to_string());
    }
    if entry.bare {
        return "bare".to_string();
    }
    "unknown".to_string()
}
