use crossterm::event::{KeyCode, KeyEvent};

use super::super::{App, navigation::scroll_u16};

impl App {
    pub(in crate::app) async fn handle_blame_modal_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if !self.blame_modal_open {
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.close_blame_modal();
            }
            KeyCode::Enter | KeyCode::Char('o') => {
                self.open_blame_commit_compare().await?;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_blame(3);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_blame(-3);
            }
            KeyCode::PageDown => {
                self.scroll_blame(10);
            }
            KeyCode::PageUp => {
                self.scroll_blame(-10);
            }
            _ => {}
        }

        Ok(true)
    }

    pub(in crate::app) fn scroll_blame(&mut self, delta: i32) {
        self.blame_scroll = scroll_u16(self.blame_scroll, delta);
    }
}
