use crossterm::event::{KeyCode, KeyEvent};
use tokio::task;

use crate::{event::Event, git};

use super::super::{App, WorktreeEntry, input::is_plain_text_key};

impl App {
    pub(in crate::app) async fn handle_worktree_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if !self.worktree_modal_open {
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc => self.close_worktree_modal(),
            KeyCode::Enter => {
                self.confirm_worktree_selection().await?;
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_worktree_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_worktree_selection(-1),
            KeyCode::Backspace => {
                self.worktree_query.pop();
                self.clamp_worktree_selection();
            }
            KeyCode::Char(ch) if is_plain_text_key(key_event) => {
                self.worktree_query.push(ch);
                self.clamp_worktree_selection();
            }
            _ => {}
        }

        Ok(true)
    }

    pub(in crate::app) fn handle_worktrees_loaded(
        &mut self,
        result: Result<Vec<WorktreeEntry>, String>,
    ) {
        if !self.worktree_modal_open {
            return;
        }

        self.worktree_loading = false;
        match result {
            Ok(entries) => {
                self.worktree_entries = entries;
                self.worktree_error = None;
                self.seed_worktree_selection();
            }
            Err(error) => {
                self.worktree_entries.clear();
                self.worktree_error = Some(error);
                self.worktree_selected_index = 0;
            }
        }
    }

    pub(in crate::app) fn open_worktree_modal(&mut self) {
        if self.worktree_modal_open {
            return;
        }

        self.worktree_modal_open = true;
        self.worktree_loading = true;
        self.worktree_error = None;
        self.worktree_query.clear();
        self.worktree_entries.clear();
        self.worktree_selected_index = 0;

        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = git::list_worktrees(&repo_root)
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::WorktreesLoaded(result));
        }));
    }

    pub(in crate::app) fn close_worktree_modal(&mut self) {
        self.worktree_modal_open = false;
        self.worktree_loading = false;
        self.worktree_error = None;
        self.worktree_query.clear();
        self.worktree_selected_index = 0;
    }

    async fn confirm_worktree_selection(&mut self) -> color_eyre::Result<()> {
        let Some(path) = self.selected_worktree_path() else {
            return Ok(());
        };

        self.close_worktree_modal();
        self.switch_to_worktree(path).await
    }
}
