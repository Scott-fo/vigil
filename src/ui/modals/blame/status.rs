use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::App;

use super::super::super::{
    error_color, primary_color, text_color, text_faint_color, text_subtle_color, warning_color,
};

pub(super) fn render_blame_status(
    frame: &mut Frame,
    app: &App,
    subject_area: Rect,
    meta_area: Rect,
) {
    if app.blame_loading {
        render_status_line(
            frame,
            subject_area,
            "Loading blamed commit…",
            Style::new().fg(text_subtle_color()),
        );
        render_status_line(
            frame,
            meta_area,
            "Waiting for git blame and commit metadata…",
            Style::new().fg(text_faint_color()),
        );
    } else if let Some(error) = app.blame_error.as_ref() {
        render_status_line(
            frame,
            subject_area,
            "Unable to load blame details.",
            Style::new().fg(error_color()),
        );
        render_status_line(
            frame,
            meta_area,
            error.clone(),
            Style::new().fg(text_subtle_color()),
        );
    } else if let Some(details) = app.blame_details.as_ref() {
        let hash_color = if details.is_uncommitted {
            warning_color()
        } else {
            primary_color()
        };
        render_status_line(
            frame,
            subject_area,
            details.subject.clone(),
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(details.short_hash.clone(), Style::new().fg(hash_color)),
                Span::styled(
                    if details.date.is_empty() {
                        format!("  {}", details.author)
                    } else {
                        format!("  {}  {}", details.author, details.date)
                    },
                    Style::new().fg(text_subtle_color()),
                ),
            ]))
            .block(Block::new().padding(Padding::horizontal(1))),
            meta_area,
        );
    }
}

fn render_status_line(frame: &mut Frame, area: Rect, content: impl Into<String>, style: Style) {
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(content.into(), style)))
            .block(Block::new().padding(Padding::horizontal(1))),
        area,
    );
}
