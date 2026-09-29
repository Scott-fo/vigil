use super::super::{
    ActivePane, App, DiffLineWrapMode, DiffViewMode,
    navigation::{scroll_u16, scroll_usize},
};

const FALLBACK_DIFF_DISPLAY_WIDTH: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffViewport {
    pub(crate) mode: DiffViewMode,
    pub(crate) width: usize,
    pub(crate) line_wrap: DiffLineWrapMode,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDiffViewport {
    pub mode: DiffViewMode,
    pub width: usize,
    pub line_wrap: DiffLineWrapMode,
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
            self.diff_line_wrap_mode,
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
            line_wrap: self.diff_line_wrap_mode,
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

        let line_wrap = self.diff_line_wrap_mode;
        let display_line_count = self.diff_view.rendered_lines(mode, width, line_wrap).len();
        if display_line_count == 0 {
            return None;
        }

        let visual_map = Self::diff_visual_line_map(display_line_count);
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
            line_wrap,
            start,
            end,
            visual_start,
            visual_end,
            rendered_line_count: visual_line_count,
            selected_index,
            visible_display_indices,
        })
    }

    /// Maps each visual row of the diff pane to the display line it shows.
    /// `None` marks a row that belongs to no diff line; every row is currently
    /// a diff line, so the map is the identity.
    fn diff_visual_line_map(display_line_count: usize) -> Vec<Option<usize>> {
        (0..display_line_count).map(Some).collect()
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
