use crossterm::event::{KeyCode, KeyEvent};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use tokio::task;

use super::input::is_plain_text_key;
use super::navigation::{clamp_index, move_index};
use super::*;

struct CommitSearchCandidate {
    index: usize,
    haystack: String,
}

impl AsRef<str> for CommitSearchCandidate {
    fn as_ref(&self) -> &str {
        &self.haystack
    }
}

impl App {
    pub(super) async fn handle_commit_search_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if !self.commit_search_modal_open {
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc => {
                self.close_commit_search_modal();
            }
            KeyCode::Enter => {
                if let Some(commit) = self.selected_commit_search_entry() {
                    self.enter_commit_compare(commit).await?;
                }
                self.close_commit_search_modal();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_commit_search_selection(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_commit_search_selection(-1);
            }
            KeyCode::Backspace => {
                self.commit_search_query.pop();
                self.clamp_commit_search_selection();
                self.commit_search_error = None;
            }
            KeyCode::Char(ch) if is_plain_text_key(key_event) => {
                self.commit_search_query.push(ch);
                self.clamp_commit_search_selection();
                self.commit_search_error = None;
            }
            _ => {}
        }

        Ok(true)
    }

    pub(super) fn handle_commit_search_loaded(
        &mut self,
        result: Result<Vec<CommitSearchEntry>, String>,
    ) {
        if !self.commit_search_modal_open {
            return;
        }

        self.commit_search_loading = false;
        match result {
            Ok(entries) => {
                self.commit_search_entries = entries;
                self.commit_search_error = None;
                self.clamp_commit_search_selection();
            }
            Err(error) => {
                self.commit_search_entries.clear();
                self.commit_search_error = Some(error);
                self.commit_search_selected_index = 0;
            }
        }
    }

    pub(super) fn open_commit_search_modal(&mut self) {
        if self.commit_search_modal_open {
            return;
        }

        self.commit_search_modal_open = true;
        self.commit_search_query.clear();
        self.commit_search_entries.clear();
        self.commit_search_loading = true;
        self.commit_search_error = None;
        self.commit_search_selected_index = 0;

        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = git::list_searchable_commits(&repo_root, 12_000)
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::CommitSearchLoaded(result));
        }));
    }

    pub(super) fn close_commit_search_modal(&mut self) {
        self.commit_search_modal_open = false;
        self.commit_search_loading = false;
        self.commit_search_error = None;
        self.commit_search_selected_index = 0;
    }

    pub fn filtered_commit_search_indices(&mut self) -> Vec<usize> {
        let query = self.commit_search_query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return (0..self.commit_search_entries.len()).collect();
        }

        let pattern = Pattern::parse(&query, CaseMatching::Ignore, Normalization::Smart);
        let candidates = self
            .commit_search_entries
            .iter()
            .enumerate()
            .map(|(index, entry)| CommitSearchCandidate {
                index,
                haystack: format!("{} {} {}", entry.short_hash, entry.hash, entry.subject),
            })
            .collect::<Vec<_>>();

        pattern
            .match_list(candidates, &mut self.commit_search_matcher)
            .into_iter()
            .map(|(candidate, _score)| candidate.index)
            .collect()
    }

    pub(super) fn clamp_commit_search_selection(&mut self) {
        let filtered_len = self.filtered_commit_search_indices().len();
        self.commit_search_selected_index =
            clamp_index(self.commit_search_selected_index, filtered_len);
    }

    pub(super) fn move_commit_search_selection(&mut self, delta: i32) {
        let filtered_len = self.filtered_commit_search_indices().len();
        self.commit_search_selected_index =
            move_index(self.commit_search_selected_index, filtered_len, delta);
    }

    pub(super) fn selected_commit_search_entry(&mut self) -> Option<CommitSearchEntry> {
        self.filtered_commit_search_indices()
            .get(self.commit_search_selected_index)
            .and_then(|index| self.commit_search_entries.get(*index))
            .cloned()
    }

    pub(super) async fn enter_commit_compare(
        &mut self,
        commit: CommitSearchEntry,
    ) -> color_eyre::Result<()> {
        self.review_mode = ReviewMode::CommitCompare(CommitCompareSelection {
            base_ref: git::resolve_commit_base_ref(&commit),
            commit_hash: commit.hash.clone(),
            short_hash: commit.short_hash.clone(),
            subject: commit.subject.clone(),
        });
        self.refresh().await
    }
}

#[cfg(test)]
mod tests;
