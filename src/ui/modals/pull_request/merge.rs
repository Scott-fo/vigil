use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph, Wrap},
};

use crate::{
    app::{AutoMergeChoice, MergeBlocker, MergeForm, MergeStep, MergeWhen},
    forge::{HeadBranchAction, MergeMethod, PullRequest},
    ui::{
        error_color, primary_color, success_color, text_color, text_faint_color, text_subtle_color,
        warning_color,
    },
};

use super::super::{
    frame::{render_danger_modal_frame, render_modal_frame},
    hints::{KeyHint, render_hint_footer},
};
use super::short_oid;

pub(super) struct MergeModal<'a> {
    pub(super) form: &'a MergeForm,
    pub(super) detail: &'a PullRequest,
    pub(super) blockers: &'a [MergeBlocker],
    pub(super) auto_merge: AutoMergeChoice,
    pub(super) head_oid: &'a str,
    pub(super) has_new_commits: bool,
    pub(super) merging: bool,
}

const LABEL_WIDTH: usize = 9;

pub(super) fn render_merge(frame: &mut Frame, modal: MergeModal<'_>) {
    let form = modal.form;
    let summary = &modal.detail.summary;
    let title = format!("Merge #{}", summary.number);
    let strong = Style::new().fg(text_color()).add_modifier(Modifier::BOLD);
    let faint = Style::new().fg(text_faint_color());
    let subtle = Style::new().fg(text_subtle_color());
    let label = |text: &str| Span::styled(format!("{text:<LABEL_WIDTH$}"), faint);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(summary.base_ref_name.clone(), strong),
            Span::styled(" ← ", faint),
            Span::styled(summary.head_ref_name.clone(), strong),
            Span::styled(
                format!("  ·  pinned to {}", short_oid(modal.head_oid)),
                faint,
            ),
        ]),
        Line::default(),
    ];

    let allowed = modal
        .detail
        .merge_settings
        .allowed_methods
        .iter()
        .map(|method| method_label(*method))
        .collect::<Vec<_>>()
        .join(", ");
    lines.push(Line::from(vec![
        label("Method"),
        Span::styled(format!("‹ {} ›", method_label(form.method())), strong),
        Span::styled(format!("   allowed: {allowed}"), faint),
    ]));

    match form.when() {
        MergeWhen::Now => lines.push(Line::from(vec![
            label("Branch"),
            match form.head_branch() {
                HeadBranchAction::Delete => Span::styled(
                    format!("delete {} on GitHub after merging", summary.head_ref_name),
                    Style::new().fg(text_color()),
                ),
                HeadBranchAction::Keep => {
                    Span::styled(format!("keep {}", summary.head_ref_name), subtle)
                }
            },
        ])),
        MergeWhen::WhenReady => lines.push(Line::from(vec![
            label("Branch"),
            Span::styled(
                if modal.detail.merge_settings.delete_branch_on_merge {
                    "GitHub deletes it after merging (repository setting)"
                } else {
                    "kept (auto-merge follows the repository setting)"
                },
                subtle,
            ),
        ])),
    }

    match modal.auto_merge {
        AutoMergeChoice::Offered => lines.push(Line::from(vec![
            label("When"),
            when_span(MergeWhen::Now, form.when()),
            Span::raw("   "),
            when_span(MergeWhen::WhenReady, form.when()),
        ])),
        AutoMergeChoice::Enabled { .. } => {
            let auto_merge = modal.detail.auto_merge.as_ref();
            let mut text = format!(
                "auto-merge is on ({})",
                auto_merge.map_or("?", |auto| method_label(auto.method))
            );
            if let Some(by) = auto_merge.and_then(|auto| auto.enabled_by.as_ref()) {
                text.push_str(&format!(" · enabled by {by}"));
            }
            lines.push(Line::from(vec![
                label("When"),
                Span::styled(text, Style::new().fg(primary_color())),
            ]));
        }
        AutoMergeChoice::Unavailable => {}
    }

    lines.push(Line::default());
    if modal.blockers.is_empty() {
        lines.push(Line::from(Span::styled(
            "✓ nothing blocks a merge",
            Style::new().fg(success_color()),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "BLOCKING",
            faint.add_modifier(Modifier::BOLD),
        )));
        for blocker in modal.blockers {
            let color = if blocker.is_final() {
                error_color()
            } else {
                warning_color()
            };
            lines.push(Line::from(vec![
                Span::styled("  · ", faint),
                Span::styled(blocker.to_string(), Style::new().fg(color)),
            ]));
        }
    }
    if modal.has_new_commits {
        lines.push(Line::from(Span::styled(
            "⚠ GitHub has newer commits than the head shown, so it will refuse the merge; r reloads.",
            Style::new().fg(warning_color()),
        )));
    }

    lines.push(Line::default());
    let danger = Style::new().fg(error_color()).add_modifier(Modifier::BOLD);
    if modal.merging {
        lines.push(Line::from(Span::styled(
            "Working…",
            Style::new().fg(warning_color()),
        )));
    } else {
        match form.step() {
            MergeStep::ConfirmMerge => {
                let action = match form.when() {
                    MergeWhen::Now => format!(
                        "⏎ again merges #{} into {} with {}{}.",
                        summary.number,
                        summary.base_ref_name,
                        method_label(form.method()),
                        match form.head_branch() {
                            HeadBranchAction::Delete => ", deleting the head branch",
                            HeadBranchAction::Keep => "",
                        }
                    ),
                    MergeWhen::WhenReady => format!(
                        "⏎ again enables auto-merge ({}) on #{}.",
                        method_label(form.method()),
                        summary.number
                    ),
                };
                lines.push(Line::from(Span::styled(action, danger)));
            }
            MergeStep::ConfirmDisableAutoMerge => lines.push(Line::from(Span::styled(
                format!("⏎ again turns auto-merge off on #{}.", summary.number),
                danger,
            ))),
            MergeStep::Choose => {}
        }
        if let Some(error) = form.error() {
            lines.push(Line::from(Span::styled(
                error.to_string(),
                Style::new().fg(error_color()),
            )));
        }
    }

    // Content, a blank row, the footer, the frame, and the row under the
    // title; one spare row for a wrapped warning.
    let height = lines.len() as u16 + 6;
    let inner = match form.step() {
        MergeStep::Choose => render_modal_frame(frame, 84, height, title),
        MergeStep::ConfirmMerge | MergeStep::ConfirmDisableAutoMerge => {
            render_danger_modal_frame(frame, 84, height, title)
        }
    };
    let [body, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .areas(inner);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(Block::new().padding(Padding::horizontal(1))),
        body,
    );

    let hints: Vec<KeyHint> = match form.step() {
        MergeStep::Choose => {
            let mut hints = vec![("←/→", "method")];
            if form.when() == MergeWhen::Now {
                hints.push(("d", "delete branch"));
            }
            match modal.auto_merge {
                AutoMergeChoice::Offered => hints.push(("w", "when ready")),
                AutoMergeChoice::Enabled { can_disable: true } => {
                    hints.push(("x", "turn off auto-merge"))
                }
                _ => {}
            }
            hints.extend([("⏎", "merge"), ("esc", "cancel")]);
            hints
        }
        MergeStep::ConfirmMerge | MergeStep::ConfirmDisableAutoMerge => {
            vec![("⏎", "confirm"), ("esc", "back")]
        }
    };
    render_hint_footer(frame, footer, &hints, None);
}

fn when_span(when: MergeWhen, chosen: MergeWhen) -> Span<'static> {
    let label = match when {
        MergeWhen::Now => "now",
        MergeWhen::WhenReady => "when ready (auto-merge)",
    };
    if when == chosen {
        Span::styled(
            format!("● {label}"),
            Style::new()
                .fg(primary_color())
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(format!("○ {label}"), Style::new().fg(text_subtle_color()))
    }
}

fn method_label(method: MergeMethod) -> &'static str {
    match method {
        MergeMethod::Merge => "merge commit",
        MergeMethod::Squash => "squash",
        MergeMethod::Rebase => "rebase",
    }
}
