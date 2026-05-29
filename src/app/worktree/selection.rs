use std::path::PathBuf;

use super::super::{
    App,
    navigation::{clamp_index, move_index},
};

impl App {
    pub(super) fn clamp_worktree_selection(&mut self) {
        let filtered_len = self.filtered_worktree_indices().len();
        self.worktree_selected_index = clamp_index(self.worktree_selected_index, filtered_len);
    }

    pub(super) fn move_worktree_selection(&mut self, delta: i32) {
        let filtered_len = self.filtered_worktree_indices().len();
        self.worktree_selected_index =
            move_index(self.worktree_selected_index, filtered_len, delta);
    }

    pub fn selected_worktree_path(&mut self) -> Option<PathBuf> {
        self.filtered_worktree_indices()
            .get(self.worktree_selected_index)
            .and_then(|index| self.worktree_entries.get(*index))
            .map(|entry| entry.path.clone())
    }

    pub(super) fn seed_worktree_selection(&mut self) {
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
