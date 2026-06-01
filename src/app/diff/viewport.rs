use super::super::{
    ActivePane, App, DiffViewMode,
    navigation::{scroll_u16, scroll_usize},
};
use crate::review::ReviewSeverity;
use unicode_width::UnicodeWidthStr;

const FALLBACK_DIFF_DISPLAY_WIDTH: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffViewport {
    pub(crate) mode: DiffViewMode,
    pub(crate) width: usize,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDiffViewport {
    pub mode: DiffViewMode,
    pub width: usize,
    pub start: usize,
    pub end: usize,
    pub visual_start: usize,
    pub visual_end: usize,
    pub rendered_line_count: usize,
    pub selected_index: usize,
    pub visible_display_indices: Vec<Option<usize>>,
}

impl App {
    pub(crate) fn current_diff_display_width(&self) -> usize {
        self.diff_viewport
            .map(|viewport| viewport.width)
            .unwrap_or(FALLBACK_DIFF_DISPLAY_WIDTH)
    }

    pub(crate) fn move_diff_selection(&mut self, delta: i32) {
        self.selected_diff_line_index = self.diff_view.move_selection(
            self.diff_view_mode,
            self.current_diff_display_width(),
            self.selected_diff_line_index,
            delta,
        );
    }

    pub fn update_diff_viewport(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        visible_start: usize,
        visible_end: usize,
    ) {
        self.diff_viewport = (width > 0 && visible_start < visible_end).then_some(DiffViewport {
            mode,
            width,
            start: visible_start,
            end: visible_end,
        });
    }

    pub fn prepare_diff_viewport(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        viewport_height: usize,
    ) -> Option<PreparedDiffViewport> {
        if width == 0 || viewport_height == 0 {
            return None;
        }

        let display_line_count = self.diff_view.rendered_lines(mode, width).len();
        if display_line_count == 0 {
            return None;
        }

        let visual_map = self.diff_visual_line_map(mode, width, display_line_count);
        let visual_line_count = visual_map.len();
        let max_scroll = visual_line_count
            .saturating_sub(viewport_height)
            .min(u16::MAX as usize) as u16;
        if self.diff_scroll > max_scroll {
            self.diff_scroll = max_scroll;
        }

        let selected_index = self
            .selected_diff_line_index
            .min(display_line_count.saturating_sub(1));
        let selected_visual_index = visual_map
            .iter()
            .position(|display_index| *display_index == Some(selected_index))
            .unwrap_or(selected_index);
        if self.active_pane == ActivePane::Diff {
            if selected_visual_index < self.diff_scroll as usize {
                self.diff_scroll = selected_visual_index.min(max_scroll as usize) as u16;
            } else {
                let visible_end = (self.diff_scroll as usize).saturating_add(viewport_height);
                if selected_visual_index >= visible_end {
                    self.diff_scroll = selected_visual_index
                        .saturating_add(1)
                        .saturating_sub(viewport_height)
                        .min(max_scroll as usize) as u16;
                }
            }
        }

        let visual_start = (self.diff_scroll as usize).min(max_scroll as usize);
        let visual_end = (visual_start + viewport_height).min(visual_line_count);
        if visual_start >= visual_end {
            return None;
        }
        let visible_display_indices = visual_map[visual_start..visual_end].to_vec();
        let mut visible_code_indices = visible_display_indices
            .iter()
            .filter_map(|display_index| *display_index);
        let first_visible_code = visible_code_indices.next();
        let (start, end) = if let Some(first) = first_visible_code {
            let mut last = first;
            for display_index in visible_code_indices {
                last = display_index;
            }
            (first, last.saturating_add(1).min(display_line_count))
        } else {
            let fallback = selected_index.min(display_line_count.saturating_sub(1));
            (fallback, fallback.saturating_add(1).min(display_line_count))
        };

        Some(PreparedDiffViewport {
            mode,
            width,
            start,
            end,
            visual_start,
            visual_end,
            rendered_line_count: visual_line_count,
            selected_index,
            visible_display_indices,
        })
    }

    fn diff_visual_line_map(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        display_line_count: usize,
    ) -> Vec<Option<usize>> {
        let mut map = Vec::with_capacity(display_line_count);
        for display_index in 0..display_line_count {
            map.push(Some(display_index));
            let comment_lines = self.review_comment_visual_line_count(mode, width, display_index);
            map.extend(std::iter::repeat_n(None, comment_lines));
        }
        map
    }

    fn review_comment_visual_line_count(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        display_index: usize,
    ) -> usize {
        self.review_comments_for_display_index_in_mode(mode, display_index, width)
            .iter()
            .map(|comment| {
                let text_width = review_comment_text_width(width);
                let heading = format!("{} · {}", severity_label(comment.severity), comment.title);
                wrapped_line_count(&heading, text_width)
                    + wrapped_line_count(&comment.body, text_width)
                    + 1
            })
            .sum()
    }

    pub(crate) fn page_diff(&mut self, delta: i32) {
        self.move_diff_selection(delta);
    }

    pub(crate) fn scroll_diff(&mut self, delta: i32) {
        self.diff_scroll = scroll_u16(self.diff_scroll, delta);
    }

    pub(crate) fn scroll_sidebar(&mut self, delta: i32) {
        self.sidebar_scroll = scroll_usize(self.sidebar_scroll, delta);
    }

    pub(crate) fn page_or_scroll_diff(&mut self, delta: i32) {
        match self.active_pane {
            ActivePane::Diff => self.page_diff(delta),
            ActivePane::Sidebar => self.scroll_diff(delta),
        }
    }
}

fn review_comment_text_width(width: usize) -> usize {
    width.saturating_sub(5).saturating_sub(2).max(16)
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    let width = width.max(1);
    let mut count = 0usize;
    let mut current_width = 0usize;

    for word in text.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if word_width > width {
            if current_width > 0 {
                count = count.saturating_add(1);
                current_width = 0;
            }
            count = count.saturating_add(word_width.div_ceil(width));
        } else if current_width == 0 {
            current_width = word_width;
        } else if current_width + 1 + word_width <= width {
            current_width += 1 + word_width;
        } else {
            count = count.saturating_add(1);
            current_width = word_width;
        }
    }

    if current_width > 0 {
        count = count.saturating_add(1);
    }

    count.max(1)
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
