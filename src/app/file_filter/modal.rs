use crossterm::event::{KeyCode, KeyEvent};

use crate::theme::config;

use super::super::{App, input::is_plain_text_key};
use super::ExcludeSuffixes;

impl App {
    pub(in crate::app) fn handle_file_filter_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if !self.file_filter_modal_open {
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc => {
                self.close_file_filter_modal();
            }
            KeyCode::Enter => {
                self.confirm_file_filter_modal();
            }
            KeyCode::Backspace => {
                self.file_filter_query.pop();
            }
            KeyCode::Char(ch) if is_plain_text_key(key_event) => {
                self.file_filter_query.push(ch);
            }
            _ => {}
        }

        Ok(true)
    }

    pub(in crate::app) fn open_file_filter_modal(&mut self) {
        if self.file_filter_modal_open {
            return;
        }

        self.file_filter_modal_open = true;
        self.file_filter_query = self.file_exclude_suffixes.to_query();
    }

    fn close_file_filter_modal(&mut self) {
        self.file_filter_modal_open = false;
        self.file_filter_query.clear();
    }

    fn confirm_file_filter_modal(&mut self) {
        let suffixes = ExcludeSuffixes::from_query(&self.file_filter_query);
        self.apply_file_exclude_suffixes(suffixes.clone());
        match config::persist_exclude_file_suffixes(suffixes.as_slice()) {
            Ok(()) => {
                self.status_message = Some(file_filter_status_message(
                    &suffixes,
                    self.hidden_file_count(),
                ));
            }
            Err(error) => {
                self.status_message =
                    Some(format!("failed to persist file hide suffixes: {error}"));
            }
        }
        self.close_file_filter_modal();
    }
}

fn file_filter_status_message(suffixes: &ExcludeSuffixes, hidden_count: usize) -> String {
    if suffixes.is_empty() {
        return "showing all files".to_string();
    }

    let suffix_list = suffixes.to_query();
    if hidden_count == 0 {
        format!("hiding files matching {suffix_list}")
    } else {
        format!(
            "hiding {hidden_count} file{} matching {suffix_list}",
            if hidden_count == 1 { "" } else { "s" }
        )
    }
}
