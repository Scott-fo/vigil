use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::Span,
};

use crate::{app::App, git};

use super::super::{text_color, text_subtle_color};
use super::frame::render_modal_frame;
use super::hints::render_hint_footer;
use super::list::{PickerLayout, render_list_message, render_visible_list};
use super::prompt::{Prompt, render_prompt};

pub(super) fn render_file_search_modal(frame: &mut Frame, app: &mut App) {
    let inner = render_modal_frame(frame, 92, 22, "Find file");
    let layout = PickerLayout::new(inner, 1);

    render_prompt(
        frame,
        layout.prompt,
        Prompt::new(
            &app.file_search_query,
            "Search changed files by name or path",
        ),
    );

    let filtered_indices = app.filtered_file_search_indices();
    if filtered_indices.is_empty() {
        render_list_message(frame, layout.list, "No matching files.");
    } else {
        let selected_index = app
            .file_search_selected_index
            .min(filtered_indices.len().saturating_sub(1));

        render_visible_list(
            frame,
            layout.list,
            filtered_indices.len(),
            selected_index,
            |display_index, selected| {
                let file = &app.files[filtered_indices[display_index]];
                let directory = file
                    .path
                    .strip_suffix(file.label.as_str())
                    .unwrap_or_default()
                    .to_string();
                let mut name_style = Style::new().fg(text_color());
                if selected {
                    name_style = name_style.add_modifier(Modifier::BOLD);
                }
                vec![
                    Span::styled(
                        format!("{:1}  ", git::status_label(&file.status)),
                        Style::new().fg(git::status_color(&file.status)),
                    ),
                    Span::styled(file.label.clone(), name_style),
                    Span::styled(
                        format!("  {}", directory.trim_end_matches('/')),
                        Style::new().fg(text_subtle_color()),
                    ),
                ]
            },
        );
    }

    render_hint_footer(
        frame,
        layout.footer,
        &[("j/k", "preview"), ("⏎", "keep"), ("esc", "restore")],
        Some(format!("{} of {}", filtered_indices.len(), app.files.len())),
    );
}
