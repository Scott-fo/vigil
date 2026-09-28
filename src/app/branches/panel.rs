use std::collections::HashSet;

use nucleo_matcher::{
    Config as MatcherConfig, Matcher,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::git::{BranchEntry, BranchOperation, BranchSnapshot};

use super::super::{App, navigation::move_index};

/// Branch panel state. Selection is kept by branch name so it survives
/// filtering and snapshot reloads.
#[derive(Debug)]
pub struct BranchPanel {
    query: String,
    mode: BranchPanelMode,
    selected: Option<String>,
    error: Option<String>,
    matcher: Matcher,
}

/// What keys do in the branch panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchPanelMode {
    /// Single-key actions on the selected branch.
    Browse,
    /// Typing edits the filter query.
    Filter,
    /// Typing edits the name of a branch to create at `start_point`.
    Create { start_point: String, name: String },
    /// Typing edits the new name for `branch`.
    Rename { branch: String, name: String },
    /// Waiting for confirmation. `force` is set after git refused a safe
    /// delete because the branch is not fully merged.
    Delete { branch: String, force: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchSection {
    Local,
    Remote,
}

/// One row of the branch list: a section heading or an index into
/// [`BranchSnapshot::branches`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchPanelRow {
    Section(BranchSection),
    Branch(usize),
}

/// Everything the UI needs to draw the branch panel for one frame.
#[derive(Debug)]
pub struct BranchPanelView<'a> {
    pub panel: &'a BranchPanel,
    pub snapshot: Option<&'a BranchSnapshot>,
    pub loading: bool,
    pub load_error: Option<&'a str>,
    /// The operation in flight, if any. The panel shows its progress instead
    /// of accepting new actions.
    pub operation: Option<&'a BranchOperation>,
    pub rows: Vec<BranchPanelRow>,
    /// Index into `rows` of the selected branch.
    pub selected_row: Option<usize>,
}

