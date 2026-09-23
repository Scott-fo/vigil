//! Collapsed display of machine-generated files.
//!
//! Lockfiles, minified bundles, and snapshots (see [`git::is_generated_file`])
//! are shown as a one-line placeholder until the user expands them. Collapsed
//! files skip diff loading entirely, which keeps huge lockfile diffs off the
//! interactive path. Expansion lasts for the session, survives refreshes, and
//! resets when switching worktrees. Diff search still indexes generated files;
//! jumping to a search hit expands the file first.

use super::*;

impl App {
    /// Whether `file` is generated and has not been expanded this session.
    pub fn is_collapsed_generated_file(&self, file: &FileEntry) -> bool {
        git::is_generated_file(&file.path) && !self.expanded_generated_files.contains(&file.path)
    }

    pub fn selected_file_is_collapsed_generated(&self) -> bool {
        self.selected_file()
            .is_some_and(|file| self.is_collapsed_generated_file(file))
    }

    /// Marks `path` as expanded without reloading; callers that change the
    /// selection afterwards load the diff themselves.
    pub(in crate::app) fn expand_generated_file(&mut self, path: &str) {
        if git::is_generated_file(path) {
            self.expanded_generated_files.insert(path.to_string());
        }
    }

    /// Expands the selected file if it is a collapsed generated file and loads
    /// its diff. Returns false when there was nothing to expand.
    pub(in crate::app) fn expand_selected_generated_file(&mut self) -> bool {
        let Some(path) = self
            .selected_file()
            .filter(|file| self.is_collapsed_generated_file(file))
            .map(|file| file.path.clone())
        else {
            return false;
        };

        self.expanded_generated_files.insert(path);
        self.queue_selected_diff_load(true, true);
        true
    }
}
