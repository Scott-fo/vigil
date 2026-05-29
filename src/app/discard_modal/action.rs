use crate::git;

use super::super::App;

impl App {
    pub(in crate::app) fn open_discard_modal(&mut self) {
        if !self.is_working_tree_mode() {
            self.status_message = Some("discard is unavailable in compare mode".to_string());
            return;
        }
        self.discard_target = self.selected_file().cloned();
    }

    pub(in crate::app) fn close_discard_modal(&mut self) {
        self.discard_target = None;
    }

    pub(in crate::app) async fn confirm_discard(&mut self) -> color_eyre::Result<()> {
        let Some(file) = self.discard_target.take() else {
            return Ok(());
        };

        git::discard_file_changes(&self.repo_root, &file).await?;
        self.refresh().await?;
        self.status_message = Some(format!("discarded {}", file.path));
        Ok(())
    }
}
