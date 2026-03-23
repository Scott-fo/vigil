use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Padding, Paragraph},
};

use crate::app::{App, RemoteSyncDirection, SnackbarVariant};

use super::layout::top_right_rect;
use super::{NOTICE_WIDTH, error_color, panel_color, primary_color, text_color, text_muted_color};

pub(super) fn render_status_line(frame: &mut Frame, app: &App, area: Rect) {
    let status = app.status_message.clone().unwrap_or_else(|| {
        format!(
            "{} changed file{}",
            app.files.len(),
            if app.files.len() == 1 { "" } else { "s" }
        )
    });
    let line = Paragraph::new(Line::from(Span::styled(
        status,
        Style::new().fg(text_muted_color()),
    )))
    .style(Style::new().bg(panel_color()))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(line, area);
}

pub(super) fn render_notifications(frame: &mut Frame, app: &App) {
    let mut top = frame.area().y + 1;

    if let Some(direction) = app.remote_sync {
        let label = match direction {
            RemoteSyncDirection::Pull => "Pulling from remote...",
            RemoteSyncDirection::Push => "Pushing to remote...",
        };
        let area = top_right_rect(NOTICE_WIDTH, 3, top, frame.area());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(primary_color()))
            .style(Style::new().bg(panel_color()));
        let inner = block.inner(area);
        frame.render_widget(ratatui::widgets::Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Text::from(Line::from(Span::styled(
                label,
                Style::new().fg(text_muted_color()),
            ))))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(0))),
            inner,
        );
        top = top.saturating_add(4);
    }

    if let Some(notice) = app.snackbar_notice.as_ref() {
        let area = top_right_rect(NOTICE_WIDTH, 3, top, frame.area());
        let border_color = match notice.variant {
            SnackbarVariant::Info => primary_color(),
            SnackbarVariant::Error => error_color(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color))
            .style(Style::new().bg(panel_color()));
        let inner = block.inner(area);
        frame.render_widget(ratatui::widgets::Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Text::from(Line::from(Span::styled(
                notice.message.clone(),
                Style::new().fg(text_color()),
            ))))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(0))),
            inner,
        );
    }
}
