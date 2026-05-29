use super::super::{
    ActivePane, App, DiffViewMode,
    navigation::{scroll_u16, scroll_usize},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffViewport {
    pub(crate) mode: DiffViewMode,
    pub(crate) width: usize,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedDiffViewport {
    pub mode: DiffViewMode,
    pub width: usize,
    pub start: usize,
    pub end: usize,
    pub rendered_line_count: usize,
    pub selected_index: usize,
}

impl App {
    pub(crate) fn current_diff_display_width(&self) -> usize {
        self.diff_viewport
            .map(|viewport| viewport.width)
            .unwrap_or(usize::MAX)
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

        let rendered_lines = self.diff_view.rendered_lines(mode, width);
        if rendered_lines.is_empty() {
            return None;
        }

        let max_scroll = rendered_lines
            .len()
            .saturating_sub(viewport_height)
            .min(u16::MAX as usize) as u16;
        if self.diff_scroll > max_scroll {
            self.diff_scroll = max_scroll;
        }

        let selected_index = self
            .selected_diff_line_index
            .min(rendered_lines.len().saturating_sub(1));
        if self.active_pane == ActivePane::Diff {
            if selected_index < self.diff_scroll as usize {
                self.diff_scroll = selected_index.min(max_scroll as usize) as u16;
            } else {
                let visible_end = (self.diff_scroll as usize).saturating_add(viewport_height);
                if selected_index >= visible_end {
                    self.diff_scroll = selected_index
                        .saturating_add(1)
                        .saturating_sub(viewport_height)
                        .min(max_scroll as usize) as u16;
                }
            }
        }

        let visible_start = (self.diff_scroll as usize).min(max_scroll as usize);
        let visible_end = (visible_start + viewport_height).min(rendered_lines.len());
        if visible_start >= visible_end {
            return None;
        }

        Some(PreparedDiffViewport {
            mode,
            width,
            start: visible_start,
            end: visible_end,
            rendered_line_count: rendered_lines.len(),
            selected_index,
        })
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
