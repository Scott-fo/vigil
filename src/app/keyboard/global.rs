use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{KeyOutcome, handled};
use crate::{git, theme::config};

use super::super::{ActivePane, App, DiffViewMode};

impl App {
    pub(super) async fn handle_global_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<Option<KeyOutcome>> {
        if self.find_prefix_pending {
            return self.handle_find_prefix_key(key_event).await;
        }

        match key_event.code {
            KeyCode::Esc if self.diff_text_selection.is_some() => {
                self.clear_diff_text_selection();
                handled()
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.quit();
                handled()
            }
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                if !self.copy_diff_selection_to_clipboard()? {
                    self.quit();
                }
                handled()
            }
            KeyCode::Tab => {
                self.clear_diff_text_selection();
                if !self.sidebar_hidden {
                    self.active_pane = match self.active_pane {
                        ActivePane::Sidebar => ActivePane::Diff,
                        ActivePane::Diff => ActivePane::Sidebar,
                    };
                }
                handled()
            }
            KeyCode::Char('?') => {
                self.help_modal_open = true;
                handled()
            }
            KeyCode::Char('t') => {
                self.open_theme_modal();
                handled()
            }
            KeyCode::Char('r') => {
                self.refresh().await?;
                handled()
            }
            KeyCode::Char('R') => {
                self.start_codex_review();
                handled()
            }
            KeyCode::Char('E') => {
                self.open_review_context_modal();
                handled()
            }
            KeyCode::Char('S') => {
                self.open_review_summary_modal();
                handled()
            }
            KeyCode::Char('i') => {
                self.initialize_repo_if_needed().await?;
                handled()
            }
            KeyCode::Char('b') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.toggle_sidebar_hidden();
                handled()
            }
            KeyCode::Char('l') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.reset_to_working_tree().await?;
                handled()
            }
            KeyCode::Char('p') => {
                self.start_pull();
                handled()
            }
            KeyCode::Char('P') => {
                self.start_push();
                handled()
            }
            KeyCode::Char('f') if key_event.modifiers == KeyModifiers::NONE => {
                self.find_prefix_pending = true;
                self.status_message = Some("f: f files, g diff search".to_string());
                handled()
            }
            KeyCode::Char('c') => {
                self.open_commit_modal();
                handled()
            }
            KeyCode::Char('A') => {
                self.stage_all_files().await?;
                handled()
            }
            KeyCode::Char('1') => {
                self.resolve_selected_merge_conflict(git::MergeConflictResolution::Current)
                    .await?;
                handled()
            }
            KeyCode::Char('2') => {
                self.resolve_selected_merge_conflict(git::MergeConflictResolution::Incoming)
                    .await?;
                handled()
            }
            KeyCode::Char('3') => {
                self.resolve_selected_merge_conflict(git::MergeConflictResolution::Both)
                    .await?;
                handled()
            }
            KeyCode::Char('b') => {
                self.open_branch_compare_modal();
                handled()
            }
            KeyCode::Char('m') => {
                self.open_branch_merge_modal();
                handled()
            }
            KeyCode::Char('w') => {
                self.open_worktree_modal();
                handled()
            }
            KeyCode::Char('g') => {
                self.open_commit_search_modal();
                handled()
            }
            KeyCode::Char('v') => {
                self.toggle_diff_view_mode();
                handled()
            }
            _ => Ok(None),
        }
    }

    async fn handle_find_prefix_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<Option<KeyOutcome>> {
        self.find_prefix_pending = false;
        match key_event.code {
            KeyCode::Esc => handled(),
            KeyCode::Char('f') if key_event.modifiers == KeyModifiers::NONE => {
                self.open_file_search_modal().await?;
                handled()
            }
            KeyCode::Char('g') if key_event.modifiers == KeyModifiers::NONE => {
                self.open_diff_search_modal();
                handled()
            }
            _ => handled(),
        }
    }

    fn toggle_diff_view_mode(&mut self) {
        self.clear_diff_text_selection();
        self.diff_view_mode = match self.diff_view_mode {
            DiffViewMode::Unified => DiffViewMode::Split,
            DiffViewMode::Split => DiffViewMode::Unified,
        };
        if let Err(error) = config::persist_diff_view_mode(self.diff_view_mode.as_str()) {
            self.status_message = Some(format!("failed to persist diff view mode: {error}"));
        }
        self.diff_scroll = 0;
        self.selected_diff_line_index = self
            .diff_view
            .first_selectable_index(self.diff_view_mode, self.current_diff_display_width());
    }
}
