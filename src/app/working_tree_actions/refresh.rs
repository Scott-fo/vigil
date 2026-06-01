use crate::git;

use super::super::App;

impl App {
    pub(in crate::app) async fn refresh_working_tree_file(
        &mut self,
        path: &str,
    ) -> color_eyre::Result<()> {
        let previously_selected = self.selected_file().map(|file| file.path.clone());
        let updated_file = git::load_status_for_path(&self.repo_root, path).await?;

        if let Some(index) = self.file_index_by_path(path) {
            match updated_file {
                Some(file) => self.files[index] = file,
                None => {
                    self.files.remove(index);
                }
            }
        } else if let Some(file) = updated_file {
            self.files.push(file);
        }

        self.diff_cache_generation = self.diff_cache_generation.saturating_add(1);
        self.diff_view_cache.clear();
        self.pending_diff_cache_key = None;
        self.rebuild_sidebar_items();

        self.selected_file_index = previously_selected
            .as_deref()
            .and_then(|selected_path| self.file_index_by_path(selected_path))
            .or_else(|| {
                self.first_sidebar_file_path()
                    .and_then(|first_path| self.file_index_by_path(first_path))
            })
            .unwrap_or(0);

        self.sync_sidebar_state();
        self.queue_diff_search_index_load();
        self.queue_selected_diff_load(true, true);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }
}
