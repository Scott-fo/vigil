//! The lines of a file's patch, exactly as `git diff` produced them.
//!
//! A [`DiffView`] can show more than the patch: context the reviewer expands
//! between hunks is read from the file itself. Anything that must name lines
//! the way the patch does (review comments, which a forge only accepts on
//! lines inside the patch's hunks) reads [`PatchLine`]s instead of display
//! rows. Expanded context, gap bands, and merge-conflict chrome never appear
//! here.
//!
//! Each line records how far it sits from the nearest change in its hunk, so
//! a caller can reproduce the hunks any context size would give without
//! running `git diff` again: a context line belongs to the `-U<n>` hunks
//! exactly when its [`PatchLine::distance_to_change`] is at most `n`. That
//! holds whatever `diff.context` the user configured, as long as it is at
//! least `n`.

use crate::app::{DiffLineWrapMode, DiffViewMode};

use super::{DiffLineKind, DiffSelectionPane, DiffView, display::DisplayRowRefs};

/// Whether a patch line was added, removed, or left unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PatchLineKind {
    Added,
    Removed,
    Context,
}

/// One line of a file's patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchLine {
    pub kind: PatchLineKind,
    /// Line number in the old file; `None` for added lines.
    pub old_line: Option<usize>,
    /// Line number in the new file; `None` for removed lines.
    pub new_line: Option<usize>,
    /// The line's text without its line ending.
    pub text: String,
    /// Index of the hunk the line belongs to within the file's patch.
    pub hunk: usize,
    /// Changed lines are 0. A context line counts the context lines from it
    /// to the nearest added or removed line of its hunk, itself included: 1
    /// right next to a change. `usize::MAX` in a hunk without changes.
    pub distance_to_change: usize,
}

impl DiffView {
    /// Every line of the patch in order, hunk by hunk.
    pub fn patch_lines(&self) -> Vec<PatchLine> {
        (0..self.hunks.len())
            .flat_map(|hunk| self.hunk_patch_lines(hunk))
            .collect()
    }

    /// The patch line a display row shows, or `None` for rows the patch does
    /// not contain: expanded context, gap bands, headers, and conflict
    /// chrome. Every soft-wrapped row of a line reports that line.
    ///
    /// A split-view row can pair a removed line (left) with an added one
    /// (right); `pane` picks which. [`DiffSelectionPane::Unified`] prefers
    /// the right side and falls back to the left.
    pub fn patch_line_at(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        display_index: usize,
        pane: DiffSelectionPane,
    ) -> Option<PatchLine> {
        self.ensure_display_cache(mode, width, line_wrap);
        let DisplayRowRefs { left, right } =
            *self.display_cache.entry(mode).row_refs.get(display_index)?;
        let row_index = match pane {
            DiffSelectionPane::Left => left,
            DiffSelectionPane::Right => right,
            DiffSelectionPane::Unified => right.or(left),
        }?;
        let (hunk, _) = self
            .hunks
            .iter()
            .enumerate()
            .find(|(_, hunk)| (hunk.row_start..hunk.row_end).contains(&row_index))?;
        let target = self.rows.get(row_index)?;
        if matches!(
            target.kind,
            DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_)
        ) {
            return None;
        }
        self.hunk_patch_lines(hunk)
            .into_iter()
            .find(|line| line.old_line == target.old_line && line.new_line == target.new_line)
    }

    fn hunk_patch_lines(&self, hunk_index: usize) -> Vec<PatchLine> {
        let Some(hunk) = self.hunks.get(hunk_index) else {
            return Vec::new();
        };
        let mut lines = self.rows[hunk.row_start..hunk.row_end]
            .iter()
            .filter_map(|row| {
                let kind = match row.kind {
                    DiffLineKind::Added => PatchLineKind::Added,
                    DiffLineKind::Removed => PatchLineKind::Removed,
                    DiffLineKind::Context => PatchLineKind::Context,
                    DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => return None,
                };
                Some(PatchLine {
                    kind,
                    old_line: row.old_line,
                    new_line: row.new_line,
                    text: row.text.clone(),
                    hunk: hunk_index,
                    distance_to_change: 0,
                })
            })
            .collect::<Vec<_>>();
        assign_change_distances(&mut lines);
        lines
    }
}

