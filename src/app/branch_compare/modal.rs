use crossterm::event::{KeyCode, KeyEvent};
use tokio::task;

use crate::{event::Event, git};

use super::super::{
    App, BranchCompareField, BranchCompareSelection, ReviewMode, input::is_plain_text_key,
};

impl App {
    pub(in crate::app) async fn handle_branch_compare_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if !self.branch_compare_modal_open {
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc => self.close_branch_compare_modal(),
            KeyCode::Tab => self.toggle_branch_compare_field(),
            KeyCode::Enter => {
                self.confirm_branch_compare().await?;
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_branch_compare_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_branch_compare_selection(-1),
            KeyCode::Backspace => {
                self.active_branch_compare_query_mut().pop();
                self.sync_branch_compare_selection_after_query_change();
            }
            KeyCode::Char(ch) if is_plain_text_key(key_event) => {
                self.active_branch_compare_query_mut().push(ch);
                self.sync_branch_compare_selection_after_query_change();
            }
            _ => {}
        }

        Ok(true)
    }

    pub(in crate::app) fn handle_branch_compare_loaded(
        &mut self,
        result: Result<git::BranchCompareRefs, String>,
    ) {
        if !self.branch_compare_modal_open {
            return;
        }

        self.branch_compare_loading = false;
        match result {
            Ok(branch_compare_refs) => {
                self.branch_compare_available_refs = branch_compare_refs.refs;
                self.rebuild_branch_compare_ref_index();
                self.branch_compare_error = None;
                self.seed_branch_compare_selection(branch_compare_refs.current_ref.as_deref());
            }
            Err(error) => {
                self.branch_compare_available_refs.clear();
                self.branch_compare_ref_index.clear();
                self.branch_compare_error = Some(error);
                self.branch_compare_selected_source_index = 0;
                self.branch_compare_selected_destination_index = 0;
            }
        }
    }

    pub(in crate::app) fn open_branch_compare_modal(&mut self) {
        if self.branch_compare_modal_open {
            return;
        }

        self.branch_compare_modal_open = true;
        self.branch_compare_loading = true;
        self.branch_compare_error = None;
        self.branch_compare_active_field = BranchCompareField::Source;
        self.branch_compare_available_refs.clear();
        self.branch_compare_ref_index.clear();
        self.branch_compare_source_query.clear();
        self.branch_compare_destination_query.clear();
        self.branch_compare_source_ref = None;
        self.branch_compare_destination_ref = None;
        self.branch_compare_selected_source_index = 0;
        self.branch_compare_selected_destination_index = 0;

        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = git::load_branch_compare_refs(&repo_root)
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::BranchCompareLoaded(result));
        }));
    }

    pub(in crate::app) fn close_branch_compare_modal(&mut self) {
        self.branch_compare_modal_open = false;
        self.branch_compare_loading = false;
        self.branch_compare_error = None;
    }

    async fn confirm_branch_compare(&mut self) -> color_eyre::Result<()> {
        let Some(source_ref) = self.branch_compare_source_ref.clone() else {
            self.branch_compare_error = Some("Select a source ref.".to_string());
            return Ok(());
        };
        let Some(destination_ref) = self.branch_compare_destination_ref.clone() else {
            self.branch_compare_error = Some("Select a destination ref.".to_string());
            return Ok(());
        };

        if source_ref == destination_ref {
            self.branch_compare_error =
                Some("Source and destination refs must differ.".to_string());
            return Ok(());
        }

        self.review_mode = ReviewMode::BranchCompare(BranchCompareSelection {
            source_ref,
            destination_ref,
        });
        self.close_branch_compare_modal();
        self.refresh().await
    }
}
