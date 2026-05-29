use crossterm::event::{KeyCode, KeyEvent};

use super::super::{App, input::is_plain_text_key};

impl App {
    pub(in crate::app) async fn handle_commit_modal_key(
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
}
