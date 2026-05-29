use std::path::PathBuf;

use crate::{git, watcher::RepoWatcher};

use super::super::super::App;

impl App {
    pub(in crate::app) fn handle_repo_watcher_ready(
        &mut self,
        repo_root: PathBuf,
        result: Result<RepoWatcher, String>,
    ) -> bool {
        if repo_root != self.repo_root {
            return false;
        }

        self.repo_watcher_loading = false;
        match result {
            Ok(watcher) => {
                self.repo_watcher = Some(watcher);
            }
            Err(error) => {
                self.repo_watcher = None;
                self.status_message = Some(format!("watcher unavailable: {error}"));
            }
        }
        true
    }

    pub(in crate::app) async fn handle_repo_changed(
        &mut self,
        paths: Vec<PathBuf>,
    ) -> color_eyre::Result<bool> {
        if !self.is_working_tree_mode()
            || !git::should_refresh_for_paths(&self.repo_root, &paths).await?
        {
            return Ok(false);
        }

        self.refresh().await?;
        if self.should_restart_watcher_for_paths(&paths).await {
            self.restart_repo_watcher();
        }
        Ok(true)
    }
}
