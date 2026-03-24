use tokio::task;

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
        self.commit_search_selected_index = self
            .commit_search_selected_index
            .min(filtered_len.saturating_sub(1));
    }

    pub(super) fn move_commit_search_selection(&mut self, delta: i32) {
        let filtered_len = self.filtered_commit_search_indices().len();
        if filtered_len == 0 {
            self.commit_search_selected_index = 0;
            return;
        }

        let current = self.commit_search_selected_index.min(filtered_len - 1);
        let next = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            current.saturating_add(delta as usize)
        }
        .min(filtered_len - 1);
        self.commit_search_selected_index = next;
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
mod tests {
    use super::*;

    fn build_test_app() -> App {
        App::new_for_benchmarks(PathBuf::from("/tmp/vigil-app-tests"))
    }

    fn build_commit_entry(hash: &str, short_hash: &str, subject: &str) -> CommitSearchEntry {
        CommitSearchEntry {
            hash: hash.to_string(),
            short_hash: short_hash.to_string(),
            parent_hashes: vec!["parent".to_string()],
            author: "Author".to_string(),
            date: "2026-03-24".to_string(),
            subject: subject.to_string(),
        }
    }

    #[test]
    fn commit_search_filter_and_clamp_follow_filtered_entries() {
        let mut app = build_test_app();
        app.commit_search_entries = vec![
            build_commit_entry("aaaaaaaa", "aaaaaaa", "initial import"),
            build_commit_entry("bbbbbbbb", "bbbbbbb", "refactor parser"),
            build_commit_entry("cccccccc", "ccccccc", "fix renderer"),
        ];
        app.commit_search_query = "parser".to_string();

        assert_eq!(app.filtered_commit_search_indices(), vec![1]);

        app.commit_search_selected_index = 99;
        app.clamp_commit_search_selection();
        assert_eq!(app.commit_search_selected_index, 0);
        assert_eq!(
            app.selected_commit_search_entry()
                .map(|entry| entry.subject),
            Some("refactor parser".to_string())
        );
    }
}
