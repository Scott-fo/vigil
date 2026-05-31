use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::App,
    review::{ReviewSeverity, ReviewVerdict},
};

use super::super::{
    error_color, panel_color, primary_color, success_color, text_color, text_muted_color,
    warning_color,
};
use super::frame::render_modal_frame;

pub(super) fn render_review_summary_modal(frame: &mut Frame, app: &mut App) {
    let inner = render_modal_frame(frame, 110, 28, "Codex Review");
    let lines = review_lines(app);
    clamp_review_summary_scroll(app, &lines, inner.width, inner.height);
    let paragraph = Paragraph::new(Text::from(lines))
        .style(Style::new().fg(text_color()).bg(panel_color()))
        .block(Block::new().padding(Padding::horizontal(1)))
        .wrap(Wrap { trim: false })
        .scroll((app.review_summary_scroll, 0));
    frame.render_widget(paragraph, inner);
}

fn clamp_review_summary_scroll(app: &mut App, lines: &[Line<'static>], width: u16, height: u16) {
    let content_width = usize::from(width).saturating_sub(2).max(1);
    let content_height = usize::from(height);
    let visual_lines = lines
        .iter()
        .map(|line| wrapped_line_count(line_width(line), content_width))
        .sum::<usize>();
    let max_scroll = visual_lines
        .saturating_sub(content_height)
        .min(u16::MAX as usize) as u16;
    if app.review_summary_scroll > max_scroll {
        app.review_summary_scroll = max_scroll;
    }
}

fn line_width(line: &Line<'static>) -> usize {
    line.spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

fn wrapped_line_count(width: usize, content_width: usize) -> usize {
    width.div_ceil(content_width).max(1)
}

fn review_lines(app: &App) -> Vec<Line<'static>> {
    if app.review_loading {
        return vec![
            Line::from(Span::styled(
                "Codex review running...",
                Style::new()
                    .fg(primary_color())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Results will appear here when the review finishes.",
                Style::new().fg(text_muted_color()),
            )),
        ];
    }

    if let Some(error) = app.review_error.as_deref() {
        return vec![
            Line::from(Span::styled(
                "Codex review failed",
                Style::new().fg(error_color()).add_modifier(Modifier::BOLD),
            )),
            Line::default(),
            Line::from(Span::styled(
                error.to_string(),
                Style::new().fg(text_color()),
            )),
        ];
    }

    let Some(report) = app.review_report.as_ref() else {
        return vec![Line::from(Span::styled(
            "No Codex review loaded.",
            Style::new().fg(text_muted_color()),
        ))];
    };

    let mut lines = vec![
        Line::from(Span::styled(
            report.summary.headline.clone(),
            Style::new()
                .fg(primary_color())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("Verdict ", Style::new().fg(text_muted_color())),
            Span::styled(
                verdict_label(report.summary.verdict),
                verdict_style(report.summary.verdict).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  {} comment{}",
                    report.findings.len(),
                    if report.findings.len() == 1 { "" } else { "s" }
                ),
                Style::new().fg(text_muted_color()),
            ),
        ]),
        Line::default(),
        Line::from(Span::styled(
            report.summary.body.clone(),
            Style::new().fg(text_color()),
        )),
    ];

    if !report.summary.risk_areas.is_empty() {
        lines.push(Line::default());
        lines.push(section_line("Risk Areas"));
        for area in &report.summary.risk_areas {
            lines.push(Line::from(vec![
                Span::styled("- ", Style::new().fg(text_muted_color())),
                Span::styled(area.clone(), Style::new().fg(text_color())),
            ]));
        }
    }

    if !report.findings.is_empty() {
        lines.push(Line::default());
        lines.push(section_line("Comments"));
        for finding in &report.findings {
            lines.push(Line::default());
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", severity_label(finding.severity)),
                    severity_style(finding.severity).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    finding.title.clone(),
                    Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                finding_location(finding),
                Style::new().fg(text_muted_color()),
            )));
            lines.push(Line::from(Span::styled(
                finding.body.clone(),
                Style::new().fg(text_color()),
            )));
        }
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "c copies. Esc closes. S reopens this summary.",
        Style::new().fg(text_muted_color()),
    )));
    lines
}

fn section_line(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
    ))
}

fn finding_location(finding: &crate::review::ReviewFinding) -> String {
    match (finding.line, finding.end_line) {
        (Some(line), Some(end_line)) if end_line != line => {
            format!("{}:{}-{}", finding.path, line, end_line)
        }
        (Some(line), _) => format!("{}:{}", finding.path, line),
        _ => finding.path.clone(),
    }
}

fn verdict_label(verdict: ReviewVerdict) -> &'static str {
    match verdict {
        ReviewVerdict::Clean => "clean",
        ReviewVerdict::HasConcerns => "has concerns",
        ReviewVerdict::NeedsWork => "needs work",
    }
}

fn verdict_style(verdict: ReviewVerdict) -> Style {
    match verdict {
        ReviewVerdict::Clean => Style::new().fg(success_color()),
        ReviewVerdict::HasConcerns => Style::new().fg(warning_color()),
        ReviewVerdict::NeedsWork => Style::new().fg(error_color()),
    }
}

fn severity_label(severity: ReviewSeverity) -> &'static str {
    match severity {
        ReviewSeverity::Critical => "critical",
        ReviewSeverity::High => "high",
        ReviewSeverity::Medium => "medium",
        ReviewSeverity::Low => "low",
        ReviewSeverity::Info => "info",
    }
}

fn severity_style(severity: ReviewSeverity) -> Style {
    match severity {
        ReviewSeverity::Critical | ReviewSeverity::High => Style::new().fg(error_color()),
        ReviewSeverity::Medium => Style::new().fg(warning_color()),
        ReviewSeverity::Low | ReviewSeverity::Info => Style::new().fg(text_muted_color()),
    }
}
