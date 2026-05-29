use crate::git;

use super::{App, ReviewMode};

mod remote;
mod selection;
mod status;
mod watcher;

use self::selection::refreshed_file_index;

impl App {
    pub(super) async fn refresh(&mut self) -> color_eyre::Result<()> {
        let previously_selected = self.selected_file().map(|file| file.path.clone());
        let files = match &self.review_mode {
            ReviewMode::WorkingTree => match git::load_working_tree_status(&self.repo_root).await {
                Ok(status) => {
                    self.apply_working_tree_status_root(status.repo_root);
                    status.files
                }
                Err(error) => {
                    self.enter_repo_error_state(error.to_string()).await?;
                    return Ok(());
                }
            },
            ReviewMode::CommitCompare(selection) => {
                git::load_files_with_commit_diff(&self.repo_root, selection).await?
            }
            ReviewMode::BranchCompare(selection) => {
                git::load_files_with_branch_diff(&self.repo_root, selection).await?
            }
        };
        self.diff_cache_generation = self.diff_cache_generation.saturating_add(1);
        self.diff_view_cache.clear();
        self.pending_diff_cache_key = None;
        self.files = files;
        self.rebuild_sidebar_items();

        self.selected_file_index = refreshed_file_index(
            previously_selected.as_deref(),
            &self.files,
            &self.sidebar_items,
        );

        self.sync_sidebar_state();
        self.queue_selected_diff_load(true, true);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }

    pub(super) fn is_working_tree_mode(&self) -> bool {
        matches!(self.review_mode, ReviewMode::WorkingTree)
    }

    pub(super) async fn reset_to_working_tree(&mut self) -> color_eyre::Result<()> {
        if self.is_working_tree_mode() {
            return Ok(());
        }

        self.review_mode = ReviewMode::WorkingTree;
        self.refresh().await
    }

    pub(super) async fn initialize_repo_if_needed(&mut self) -> color_eyre::Result<()> {
        if !self.can_initialize_git_repo() {
            return Ok(());
        }

        git::init_repo(&self.repo_root).await?;
        self.review_mode = ReviewMode::WorkingTree;
        self.refresh().await?;
        self.status_message = Some(format!(
            "initialized git repo in {}",
            self.repo_root.display()
        ));
        Ok(())
    }

    fn apply_working_tree_status_root(&mut self, resolved_root: std::path::PathBuf) {
        let watcher_needs_restart = self.repo_error.is_some()
            || (!self.repo_watcher_loading && self.repo_watcher.is_none())
            || self.repo_root != resolved_root;

        self.repo_root = resolved_root;
        self.repo_error = None;

        if watcher_needs_restart {
            self.restart_repo_watcher();
        }
    }

    async fn enter_repo_error_state(&mut self, error: String) -> color_eyre::Result<()> {
        self.repo_error = Some(error);
        self.repo_watcher = None;
        self.repo_watcher_loading = false;
        self.files.clear();
        self.rebuild_sidebar_items();
        self.selected_file_index = 0;
        self.sync_sidebar_state();
        self.queue_selected_diff_load(true, true);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }
}
