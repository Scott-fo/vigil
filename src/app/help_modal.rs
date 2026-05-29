use crossterm::event::{KeyCode, KeyEvent};

use super::App;

impl App {
    pub(super) fn handle_help_modal_key(&mut self, key_event: KeyEvent) -> bool {
        if !self.help_modal_open {
            return false;
        }

        match key_event.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter | KeyCode::Char('q') => {
                self.help_modal_open = false;
            }
            _ => {}
        }

        true
    }
}
