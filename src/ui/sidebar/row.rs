use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use unicode_width::UnicodeWidthStr;

use crate::{
    git,
    sidebar::{SidebarItem, SidebarSection},
};

use super::{
    super::{
        chip_color, hover_color, primary_color, selection_color, success_color, text_color,
        text_faint_color, text_subtle_color, warning_color,
    },
    text::{devicon_for_path, display_width, truncate_middle},
};

const INDENT_UNIT: &str = "  ";
const ACCENT_BAR: &str = "▎";
/// Status letters render as a three-cell ` M ` slot so staged chips align.
const STATUS_SLOT_WIDTH: usize = 3;

/// How the row relates to the sidebar cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowSelection {
    None,
    /// Cursor row while the sidebar has focus: accent bar plus tinted row.
    Focused,
    /// Cursor row while another pane has focus: quiet tint only.
    Unfocused,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RowContext {
    pub(super) width: u16,
    pub(super) selection: RowSelection,
    /// The mouse is over this row. Selection tints take precedence.
    pub(super) hovered: bool,
    pub(super) review_comment_count: usize,
    /// Marked viewed at its current diff; the row dims and shows a check.
    pub(super) viewed: bool,
}

pub(super) fn row_line(item: &SidebarItem, context: RowContext) -> Line<'static> {
    let background = match context.selection {
        RowSelection::None if context.hovered => Some(hover_color()),
        RowSelection::None => None,
        RowSelection::Focused => Some(selection_color()),
        RowSelection::Unfocused => Some(chip_color()),
    };
    let accent = match context.selection {
        RowSelection::Focused => Span::styled(ACCENT_BAR, Style::new().fg(primary_color())),
        RowSelection::None | RowSelection::Unfocused => Span::raw(" "),
    };

    let mut spans = vec![accent];
    match item {
        SidebarItem::Section {
            section,
            file_count,
            collapsed,
        } => spans.extend(section_spans(*section, *file_count, *collapsed, context)),
        SidebarItem::Header {
            label,
            depth,
            collapsed,
            matches_search,
            ..
        } => spans.extend(header_spans(label, *depth, *collapsed, *matches_search)),
        SidebarItem::File {
            section,
            file,
            label,
            depth,
            matches_search,
            ..
        } => spans.extend(file_spans(
            file,
            label,
            *depth,
            *matches_search,
            *section,
            context,
        )),
    }

    let line = Line::from(spans);
    match background {
        Some(color) => line.style(Style::new().bg(color)),
        None => line,
    }
}

fn indent(depth: usize) -> String {
    format!(" {}", INDENT_UNIT.repeat(depth))
}

/// `▾ Staged ········ 3`: a heading row that groups files by staging state.
fn section_spans(
    section: SidebarSection,
    file_count: usize,
    collapsed: bool,
    context: RowContext,
) -> Vec<Span<'static>> {
    let chevron = if collapsed { "▸ " } else { "▾ " };
    let count = file_count.to_string();
    // accent + leading space + chevron + label + gap + count + trailing space
    let used = 1 + 1 + 2 + section.label().width() + count.width() + 1;
    let gap = (context.width as usize).saturating_sub(used).max(1);
    vec![
        Span::raw(" "),
        Span::styled(chevron, Style::new().fg(text_faint_color())),
        Span::styled(
            section.label(),
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(gap)),
        Span::styled(count, Style::new().fg(text_faint_color())),
        Span::raw(" "),
    ]
}

fn header_spans(
    label: &str,
    depth: usize,
    collapsed: bool,
    matches_search: bool,
) -> Vec<Span<'static>> {
    let chevron = if collapsed { "▸ " } else { "▾ " };
    let label_style = if matches_search {
        Style::new().fg(text_color()).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(text_subtle_color())
    };

    vec![
        Span::raw(indent(depth)),
        Span::styled(chevron, Style::new().fg(text_faint_color())),
        Span::styled(label.to_string(), label_style),
    ]
}

