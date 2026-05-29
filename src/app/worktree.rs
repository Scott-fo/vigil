use std::path::PathBuf;

use tokio::task;

use super::*;

struct WorktreeSearchCandidate {
    index: usize,
    haystack: String,
}

impl AsRef<str> for WorktreeSearchCandidate {
    fn as_ref(&self) -> &str {
        &self.haystack
    }
}

impl App {
    pub(super) fn handle_worktrees_loaded(&mut self, result: Result<Vec<WorktreeEntry>, String>) {
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

    pub(super) fn open_worktree_modal(&mut self) {
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

    pub(super) fn close_worktree_modal(&mut self) {
        self.worktree_modal_open = false;
        self.worktree_loading = false;
        self.worktree_error = None;
        self.worktree_query.clear();
        self.worktree_selected_index = 0;
    }

    pub(super) async fn confirm_worktree_selection(&mut self) -> color_eyre::Result<()> {
        let Some(path) = self.selected_worktree_path() else {
            return Ok(());
        };

        self.close_worktree_modal();
        self.switch_to_worktree(path).await
    }

    pub(super) async fn switch_to_worktree(&mut self, path: PathBuf) -> color_eyre::Result<()> {
        if self.repo_root == path && self.is_working_tree_mode() {
            self.status_message = Some(format!("already watching {}", path.display()));
            return Ok(());
        }

        self.cancel_inflight_diff_load();
        self.review_mode = ReviewMode::WorkingTree;
        self.repo_root = path.clone();
        self.repo_watcher = None;
        self.repo_watcher_loading = false;
        self.collapsed_directories.clear();
        self.refresh().await?;
        self.status_message = Some(format!("watching {}", path.display()));
        Ok(())
    }

    pub fn filtered_worktree_indices(&mut self) -> Vec<usize> {
        let query = self.worktree_query.trim();
        if query.is_empty() {
            return (0..self.worktree_entries.len()).collect();
        }

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let candidates = self
            .worktree_entries
            .iter()
            .enumerate()
            .map(|(index, entry)| WorktreeSearchCandidate {
                index,
                haystack: format!(
                    "{} {} {} {}",
                    worktree_display_name(entry),
                    entry.path.display(),
                    entry.branch.as_deref().unwrap_or("detached"),
                    if entry.dirty { "dirty" } else { "clean" }
                ),
            })
            .collect::<Vec<_>>();

        pattern
            .match_list(candidates, &mut self.worktree_matcher)
            .into_iter()
            .map(|(candidate, _score)| candidate.index)
            .collect()
    }

    pub(super) fn clamp_worktree_selection(&mut self) {
        let filtered_len = self.filtered_worktree_indices().len();
        self.worktree_selected_index = self
            .worktree_selected_index
            .min(filtered_len.saturating_sub(1));
    }

    pub(super) fn move_worktree_selection(&mut self, delta: i32) {
        let filtered_len = self.filtered_worktree_indices().len();
        if filtered_len == 0 {
            self.worktree_selected_index = 0;
            return;
        }

        let current = self.worktree_selected_index.min(filtered_len - 1);
        self.worktree_selected_index = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            current.saturating_add(delta as usize)
        }
        .min(filtered_len - 1);
    }

    pub fn selected_worktree_path(&mut self) -> Option<PathBuf> {
        self.filtered_worktree_indices()
            .get(self.worktree_selected_index)
            .and_then(|index| self.worktree_entries.get(*index))
            .map(|entry| entry.path.clone())
    }

    fn seed_worktree_selection(&mut self) {
        let filtered = self.filtered_worktree_indices();
        if filtered.is_empty() {
            self.worktree_selected_index = 0;
            return;
        }

        self.worktree_selected_index = filtered
            .iter()
            .position(|entry_index| self.worktree_entries[*entry_index].path == self.repo_root)
            .unwrap_or(0);
    }
}

pub(crate) fn worktree_display_name(entry: &WorktreeEntry) -> String {
    entry.branch.clone().unwrap_or_else(|| {
        entry
            .head
            .as_deref()
            .map(|head| head.chars().take(7).collect())
            .unwrap_or_else(|| "unknown".to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, branch: Option<&str>, dirty: bool) -> WorktreeEntry {
        WorktreeEntry {
            path: PathBuf::from(path),
            head: Some("abcdef0123456789".to_string()),
            branch: branch.map(ToOwned::to_owned),
            detached: branch.is_none(),
            bare: false,
            prunable: false,
            dirty,
            change_count: usize::from(dirty),
        }
    }

    #[test]
    fn worktree_filter_matches_branch_path_and_state() {
        let mut app = App::new_for_benchmarks(PathBuf::from("/repo/main"));
        app.worktree_entries = vec![
            entry("/repo/main", Some("main"), false),
            entry("/repo/feature-auth", Some("feature/auth"), true),
        ];

        app.worktree_query = "dirty auth".to_string();

        assert_eq!(app.filtered_worktree_indices(), vec![1]);
    }

    #[test]
    fn seed_worktree_selection_prefers_current_repo_root() {
        let mut app = App::new_for_benchmarks(PathBuf::from("/repo/feature-auth"));
        app.worktree_entries = vec![
            entry("/repo/main", Some("main"), false),
            entry("/repo/feature-auth", Some("feature/auth"), true),
        ];

        app.seed_worktree_selection();

        assert_eq!(app.worktree_selected_index, 1);
    }
}