impl BranchPanel {
    pub(super) fn new() -> Self {
        Self {
            query: String::new(),
            mode: BranchPanelMode::Browse,
            selected: None,
            error: None,
            matcher: Matcher::new(MatcherConfig::DEFAULT.match_paths()),
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn mode(&self) -> &BranchPanelMode {
        &self.mode
    }

    /// The last operation failure, shown until the next action.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(super) fn set_mode(&mut self, mode: BranchPanelMode) {
        self.mode = mode;
        self.error = None;
    }

    pub(super) fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    pub(super) fn clear_error(&mut self) {
        self.error = None;
    }

    pub(super) fn query_mut(&mut self) -> &mut String {
        &mut self.query
    }

    pub(super) fn name_mut(&mut self) -> Option<&mut String> {
        match &mut self.mode {
            BranchPanelMode::Create { name, .. } | BranchPanelMode::Rename { name, .. } => {
                Some(name)
            }
            _ => None,
        }
    }

    /// Local branches (checked out first, then most recently committed),
    /// then remote branches that no local branch tracks, narrowed by the
    /// filter query. Remote branches a local branch already tracks are left
    /// out; the local row stands for both.
    pub fn rows(&mut self, snapshot: &BranchSnapshot) -> Vec<BranchPanelRow> {
        let tracked = snapshot
            .branches
            .iter()
            .filter_map(|branch| branch.upstream.as_ref())
            .map(|upstream| upstream.name.as_str())
            .collect::<HashSet<_>>();
        let candidates = snapshot
            .branches
            .iter()
            .enumerate()
            .filter(|(_, branch)| !(branch.is_remote() && tracked.contains(branch.name.as_str())))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let matching = self.matching(snapshot, candidates);

        let (mut local, remote): (Vec<usize>, Vec<usize>) = matching
            .into_iter()
            .partition(|index| !snapshot.branches[*index].is_remote());
        local.sort_by_key(|index| !snapshot.branches[*index].is_head);

        let mut rows = Vec::with_capacity(local.len() + remote.len() + 2);
        for (section, indices) in [
            (BranchSection::Local, local),
            (BranchSection::Remote, remote),
        ] {
            if !indices.is_empty() {
                rows.push(BranchPanelRow::Section(section));
                rows.extend(indices.into_iter().map(BranchPanelRow::Branch));
            }
        }
        rows
    }

    /// Candidates the query matches, in their original order.
    fn matching(&mut self, snapshot: &BranchSnapshot, candidates: Vec<usize>) -> Vec<usize> {
        let query = self.query.trim();
        if query.is_empty() {
            return candidates;
        }

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let matched = pattern
            .match_list(
                candidates
                    .iter()
                    .map(|index| BranchName(*index, &snapshot.branches[*index].name)),
                &mut self.matcher,
            )
            .into_iter()
            .map(|(candidate, _)| candidate.0)
            .collect::<HashSet<_>>();
        candidates
            .into_iter()
            .filter(|index| matched.contains(index))
            .collect()
    }

    /// Row index of the selected branch, falling back to the first branch
    /// when the selection is filtered out.
    pub fn selected_row(
        &self,
        snapshot: &BranchSnapshot,
        rows: &[BranchPanelRow],
    ) -> Option<usize> {
        let branch_rows = || {
            rows.iter()
                .enumerate()
                .filter_map(|(row, item)| match item {
                    BranchPanelRow::Branch(index) => Some((row, *index)),
                    BranchPanelRow::Section(_) => None,
                })
        };
        self.selected
            .as_deref()
            .and_then(|selected| {
                branch_rows()
                    .find(|(_, index)| snapshot.branches[*index].name == selected)
                    .map(|(row, _)| row)
            })
            .or_else(|| branch_rows().next().map(|(row, _)| row))
    }

    pub(super) fn selected_branch<'a>(
        &mut self,
        snapshot: &'a BranchSnapshot,
    ) -> Option<&'a BranchEntry> {
        let rows = self.rows(snapshot);
        match rows.get(self.selected_row(snapshot, &rows)?)? {
            BranchPanelRow::Branch(index) => snapshot.branches.get(*index),
            BranchPanelRow::Section(_) => None,
        }
    }

    pub(super) fn move_selection(&mut self, snapshot: &BranchSnapshot, delta: i32) {
        let rows = self.rows(snapshot);
        let branches = branch_indices(&rows);
        let current = self
            .selected_row(snapshot, &rows)
            .and_then(|row| match rows[row] {
                BranchPanelRow::Branch(index) => branches.iter().position(|item| *item == index),
                BranchPanelRow::Section(_) => None,
            })
            .unwrap_or(0);
        let next = move_index(current, branches.len(), delta);
        self.selected = branches
            .get(next)
            .map(|index| snapshot.branches[*index].name.clone());
    }

    /// Keeps a visible selection. Otherwise selects the branch `git switch -`
    /// would return to, or the first branch that is not checked out, so the
    /// most likely switch is one keypress away.
    pub(super) fn seed_selection(&mut self, snapshot: &BranchSnapshot) {
        let rows = self.rows(snapshot);
        let visible = branch_indices(&rows)
            .into_iter()
            .map(|index| &snapshot.branches[index])
            .collect::<Vec<_>>();
        if self
            .selected
            .as_deref()
            .is_some_and(|selected| visible.iter().any(|branch| branch.name == selected))
        {
            return;
        }

        let previous = snapshot.previous_branch.as_deref().and_then(|previous| {
            visible
                .iter()
                .find(|branch| !branch.is_remote() && branch.name == previous)
        });
        self.selected = previous
            .or_else(|| visible.iter().find(|branch| !branch.is_head))
            .or_else(|| visible.first())
            .map(|branch| branch.name.clone());
    }
}

fn branch_indices(rows: &[BranchPanelRow]) -> Vec<usize> {
    rows.iter()
        .filter_map(|row| match row {
            BranchPanelRow::Branch(index) => Some(*index),
            BranchPanelRow::Section(_) => None,
        })
        .collect()
}

struct BranchName<'a>(usize, &'a str);

impl AsRef<str> for BranchName<'_> {
    fn as_ref(&self) -> &str {
        self.1
    }
}

impl App {
    /// Prepared branch panel state, or `None` when the panel is closed.
    pub fn branch_panel_view(&mut self) -> Option<BranchPanelView<'_>> {
        let snapshot = self.branch_status.snapshot();
        let panel = self.branch_panel.as_mut()?;
        let (rows, selected_row) = match snapshot {
            Some(snapshot) => {
                let rows = panel.rows(snapshot);
                let selected_row = panel.selected_row(snapshot, &rows);
                (rows, selected_row)
            }
            None => (Vec::new(), None),
        };
        Some(BranchPanelView {
            panel,
            snapshot,
            loading: self.branch_status.loading(),
            load_error: self.branch_status.error(),
            operation: self.branch_operation.as_ref(),
            rows,
            selected_row,
        })
    }

    pub fn branch_panel_open(&self) -> bool {
        self.branch_panel.is_some()
    }

    pub(crate) fn open_branch_panel(&mut self) {
        if self.branch_panel.is_some() {
            return;
        }

        let mut panel = BranchPanel::new();
        if let Some(snapshot) = self.branch_status.snapshot() {
            panel.seed_selection(snapshot);
        }
        self.branch_panel = Some(panel);
        self.queue_branch_status_load();
    }

    pub(in crate::app) fn close_branch_panel(&mut self) {
        self.branch_panel = None;
    }
}
