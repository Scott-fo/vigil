//! The keyboard shortcuts modal. It scrolls, so it stays usable on short
//! terminals; the renderer clamps the scroll to the content.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::App;

/// Rows a page key scrolls the help by.
const HELP_PAGE: usize = 10;

impl App {
    pub(super) fn handle_help_modal_key(&mut self, key_event: KeyEvent) -> bool {
        if !self.help_modal_open {
            return false;
        }

        let control = key_event.modifiers.contains(KeyModifiers::CONTROL);
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter | KeyCode::Char('q') => {
                self.help_modal_open = false;
                self.help_scroll = 0;
            }
            KeyCode::Char('d') if control => self.scroll_help(HELP_PAGE as isize),
            KeyCode::Char('u') if control => self.scroll_help(-(HELP_PAGE as isize)),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_help(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_help(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_help(HELP_PAGE as isize),
            KeyCode::PageUp => self.scroll_help(-(HELP_PAGE as isize)),
            KeyCode::Home | KeyCode::Char('g') => self.help_scroll = 0,
            KeyCode::End | KeyCode::Char('G') => self.help_scroll = usize::MAX,
            _ => {}
        }

        true
    }

    fn scroll_help(&mut self, delta: isize) {
        self.help_scroll = self.help_scroll.saturating_add_signed(delta);
    }
}
