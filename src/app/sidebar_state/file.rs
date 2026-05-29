use crate::git::FileEntry;

use super::super::App;
use super::focus::{first_file_path, selected_visible_file_index, visible_file_paths};

impl App {
    pub(in crate::app) fn selected_file(&self) -> Option<&FileEntry> {
        self.files.get(self.selected_file_index)
    }

    pub(in crate::app) fn file_index_by_path(&self, path: &str) -> Option<usize> {
        self.files.iter().position(|file| file.path == path)
    }

    pub(in crate::app) fn visible_file_paths(&self) -> Vec<String> {
        visible_file_paths(&self.sidebar_items)
    }

    pub(in crate::app) fn first_sidebar_file_path(&self) -> Option<&str> {
        first_file_path(&self.sidebar_items)
    }

    pub(in crate::app) fn selected_visible_file_index(&self) -> Option<usize> {
        let selected_path = self.selected_file()?.path.as_str();
        selected_visible_file_index(&self.sidebar_items, selected_path)
    }

    pub(in crate::app) async fn select_file_by_path(
        &mut self,
        path: &str,
    ) -> color_eyre::Result<()> {
        if let Some(index) = self.file_index_by_path(path) {
            self.select_file_at(index).await?;
        }
        Ok(())
    }

    pub(in crate::app) async fn select_file_at(&mut self, index: usize) -> color_eyre::Result<()> {
        if self.files.is_empty() {
            self.selected_file_index = 0;
            self.sync_sidebar_state();
            self.queue_selected_diff_load(true, true);
            return Ok(());
        }

        let bounded_index = index.min(self.files.len().saturating_sub(1));
        if bounded_index != self.selected_file_index {
            self.selected_file_index = bounded_index;
            self.sync_sidebar_state();
            self.queue_selected_diff_load(true, true);
        } else {
            self.sync_sidebar_state();
        }

        Ok(())
    }
}
