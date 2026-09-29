use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::review::ThreadRow;

use super::{
    super::{
        primary_color, success_color, text_color, text_faint_color, text_subtle_color,
        warning_color,
    },
    labels::relative_time,
};

const HEADING_PREFIX: &str = "  ╭─ ";
const BODY_PREFIX: &str = "  │  ";
const END_PREFIX: &str = "  ╰─";
const COLLAPSED_PREFIX: &str = "  ── ";

/// One row of a review thread box drawn under a diff line. Every
/// [`ThreadRow`] becomes exactly one terminal row, which the diff viewport
/// relies on when it counts rows.
pub(in crate::ui) fn thread_row_line(row: &ThreadRow, now: i64) -> Line<'static> {
    let chrome = Style::new().fg(text_faint_color());
    match row {
        ThreadRow::Collapsed {
            resolved_by,
            comment_count,
        } => {
            let mut spans = vec![
                Span::styled(COLLAPSED_PREFIX, chrome),
                Span::styled("✓ resolved", Style::new().fg(success_color())),
                Span::styled(format!(" · {}", comment_label(*comment_count)), chrome),
            ];
            if let Some(by) = resolved_by {
                spans.push(Span::styled(format!(" · by {by}"), chrome));
            }
            Line::from(spans)
        }
        ThreadRow::Heading {
            start_line,
            end_line,
            comment_count,
        } => {
            let mut spans = vec![
                Span::styled(HEADING_PREFIX, chrome),
                Span::styled("●", Style::new().fg(warning_color())),
            ];
            if let (Some(start), Some(end)) = (start_line, end_line) {
                spans.push(Span::styled(
                    format!(" lines {start}–{end} ·"),
                    Style::new().fg(text_subtle_color()),
                ));
            }
            spans.push(Span::styled(
                format!(" {}", comment_label(*comment_count)),
                Style::new().fg(text_subtle_color()),
            ));
            Line::from(spans)
        }
        ThreadRow::DraftHeading {
            start_line,
            end_line,
        } => {
            let mut spans = vec![
                Span::styled(HEADING_PREFIX, chrome),
                Span::styled(
                    "◌ pending",
                    Style::new()
                        .fg(primary_color())
                        .add_modifier(Modifier::ITALIC),
                ),
            ];
            match (start_line, end_line) {
                (Some(start), Some(end)) => spans.push(Span::styled(
                    format!(" · lines {start}–{end}"),
                    Style::new().fg(text_subtle_color()),
                )),
                (None, Some(end)) => spans.push(Span::styled(
                    format!(" · line {end}"),
                    Style::new().fg(text_subtle_color()),
                )),
                _ => {}
            }
            spans.push(Span::styled(" · your draft", chrome));
            Line::from(spans)
        }
        ThreadRow::Author {
            author,
            created_at,
            pending,
        } => {
            let mut spans = vec![
                Span::styled(BODY_PREFIX, chrome),
                Span::styled(
                    author.clone(),
                    Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" · {}", relative_time(created_at, now)), chrome),
            ];
            if *pending {
                spans.push(Span::styled(" · pending", Style::new().fg(warning_color())));
            }
            Line::from(spans)
        }
        ThreadRow::Body(text) => Line::from(vec![
            Span::styled(BODY_PREFIX, chrome),
            Span::styled(text.clone(), Style::new().fg(text_subtle_color())),
        ]),
        ThreadRow::Gap => Line::from(Span::styled(BODY_PREFIX.trim_end().to_string(), chrome)),
        ThreadRow::End => Line::from(Span::styled(END_PREFIX, chrome)),
    }
}

fn comment_label(count: usize) -> String {
    format!("{count} comment{}", if count == 1 { "" } else { "s" })
}
