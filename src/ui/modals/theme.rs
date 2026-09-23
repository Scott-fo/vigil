use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::App;

use super::super::{chip_color, primary_color, text_color, text_faint_color, text_subtle_color};
use super::frame::render_modal_frame;
use super::hints::render_hint_footer;
use super::list::{PickerLayout, render_list_message, render_visible_list};
use super::prompt::{Prompt, render_prompt};

pub(super) fn render_theme_modal(frame: &mut Frame, app: &mut App) {
    let inner = render_modal_frame(frame, 76, 22, "Theme");
    let layout = PickerLayout::new(inner, 3);
    let [prompt_row, _, mode_row] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(layout.prompt);

    render_prompt(
        frame,
        prompt_row,
        Prompt::new(&app.theme_modal_query, "Search themes"),
    );

    let mode = app.theme_mode.as_str();
    let mut mode_spans = vec![Span::styled(" mode  ", Style::new().fg(text_faint_color()))];
    for option in ["dark", "light"] {
        let style = if option == mode {
            Style::new()
                .fg(primary_color())
                .bg(chip_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(text_subtle_color())
        };
        mode_spans.push(Span::styled(format!(" {option} "), style));
        mode_spans.push(Span::raw(" "));
    }
    mode_spans.push(Span::styled(
        " m",
        Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
    ));
    mode_spans.push(Span::styled(" toggle", Style::new().fg(text_faint_color())));
    frame.render_widget(Paragraph::new(Line::from(mode_spans)), mode_row);

    let filtered_theme_names = app.filtered_theme_names();
    if filtered_theme_names.is_empty() {
        render_list_message(frame, layout.list, "No matching themes.");
    } else {
        let selected_index = app
            .theme_modal_selected_index
            .min(filtered_theme_names.len().saturating_sub(1));

        render_visible_list(
            frame,
            layout.list,
            filtered_theme_names.len(),
            selected_index,
            |display_index, selected| {
                let mut style = Style::new().fg(text_color());
                if selected {
                    style = style.add_modifier(Modifier::BOLD);
                }
                vec![Span::styled(
                    filtered_theme_names[display_index].to_string(),
                    style,
                )]
            },
        );
    }

    render_hint_footer(
        frame,
        layout.footer,
        &[("j/k", "preview"), ("⏎", "save"), ("esc", "restore")],
        Some(app.theme_name.clone()),
    );
}
