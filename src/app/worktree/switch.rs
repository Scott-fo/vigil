use std::path::PathBuf;

use super::super::{App, ReviewMode};

impl App {
    pub(in crate::app) async fn switch_to_worktree(
        &mut self,
        path: PathBuf,
    ) -> color_eyre::Result<()> {
        if self.repo_root == path && self.is_working_tree_mode() {
            self.status_message = Some(format!("already watching {}", path.display()));
            return Ok(());
        }

        self.cancel_inflight_diff_load();
        self.review_mode = ReviewMode::WorkingTree;
        self.repo_root = path.clone();
        self.repo_watcher = None;
        self.repo_watcher_loading = false;
        self.collapsed_directories.clear();
        self.refresh().await?;
        self.status_message = Some(format!("watching {}", path.display()));
        Ok(())
    }
}
