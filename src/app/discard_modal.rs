use crossterm::event::{KeyCode, KeyEvent};

use super::App;
use crate::git;

impl App {
    pub(super) async fn handle_discard_modal_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if self.discard_target.is_none() {
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc => {
                self.discard_target = None;
            }
            KeyCode::Enter => {
                self.confirm_discard().await?;
            }
            _ => {}
        }

        Ok(true)
    }

    pub(super) fn open_discard_modal(&mut self) {
        if !self.is_working_tree_mode() {
            self.status_message = Some("discard is unavailable in compare mode".to_string());
            return;
        }
        self.discard_target = self.selected_file().cloned();
    }

    async fn confirm_discard(&mut self) -> color_eyre::Result<()> {
        let Some(file) = self.discard_target.take() else {
            return Ok(());
        };

        git::discard_file_changes(&self.repo_root, &file).await?;
        self.refresh().await?;
        self.status_message = Some(format!("discarded {}", file.path));
        Ok(())
    }
}
