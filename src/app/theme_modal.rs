use crossterm::event::{KeyCode, KeyEvent};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};

use crate::theme::{self, ThemeMode, config};

use super::App;
use super::input::is_plain_text_key;
use super::navigation::move_index;

impl App {
    pub(super) async fn handle_theme_modal_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if !self.theme_modal_open {
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc => {
                self.cancel_theme_modal().await?;
            }
            KeyCode::Enter => {
                self.confirm_theme_modal()?;
            }
            KeyCode::Char('m') => {
                self.toggle_theme_mode_preview().await?;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_theme_selection(1).await?;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_theme_selection(-1).await?;
            }
            KeyCode::Backspace => {
                self.theme_modal_query.pop();
                self.sync_theme_selection_after_query_change().await?;
            }
            KeyCode::Char(ch) if is_plain_text_key(key_event) => {
                self.theme_modal_query.push(ch);
                self.sync_theme_selection_after_query_change().await?;
            }
            _ => {}
        }

        Ok(true)
    }

    pub(super) fn open_theme_modal(&mut self) {
        if self.theme_modal_open {
            return;
        }

        self.theme_modal_open = true;
        self.theme_modal_query.clear();
        self.theme_modal_initial_name = self.theme_name.clone();
        self.theme_modal_initial_mode = self.theme_mode;
        self.theme_modal_selected_index = theme::all()
            .iter()
            .position(|theme_entry| theme_entry.name == self.theme_name)
            .unwrap_or(0);
    }

    pub(super) async fn cancel_theme_modal(&mut self) -> color_eyre::Result<()> {
        self.theme_name = self.theme_modal_initial_name.clone();
        self.theme_mode = self.theme_modal_initial_mode;
        theme::set_active_theme(&self.theme_name, self.theme_mode);
        self.theme_modal_open = false;
        self.theme_modal_query.clear();
        self.queue_selected_diff_load(false, false);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }

    pub(super) fn confirm_theme_modal(&mut self) -> color_eyre::Result<()> {
        self.theme_modal_open = false;
        self.theme_modal_query.clear();
        match config::persist_theme_preference(&self.theme_name, self.theme_mode) {
            Ok(()) => {
                self.status_message = Some(format!(
                    "theme set to {} ({})",
                    self.theme_name,
                    self.theme_mode.as_str()
                ));
            }
            Err(error) => {
                self.status_message = Some(format!("failed to persist theme: {error}"));
            }
        }
        Ok(())
    }

    pub fn filtered_theme_names(&mut self) -> Vec<&'static str> {
        let query = self.theme_modal_query.trim();
        if query.is_empty() {
            return theme::names().collect();
        }

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let candidates = theme::names().collect::<Vec<_>>();
        pattern
            .match_list(candidates, &mut self.theme_matcher)
            .into_iter()
            .map(|(candidate, _score)| candidate)
            .collect()
    }

    pub(super) async fn sync_theme_selection_after_query_change(
        &mut self,
    ) -> color_eyre::Result<()> {
        let filtered = self.filtered_theme_names();
        if filtered.is_empty() {
            self.theme_modal_selected_index = 0;
            return Ok(());
        }

        if let Some(index) = filtered
            .iter()
            .position(|name| *name == self.theme_name.as_str())
        {
            self.theme_modal_selected_index = index;
            return Ok(());
        }

        self.theme_modal_selected_index = 0;
        self.preview_theme(filtered[0], self.theme_mode).await
    }

    pub(super) async fn move_theme_selection(&mut self, delta: i32) -> color_eyre::Result<()> {
        let filtered = self.filtered_theme_names();
        if filtered.is_empty() {
            self.theme_modal_selected_index = 0;
            return Ok(());
        }

        let next = move_index(self.theme_modal_selected_index, filtered.len(), delta);

        self.theme_modal_selected_index = next;
        self.preview_theme(filtered[next], self.theme_mode).await
    }

    pub(super) async fn toggle_theme_mode_preview(&mut self) -> color_eyre::Result<()> {
        self.theme_mode = self.theme_mode.toggle();
        theme::set_active_theme(&self.theme_name, self.theme_mode);
        self.queue_selected_diff_load(false, false);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }

    async fn preview_theme(
        &mut self,
        theme_name: &str,
        theme_mode: ThemeMode,
    ) -> color_eyre::Result<()> {
        let resolved_name = theme::resolve_theme_name(Some(theme_name)).to_string();
        self.theme_name = resolved_name;
        self.theme_mode = theme_mode;
        theme::set_active_theme(&self.theme_name, self.theme_mode);
        self.queue_selected_diff_load(false, false);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }
}
