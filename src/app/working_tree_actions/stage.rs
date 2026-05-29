use crate::git;

use super::super::App;

impl App {
    pub(in crate::app) async fn toggle_selected_file_stage(&mut self) -> color_eyre::Result<()> {
        if !self.is_working_tree_mode() {
            self.status_message = Some("stage/unstage is unavailable in compare mode".to_string());
            return Ok(());
        }

        let Some(file) = self.selected_file().cloned() else {
            return Ok(());
        };

        git::toggle_file_stage(&self.repo_root, &file).await?;
        self.refresh_working_tree_file(&file.path).await?;
        self.status_message = Some(format!(
            "{} {}",
            if git::is_file_staged(&file.status) {
                "unstaged"
            } else {
                "staged"
            },
            file.path
        ));
        Ok(())
    }

    pub(in crate::app) async fn stage_all_files(&mut self) -> color_eyre::Result<()> {
        if !self.is_working_tree_mode() {
            self.status_message = Some("stage all is unavailable in compare mode".to_string());
            return Ok(());
        }

        if self.files.is_empty() {
            self.status_message = Some("no changes to stage".to_string());
            return Ok(());
        }

        let should_unstage = self
            .files
            .iter()
            .all(|file| git::is_file_fully_staged(&file.status));

        if should_unstage {
            git::unstage_all_changes(&self.repo_root).await?;
        } else {
            git::stage_all_changes(&self.repo_root).await?;
        }

        self.refresh().await?;
        self.status_message = Some(if should_unstage {
            "unstaged all changes".to_string()
        } else {
            "staged all changes".to_string()
        });
        Ok(())
    }
}
