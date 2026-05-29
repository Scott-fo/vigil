use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::App;

use super::super::{error_color, panel_color, text_color, text_muted_color, warning_color};
use super::frame::render_modal_frame;

pub(super) fn render_branch_merge_modal(frame: &mut Frame, app: &App) {
    let Some(target) = app.branch_merge_target.as_ref() else {
        return;
    };

    let inner = render_modal_frame(frame, 78, 12, "Prepare Branch Merge");
    let status_line = if app.branch_merge_loading {
        Line::from(Span::styled(
            "Merging...",
            Style::new()
                .fg(warning_color())
                .add_modifier(Modifier::BOLD),
        ))
    } else if let Some(error) = app.branch_merge_error.as_ref() {
        Line::from(Span::styled(error.clone(), Style::new().fg(error_color())))
    } else {
        Line::from(Span::styled(
            "Enter prepares the merge without committing. Esc cancels.",
            Style::new().fg(text_muted_color()),
        ))
    };

    let content = vec![
        Line::from(Span::styled(
            "This will switch to the destination branch, then merge:",
            Style::new().fg(text_color()),
        )),
        Line::default(),
        ref_line("source", &target.source_ref),
        ref_line("destination", &target.destination_ref),
        Line::default(),
        Line::from(Span::styled(
            "Clean merges stay staged for review. Conflicts open in the working tree.",
            Style::new().fg(text_muted_color()),
        )),
        Line::default(),
        status_line,
    ];
    let paragraph = Paragraph::new(Text::from(content))
        .style(Style::new().bg(panel_color()))
        .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(paragraph, inner);
}

fn ref_line(label: &'static str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::new().fg(text_muted_color())),
        Span::styled(value.to_string(), Style::new().fg(warning_color())),
    ])
}
