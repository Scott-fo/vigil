//! Diff search modal state and background work.
//!
//! The app owns request IDs, cancellation, and result selection for searching
//! across the current review scope. The git module owns the searchable index;
//! UI code only receives prepared search results.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};
use tokio::task;

use super::{
    App, ReviewMode,
    input::is_plain_text_key,
    navigation::{clamp_index, move_index},
};
use crate::{
    event::Event,
    git::{self, DiffSearchOptions, DiffSearchResult},
};

const DIFF_SEARCH_RESULT_LIMIT: usize = 200;
const EMPTY_DIFF_SEARCH_MESSAGE: &str = "No searchable diff lines.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffSearchNavigationTarget {
    file_path: String,
    old_line: Option<usize>,
    new_line: Option<usize>,
}

impl DiffSearchNavigationTarget {
    fn from_result(result: &DiffSearchResult) -> Self {
        Self {
            file_path: result.file_path.clone(),
            old_line: result.old_line,
            new_line: result.new_line,
        }
    }
}

impl App {
    pub(in crate::app) async fn handle_diff_search_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if !self.diff_search_modal_open {
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc => {
                self.close_diff_search_modal();
            }
            KeyCode::Enter => {
                self.confirm_diff_search_modal().await?;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_diff_search_selection(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_diff_search_selection(-1);
            }
            KeyCode::Backspace => {
                self.diff_search_query.pop();
                self.queue_diff_search_results();
            }
            KeyCode::Char(ch) if is_plain_text_key(key_event) => {
                self.diff_search_query.push(ch);
                self.queue_diff_search_results();
            }
            _ => {}
        }

        Ok(true)
    }

    pub(in crate::app) fn open_diff_search_modal(&mut self) {
        if self.diff_search_modal_open {
            return;
        }

        self.find_prefix_pending = false;
        self.diff_search_modal_open = true;
        self.diff_search_query.clear();
        self.diff_search_results = Default::default();
        self.diff_search_selected_index = 0;
        self.cancel_diff_search_query_task();
        self.ensure_diff_search_index_load();
        self.refresh_diff_search_modal_status();
    }

    pub(in crate::app) fn close_diff_search_modal(&mut self) {
        self.diff_search_modal_open = false;
        self.diff_search_query.clear();
        self.diff_search_loading = false;
        self.diff_search_error = None;
        self.diff_search_results = Default::default();
        self.diff_search_selected_index = 0;
        self.cancel_diff_search_query_task();
    }

    pub(in crate::app) fn cancel_diff_search_tasks(&mut self) {
        self.cancel_diff_search_index_task();
        self.cancel_diff_search_query_task();
    }

    pub(in crate::app) fn queue_diff_search_index_load(&mut self) {
        self.cancel_diff_search_tasks();
        self.diff_search_index = None;
        self.diff_search_index_error = None;
        self.diff_search_results = Default::default();
        self.diff_search_selected_index = 0;
        if self.diff_search_modal_open {
            self.diff_search_loading = true;
            self.diff_search_error = None;
        }
        self.spawn_diff_search_index_load();
    }

    pub(in crate::app) fn clear_diff_search_index(&mut self) {
        self.cancel_diff_search_tasks();
        self.diff_search_index_request_id = self.diff_search_index_request_id.saturating_add(1);
        self.diff_search_index = None;
        self.diff_search_index_error = None;
        self.diff_search_results = Default::default();
        self.diff_search_selected_index = 0;
        if self.diff_search_modal_open {
            self.diff_search_loading = false;
            self.diff_search_error = None;
        }
    }

    fn cancel_diff_search_index_task(&mut self) {
        if let Some(task) = self.diff_search_load_task.take() {
            task.abort();
        }
    }

    fn cancel_diff_search_query_task(&mut self) {
        if let Some(task) = self.diff_search_query_task.take() {
            task.abort();
        }
    }

    pub(in crate::app) fn handle_diff_search_index_loaded(
        &mut self,
        request_id: u64,
        result: Result<git::DiffSearchIndex, String>,
    ) -> bool {
        if request_id != self.diff_search_index_request_id {
            return false;
        }

        self.diff_search_load_task = None;
        self.diff_search_loading = false;
        match result {
            Ok(index) => {
                self.diff_search_index = Some(Arc::new(index));
                self.diff_search_index_error = None;
                self.diff_search_results = Default::default();
                if self.diff_search_modal_open {
                    self.queue_diff_search_results();
                }
            }
            Err(error) => {
                self.diff_search_index = None;
                self.diff_search_index_error = Some(error);
                self.diff_search_results = Default::default();
                self.diff_search_selected_index = 0;
                if self.diff_search_modal_open {
                    self.refresh_diff_search_modal_status();
                }
            }
        }
        self.diff_search_modal_open
    }

    pub(in crate::app) fn handle_diff_search_results_loaded(
        &mut self,
        request_id: u64,
        result: Result<git::DiffSearchResults, String>,
    ) -> bool {
        if request_id != self.diff_search_query_request_id || !self.diff_search_modal_open {
            return false;
        }

        self.diff_search_query_task = None;
        self.diff_search_loading = false;
        match result {
            Ok(mut results) => {
                results.group_items_by_file();
                self.diff_search_results = results;
                self.diff_search_error = None;
                self.clamp_diff_search_selection();
            }
            Err(error) => {
                self.diff_search_results = Default::default();
                self.diff_search_error = Some(error);
                self.diff_search_selected_index = 0;
            }
        }
        true
    }

