use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::{ActivePane, App};
use super::{KeyOutcome, handled};

impl App {
    pub(super) async fn handle_pane_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<Option<KeyOutcome>> {
        match key_event.code {
            KeyCode::Char('d') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.clear_diff_text_selection();
                self.page_or_scroll_diff(12);
                handled()
            }
            KeyCode::Char('u') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.clear_diff_text_selection();
                self.page_or_scroll_diff(-12);
                handled()
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_active_pane_selection(1).await?;
                handled()
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_active_pane_selection(-1).await?;
                handled()
            }
            KeyCode::Char(' ') => {
                if self.active_pane == ActivePane::Sidebar
                    && !self.toggle_focused_sidebar_directory()
                {
                    self.toggle_selected_file_stage().await?;
                }
                handled()
            }
            KeyCode::Right => {
                if self.active_pane == ActivePane::Sidebar {
                    self.expand_focused_sidebar_directory();
                }
                handled()
            }
            KeyCode::Left => {
                if self.active_pane == ActivePane::Sidebar {
                    self.collapse_focused_sidebar_directory_or_focus_parent();
                }
                handled()
            }
            KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('e') => {
                if let Some(command) = self.handle_open_key(key_event.code) {
                    Ok(Some(KeyOutcome::Command(command)))
                } else {
                    handled()
                }
            }
            KeyCode::Char('d') => {
                self.open_discard_modal();
                handled()
            }
            KeyCode::PageDown => {
                self.page_active_pane(12).await?;
                handled()
            }
            KeyCode::PageUp => {
                self.page_active_pane(-12).await?;
                handled()
            }
            KeyCode::Home => {
                self.select_active_pane_start().await?;
                handled()
            }
            KeyCode::End => {
                self.select_active_pane_end().await?;
                handled()
            }
            _ => Ok(None),
        }
    }

    async fn move_active_pane_selection(&mut self, delta: i32) -> color_eyre::Result<()> {
        match self.active_pane {
            ActivePane::Sidebar if delta > 0 => self.select_next_sidebar_row().await,
            ActivePane::Sidebar => self.select_previous_sidebar_row().await,
            ActivePane::Diff => {
                self.clear_diff_text_selection();
                self.move_diff_selection(delta);
                Ok(())
            }
        }
    }

    async fn page_active_pane(&mut self, delta: i32) -> color_eyre::Result<()> {
        match self.active_pane {
            ActivePane::Sidebar if delta > 0 => self.page_sidebar_down().await,
            ActivePane::Sidebar => self.page_sidebar_up().await,
            ActivePane::Diff => {
                self.clear_diff_text_selection();
                self.page_diff(delta);
                Ok(())
            }
        }
    }

    async fn select_active_pane_start(&mut self) -> color_eyre::Result<()> {
        match self.active_pane {
            ActivePane::Sidebar => self.select_sidebar_row(0).await,
            ActivePane::Diff => {
                self.clear_diff_text_selection();
                self.selected_diff_line_index = self
                    .diff_view
                    .first_selectable_index(self.diff_view_mode, self.current_diff_display_width());
                self.diff_scroll = 0;
                Ok(())
            }
        }
    }

    async fn select_active_pane_end(&mut self) -> color_eyre::Result<()> {
        match self.active_pane {
            ActivePane::Sidebar => {
                if let Some(last_index) = self.sidebar_items.len().checked_sub(1) {
                    self.select_sidebar_row(last_index).await?;
                }
                Ok(())
            }
            ActivePane::Diff => {
                self.clear_diff_text_selection();
                self.selected_diff_line_index = self
                    .diff_view
                    .last_selectable_index(self.diff_view_mode, self.current_diff_display_width());
                self.diff_scroll = u16::MAX;
                Ok(())
            }
        }
    }
}
