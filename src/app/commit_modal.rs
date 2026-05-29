use crossterm::event::{KeyCode, KeyEvent};

use super::App;
use super::input::is_plain_text_key;
use crate::git;

impl App {
    pub(super) async fn handle_commit_modal_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if !self.commit_modal_open {
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc => {
                self.close_commit_modal();
            }
            KeyCode::Enter => {
                self.confirm_commit().await?;
            }
            KeyCode::Backspace => {
                self.commit_message.pop();
                self.commit_error = None;
            }
            KeyCode::Char(ch) if is_plain_text_key(key_event) => {
                self.commit_message.push(ch);
                self.commit_error = None;
            }
            _ => {}
        }

        Ok(true)
    }

    pub(super) fn open_commit_modal(&mut self) {
        if !self.is_working_tree_mode() {
            self.status_message = Some("commit is unavailable in compare mode".to_string());
            return;
        }

        if self.staged_file_count() == 0 {
            return;
        }

        self.commit_modal_open = true;
        self.commit_message.clear();
        self.commit_error = None;
    }

    fn close_commit_modal(&mut self) {
        self.commit_modal_open = false;
        self.commit_message.clear();
        self.commit_error = None;
    }

    async fn confirm_commit(&mut self) -> color_eyre::Result<()> {
        match git::commit_staged_changes(&self.repo_root, &self.commit_message).await {
            Ok(()) => {
                let committed_message = self.commit_message.trim().to_string();
                self.close_commit_modal();
                self.refresh().await?;
                self.status_message = Some(format!("committed {}", committed_message));
            }
            Err(error) => {
                self.commit_error = Some(error.to_string());
            }
        }
        Ok(())
    }

    fn staged_file_count(&self) -> usize {
        self.files
            .iter()
            .filter(|file| git::is_file_staged(&file.status))
            .count()
    }
}