    pub(in crate::app) fn apply_pending_diff_search_target(&mut self) {
        let Some(target) = self.pending_diff_search_target.clone() else {
            return;
        };
        if self.selected_file().map(|file| file.path.as_str()) != Some(target.file_path.as_str()) {
            return;
        }

        if let Some(display_index) = self.diff_view.display_index_for_line(
            self.diff_view_mode,
            self.current_diff_display_width(),
            target.old_line,
            target.new_line,
        ) {
            self.selected_diff_line_index = display_index;
            self.diff_scroll = display_index.min(u16::MAX as usize) as u16;
            self.active_pane = super::ActivePane::Diff;
            self.pending_diff_search_target = None;
        } else if self.diff_load_task.is_none() {
            self.pending_diff_search_target = None;
        }
    }

    fn spawn_diff_search_index_load(&mut self) {
        self.diff_search_index_request_id = self.diff_search_index_request_id.saturating_add(1);
        let request_id = self.diff_search_index_request_id;
        let repo_root = self.repo_root.clone();
        let files = self.files.clone();
        let review_mode = self.review_mode.clone();
        let sender = self.events.sender();

        self.diff_search_load_task = Some(task::spawn(async move {
            let result = load_diff_search_index(&repo_root, &files, &review_mode)
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::DiffSearchIndexLoaded { request_id, result });
        }));
    }

    fn queue_diff_search_results(&mut self) {
        self.cancel_diff_search_query_task();

        if let Some(index) = self.diff_search_index.as_ref() {
            if index.is_empty() {
                self.diff_search_loading = false;
                self.diff_search_error = Some(EMPTY_DIFF_SEARCH_MESSAGE.to_string());
                self.diff_search_results = Default::default();
                self.diff_search_selected_index = 0;
                return;
            }
        }

        let query = self.diff_search_query.trim().to_string();
        if query.is_empty() {
            self.diff_search_loading = false;
            self.diff_search_error = None;
            self.diff_search_results = Default::default();
            self.diff_search_selected_index = 0;
            return;
        }

        let Some(index) = self.diff_search_index.clone() else {
            self.refresh_diff_search_modal_status();
            return;
        };

        self.diff_search_query_request_id = self.diff_search_query_request_id.saturating_add(1);
        let request_id = self.diff_search_query_request_id;
        let sender = self.events.sender();
        let highlight_registry = self.highlight_registry.clone();
        self.diff_search_loading = true;

        self.diff_search_query_task = Some(task::spawn(async move {
            let result = task::spawn_blocking(move || {
                let mut matcher = git::DiffSearchMatcher::default();
                let mut results = index.search(
                    &query,
                    DiffSearchOptions {
                        limit: DIFF_SEARCH_RESULT_LIMIT,
                        include_context: true,
                    },
                    &mut matcher,
                );
                if let Some(registry) = highlight_registry {
                    results.apply_syntax_highlighting(registry.as_ref());
                }
                Ok::<_, String>(results)
            })
            .await
            .unwrap_or_else(|error| Err(error.to_string()));
            let _ = sender.send(Event::DiffSearchResultsLoaded { request_id, result });
        }));
    }

    fn ensure_diff_search_index_load(&mut self) {
        if self.diff_search_index.is_some()
            || self.diff_search_index_error.is_some()
            || self.diff_search_load_task.is_some()
        {
            return;
        }

        self.queue_diff_search_index_load();
    }

    fn refresh_diff_search_modal_status(&mut self) {
        if self.diff_search_load_task.is_some() {
            self.diff_search_loading = true;
            self.diff_search_error = None;
            return;
        }

        self.diff_search_loading = false;
        self.diff_search_error = self.diff_search_index_error.clone().or_else(|| {
            match self.diff_search_index.as_ref() {
                Some(index) if index.is_empty() => Some(EMPTY_DIFF_SEARCH_MESSAGE.to_string()),
                _ => None,
            }
        });
    }

    fn selected_diff_search_result(&self) -> Option<DiffSearchResult> {
        self.diff_search_results
            .items
            .get(self.diff_search_selected_index)
            .cloned()
    }

    async fn confirm_diff_search_modal(&mut self) -> color_eyre::Result<()> {
        let Some(result) = self.selected_diff_search_result() else {
            return Ok(());
        };

        let target = DiffSearchNavigationTarget::from_result(&result);
        self.close_diff_search_modal();
        self.pending_diff_search_target = Some(target.clone());
        self.select_file_by_path(&target.file_path).await?;
        self.apply_pending_diff_search_target();
        Ok(())
    }

    fn clamp_diff_search_selection(&mut self) {
        self.diff_search_selected_index = clamp_index(
            self.diff_search_selected_index,
            self.diff_search_results.items.len(),
        );
    }

    fn move_diff_search_selection(&mut self, delta: i32) {
        self.diff_search_selected_index = move_index(
            self.diff_search_selected_index,
            self.diff_search_results.items.len(),
            delta,
        );
    }
}

async fn load_diff_search_index(
    repo_root: &std::path::Path,
    files: &[git::FileEntry],
    review_mode: &ReviewMode,
) -> color_eyre::Result<git::DiffSearchIndex> {
    if files.is_empty() {
        return Ok(git::DiffSearchIndex::default());
    }

    match review_mode {
        ReviewMode::WorkingTree => {
            git::load_diff_search_index_for_working_tree(repo_root, files).await
        }
        ReviewMode::CommitCompare(selection) => {
            git::load_diff_search_index_for_commit_compare(repo_root, selection).await
        }
        ReviewMode::BranchCompare(selection) => {
            git::load_diff_search_index_for_branch_compare(repo_root, selection).await
        }
    }
}
