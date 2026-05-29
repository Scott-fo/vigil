use crate::sidebar::SidebarItem;

use super::super::{App, navigation::move_index};

impl App {
    pub(in crate::app) async fn select_sidebar_row(
        &mut self,
        index: usize,
    ) -> color_eyre::Result<()> {
        if self.sidebar_items.is_empty() {
            self.selected_sidebar_row = 0;
            self.sidebar_state.select(None);
            return Ok(());
        }

        let bounded_index = index.min(self.sidebar_items.len().saturating_sub(1));
        self.selected_sidebar_row = bounded_index;
        self.sidebar_state.select(Some(bounded_index));
        self.ensure_selected_sidebar_item_visible(Some(bounded_index));

        let selected_file_path = self
            .sidebar_items
            .get(bounded_index)
            .and_then(SidebarItem::file)
            .map(|file| file.path.clone());
        if let Some(path) = selected_file_path
            && let Some(file_index) = self.file_index_by_path(&path)
            && file_index != self.selected_file_index
        {
            self.selected_file_index = file_index;
            self.queue_selected_diff_load(true, true);
        }

        Ok(())
    }

    pub(in crate::app) async fn select_next_sidebar_row(&mut self) -> color_eyre::Result<()> {
        let next_index = move_index(self.selected_sidebar_row, self.sidebar_items.len(), 1);
        self.select_sidebar_row(next_index).await
    }

    pub(in crate::app) async fn select_previous_sidebar_row(&mut self) -> color_eyre::Result<()> {
        let next_index = move_index(self.selected_sidebar_row, self.sidebar_items.len(), -1);
        self.select_sidebar_row(next_index).await
    }

    pub(in crate::app) async fn page_sidebar_down(&mut self) -> color_eyre::Result<()> {
        let next_index = move_index(self.selected_sidebar_row, self.sidebar_items.len(), 10);
        self.select_sidebar_row(next_index).await
    }

    pub(in crate::app) async fn page_sidebar_up(&mut self) -> color_eyre::Result<()> {
        let next_index = move_index(self.selected_sidebar_row, self.sidebar_items.len(), -10);
        self.select_sidebar_row(next_index).await
    }
}
