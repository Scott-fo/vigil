use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::Span,
};

use crate::{app::App, git::WorktreeEntry};

use super::super::{
    primary_color, success_color, text_color, text_faint_color, text_subtle_color, warning_color,
};
use super::frame::render_modal_frame;
use super::hints::render_hint_footer;
use super::list::{PickerLayout, render_list_error, render_list_message, render_visible_list};
use super::prompt::{Prompt, render_prompt};

const NAME_WIDTH: usize = 28;

pub(super) fn render_worktree_modal(frame: &mut Frame, app: &mut App) {
    let inner = render_modal_frame(frame, 96, 22, "Worktrees");
    let layout = PickerLayout::new(inner, 1);

    render_prompt(
        frame,
        layout.prompt,
        Prompt::new(
            &app.worktree_query,
            "Search by branch, path, dirty, or clean",
        ),
    );

    let filtered_indices = app.filtered_worktree_indices();
    if app.worktree_loading {
        render_list_message(frame, layout.list, "Loading worktrees…");
    } else if let Some(error) = app.worktree_error.as_ref() {
        render_list_error(frame, layout.list, "Unable to load worktrees.", error);
    } else if filtered_indices.is_empty() {
        render_list_message(frame, layout.list, "No matching worktrees.");
    } else {
        let selected_index = app
            .worktree_selected_index
            .min(filtered_indices.len().saturating_sub(1));

        render_visible_list(
            frame,
            layout.list,
            filtered_indices.len(),
            selected_index,
            |display_index, selected| {
                let entry = &app.worktree_entries[filtered_indices[display_index]];
                worktree_row(entry, selected, entry.path == app.repo_root)
            },
        );
    }

    render_hint_footer(
        frame,
        layout.footer,
        &[("j/k", "move"), ("⏎", "watch"), ("esc", "close")],
        Some("● current  ◆ dirty".to_string()).filter(|_| !filtered_indices.is_empty()),
    );
}

fn worktree_row(entry: &WorktreeEntry, selected: bool, current: bool) -> Vec<Span<'static>> {
    let current_marker = if current {
        Span::styled("● ", Style::new().fg(primary_color()))
    } else {
        Span::raw("  ")
    };
    let state = if entry.dirty {
        Span::styled("◆ ", Style::new().fg(warning_color()))
    } else {
        Span::styled("◇ ", Style::new().fg(success_color()))
    };
    let mut name_style = Style::new().fg(text_color());
    if selected || current {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }
    let name = worktree_name(entry);
    let padding = NAME_WIDTH.saturating_sub(name.chars().count()).max(2);

    vec![
        current_marker,
        state,
        Span::styled(name, name_style),
        Span::raw(" ".repeat(padding)),
        Span::styled(
            entry.path.display().to_string(),
            Style::new().fg(if selected {
                text_subtle_color()
            } else {
                text_faint_color()
            }),
        ),
    ]
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
