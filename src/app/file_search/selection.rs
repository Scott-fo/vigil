use super::super::{App, navigation::move_index};

impl App {
    pub(super) async fn sync_file_search_selection_after_query_change(
        &mut self,
    ) -> color_eyre::Result<()> {
        let filtered = self.filtered_file_search_indices();
        if filtered.is_empty() {
            self.file_search_selected_index = 0;
            return Ok(());
        }

        let selected_path = self.selected_file().map(|file| file.path.clone());
        if let Some(selected_path) = selected_path
            && let Some(index) = filtered
                .iter()
                .position(|file_index| self.files[*file_index].path == selected_path)
        {
            self.file_search_selected_index = index;
            return Ok(());
        }

        self.file_search_selected_index = 0;
        self.preview_file_search_selection().await
    }

    pub(in crate::app) async fn move_file_search_selection(
        &mut self,
        delta: i32,
    ) -> color_eyre::Result<()> {
        let filtered_len = self.filtered_file_search_indices().len();
        self.file_search_selected_index =
            move_index(self.file_search_selected_index, filtered_len, delta);
        self.preview_file_search_selection().await
    }

    pub(crate) fn selected_file_search_path(&mut self) -> Option<String> {
        self.filtered_file_search_indices()
            .get(self.file_search_selected_index)
            .and_then(|index| self.files.get(*index))
            .map(|file| file.path.clone())
    }

    async fn preview_file_search_selection(&mut self) -> color_eyre::Result<()> {
        let Some(path) = self.selected_file_search_path() else {
            return Ok(());
        };

        self.select_file_by_path(&path).await
    }
}
