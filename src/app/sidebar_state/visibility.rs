use super::super::{ActivePane, App};
use super::{
    focus::{row_for_file_path, row_for_path_or_nearest},
    viewport::scroll_to_make_row_visible,
};

impl App {
    pub(in crate::app) fn focus_sidebar_path_or_nearest(&mut self, path: &str) {
        let Some(row) =
            row_for_path_or_nearest(&self.sidebar_items, path, self.selected_sidebar_row)
        else {
            self.selected_sidebar_row = 0;
            self.sidebar_state.select(None);
            return;
        };
        self.selected_sidebar_row = row;
        self.sidebar_state.select(Some(row));
        self.ensure_selected_sidebar_item_visible(Some(row));
    }

    pub(in crate::app) fn sync_sidebar_state(&mut self) {
        let selected_path = self.selected_file().map(|file| file.path.as_str());
        let selected_row =
            selected_path.and_then(|path| row_for_file_path(&self.sidebar_items, path));
        self.selected_sidebar_row = selected_row.unwrap_or_else(|| {
            self.selected_sidebar_row
                .min(self.sidebar_items.len().saturating_sub(1))
        });
        let selected_row = (!self.sidebar_items.is_empty()).then_some(self.selected_sidebar_row);
        self.sidebar_state.select(selected_row);
        self.ensure_selected_sidebar_item_visible(selected_row);
    }

    pub(in crate::app) fn ensure_selected_sidebar_item_visible(
        &mut self,
        selected_row: Option<usize>,
    ) {
        let Some(selected_row) = selected_row else {
            return;
        };
        if self.sidebar_viewport_height == 0 {
            return;
        }

        self.sidebar_scroll = scroll_to_make_row_visible(
            self.sidebar_scroll,
            self.sidebar_viewport_height,
            selected_row,
        );
    }

    pub(in crate::app) fn toggle_sidebar_hidden(&mut self) {
        self.sidebar_hidden = !self.sidebar_hidden;
        if self.sidebar_hidden {
            self.active_pane = ActivePane::Diff;
        }
    }
}
