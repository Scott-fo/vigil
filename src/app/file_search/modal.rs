use crossterm::event::{KeyCode, KeyEvent};

use super::super::{App, input::is_plain_text_key};

impl App {
    pub(in crate::app) async fn handle_file_search_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if !self.file_search_modal_open {
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc => {
                self.cancel_file_search_modal().await?;
            }
            KeyCode::Enter => {
                self.confirm_file_search_modal();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_file_search_selection(1).await?;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_file_search_selection(-1).await?;
            }
            KeyCode::Backspace => {
                self.file_search_query.pop();
                self.sync_file_search_selection_after_query_change().await?;
            }
            KeyCode::Char(ch) if is_plain_text_key(key_event) => {
                self.file_search_query.push(ch);
                self.sync_file_search_selection_after_query_change().await?;
            }
            _ => {}
        }

        Ok(true)
    }

    pub(in crate::app) async fn open_file_search_modal(&mut self) -> color_eyre::Result<()> {
        if self.file_search_modal_open {
            return Ok(());
        }

        self.file_search_modal_open = true;
        self.file_search_query.clear();
        self.file_search_selected_index = 0;
        self.file_search_initial_path = self.selected_file().map(|file| file.path.clone());
        self.sync_file_search_selection_after_query_change().await
    }

    pub(in crate::app) async fn cancel_file_search_modal(&mut self) -> color_eyre::Result<()> {
        let initial_path = self.file_search_initial_path.clone();
        self.close_file_search_modal();
        if let Some(path) = initial_path {
            self.select_file_by_path(&path).await?;
        }
        Ok(())
    }

    fn confirm_file_search_modal(&mut self) {
        if self.selected_file_search_path().is_none() {
            return;
        }
        self.close_file_search_modal();
    }

    fn close_file_search_modal(&mut self) {
        self.file_search_modal_open = false;
        self.file_search_query.clear();
        self.file_search_selected_index = 0;
        self.file_search_initial_path = None;
    }
}
