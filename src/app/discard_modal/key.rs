use crossterm::event::{KeyCode, KeyEvent};

use super::super::App;

impl App {
    pub(in crate::app) async fn handle_discard_modal_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if self.discard_target.is_none() {
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc => {
                self.close_discard_modal();
            }
            KeyCode::Enter => {
                self.confirm_discard().await?;
            }
            _ => {}
        }

        Ok(true)
    }
}
