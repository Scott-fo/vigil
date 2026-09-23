use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::Span,
};

use crate::app::App;

use super::super::{text_color, text_subtle_color};
use super::frame::render_modal_frame;
use super::hints::render_hint_footer;
use super::list::{PickerLayout, render_list_error, render_list_message, render_visible_list};
use super::prompt::{Prompt, render_prompt};

pub(super) fn render_commit_search_modal(frame: &mut Frame, app: &mut App) {
    let inner = render_modal_frame(frame, 92, 22, "Compare commit");
    let layout = PickerLayout::new(inner, 1);

    render_prompt(
        frame,
        layout.prompt,
        Prompt::new(&app.commit_search_query, "Search by hash or subject"),
    );

    let filtered_indices = app.filtered_commit_search_indices();
    if app.commit_search_loading {
        render_list_message(frame, layout.list, "Loading commits…");
    } else if let Some(error) = app.commit_search_error.as_ref() {
        render_list_error(frame, layout.list, "Unable to load commits.", error);
    } else if filtered_indices.is_empty() {
        render_list_message(frame, layout.list, "No matching commits.");
    } else {
        let selected_index = app
            .commit_search_selected_index
            .min(filtered_indices.len().saturating_sub(1));

        render_visible_list(
            frame,
            layout.list,
            filtered_indices.len(),
            selected_index,
            |display_index, selected| {
                let commit = &app.commit_search_entries[filtered_indices[display_index]];
                let mut subject_style = Style::new().fg(text_color());
                if selected {
                    subject_style = subject_style.add_modifier(Modifier::BOLD);
                }
                vec![
                    Span::styled(
                        format!("{:<9}", commit.short_hash),
                        Style::new().fg(text_subtle_color()),
                    ),
                    Span::raw(" "),
                    Span::styled(commit.subject.clone(), subject_style),
                ]
            },
        );
    }

    let selected = filtered_indices
        .get(app.commit_search_selected_index)
        .and_then(|index| app.commit_search_entries.get(*index))
        .map(|commit| commit.short_hash.clone());
    render_hint_footer(
        frame,
        layout.footer,
        &[("j/k", "move"), ("⏎", "compare"), ("esc", "close")],
        selected,
    );
}