fn file_spans(
    file: &git::FileEntry,
    label: &str,
    depth: usize,
    matches_search: bool,
    section: Option<SidebarSection>,
    context: RowContext,
) -> Vec<Span<'static>> {
    let indent = indent(depth);
    // Grouped sidebars list a partially staged file under Unstaged; the half
    // circle says some of its changes are already in the index.
    let partially_staged = section == Some(SidebarSection::Unstaged)
        && git::stage_state(&file.status) == git::StageState::PartiallyStaged;
    let status = git::status_label(&file.status);
    let status_color = git::status_color(&file.status);
    let deleted = status == "D";

    let (icon, icon_color) = devicon_for_path(&file.path)
        .map(|(icon, color)| (format!("{icon} "), color))
        .unwrap_or_else(|| ("· ".to_string(), text_faint_color()));
    let review_marker = if context.review_comment_count > 0 {
        "● "
    } else {
        ""
    };
    let viewed_marker = if context.viewed { "✓ " } else { "" };
    let stage_marker = if partially_staged { "◐" } else { "" };

    // accent + indent + icon + label + gap + markers + status slot
    let fixed_width = 1
        + display_width(&indent)
        + display_width(&icon)
        + display_width(review_marker)
        + display_width(viewed_marker)
        + display_width(stage_marker)
        + STATUS_SLOT_WIDTH;
    let label_width = (context.width as usize)
        .saturating_sub(fixed_width + 1)
        .max(1);
    let display_label = truncate_middle(label, label_width);
    let gap = (context.width as usize)
        .saturating_sub(fixed_width + display_width(&display_label))
        .max(1);

    let mut label_style = Style::new().fg(if deleted || context.viewed {
        text_faint_color()
    } else {
        text_color()
    });
    if deleted {
        label_style = label_style.add_modifier(Modifier::CROSSED_OUT);
    }
    if matches_search || context.selection != RowSelection::None {
        label_style = label_style.add_modifier(Modifier::BOLD);
    }

    let mut spans = vec![
        Span::raw(indent),
        Span::styled(
            icon,
            Style::new().fg(if context.viewed {
                text_faint_color()
            } else {
                icon_color
            }),
        ),
        Span::styled(display_label, label_style),
        Span::raw(" ".repeat(gap)),
    ];
    if !review_marker.is_empty() {
        spans.push(Span::styled(
            review_marker,
            Style::new().fg(warning_color()),
        ));
    }
    if !viewed_marker.is_empty() {
        spans.push(Span::styled(
            viewed_marker,
            Style::new().fg(success_color()),
        ));
    }
    if !stage_marker.is_empty() {
        spans.push(Span::styled(
            stage_marker,
            Style::new().fg(text_subtle_color()),
        ));
    }
    spans.push(Span::styled(
        format!(" {status:1} "),
        Style::new().fg(if context.viewed {
            text_faint_color()
        } else {
            status_color
        }),
    ));
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_item(path: &str, status: &str, depth: usize) -> SidebarItem {
        SidebarItem::File {
            section: None,
            file: git::FileEntry {
                status: status.to_string(),
                path: path.to_string(),
                label: path.rsplit('/').next().unwrap_or(path).to_string(),
                filetype: None,
            },
            label: path.rsplit('/').next().unwrap_or(path).to_string(),
            depth,
            pos_in_set: 0,
            set_size: 1,
            matches_search: false,
        }
    }

    fn context(width: u16) -> RowContext {
        RowContext {
            width,
            selection: RowSelection::None,
            hovered: false,
            review_comment_count: 0,
            viewed: false,
        }
    }

    #[test]
    fn file_rows_fill_the_row_width_exactly() {
        for (width, review_comment_count, viewed) in [
            (24, 0, false),
            (24, 2, false),
            (40, 0, false),
            (24, 0, true),
            (24, 2, true),
        ] {
            let line = row_line(
                &file_item("src/ui/JavaScriptSyntaxHighlighter.tsx", " M", 2),
                RowContext {
                    review_comment_count,
                    viewed,
                    ..context(width)
                },
            );

            assert_eq!(line.width(), width as usize);
        }
    }

    #[test]
    fn status_letter_stays_visible_when_label_is_truncated() {
        let line = row_line(
            &file_item("src/a_very_long_file_name_that_will_not_fit.rs", "A ", 3),
            context(24),
        );
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert!(text.contains('…'));
        assert!(text.ends_with(" A "));
    }

    #[test]
    fn only_the_focused_cursor_row_shows_the_accent_bar() {
        let item = file_item("src/main.rs", " M", 0);
        let focused = row_line(
            &item,
            RowContext {
                selection: RowSelection::Focused,
                ..context(30)
            },
        );
        let unfocused = row_line(
            &item,
            RowContext {
                selection: RowSelection::Unfocused,
                ..context(30)
            },
        );

        assert_eq!(focused.spans[0].content, ACCENT_BAR);
        assert_eq!(unfocused.spans[0].content, " ");
        assert!(unfocused.style.bg.is_some());
    }

    #[test]
    fn hovered_rows_get_the_hover_tint_but_selection_wins() {
        let item = file_item("src/main.rs", " M", 0);
        let plain = row_line(&item, context(30));
        let hovered = row_line(
            &item,
            RowContext {
                hovered: true,
                ..context(30)
            },
        );
        let hovered_and_selected = row_line(
            &item,
            RowContext {
                hovered: true,
                selection: RowSelection::Focused,
                ..context(30)
            },
        );

        assert_eq!(plain.style.bg, None);
        assert_eq!(hovered.style.bg, Some(hover_color()));
        assert_eq!(hovered_and_selected.style.bg, Some(selection_color()));
    }
}
