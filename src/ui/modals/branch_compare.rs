use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::Span,
};

use crate::app::{App, BranchCompareField};

use super::super::text_color;
use super::frame::render_modal_frame;
use super::hints::render_hint_footer;
use super::list::{PickerLayout, render_list_error, render_list_message, render_visible_list};
use super::prompt::{Prompt, render_prompt};

const FIELD_LABEL_WIDTH: usize = "Destination".len();

pub(super) fn render_branch_compare_modal(frame: &mut Frame, app: &mut App) {
    let inner = render_modal_frame(frame, 84, 23, "Compare branches");
    let layout = PickerLayout::new(inner, 2);
    let [source_row, destination_row] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .areas(layout.prompt);

    let source_active = app.branch_compare_active_field == BranchCompareField::Source;
    render_prompt(
        frame,
        source_row,
        Prompt::new(
            &app.branch_compare_source_query,
            app.branch_compare_source_ref
                .as_deref()
                .unwrap_or("source ref"),
        )
        .active(source_active)
        .label("Source", FIELD_LABEL_WIDTH),
    );
    render_prompt(
        frame,
        destination_row,
        Prompt::new(
            &app.branch_compare_destination_query,
            app.branch_compare_destination_ref
                .as_deref()
                .unwrap_or("destination ref"),
        )
        .active(!source_active)
        .label("Destination", FIELD_LABEL_WIDTH),
    );

    let filtered_ref_indices = app.filtered_branch_compare_ref_indices();
    if app.branch_compare_loading {
        render_list_message(frame, layout.list, "Loading refs…");
    } else if let Some(error) = app.branch_compare_error.as_ref() {
        render_list_error(frame, layout.list, "Unable to load refs.", error);
    } else if filtered_ref_indices.is_empty() {
        render_list_message(frame, layout.list, "No matching refs.");
    } else {
        let selected_index = match app.branch_compare_active_field {
            BranchCompareField::Source => app.branch_compare_selected_source_index,
            BranchCompareField::Destination => app.branch_compare_selected_destination_index,
        }
        .min(filtered_ref_indices.len().saturating_sub(1));

        render_visible_list(
            frame,
            layout.list,
            filtered_ref_indices.len(),
            selected_index,
            |display_index, selected| {
                let ref_name =
                    &app.branch_compare_available_refs[filtered_ref_indices[display_index]];
                let mut style = Style::new().fg(text_color());
                if selected {
                    style = style.add_modifier(Modifier::BOLD);
                }
                vec![Span::styled(ref_name.clone(), style)]
            },
        );
    }

    let status = format!(
        "{} → {}",
        app.branch_compare_source_ref.as_deref().unwrap_or("?"),
        app.branch_compare_destination_ref.as_deref().unwrap_or("?"),
    );
    render_hint_footer(
        frame,
        layout.footer,
        &[
            ("tab", "switch field"),
            ("j/k", "move"),
            ("⏎", "compare"),
            ("esc", "close"),
        ],
        Some(status),
    );
}
