use crate::git;

use super::super::App;

impl App {
    pub(in crate::app) async fn refresh_working_tree_file(
        &mut self,
        path: &str,
    ) -> color_eyre::Result<()> {
        let previously_selected = self.selected_file().map(|file| file.path.clone());
        let updated_file = git::load_status_for_path(&self.repo_root, path).await?;

        if let Some(index) = self.loaded_files.iter().position(|file| file.path == path) {
            match updated_file {
                Some(file) => self.loaded_files[index] = file,
                None => {
                    self.loaded_files.remove(index);
                }
            }
        } else if let Some(file) = updated_file {
            self.loaded_files.push(file);
        }

        self.diff_cache_generation = self.diff_cache_generation.saturating_add(1);
        self.clear_review_diff_snapshot();
        self.clear_review_diff_stats();
        self.diff_view_cache.clear();
        self.pending_diff_cache_key = None;
        self.diff_prefetch_direction = Default::default();
        self.diff_prefetch_anchor_file_index = None;
        self.rebuild_visible_file_list(previously_selected.as_deref());
        self.queue_review_diff_stats_load();
        self.queue_review_diff_snapshot_load();
        self.queue_diff_search_index_load();
        self.queue_selected_diff_load(true, true);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }
}
