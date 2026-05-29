use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};

use super::{super::App, worktree_display_name};

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
}
