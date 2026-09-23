//! Change-block navigation over the rendered diff.
//!
//! A change block is a run of consecutive display lines that show added,
//! removed, or merge-conflict rows. Context lines, collapsed-gap rows, and
//! expanded context end a block. Wrapped continuation lines belong to the block
//! of the row they continue, so a long changed line never counts twice.
//!
//! Indices are display indices for the given mode, width, and wrap setting,
//! the same space as the diff cursor. Results are computed from the display
//! cache, so the first call after a layout change pays for building it.

use crate::app::{DiffLineWrapMode, DiffViewMode};

use super::{DiffLineKind, DiffView, DisplayRowRefs};

/// Which way to look for a change block relative to the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeDirection {
    Next,
    Previous,
}

impl DiffView {
    /// Display index where the nearest change block in `direction` starts,
    /// strictly after (or before) `current`. `None` when there is no such block.
    pub fn adjacent_change_index(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        current: usize,
        direction: ChangeDirection,
    ) -> Option<usize> {
        let starts = self.change_block_starts(mode, width, line_wrap);
        match direction {
            ChangeDirection::Next => starts.into_iter().find(|start| *start > current),
            ChangeDirection::Previous => starts.into_iter().rev().find(|start| *start < current),
        }
    }

    /// Start of the first change block (`Next`) or the last one (`Previous`),
    /// used when landing in a file from a neighbouring one.
    pub fn edge_change_index(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        direction: ChangeDirection,
    ) -> Option<usize> {
        let starts = self.change_block_starts(mode, width, line_wrap);
        match direction {
            ChangeDirection::Next => starts.first().copied(),
            ChangeDirection::Previous => starts.last().copied(),
        }
    }

    /// First selectable display index of every change block, in order.
    fn change_block_starts(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
    ) -> Vec<usize> {
        self.ensure_display_cache(mode, width, line_wrap);
        let cache = self.display_cache.entry(mode);
        let is_change = |refs: &DisplayRowRefs| {
            [refs.left, refs.right]
                .into_iter()
                .flatten()
                .filter_map(|row_index| self.rows.get(row_index))
                .any(|row| row.kind != DiffLineKind::Context)
        };

        let mut starts = Vec::new();
        let mut in_block = false;
        let mut block_start_pending = false;
        for (index, refs) in cache.row_refs.iter().enumerate() {
            let changed = is_change(refs);
            if changed && !in_block {
                block_start_pending = true;
            }
            in_block = changed;
            // Land on the first line of the block the cursor can rest on.
            if block_start_pending && changed && cache.nav.get(index).copied().flatten().is_some() {
                starts.push(index);
                block_start_pending = false;
            }
            if !changed {
                block_start_pending = false;
            }
        }
        starts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::build_diff_view_from_diff_text;

    const WIDTH: usize = 120;

    /// Two hunks: the first has a modified line and, after two context lines,
    /// an added line; the second (after a collapsed gap) removes a line.
    fn two_hunk_diff() -> DiffView {
        build_diff_view_from_diff_text(
            concat!(
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -1,5 +1,6 @@\n",
                "-fn old() {}\n",
                "+fn new() {}\n",
                " fn a() {}\n",
                " fn b() {}\n",
                "+fn added() {}\n",
                " fn c() {}\n",
                " fn d() {}\n",
                "@@ -40,3 +41,2 @@\n",
                " fn x() {}\n",
                "-fn removed() {}\n",
                " fn y() {}\n",
            ),
            Some("rust"),
        )
    }

    fn starts(view: &mut DiffView, mode: DiffViewMode, wrap: DiffLineWrapMode) -> Vec<usize> {
        view.change_block_starts(mode, WIDTH, wrap)
    }

    #[test]
    fn unified_blocks_start_at_each_run_of_changes() {
        let mut view = two_hunk_diff();
        let starts = starts(&mut view, DiffViewMode::Unified, DiffLineWrapMode::Wrap);

        // Rows: -old +new a b +added c d [gap ↓] [gap ↑] x -removed y
        assert_eq!(starts, vec![0, 4, 10]);
    }

    #[test]
    fn split_pairs_a_modification_into_one_block() {
        let mut view = two_hunk_diff();
        let starts = starts(&mut view, DiffViewMode::Split, DiffLineWrapMode::Wrap);

        // Rows: old|new a b _|added c d [gap ↓] [gap ↑] x removed|_ y
        assert_eq!(starts, vec![0, 3, 9]);
    }

    #[test]
    fn next_and_previous_skip_to_neighbouring_blocks() {
        let mut view = two_hunk_diff();
        let mode = DiffViewMode::Unified;
        let wrap = DiffLineWrapMode::Wrap;
        let next = |view: &mut DiffView, current| {
            view.adjacent_change_index(mode, WIDTH, wrap, current, ChangeDirection::Next)
        };
        let previous = |view: &mut DiffView, current| {
            view.adjacent_change_index(mode, WIDTH, wrap, current, ChangeDirection::Previous)
        };

        assert_eq!(next(&mut view, 0), Some(4));
        // From inside the first block, next still means the following block.
        assert_eq!(next(&mut view, 1), Some(4));
        assert_eq!(next(&mut view, 4), Some(10));
        assert_eq!(next(&mut view, 10), None);
        assert_eq!(previous(&mut view, 10), Some(4));
        // From inside a block, previous goes to the block before it.
        assert_eq!(previous(&mut view, 5), Some(4));
        assert_eq!(previous(&mut view, 0), None);
    }

    #[test]
    fn edge_changes_are_first_and_last_blocks() {
        let mut view = two_hunk_diff();
        let mode = DiffViewMode::Unified;
        let wrap = DiffLineWrapMode::Wrap;

        assert_eq!(
            view.edge_change_index(mode, WIDTH, wrap, ChangeDirection::Next),
            Some(0)
        );
        assert_eq!(
            view.edge_change_index(mode, WIDTH, wrap, ChangeDirection::Previous),
            Some(10)
        );
    }

    #[test]
    fn wrapped_changed_line_counts_as_one_block() {
        let long = "x".repeat(WIDTH * 3);
        let mut view = build_diff_view_from_diff_text(
            &format!(
                "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,4 @@\n fn a() {{}}\n+let s = \"{long}\";\n fn b() {{}}\n+fn c() {{}}\n"
            ),
            Some("rust"),
        );
        let wrapped = starts(&mut view, DiffViewMode::Unified, DiffLineWrapMode::Wrap);
        let unwrapped = starts(&mut view, DiffViewMode::Unified, DiffLineWrapMode::NoWrap);

        assert_eq!(unwrapped, vec![1, 3]);
        assert_eq!(wrapped.len(), 2);
        assert_eq!(wrapped[0], 1);
        assert!(
            wrapped[1] > 3,
            "second block starts after the wrapped continuation lines"
        );
    }

    #[test]
    fn diff_without_changes_has_no_blocks() {
        let mut view = DiffView::empty("No textual diff available.");

        assert!(starts(&mut view, DiffViewMode::Unified, DiffLineWrapMode::Wrap).is_empty());
        assert_eq!(
            view.edge_change_index(
                DiffViewMode::Unified,
                WIDTH,
                DiffLineWrapMode::Wrap,
                ChangeDirection::Next
            ),
            None
        );
    }
}
