use crossterm::event::{KeyCode, KeyEvent};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use tokio::task;

use super::input::is_plain_text_key;
use super::navigation::move_index;
use super::*;

impl App {
    pub(super) async fn handle_branch_compare_key(
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

    pub(super) fn handle_branch_compare_loaded(
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
                self.branch_compare_error = None;
                self.seed_branch_compare_selection(branch_compare_refs.current_ref.as_deref());
            }
            Err(error) => {
                self.branch_compare_available_refs.clear();
                self.branch_compare_error = Some(error);
                self.branch_compare_selected_source_index = 0;
                self.branch_compare_selected_destination_index = 0;
            }
        }
    }

    pub(super) fn open_branch_compare_modal(&mut self) {
        if self.branch_compare_modal_open {
            return;
        }

        self.branch_compare_modal_open = true;
        self.branch_compare_loading = true;
        self.branch_compare_error = None;
        self.branch_compare_active_field = BranchCompareField::Source;
        self.branch_compare_available_refs.clear();
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

    pub(super) fn close_branch_compare_modal(&mut self) {
        self.branch_compare_modal_open = false;
        self.branch_compare_loading = false;
        self.branch_compare_error = None;
    }

    pub(super) fn toggle_branch_compare_field(&mut self) {
        self.branch_compare_active_field = match self.branch_compare_active_field {
            BranchCompareField::Source => BranchCompareField::Destination,
            BranchCompareField::Destination => BranchCompareField::Source,
        };
        self.sync_branch_compare_selection_after_query_change();
    }

    pub(super) fn seed_branch_compare_selection(&mut self, preferred_source_ref: Option<&str>) {
        if self.branch_compare_available_refs.is_empty() {
            self.branch_compare_source_ref = None;
            self.branch_compare_destination_ref = None;
            self.branch_compare_selected_source_index = 0;
            self.branch_compare_selected_destination_index = 0;
            return;
        }

        match &self.review_mode {
            ReviewMode::BranchCompare(selection) => {
                self.branch_compare_source_ref = Some(selection.source_ref.clone());
                self.branch_compare_destination_ref = Some(selection.destination_ref.clone());
            }
            _ => {
                self.branch_compare_source_ref = preferred_source_ref
                    .and_then(|current_ref| {
                        self.branch_compare_available_refs
                            .iter()
                            .find(|ref_name| ref_name.as_str() == current_ref)
                            .cloned()
                    })
                    .or_else(|| self.branch_compare_available_refs.first().cloned());
                self.branch_compare_destination_ref = resolve_default_destination_ref(
                    &self.branch_compare_available_refs,
                    self.branch_compare_source_ref.as_deref(),
                );
            }
        }

        self.sync_branch_compare_selection_after_query_change();
    }

    pub(super) async fn confirm_branch_compare(&mut self) -> color_eyre::Result<()> {
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

    pub fn filtered_branch_compare_refs(&mut self) -> Vec<String> {
        let query = match self.branch_compare_active_field {
            BranchCompareField::Source => self.branch_compare_source_query.trim(),
            BranchCompareField::Destination => self.branch_compare_destination_query.trim(),
        };

        if query.is_empty() {
            return self.branch_compare_available_refs.clone();
        }

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let candidates = self.branch_compare_available_refs.to_vec();
        pattern
            .match_list(candidates, &mut self.branch_compare_matcher)
            .into_iter()
            .map(|(candidate, _score)| candidate)
            .collect()
    }

    pub(super) fn active_branch_compare_query_mut(&mut self) -> &mut String {
        match self.branch_compare_active_field {
            BranchCompareField::Source => &mut self.branch_compare_source_query,
            BranchCompareField::Destination => &mut self.branch_compare_destination_query,
        }
    }

    pub(super) fn sync_branch_compare_selection_after_query_change(&mut self) {
        let filtered = self.filtered_branch_compare_refs();
        let current_ref = match self.branch_compare_active_field {
            BranchCompareField::Source => &mut self.branch_compare_source_ref,
            BranchCompareField::Destination => &mut self.branch_compare_destination_ref,
        };
        let current_index = match self.branch_compare_active_field {
            BranchCompareField::Source => &mut self.branch_compare_selected_source_index,
            BranchCompareField::Destination => &mut self.branch_compare_selected_destination_index,
        };

        if filtered.is_empty() {
            *current_ref = None;
            *current_index = 0;
            return;
        }

        if let Some(existing) = current_ref.as_ref()
            && let Some(index) = filtered.iter().position(|ref_name| ref_name == existing)
        {
            *current_index = index;
            return;
        }

        *current_ref = filtered.first().cloned();
        *current_index = 0;
    }

    pub(super) fn move_branch_compare_selection(&mut self, delta: i32) {
        let filtered = self.filtered_branch_compare_refs();
        if filtered.is_empty() {
            return;
        }

        let current_index = match self.branch_compare_active_field {
            BranchCompareField::Source => &mut self.branch_compare_selected_source_index,
            BranchCompareField::Destination => &mut self.branch_compare_selected_destination_index,
        };
        let current_ref = match self.branch_compare_active_field {
            BranchCompareField::Source => &mut self.branch_compare_source_ref,
            BranchCompareField::Destination => &mut self.branch_compare_destination_ref,
        };

        let next_index = move_index(*current_index, filtered.len(), delta);

        *current_index = next_index;
        *current_ref = filtered.get(next_index).cloned();
    }
}

fn resolve_default_destination_ref(refs: &[String], source_ref: Option<&str>) -> Option<String> {
    let preferred = refs.iter().find(|ref_name| {
        (**ref_name == "main" || **ref_name == "master")
            && source_ref.is_none_or(|source| source != ref_name.as_str())
    });
    if let Some(preferred) = preferred {
        return Some(preferred.clone());
    }

    refs.iter()
        .find(|ref_name| source_ref.is_none_or(|source| source != ref_name.as_str()))
        .cloned()
        .or_else(|| refs.first().cloned())
}

#[cfg(test)]
mod tests;
