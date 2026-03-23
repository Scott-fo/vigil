use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
};

use crate::app::App;

use super::super::layout::centered_rect;
use super::super::{
    border_active_color, border_color, diff_context_color, error_color, panel_color, primary_color,
    text_color, text_muted_color, warning_color,
};

pub(super) fn render_blame_modal(frame: &mut Frame, app: &mut App) {
    let area = centered_rect(86, 20, frame.area());
    frame.render_widget(Clear, area);

    let title = app
        .blame_target
        .as_ref()
        .map(|target| format!(" Blame {}:{} ", target.file_path, target.line_number))
        .unwrap_or_else(|| " Blame ".to_string());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_active_color()))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            title,
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    if app.blame_loading {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Loading blamed commit...",
                Style::new().fg(text_muted_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Waiting for git blame and commit metadata...",
                Style::new().fg(diff_context_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            chunks[1],
        );
    } else if let Some(error) = app.blame_error.as_ref() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Unable to load blame details.",
                Style::new().fg(error_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                error.clone(),
                Style::new().fg(text_muted_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            chunks[1],
        );
    } else if let Some(details) = app.blame_details.as_ref() {
        let hash_color = if details.is_uncommitted {
            warning_color()
        } else {
            primary_color()
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                details.subject.clone(),
                Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            chunks[0],
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
                    Style::new().fg(text_muted_color()),
                ),
            ]))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            chunks[1],
        );
        let content_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color()))
            .style(Style::new().bg(panel_color()));
        let content_inner = content_block.inner(chunks[2]);
        frame.render_widget(content_block, chunks[2]);

        let body_lines = details
            .description
            .lines()
            .map(|line| {
                Line::from(Span::styled(
                    if line.is_empty() {
                        " ".to_string()
                    } else {
                        line.to_string()
                    },
                    Style::new().fg(text_color()),
                ))
            })
            .collect::<Vec<_>>();
        let viewport_height = content_inner.height as usize;
        let max_scroll = body_lines.len().saturating_sub(viewport_height);
        if app.blame_scroll as usize > max_scroll {
            app.blame_scroll = max_scroll as u16;
        }
        let visible_start = app.blame_scroll as usize;
        let visible_end = (visible_start + viewport_height).min(body_lines.len());
        let visible_lines = if body_lines.is_empty() {
            vec![Line::from(Span::styled(
                "No commit description.",
                Style::new().fg(text_muted_color()),
            ))]
        } else {
            body_lines[visible_start..visible_end].to_vec()
        };
        frame.render_widget(
            Paragraph::new(Text::from(visible_lines))
                .style(Style::new().bg(panel_color()))
                .block(Block::new().padding(Padding::horizontal(1))),
            content_inner,
        );

        if body_lines.len() > viewport_height {
            let mut scrollbar_state = ScrollbarState::new(body_lines.len())
                .position(visible_start)
                .viewport_content_length(viewport_height);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::new().fg(border_active_color()))
                .track_style(Style::new().fg(border_color()));
            frame.render_stateful_widget(scrollbar, content_inner, &mut scrollbar_state);
        }
    }

    let hint = if app.blame_loading {
        "Esc closes."
    } else if app
        .blame_details
        .as_ref()
        .and_then(|details| details.compare_selection.as_ref())
        .is_some()
    {
        "Enter or o opens commit compare. j/k scroll. Esc closes."
    } else {
        "No commit compare available for this line. j/k scroll. Esc closes."
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint,
            Style::new().fg(text_muted_color()),
        )))
        .style(Style::new().bg(panel_color()))
        .block(Block::new().padding(Padding::horizontal(1))),
        chunks[3],
    );
}
