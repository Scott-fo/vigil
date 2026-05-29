use crossterm::event::{KeyCode, KeyEvent};

use super::super::App;

impl App {
    pub(in crate::app) fn handle_branch_merge_key(&mut self, key_event: KeyEvent) -> bool {
        if self.branch_merge_target.is_none() {
            return false;
        }

        match key_event.code {
            KeyCode::Esc => self.close_branch_merge_modal(),
            KeyCode::Enter => self.confirm_branch_merge(),
            _ => {}
        }

        true
    }
}