/// Fills `distance_to_change` for one hunk's lines.
fn assign_change_distances(lines: &mut [PatchLine]) {
    let is_change = |line: &PatchLine| line.kind != PatchLineKind::Context;
    let mut since_change: Option<usize> = None;
    let mut forward = vec![usize::MAX; lines.len()];
    for (index, line) in lines.iter().enumerate() {
        since_change = if is_change(line) {
            Some(0)
        } else {
            since_change.map(|count| count + 1)
        };
        forward[index] = since_change.unwrap_or(usize::MAX);
    }
    let mut until_change: Option<usize> = None;
    for index in (0..lines.len()).rev() {
        until_change = if is_change(&lines[index]) {
            Some(0)
        } else {
            until_change.map(|count| count + 1)
        };
        lines[index].distance_to_change = forward[index].min(until_change.unwrap_or(usize::MAX));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::build_diff_view_from_diff_text;

    fn view() -> DiffView {
        build_diff_view_from_diff_text(
            "diff --git a/f.rs b/f.rs\n\
             --- a/f.rs\n\
             +++ b/f.rs\n\
             @@ -1,9 +1,9 @@\n \
             one\n \
             two\n \
             three\n \
             four\n\
             -five\n\
             +FIVE\n \
             six\n \
             seven\n \
             eight\n \
             nine\n\
             @@ -20,2 +20,3 @@\n \
             twenty\n\
             +added\n \
             twenty-one\n",
            None,
        )
    }

    #[test]
    fn patch_lines_number_both_files_and_measure_distance_to_the_nearest_change() {
        let lines = view().patch_lines();
        let summary = lines
            .iter()
            .map(|line| {
                (
                    line.kind,
                    line.old_line,
                    line.new_line,
                    line.distance_to_change,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            summary[..6],
            [
                (PatchLineKind::Context, Some(1), Some(1), 4),
                (PatchLineKind::Context, Some(2), Some(2), 3),
                (PatchLineKind::Context, Some(3), Some(3), 2),
                (PatchLineKind::Context, Some(4), Some(4), 1),
                (PatchLineKind::Removed, Some(5), None, 0),
                (PatchLineKind::Added, None, Some(5), 0),
            ]
        );
        assert_eq!(summary[9], (PatchLineKind::Context, Some(9), Some(9), 4));
        assert_eq!(lines[4].text, "five");
        assert!(lines[..10].iter().all(|line| line.hunk == 0));
        assert_eq!(lines[11].kind, PatchLineKind::Added);
        assert_eq!((lines[11].hunk, lines[11].new_line), (1, Some(21)));
    }

    #[test]
    fn display_rows_map_to_patch_lines_and_expanded_context_maps_to_none() {
        let mut view = view();
        let (mode, width, wrap) = (DiffViewMode::Unified, 120, DiffLineWrapMode::NoWrap);
        let count = view.display_line_count(mode, width, wrap);
        let mapped = (0..count)
            .map(|index| {
                view.patch_line_at(mode, width, wrap, index, DiffSelectionPane::Unified)
                    .map(|line| (line.old_line, line.new_line))
            })
            .collect::<Vec<_>>();

        assert_eq!(mapped[4], Some((Some(5), None)), "removed line");
        assert_eq!(mapped[5], Some((None, Some(5))), "added line");
        let unmapped = mapped.iter().filter(|line| line.is_none()).count();
        assert_eq!(unmapped, 2, "the collapsed gap band between hunks");
        assert_eq!(mapped.last(), Some(&Some((Some(21), Some(22)))));
    }

    #[test]
    fn split_rows_pick_the_side_the_pane_names() {
        let mut view = view();
        let (mode, width, wrap) = (DiffViewMode::Split, 120, DiffLineWrapMode::NoWrap);
        let paired = (0..view.display_line_count(mode, width, wrap))
            .find(|index| {
                view.patch_line_at(mode, width, wrap, *index, DiffSelectionPane::Left)
                    .is_some_and(|line| line.kind == PatchLineKind::Removed)
            })
            .expect("the removed line renders on the left");

        let right = view
            .patch_line_at(mode, width, wrap, paired, DiffSelectionPane::Right)
            .unwrap();
        let unified = view
            .patch_line_at(mode, width, wrap, paired, DiffSelectionPane::Unified)
            .unwrap();
        assert_eq!(right.kind, PatchLineKind::Added);
        assert_eq!(unified, right, "unified prefers the new side");
    }
}
