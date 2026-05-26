use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::Path,
    sync::Arc,
};

use color_eyre::eyre::{WrapErr, eyre};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use tokio::{fs, process::Command};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{app::DiffViewMode, ui};

use super::{
    BranchCompareSelection, CommitCompareSelection, FileEntry,
    highlight::{
        HighlightRegistry, SyntaxToken, highlight_source_lines, highlight_source_lines_cached_exact,
    },
    parse::build_branch_diff_range,
    repo::git_output,
};

const VIEWPORT_HIGHLIGHT_PADDING_ROWS: usize = 64;
const DIFF_TAB_WIDTH: usize = 4;

#[derive(Debug, Default, Clone)]
pub struct DiffView {
    rows: Vec<DiffRow>,
    pub note: Option<String>,
    hunks: Vec<DiffHunkBlock>,
    gaps: Vec<DiffHunkGap>,
    gap_expansions: HashMap<usize, DiffGapExpansion>,
    old_file_source: Option<Arc<str>>,
    old_exact_highlighted_lines: Option<Arc<[Vec<SyntaxToken>]>>,
    new_file_lines: Option<Vec<String>>,
    new_file_source: Option<Arc<str>>,
    new_exact_highlighted_lines: Option<Arc<[Vec<SyntaxToken>]>>,
    display_cache: DiffDisplayCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSelectionPane {
    Unified,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffSelectionPoint {
    pub display_index: usize,
    pub pane: DiffSelectionPane,
    pub column: usize,
}

impl DiffView {
    pub fn empty(message: impl Into<String>) -> Self {
        Self {
            rows: Vec::new(),
            note: Some(message.into()),
            hunks: Vec::new(),
            gaps: Vec::new(),
            gap_expansions: HashMap::new(),
            old_file_source: None,
            old_exact_highlighted_lines: None,
            new_file_lines: None,
            new_file_source: None,
            new_exact_highlighted_lines: None,
            display_cache: DiffDisplayCache::default(),
        }
    }

    pub fn rendered_lines(&mut self, mode: DiffViewMode, width: usize) -> &[Line<'static>] {
        self.ensure_display_cache(mode, width);
        &self.display_cache.entry(mode).lines
    }

    pub fn selection_point_at(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        display_index: usize,
        column: usize,
    ) -> Option<DiffSelectionPoint> {
        self.ensure_display_cache(mode, width);
        let selection_line = self
            .display_cache
            .entry(mode)
            .selection
            .get(display_index)?;
        let (pane, segment) = match mode {
            DiffViewMode::Unified => (
                DiffSelectionPane::Unified,
                selection_line
                    .unified
                    .as_ref()
                    .filter(|segment| segment.contains(column))?,
            ),
            DiffViewMode::Split => {
                if let Some(segment) = selection_line
                    .left
                    .as_ref()
                    .filter(|segment| segment.contains(column))
                {
                    (DiffSelectionPane::Left, segment)
                } else if let Some(segment) = selection_line
                    .right
                    .as_ref()
                    .filter(|segment| segment.contains(column))
                {
                    (DiffSelectionPane::Right, segment)
                } else {
                    return None;
                }
            }
        };

        Some(DiffSelectionPoint {
            display_index,
            pane,
            column: segment.clamp_column(column),
        })
    }

    pub fn selection_point_for_pane(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        display_index: usize,
        pane: DiffSelectionPane,
        column: usize,
    ) -> Option<DiffSelectionPoint> {
        self.ensure_display_cache(mode, width);
        let selection_line = self
            .display_cache
            .entry(mode)
            .selection
            .get(display_index)?;
        let segment = selection_line.segment(pane)?;
        Some(DiffSelectionPoint {
            display_index,
            pane,
            column: segment.clamp_column(column),
        })
    }

    pub fn selected_text(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        anchor: DiffSelectionPoint,
        head: DiffSelectionPoint,
    ) -> Option<String> {
        if anchor.pane != head.pane {
            return None;
        }

        self.ensure_display_cache(mode, width);
        let selection = self.display_cache.entry(mode).selection.as_slice();
        let (start, end) = normalize_selection_points(anchor, head);
        let mut lines = Vec::new();

        for display_index in start.display_index..=end.display_index {
            let segment = selection.get(display_index)?.segment(anchor.pane)?;
            let start_column = if display_index == start.display_index {
                start.column
            } else {
                0
            };
            let end_column = if display_index == end.display_index {
                end.column.saturating_add(1)
            } else {
                segment.content_width
            };
            lines.push(segment.slice(start_column, end_column));
        }

        if lines.iter().all(|line| line.is_empty()) {
            return None;
        }

        Some(lines.join("\n"))
    }

    pub fn selection_columns(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        anchor: DiffSelectionPoint,
        head: DiffSelectionPoint,
        display_index: usize,
    ) -> Option<(usize, usize)> {
        if anchor.pane != head.pane {
            return None;
        }

        self.ensure_display_cache(mode, width);
        let selection = self.display_cache.entry(mode).selection.as_slice();
        let (start, end) = normalize_selection_points(anchor, head);
        if display_index < start.display_index || display_index > end.display_index {
            return None;
        }

        let segment = selection.get(display_index)?.segment(anchor.pane)?;
        let start_column = if display_index == start.display_index {
            start.column
        } else {
            0
        };
        let end_column = if display_index == end.display_index {
            end.column.saturating_add(1)
        } else {
            segment.content_width
        };
        let clamped_start = start_column.min(segment.content_width);
        let clamped_end = end_column.min(segment.content_width);
        (clamped_start < clamped_end).then_some((
            segment.start_column + clamped_start,
            segment.start_column + clamped_end,
        ))
    }

    pub fn first_selectable_index(&mut self, mode: DiffViewMode, width: usize) -> usize {
        self.nav_targets(mode, width)
            .iter()
            .position(|target| target.is_some())
            .unwrap_or(0)
    }

    pub fn last_selectable_index(&mut self, mode: DiffViewMode, width: usize) -> usize {
        self.nav_targets(mode, width)
            .iter()
            .rposition(|target| target.is_some())
            .unwrap_or(0)
    }

    pub fn move_selection(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        current: usize,
        delta: i32,
    ) -> usize {
        let nav = self.nav_targets(mode, width);
        if nav.is_empty() {
            return 0;
        }

        let mut index = current.min(nav.len().saturating_sub(1));
        if nav[index].is_none() {
            index = nav.iter().position(Option::is_some).unwrap_or(0);
        }

        if delta > 0 {
            for _ in 0..delta {
                let mut probe = index.saturating_add(1);
                while probe < nav.len() && nav[probe].is_none() {
                    probe += 1;
                }
                if probe < nav.len() {
                    index = probe;
                }
            }
        } else if delta < 0 {
            for _ in 0..delta.unsigned_abs() {
                let mut probe = index.saturating_sub(1);
                while probe > 0 && nav[probe].is_none() {
                    probe = probe.saturating_sub(1);
                }
                if nav[probe].is_some() {
                    index = probe;
                }
            }
        }

        index
    }

    pub fn selected_line_number(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        index: usize,
    ) -> Option<usize> {
        match self.nav_targets(mode, width).get(index).copied().flatten() {
            Some(DisplayNavTarget::Line(line_number)) => Some(line_number),
            _ => None,
        }
    }

    pub fn selected_new_line_number(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        index: usize,
    ) -> Option<usize> {
        self.ensure_display_cache(mode, width);
        let row_refs = self
            .display_cache
            .entry(mode)
            .row_refs
            .get(index)
            .copied()?;
        if let Some(row_index) = row_refs.right {
            return self.rows.get(row_index).and_then(|row| row.new_line);
        }

        if row_refs == DisplayRowRefs::default() {
            return match self
                .display_cache
                .entry(mode)
                .nav
                .get(index)
                .copied()
                .flatten()
            {
                Some(DisplayNavTarget::Line(line_number)) => Some(line_number),
                _ => None,
            };
        }

        None
    }

    pub fn selected_gap_index(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        index: usize,
    ) -> Option<usize> {
        self.selected_gap_action(mode, width, index)
            .map(|(gap_index, _)| gap_index)
    }

    pub fn selected_gap_action(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        index: usize,
    ) -> Option<(usize, GapExpandDirection)> {
        match self.nav_targets(mode, width).get(index).copied().flatten() {
            Some(DisplayNavTarget::Gap(gap_index, direction)) => Some((gap_index, direction)),
            _ => None,
        }
    }

    pub fn selected_conflict_index(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        index: usize,
    ) -> Option<usize> {
        self.ensure_display_cache(mode, width);
        if let Some(Some(DisplayNavTarget::Conflict(conflict_index))) =
            self.display_cache.entry(mode).nav.get(index).copied()
        {
            return Some(conflict_index);
        }

        let row_refs = self
            .display_cache
            .entry(mode)
            .row_refs
            .get(index)
            .copied()?;
        row_refs
            .left
            .and_then(|row_index| self.rows.get(row_index))
            .and_then(|row| row.conflict_index)
            .or_else(|| {
                row_refs
                    .right
                    .and_then(|row_index| self.rows.get(row_index))
                    .and_then(|row| row.conflict_index)
            })
    }

    pub fn display_line_count(&mut self, mode: DiffViewMode, width: usize) -> usize {
        self.nav_targets(mode, width).len()
    }

    pub fn expand_selected_gap(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        index: usize,
        amount: usize,
    ) -> usize {
        let Some((gap_index, direction)) = self.selected_gap_action(mode, width, index) else {
            return index;
        };
        let _ = self.expand_gap(gap_index, direction, amount);
        self.nav_targets(mode, width)
            .iter()
            .position(|target| {
                matches!(
                    target,
                    Some(DisplayNavTarget::Gap(candidate, candidate_direction))
                        if *candidate == gap_index && *candidate_direction == direction
                )
            })
            .unwrap_or(index.min(self.nav_targets(mode, width).len().saturating_sub(1)))
    }

    fn expand_gap(
        &mut self,
        gap_index: usize,
        direction: GapExpandDirection,
        amount: usize,
    ) -> bool {
        let Some(gap) = self.gaps.iter().find(|gap| gap.gap_index == gap_index) else {
            return false;
        };

        let expansion = self.gap_expansions.entry(gap_index).or_default();
        let remaining = gap
            .new_count
            .saturating_sub(expansion.from_previous + expansion.from_next);
        if remaining == 0 {
            return false;
        }

        let applied = amount.max(1).min(remaining);
        match direction {
            GapExpandDirection::Up => expansion.from_previous += applied,
            GapExpandDirection::Down => expansion.from_next += applied,
        }
        self.invalidate_display_cache();
        true
    }

    fn ensure_display_cache(&mut self, mode: DiffViewMode, width: usize) {
        let cache_is_stale = {
            let cache = self.display_cache.entry(mode);
            !cache.valid || cache.width != width
        };

        if !cache_is_stale {
            return;
        }

        let (lines, nav, row_refs, selection) = if self.rows.is_empty() {
            (
                vec![Line::from(Span::styled(
                    self.note
                        .clone()
                        .unwrap_or_else(|| "No textual diff available.".to_string()),
                    ui::diff_meta_style(),
                ))],
                vec![None],
                vec![DisplayRowRefs::default()],
                vec![DisplaySelectionLine::default()],
            )
        } else {
            match mode {
                DiffViewMode::Unified => self.build_unified_display(width),
                DiffViewMode::Split => self.build_split_display(width),
            }
        };

        let cache = self.display_cache.entry_mut(mode);
        cache.width = width;
        cache.lines = lines;
        cache.nav = nav;
        cache.row_refs = row_refs;
        cache.selection = selection;
        cache.valid = true;
    }

    fn nav_targets(&mut self, mode: DiffViewMode, width: usize) -> &[Option<DisplayNavTarget>] {
        self.ensure_display_cache(mode, width);
        &self.display_cache.entry(mode).nav
    }

    fn build_unified_display(
        &self,
        width: usize,
    ) -> (
        Vec<Line<'static>>,
        Vec<Option<DisplayNavTarget>>,
        Vec<DisplayRowRefs>,
        Vec<DisplaySelectionLine>,
    ) {
        let mut lines = Vec::new();
        let mut nav = Vec::new();
        let mut row_refs = Vec::new();
        let mut selection = Vec::new();

        for (hunk_offset, hunk) in self.hunks.iter().enumerate() {
            for row_index in hunk.row_start..hunk.row_end {
                let row = &self.rows[row_index];
                let line_number = match row.kind {
                    DiffLineKind::Added | DiffLineKind::Context => row.new_line,
                    DiffLineKind::Removed => row.old_line,
                    DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => None,
                };
                let display_row_refs = match row.kind {
                    DiffLineKind::Removed => DisplayRowRefs {
                        left: Some(row_index),
                        right: None,
                    },
                    DiffLineKind::Added | DiffLineKind::Context => DisplayRowRefs {
                        left: None,
                        right: Some(row_index),
                    },
                    DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => {
                        DisplayRowRefs {
                            left: Some(row_index),
                            right: Some(row_index),
                        }
                    }
                };
                for rendered_line in render_unified_code_lines(row, width) {
                    lines.push(rendered_line.line);
                    nav.push(
                        row.conflict_index
                            .filter(|_| {
                                matches!(
                                    row.kind,
                                    DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_)
                                )
                            })
                            .map(DisplayNavTarget::Conflict)
                            .or_else(|| line_number.map(DisplayNavTarget::Line)),
                    );
                    row_refs.push(display_row_refs);
                    selection.push(rendered_line.selection);
                }
            }

            if let Some(gap) = self.gaps.get(hunk_offset) {
                self.push_gap_display_rows(
                    &mut lines,
                    &mut nav,
                    &mut row_refs,
                    &mut selection,
                    gap,
                    width,
                    false,
                );
            }
        }

        (lines, nav, row_refs, selection)
    }

    fn build_split_display(
        &self,
        width: usize,
    ) -> (
        Vec<Line<'static>>,
        Vec<Option<DisplayNavTarget>>,
        Vec<DisplayRowRefs>,
        Vec<DisplaySelectionLine>,
    ) {
        let total_width = width.saturating_sub(1);
        let gutter_width = 3;
        let side_width = total_width.saturating_sub(gutter_width) / 2;
        let mut lines = Vec::new();
        let mut nav = Vec::new();
        let mut row_refs = Vec::new();
        let mut selection = Vec::new();

        for (hunk_offset, hunk) in self.hunks.iter().enumerate() {
            for (line, target_line, refs, line_selection) in render_split_hunk_rows(
                &self.rows[hunk.row_start..hunk.row_end],
                hunk.row_start,
                side_width,
            ) {
                lines.push(line);
                nav.push(target_line);
                row_refs.push(refs);
                selection.push(line_selection);
            }

            if let Some(gap) = self.gaps.get(hunk_offset) {
                self.push_gap_display_rows(
                    &mut lines,
                    &mut nav,
                    &mut row_refs,
                    &mut selection,
                    gap,
                    total_width,
                    true,
                );
            }
        }

        (lines, nav, row_refs, selection)
    }

    fn push_gap_display_rows(
        &self,
        lines: &mut Vec<Line<'static>>,
        nav: &mut Vec<Option<DisplayNavTarget>>,
        row_refs: &mut Vec<DisplayRowRefs>,
        selection: &mut Vec<DisplaySelectionLine>,
        gap: &DiffHunkGap,
        width: usize,
        split: bool,
    ) {
        let expansion = self
            .gap_expansions
            .get(&gap.gap_index)
            .copied()
            .unwrap_or_default();
        let context_after_count = expansion.from_previous.min(gap.new_count);
        let remaining_after_previous = gap.new_count.saturating_sub(context_after_count);
        let context_before_count = expansion.from_next.min(remaining_after_previous);

        if let Some(file_lines) = self.new_file_lines.as_ref() {
            let start = gap.new_start.saturating_sub(1);
            for offset in 0..context_after_count {
                let line_number = gap.new_start + offset;
                let text = file_lines.get(start + offset).cloned().unwrap_or_default();
                for rendered_line in render_expanded_context_lines(
                    line_number,
                    &text,
                    self.expanded_context_highlighting(line_number),
                    width,
                    split,
                ) {
                    lines.push(rendered_line.line);
                    nav.push(Some(DisplayNavTarget::Line(line_number)));
                    row_refs.push(DisplayRowRefs::default());
                    selection.push(rendered_line.selection);
                }
            }
        }

        let remaining = gap
            .new_count
            .saturating_sub(context_after_count + context_before_count);
        if remaining > 0 {
            lines.push(render_expand_gap_line(
                width,
                remaining,
                expansion.from_previous > 0,
                GapExpandDirection::Up,
            ));
            nav.push(Some(DisplayNavTarget::Gap(
                gap.gap_index,
                GapExpandDirection::Up,
            )));
            row_refs.push(DisplayRowRefs::default());
            selection.push(DisplaySelectionLine::default());
            lines.push(render_expand_gap_line(
                width,
                remaining,
                expansion.from_next > 0,
                GapExpandDirection::Down,
            ));
            nav.push(Some(DisplayNavTarget::Gap(
                gap.gap_index,
                GapExpandDirection::Down,
            )));
            row_refs.push(DisplayRowRefs::default());
            selection.push(DisplaySelectionLine::default());
        }

        if let Some(file_lines) = self.new_file_lines.as_ref() {
            let start = gap
                .new_start
                .saturating_sub(1)
                .saturating_add(gap.new_count.saturating_sub(context_before_count));
            for offset in 0..context_before_count {
                let line_number = gap.new_start + gap.new_count - context_before_count + offset;
                let text = file_lines.get(start + offset).cloned().unwrap_or_default();
                for rendered_line in render_expanded_context_lines(
                    line_number,
                    &text,
                    self.expanded_context_highlighting(line_number),
                    width,
                    split,
                ) {
                    lines.push(rendered_line.line);
                    nav.push(Some(DisplayNavTarget::Line(line_number)));
                    row_refs.push(DisplayRowRefs::default());
                    selection.push(rendered_line.selection);
                }
            }
        }
    }

    fn invalidate_display_cache(&mut self) {
        self.display_cache = DiffDisplayCache::default();
    }

    fn expanded_context_highlighting(&self, line_number: usize) -> Option<Vec<SyntaxToken>> {
        let line_index = line_number.checked_sub(1)?;
        self.new_exact_highlighted_lines
            .as_ref()?
            .get(line_index)
            .cloned()
    }

    pub fn apply_syntax_highlighting(
        &mut self,
        filetype: Option<&'static str>,
        registry: &HighlightRegistry,
    ) {
        let Some(filetype) = filetype else {
            return;
        };

        let left = prepare_side_highlighting(&self.rows, HighlightSide::Left);
        let right = prepare_side_highlighting(&self.rows, HighlightSide::Right);
        let (left_result, right_result) = run_optional_pair(
            left.is_some(),
            right.is_some(),
            || left.and_then(|request| request.highlight(filetype, registry)),
            || right.and_then(|request| request.highlight(filetype, registry)),
        );
        if let Some(result) = left_result {
            apply_completed_highlighting(&mut self.rows, result);
        }
        if let Some(result) = right_result {
            apply_completed_highlighting(&mut self.rows, result);
        }
        self.invalidate_display_cache();
    }

    pub fn apply_exact_syntax_highlighting(
        &mut self,
        filetype: Option<&'static str>,
        registry: &HighlightRegistry,
    ) {
        let Some(filetype) = filetype else {
            return;
        };

        let rows = &self.rows;
        let left_source = self.old_file_source.clone();
        let right_source = self.new_file_source.clone();
        let left_exact_highlighted_lines = left_source
            .as_ref()
            .and_then(|source| highlight_source_lines_cached_exact(registry, filetype, source));
        let right_exact_highlighted_lines = right_source
            .as_ref()
            .and_then(|source| highlight_source_lines_cached_exact(registry, filetype, source));
        let (left, right) = run_optional_pair(
            left_exact_highlighted_lines.is_some(),
            right_exact_highlighted_lines.is_some(),
            || {
                left_exact_highlighted_lines
                    .as_deref()
                    .and_then(|highlighted_lines| {
                        prepare_exact_side_highlighting(
                            rows,
                            HighlightSide::Left,
                            highlighted_lines,
                        )
                    })
            },
            || {
                right_exact_highlighted_lines
                    .as_deref()
                    .and_then(|highlighted_lines| {
                        prepare_exact_side_highlighting(
                            rows,
                            HighlightSide::Right,
                            highlighted_lines,
                        )
                    })
            },
        );

        if left.is_none() && right.is_none() {
            self.apply_syntax_highlighting(Some(filetype), registry);
            return;
        }

        self.old_exact_highlighted_lines = left_exact_highlighted_lines;
        self.new_exact_highlighted_lines = right_exact_highlighted_lines;

        if let Some(result) = left {
            apply_completed_highlighting(&mut self.rows, result);
        }
        if let Some(result) = right {
            apply_completed_highlighting(&mut self.rows, result);
        }
        self.invalidate_display_cache();
    }

    pub fn apply_syntax_highlighting_for_display_range(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        start: usize,
        end: usize,
        filetype: Option<&'static str>,
        registry: &HighlightRegistry,
    ) {
        let Some(filetype) = filetype else {
            return;
        };

        self.ensure_display_cache(mode, width);
        let row_ref_count = self.display_cache.entry(mode).row_refs.len();
        let start = start.min(row_ref_count);
        let end = end.min(row_ref_count);
        if start >= end {
            return;
        }

        let (left_window, right_window) = {
            let row_refs = &self.display_cache.entry(mode).row_refs;
            collect_display_highlight_windows(&row_refs[start..end], self.rows.len())
        };
        let left = left_window.and_then(|(window_start, window_end)| {
            prepare_side_highlighting_in_row_window(
                &self.rows,
                HighlightSide::Left,
                window_start,
                window_end,
            )
        });
        let right = right_window.and_then(|(window_start, window_end)| {
            prepare_side_highlighting_in_row_window(
                &self.rows,
                HighlightSide::Right,
                window_start,
                window_end,
            )
        });
        let (left_result, right_result) = run_optional_pair(
            left.is_some(),
            right.is_some(),
            || left.and_then(|request| request.highlight(filetype, registry)),
            || right.and_then(|request| request.highlight(filetype, registry)),
        );
        if let Some(result) = left_result {
            apply_completed_highlighting(&mut self.rows, result);
        }
        if let Some(result) = right_result {
            apply_completed_highlighting(&mut self.rows, result);
        }
        self.invalidate_display_cache();
    }

    pub fn is_display_range_fully_highlighted(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        start: usize,
        end: usize,
    ) -> bool {
        self.ensure_display_cache(mode, width);
        let row_refs = &self.display_cache.entry(mode).row_refs;
        let start = start.min(row_refs.len());
        let end = end.min(row_refs.len());
        if start >= end {
            return true;
        }

        row_refs[start..end]
            .iter()
            .all(|refs| refs.is_fully_highlighted(&self.rows, mode))
    }

    pub fn merge_highlighting_from(&mut self, other: &Self) {
        if self.rows.len() != other.rows.len() {
            return;
        }

        let mut changed = false;
        for (row, other_row) in self.rows.iter_mut().zip(other.rows.iter()) {
            if row.kind != other_row.kind
                || row.old_line != other_row.old_line
                || row.new_line != other_row.new_line
                || row.conflict_index != other_row.conflict_index
                || row.text != other_row.text
            {
                return;
            }

            if row.syntax.left.is_none() && other_row.syntax.left.is_some() {
                row.syntax.left = other_row.syntax.left.clone();
                changed = true;
            }
            if row.syntax.right.is_none() && other_row.syntax.right.is_some() {
                row.syntax.right = other_row.syntax.right.clone();
                changed = true;
            }
        }

        if changed {
            self.invalidate_display_cache();
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum HighlightSide {
    Left,
    Right,
}

struct PreparedHighlightSide {
    side: HighlightSide,
    row_indices: Vec<usize>,
    source: String,
}

struct CompletedHighlightSide {
    side: HighlightSide,
    row_indices: Vec<usize>,
    highlighted_lines: Vec<Vec<SyntaxToken>>,
}

impl HighlightSide {
    fn includes(self, kind: DiffLineKind) -> bool {
        matches!(
            (self, kind),
            (Self::Left, DiffLineKind::Removed | DiffLineKind::Context)
                | (Self::Right, DiffLineKind::Added | DiffLineKind::Context)
        )
    }

    fn assign(self, row: &mut DiffRow, tokens: Vec<SyntaxToken>) {
        match self {
            Self::Left => row.syntax.left = Some(tokens),
            Self::Right => row.syntax.right = Some(tokens),
        }
    }
}

impl PreparedHighlightSide {
    fn highlight(
        self,
        filetype: &'static str,
        registry: &HighlightRegistry,
    ) -> Option<CompletedHighlightSide> {
        let highlighted_lines = highlight_source_lines(registry, filetype, &self.source)?;
        if highlighted_lines.len() != self.row_indices.len() {
            return None;
        }

        Some(CompletedHighlightSide {
            side: self.side,
            row_indices: self.row_indices,
            highlighted_lines,
        })
    }
}

fn prepare_side_highlighting(
    rows: &[DiffRow],
    side: HighlightSide,
) -> Option<PreparedHighlightSide> {
    prepare_side_highlighting_in_row_window(rows, side, 0, rows.len().saturating_sub(1))
}

fn prepare_exact_side_highlighting(
    rows: &[DiffRow],
    side: HighlightSide,
    highlighted_lines: &[Vec<SyntaxToken>],
) -> Option<CompletedHighlightSide> {
    if highlighted_lines.is_empty() {
        return None;
    }
    let mut row_indices = Vec::new();
    let mut exact_row_lines = Vec::new();

    for (row_index, row) in rows.iter().enumerate() {
        if !side.includes(row.kind) {
            continue;
        }

        let line_number = match side {
            HighlightSide::Left => row.old_line,
            HighlightSide::Right => row.new_line,
        }?;
        let line_index = line_number.saturating_sub(1);
        let tokens = highlighted_lines
            .get(line_index)
            .cloned()
            .unwrap_or_default();
        row_indices.push(row_index);
        exact_row_lines.push(tokens);
    }

    Some(CompletedHighlightSide {
        side,
        row_indices,
        highlighted_lines: exact_row_lines,
    })
}

fn prepare_side_highlighting_in_row_window(
    rows: &[DiffRow],
    side: HighlightSide,
    start: usize,
    end: usize,
) -> Option<PreparedHighlightSide> {
    if rows.is_empty() || start > end {
        return None;
    }

    let start = start.min(rows.len().saturating_sub(1));
    let end = end.min(rows.len().saturating_sub(1));
    let mut source_len = 0usize;
    let mut row_count = 0usize;

    for row in &rows[start..=end] {
        if side.includes(row.kind) {
            source_len += row.text.len();
            row_count += 1;
        }
    }

    if row_count == 0 {
        return None;
    }

    let mut source = String::new();
    source.reserve(source_len + row_count.saturating_sub(1));
    let mut row_indices = Vec::with_capacity(row_count);

    for (row_offset, row) in rows[start..=end].iter().enumerate() {
        if !side.includes(row.kind) {
            continue;
        }

        if !row_indices.is_empty() {
            source.push('\n');
        }
        source.push_str(&row.text);
        row_indices.push(start + row_offset);
    }

    Some(PreparedHighlightSide {
        side,
        row_indices,
        source,
    })
}

fn apply_completed_highlighting(rows: &mut [DiffRow], completed: CompletedHighlightSide) {
    let CompletedHighlightSide {
        side,
        row_indices,
        highlighted_lines,
    } = completed;

    if highlighted_lines.len() != row_indices.len() {
        return;
    }

    for (row_index, tokens) in row_indices.into_iter().zip(highlighted_lines) {
        side.assign(&mut rows[row_index], tokens);
    }
}

#[inline]
fn run_optional_pair<T, LF, RF>(
    left_ready: bool,
    right_ready: bool,
    left_fn: LF,
    right_fn: RF,
) -> (Option<T>, Option<T>)
where
    T: Send,
    LF: FnOnce() -> Option<T> + Send,
    RF: FnOnce() -> Option<T> + Send,
{
    let should_parallelize = left_ready
        && right_ready
        && std::thread::available_parallelism()
            .map(|parallelism| parallelism.get() > 1)
            .unwrap_or(false);

    if should_parallelize {
        std::thread::scope(|scope| {
            let right_task = scope.spawn(right_fn);
            let left = left_fn();
            let right = right_task.join().ok().flatten();
            (left, right)
        })
    } else {
        (left_fn(), right_fn())
    }
}

#[derive(Debug, Default, Clone)]
struct DiffDisplayCache {
    unified: CachedDisplay,
    split: CachedDisplay,
}

impl DiffDisplayCache {
    fn entry(&self, mode: DiffViewMode) -> &CachedDisplay {
        match mode {
            DiffViewMode::Unified => &self.unified,
            DiffViewMode::Split => &self.split,
        }
    }

    fn entry_mut(&mut self, mode: DiffViewMode) -> &mut CachedDisplay {
        match mode {
            DiffViewMode::Unified => &mut self.unified,
            DiffViewMode::Split => &mut self.split,
        }
    }
}

#[derive(Debug, Default, Clone)]
struct CachedDisplay {
    width: usize,
    lines: Vec<Line<'static>>,
    nav: Vec<Option<DisplayNavTarget>>,
    row_refs: Vec<DisplayRowRefs>,
    selection: Vec<DisplaySelectionLine>,
    valid: bool,
}

#[derive(Debug, Default, Clone)]
struct DisplaySelectionLine {
    unified: Option<DisplaySelectionSegment>,
    left: Option<DisplaySelectionSegment>,
    right: Option<DisplaySelectionSegment>,
}

impl DisplaySelectionLine {
    fn segment(&self, pane: DiffSelectionPane) -> Option<&DisplaySelectionSegment> {
        match pane {
            DiffSelectionPane::Unified => self.unified.as_ref(),
            DiffSelectionPane::Left => self.left.as_ref(),
            DiffSelectionPane::Right => self.right.as_ref(),
        }
    }
}

#[derive(Debug, Clone)]
struct DisplaySelectionSegment {
    start_column: usize,
    content_width: usize,
    text: String,
}

impl DisplaySelectionSegment {
    fn contains(&self, column: usize) -> bool {
        column >= self.start_column
            && column < self.start_column.saturating_add(self.content_width)
            && self.content_width > 0
    }

    fn clamp_column(&self, column: usize) -> usize {
        if self.content_width == 0 {
            return 0;
        }

        if column <= self.start_column {
            0
        } else {
            (column - self.start_column).min(self.content_width.saturating_sub(1))
        }
    }

    fn slice(&self, start: usize, end: usize) -> String {
        let text_width = UnicodeWidthStr::width(self.text.as_str());
        if text_width == 0 {
            return String::new();
        }

        let start = start.min(text_width);
        let end = end.min(text_width);
        if start >= end {
            return String::new();
        }

        slice_string_by_width(&self.text, start, end)
    }
}

#[derive(Debug, Clone, Copy)]
enum DisplayNavTarget {
    Line(usize),
    Conflict(usize),
    Gap(usize, GapExpandDirection),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct DisplayRowRefs {
    left: Option<usize>,
    right: Option<usize>,
}

impl DisplayRowRefs {
    fn is_fully_highlighted(self, rows: &[DiffRow], mode: DiffViewMode) -> bool {
        match mode {
            DiffViewMode::Unified => self
                .left
                .map(|row_index| is_row_side_highlighted(rows, row_index, HighlightSide::Left))
                .or_else(|| {
                    self.right.map(|row_index| {
                        is_row_side_highlighted(rows, row_index, HighlightSide::Right)
                    })
                })
                .unwrap_or(true),
            DiffViewMode::Split => {
                let left_ready = self
                    .left
                    .map(|row_index| is_row_side_highlighted(rows, row_index, HighlightSide::Left))
                    .unwrap_or(true);
                let right_ready = self
                    .right
                    .map(|row_index| is_row_side_highlighted(rows, row_index, HighlightSide::Right))
                    .unwrap_or(true);
                left_ready && right_ready
            }
        }
    }
}

fn is_row_side_highlighted(rows: &[DiffRow], row_index: usize, side: HighlightSide) -> bool {
    let Some(row) = rows.get(row_index) else {
        return true;
    };
    if matches!(
        row.kind,
        DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_)
    ) {
        return true;
    }
    match side {
        HighlightSide::Left => row.syntax.left.is_some(),
        HighlightSide::Right => row.syntax.right.is_some(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffLineKind {
    Context,
    Added,
    Removed,
    ConflictAction,
    ConflictMarker(MergeConflictMarkerRowType),
}

#[derive(Debug, Clone)]
struct DiffRow {
    kind: DiffLineKind,
    old_line: Option<usize>,
    new_line: Option<usize>,
    conflict_index: Option<usize>,
    text: String,
    syntax: DiffRowSyntax,
}

#[derive(Debug, Clone, Default)]
struct DiffRowSyntax {
    left: Option<Vec<SyntaxToken>>,
    right: Option<Vec<SyntaxToken>>,
}

impl DiffRow {
    fn unified_content(&self) -> Option<&[SyntaxToken]> {
        match self.kind {
            DiffLineKind::Removed => self.syntax.left.as_deref(),
            DiffLineKind::Added | DiffLineKind::Context => self.syntax.right.as_deref(),
            DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => None,
        }
    }

    fn side_content(&self, left_side: bool) -> Option<&[SyntaxToken]> {
        if left_side {
            self.syntax.left.as_deref()
        } else {
            self.syntax.right.as_deref()
        }
    }
}

#[derive(Debug, Clone)]
struct DiffHunkBlock {
    new_start: usize,
    new_count: usize,
    row_start: usize,
    row_end: usize,
}

#[derive(Debug, Clone)]
struct DiffHunkGap {
    gap_index: usize,
    new_start: usize,
    new_count: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct DiffGapExpansion {
    from_previous: usize,
    from_next: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapExpandDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedPatch {
    #[serde(rename = "patchMetadata", skip_serializing_if = "Option::is_none")]
    pub patch_metadata: Option<String>,
    pub files: Vec<FileDiffMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeType {
    Change,
    RenamePure,
    RenameChanged,
    New,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiffIconType {
    File,
    Change,
    RenamePure,
    RenameChanged,
    New,
    Deleted,
}

impl From<ChangeType> for DiffIconType {
    fn from(change_type: ChangeType) -> Self {
        match change_type {
            ChangeType::Change => Self::Change,
            ChangeType::RenamePure => Self::RenamePure,
            ChangeType::RenameChanged => Self::RenameChanged,
            ChangeType::New => Self::New,
            ChangeType::Deleted => Self::Deleted,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HunkContent {
    #[serde(rename = "context")]
    Context {
        lines: usize,
        #[serde(rename = "additionLineIndex")]
        addition_line_index: usize,
        #[serde(rename = "deletionLineIndex")]
        deletion_line_index: usize,
    },
    #[serde(rename = "change")]
    Change {
        deletions: usize,
        #[serde(rename = "deletionLineIndex")]
        deletion_line_index: usize,
        additions: usize,
        #[serde(rename = "additionLineIndex")]
        addition_line_index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    #[serde(rename = "collapsedBefore")]
    pub collapsed_before: usize,
    #[serde(rename = "splitLineCount")]
    pub split_line_count: usize,
    #[serde(rename = "splitLineStart")]
    pub split_line_start: usize,
    #[serde(rename = "unifiedLineCount")]
    pub unified_line_count: usize,
    #[serde(rename = "unifiedLineStart")]
    pub unified_line_start: usize,
    #[serde(rename = "additionCount")]
    pub addition_count: usize,
    #[serde(rename = "additionStart")]
    pub addition_start: usize,
    #[serde(rename = "additionLines")]
    pub addition_lines: usize,
    #[serde(rename = "additionLineIndex")]
    pub addition_line_index: usize,
    #[serde(rename = "deletionCount")]
    pub deletion_count: usize,
    #[serde(rename = "deletionStart")]
    pub deletion_start: usize,
    #[serde(rename = "deletionLines")]
    pub deletion_lines: usize,
    #[serde(rename = "deletionLineIndex")]
    pub deletion_line_index: usize,
    #[serde(rename = "hunkContent")]
    pub hunk_content: Vec<HunkContent>,
    #[serde(rename = "hunkContext", skip_serializing_if = "Option::is_none")]
    pub hunk_context: Option<String>,
    #[serde(rename = "hunkSpecs")]
    pub hunk_specs: String,
    #[serde(rename = "noEOFCRAdditions")]
    pub no_eof_cr_additions: bool,
    #[serde(rename = "noEOFCRDeletions")]
    pub no_eof_cr_deletions: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiffMetadata {
    pub name: String,
    #[serde(rename = "prevName", skip_serializing_if = "Option::is_none")]
    pub prev_name: Option<String>,
    #[serde(rename = "newObjectId", skip_serializing_if = "Option::is_none")]
    pub new_object_id: Option<String>,
    #[serde(rename = "prevObjectId", skip_serializing_if = "Option::is_none")]
    pub prev_object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(rename = "prevMode", skip_serializing_if = "Option::is_none")]
    pub prev_mode: Option<String>,
    #[serde(rename = "type")]
    pub change_type: ChangeType,
    pub hunks: Vec<Hunk>,
    #[serde(rename = "splitLineCount")]
    pub split_line_count: usize,
    #[serde(rename = "unifiedLineCount")]
    pub unified_line_count: usize,
    #[serde(rename = "isPartial")]
    pub is_partial: bool,
    #[serde(rename = "deletionLines")]
    pub deletion_lines: Vec<String>,
    #[serde(rename = "additionLines")]
    pub addition_lines: Vec<String>,
    #[serde(rename = "cacheKey", skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileContents {
    pub name: String,
    pub contents: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(rename = "cacheKey", skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HunkLineType {
    Context,
    Metadata,
    Addition,
    Deletion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedLine {
    pub line: String,
    #[serde(rename = "type")]
    pub line_type: HunkLineType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionSide {
    Deletions,
    Additions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedLineRange {
    pub start: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<SelectionSide>,
    pub end: usize,
    #[serde(rename = "endSide", skip_serializing_if = "Option::is_none")]
    pub end_side: Option<SelectionSide>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineAnnotation<T = JsonValue> {
    #[serde(rename = "lineNumber")]
    pub line_number: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLineAnnotation<T = JsonValue> {
    pub side: SelectionSide,
    #[serde(rename = "lineNumber")]
    pub line_number: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<T>,
}

pub trait LineAnnotationName {
    fn annotation_side(&self) -> Option<SelectionSide>;
    fn annotation_line_number(&self) -> usize;
}

impl<T> LineAnnotationName for LineAnnotation<T> {
    fn annotation_side(&self) -> Option<SelectionSide> {
        None
    }

    fn annotation_line_number(&self) -> usize {
        self.line_number
    }
}

impl<T> LineAnnotationName for DiffLineAnnotation<T> {
    fn annotation_side(&self) -> Option<SelectionSide> {
        Some(self.side)
    }

    fn annotation_line_number(&self) -> usize {
        self.line_number
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodeColumnType {
    Unified,
    Additions,
    Deletions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HunkDataExpandable {
    pub chunked: bool,
    pub up: bool,
    pub down: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HunkData {
    #[serde(rename = "slotName")]
    pub slot_name: String,
    #[serde(rename = "hunkIndex")]
    pub hunk_index: usize,
    pub lines: usize,
    #[serde(rename = "type")]
    pub column_type: CodeColumnType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expandable: Option<HunkDataExpandable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MergeConflictLineType {
    None,
    MarkerStart,
    MarkerBase,
    MarkerSeparator,
    MarkerEnd,
    Current,
    Base,
    Incoming,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeConflictRegion {
    #[serde(rename = "conflictIndex")]
    pub conflict_index: usize,
    #[serde(rename = "startLineIndex")]
    pub start_line_index: usize,
    #[serde(rename = "startLineNumber")]
    pub start_line_number: usize,
    #[serde(rename = "separatorLineIndex")]
    pub separator_line_index: usize,
    #[serde(rename = "separatorLineNumber")]
    pub separator_line_number: usize,
    #[serde(rename = "endLineIndex")]
    pub end_line_index: usize,
    #[serde(rename = "endLineNumber")]
    pub end_line_number: usize,
    #[serde(
        rename = "baseMarkerLineIndex",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_marker_line_index: Option<usize>,
    #[serde(
        rename = "baseMarkerLineNumber",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_marker_line_number: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeConflictParseResult {
    #[serde(rename = "lineTypes")]
    pub line_types: Vec<MergeConflictLineType>,
    pub regions: Vec<MergeConflictRegion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeConflictActionSlotInput {
    #[serde(rename = "hunkIndex")]
    pub hunk_index: usize,
    #[serde(rename = "lineIndex")]
    pub line_index: usize,
    #[serde(rename = "conflictIndex")]
    pub conflict_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MergeConflictResolution {
    Current,
    Incoming,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessFileConflictData {
    #[serde(rename = "hunkIndex")]
    pub hunk_index: usize,
    #[serde(rename = "startContentIndex")]
    pub start_content_index: usize,
    #[serde(rename = "endContentIndex")]
    pub end_content_index: usize,
    #[serde(
        rename = "currentContentIndex",
        skip_serializing_if = "Option::is_none"
    )]
    pub current_content_index: Option<usize>,
    #[serde(rename = "baseContentIndex", skip_serializing_if = "Option::is_none")]
    pub base_content_index: Option<usize>,
    #[serde(
        rename = "incomingContentIndex",
        skip_serializing_if = "Option::is_none"
    )]
    pub incoming_content_index: Option<usize>,
    #[serde(rename = "endMarkerContentIndex")]
    pub end_marker_content_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeConflictMarkerLines {
    pub start: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    pub separator: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeConflictDiffAction {
    #[serde(flatten)]
    pub conflict_data: ProcessFileConflictData,
    pub conflict: MergeConflictRegion,
    #[serde(rename = "conflictIndex")]
    pub conflict_index: usize,
    #[serde(rename = "markerLines")]
    pub marker_lines: MergeConflictMarkerLines,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeConflictActionAnchor {
    #[serde(rename = "hunkIndex")]
    pub hunk_index: usize,
    #[serde(rename = "lineIndex")]
    pub line_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MergeConflictMarkerRowType {
    MarkerStart,
    MarkerBase,
    MarkerSeparator,
    MarkerEnd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeConflictMarkerRow {
    #[serde(rename = "type")]
    pub row_type: MergeConflictMarkerRowType,
    #[serde(rename = "hunkIndex")]
    pub hunk_index: usize,
    #[serde(rename = "contentIndex")]
    pub content_index: usize,
    #[serde(rename = "conflictIndex")]
    pub conflict_index: usize,
    #[serde(rename = "lineText")]
    pub line_text: String,
    #[serde(rename = "lineIndex")]
    pub line_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseMergeConflictDiffFromFileResult {
    #[serde(rename = "fileDiff")]
    pub file_diff: FileDiffMetadata,
    #[serde(rename = "currentFile")]
    pub current_file: FileContents,
    #[serde(rename = "incomingFile")]
    pub incoming_file: FileContents,
    pub actions: Vec<Option<MergeConflictDiffAction>>,
    #[serde(rename = "markerRows")]
    pub marker_rows: Vec<MergeConflictMarkerRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineEndingType {
    CRLF,
    CR,
    LF,
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseDiffOptions {
    pub ignore_whitespace: bool,
    pub context_lines: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStyle {
    Unified,
    Split,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffHunkResolution {
    Accept,
    Reject,
    Current,
    Incoming,
    Both,
    Additions,
    Deletions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkSeparatorKind {
    Simple,
    Metadata,
    LineInfo,
    LineInfoBasic,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ThemeSpec {
    Name(String),
    Pair { dark: String, light: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LineDiffType {
    WordAlt,
    Word,
    Char,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiffIndicators {
    Classic,
    Bars,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodeOverflow {
    Scroll,
    Wrap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PreNodeType {
    Diff,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrePropertiesConfig {
    #[serde(rename = "type")]
    pub node_type: PreNodeType,
    #[serde(rename = "diffIndicators")]
    pub diff_indicators: DiffIndicators,
    #[serde(rename = "disableBackground")]
    pub disable_background: bool,
    #[serde(rename = "disableLineNumbers")]
    pub disable_line_numbers: bool,
    pub overflow: CodeOverflow,
    pub split: bool,
    #[serde(rename = "totalLines")]
    pub total_lines: usize,
    #[serde(rename = "customProperties", skip_serializing_if = "Option::is_none")]
    pub custom_properties: Option<JsonMap<String, JsonValue>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderFileOptions {
    pub theme: ThemeSpec,
    #[serde(rename = "useTokenTransformer")]
    pub use_token_transformer: bool,
    #[serde(rename = "tokenizeMaxLineLength")]
    pub tokenize_max_line_length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderDiffOptions {
    pub theme: ThemeSpec,
    #[serde(rename = "useTokenTransformer")]
    pub use_token_transformer: bool,
    #[serde(rename = "tokenizeMaxLineLength")]
    pub tokenize_max_line_length: usize,
    #[serde(rename = "lineDiffType")]
    pub line_diff_type: LineDiffType,
    #[serde(rename = "maxLineDiffLength")]
    pub max_line_diff_length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerStats {
    #[serde(rename = "busyWorkers")]
    pub busy_workers: usize,
    #[serde(rename = "diffCacheSize")]
    pub diff_cache_size: usize,
    #[serde(rename = "fileCacheSize")]
    pub file_cache_size: usize,
    #[serde(rename = "managerState")]
    pub manager_state: String,
    #[serde(rename = "activeTasks")]
    pub active_tasks: usize,
    #[serde(rename = "queuedTasks")]
    pub queued_tasks: usize,
    #[serde(rename = "themeSubscribers")]
    pub theme_subscribers: usize,
    #[serde(rename = "totalWorkers")]
    pub total_workers: usize,
    #[serde(rename = "workersFailed")]
    pub workers_failed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NormalizedDiffResolution {
    Deletions,
    Additions,
    Both,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HunkExpansionRegion {
    #[serde(rename = "fromStart")]
    pub from_start: usize,
    #[serde(rename = "fromEnd")]
    pub from_end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandedHunks<'a> {
    All,
    Regions(&'a HashMap<usize, HunkExpansionRegion>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffIterationOptions<'a> {
    pub diff_style: DiffStyle,
    pub starting_line: usize,
    pub total_lines: Option<usize>,
    pub expanded_hunks: Option<ExpandedHunks<'a>>,
    pub collapsed_context_threshold: usize,
}

impl Default for DiffIterationOptions<'_> {
    fn default() -> Self {
        Self {
            diff_style: DiffStyle::Unified,
            starting_line: 0,
            total_lines: None,
            expanded_hunks: None,
            collapsed_context_threshold: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileIterationOptions {
    pub starting_line: usize,
    pub total_lines: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileLine<'a> {
    pub line_index: usize,
    pub line_number: usize,
    pub content: &'a str,
    pub is_last_line: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiffLineType {
    Change,
    Context,
    ContextExpanded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLineMetadata {
    #[serde(rename = "unifiedLineIndex")]
    pub unified_line_index: usize,
    #[serde(rename = "splitLineIndex")]
    pub split_line_index: usize,
    #[serde(rename = "lineIndex")]
    pub line_index: usize,
    #[serde(rename = "lineNumber")]
    pub line_number: usize,
    #[serde(rename = "noEOFCR")]
    pub no_eof_cr: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    #[serde(rename = "hunkIndex")]
    pub hunk_index: usize,
    #[serde(rename = "hasHunk")]
    pub has_hunk: bool,
    #[serde(rename = "collapsedBefore")]
    pub collapsed_before: usize,
    #[serde(rename = "collapsedAfter")]
    pub collapsed_after: usize,
    #[serde(rename = "type")]
    pub line_type: DiffLineType,
    #[serde(rename = "deletionLine", skip_serializing_if = "Option::is_none")]
    pub deletion_line: Option<DiffLineMetadata>,
    #[serde(rename = "additionLine", skip_serializing_if = "Option::is_none")]
    pub addition_line: Option<DiffLineMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualFileMetrics {
    #[serde(rename = "hunkLineCount")]
    pub hunk_line_count: usize,
    #[serde(rename = "lineHeight")]
    pub line_height: usize,
    #[serde(rename = "diffHeaderHeight")]
    pub diff_header_height: usize,
    pub spacing: usize,
    #[serde(rename = "paddingTop", skip_serializing_if = "Option::is_none")]
    pub padding_top: Option<usize>,
    #[serde(rename = "paddingBottom", skip_serializing_if = "Option::is_none")]
    pub padding_bottom: Option<usize>,
    #[serde(
        rename = "hunkSeparatorHeight",
        skip_serializing_if = "Option::is_none"
    )]
    pub hunk_separator_height: Option<usize>,
}

impl Default for VirtualFileMetrics {
    fn default() -> Self {
        Self {
            hunk_line_count: 50,
            line_height: 20,
            diff_header_height: 44,
            spacing: 8,
            padding_top: None,
            padding_bottom: None,
            hunk_separator_height: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpandedRegion {
    #[serde(rename = "fromStart")]
    pub from_start: usize,
    #[serde(rename = "fromEnd")]
    pub from_end: usize,
    #[serde(rename = "rangeSize")]
    pub range_size: usize,
    #[serde(rename = "collapsedLines")]
    pub collapsed_lines: usize,
    #[serde(rename = "renderAll")]
    pub render_all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HunkSeparatorLayout {
    pub height: usize,
    #[serde(rename = "gapBefore")]
    pub gap_before: usize,
    #[serde(rename = "gapAfter")]
    pub gap_after: usize,
    #[serde(rename = "totalHeight")]
    pub total_height: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderRange {
    #[serde(rename = "startingLine")]
    pub starting_line: usize,
    #[serde(rename = "totalLines")]
    pub total_lines: Option<usize>,
    #[serde(rename = "bufferBefore")]
    pub buffer_before: usize,
    #[serde(rename = "bufferAfter")]
    pub buffer_after: usize,
}

impl Default for RenderRange {
    fn default() -> Self {
        Self {
            starting_line: 0,
            total_lines: None,
            buffer_before: 0,
            buffer_after: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VirtualWindowSpecs {
    pub top: f64,
    pub bottom: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowFromScrollPositionOptions {
    #[serde(rename = "scrollTop")]
    pub scroll_top: f64,
    pub height: f64,
    #[serde(rename = "scrollHeight")]
    pub scroll_height: f64,
    #[serde(rename = "fitPerfectly")]
    pub fit_perfectly: bool,
    #[serde(rename = "fitPerfectlyOverscroll")]
    pub fit_perfectly_overscroll: f64,
    #[serde(rename = "overscrollSize")]
    pub overscroll_size: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EstimatedDiffHeights {
    #[serde(rename = "splitHeight")]
    pub split_height: usize,
    #[serde(rename = "unifiedHeight")]
    pub unified_height: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EstimatedDiffHeightOptions<'a> {
    pub metrics: VirtualFileMetrics,
    pub disable_file_header: bool,
    pub hunk_separator_kind: HunkSeparatorKind,
    pub expand_unchanged: bool,
    pub expanded_hunks: Option<ExpandedHunks<'a>>,
    pub collapsed_context_threshold: usize,
}

impl Default for EstimatedDiffHeightOptions<'_> {
    fn default() -> Self {
        Self {
            metrics: VirtualFileMetrics::default(),
            disable_file_header: false,
            hunk_separator_kind: HunkSeparatorKind::LineInfo,
            expand_unchanged: false,
            expanded_hunks: None,
            collapsed_context_threshold: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExpandedRegionResult {
    from_start: usize,
    from_end: usize,
    range_size: usize,
    collapsed_lines: usize,
}

impl From<ExpandedRegionResult> for ExpandedRegion {
    fn from(region: ExpandedRegionResult) -> Self {
        Self {
            from_start: region.from_start,
            from_end: region.from_end,
            range_size: region.range_size,
            collapsed_lines: region.collapsed_lines,
            render_all: region.collapsed_lines == 0 && region.range_size > 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrimmedPatchHunk {
    addition_start: usize,
    deletion_start: usize,
    addition_count: usize,
    deletion_count: usize,
    hunk_lines: Vec<String>,
    context_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrimContextFlushMode {
    BeforeChange,
    Leading,
    Trailing,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ResolveCursor {
    next_addition_line_index: usize,
    next_deletion_line_index: usize,
    next_addition_start: usize,
    next_deletion_start: usize,
    split_line_count: usize,
    unified_line_count: usize,
}

#[derive(Debug, Clone, Copy)]
struct PatchHunkHeader<'a> {
    addition_count: usize,
    addition_start: usize,
    deletion_count: usize,
    deletion_start: usize,
    hunk_context: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedRawLineType {
    Context,
    Addition,
    Deletion,
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FullDiffOp {
    Equal { old_index: usize, new_index: usize },
    Delete { old_index: usize, new_index: usize },
    Insert { old_index: usize, new_index: usize },
}

pub fn parse_patch_files(
    data: &str,
    cache_key_prefix: Option<&str>,
    throw_on_error: bool,
) -> color_eyre::Result<Vec<ParsedPatch>> {
    let raw_patches = if has_commit_metadata_boundary(data) {
        split_at_line_prefix(data, "From ")
    } else {
        vec![data]
    };
    let mut patches = Vec::with_capacity(raw_patches.len());

    for patch in raw_patches {
        match process_patch(
            patch,
            cache_key_prefix.map(|prefix| format!("{prefix}-{}", patches.len())),
            throw_on_error,
        ) {
            Ok(parsed) => patches.push(parsed),
            Err(error) if throw_on_error => return Err(error),
            Err(_) => {}
        }
    }

    Ok(patches)
}

pub fn process_patch(
    data: &str,
    cache_key_prefix: Option<String>,
    throw_on_error: bool,
) -> color_eyre::Result<ParsedPatch> {
    let is_git_diff = is_git_diff_patch(data);
    let raw_files = if is_git_diff {
        split_at_line_prefix(data, "diff --git")
    } else {
        split_at_unified_file_break(data)
    };
    let mut patch_metadata = None;
    let mut files = Vec::new();

    for file_or_patch_metadata in raw_files {
        let is_file_blob = if is_git_diff {
            file_or_patch_metadata.starts_with("diff --git")
        } else {
            is_unified_file_break(file_or_patch_metadata)
        };

        if !is_file_blob {
            if patch_metadata.is_none() {
                patch_metadata = Some(file_or_patch_metadata.to_string());
            } else if throw_on_error {
                return Err(eyre!("parsePatchContent: unknown file blob"));
            }
            continue;
        }

        let cache_key = cache_key_prefix
            .as_ref()
            .map(|prefix| format!("{prefix}-{}", files.len()));
        if let Some(file) = process_file(
            file_or_patch_metadata,
            cache_key,
            Some(is_git_diff),
            throw_on_error,
        )? {
            files.push(file);
        }
    }

    Ok(ParsedPatch {
        patch_metadata,
        files,
    })
}

pub fn process_file(
    file_diff_string: &str,
    cache_key: Option<String>,
    is_git_diff: Option<bool>,
    throw_on_error: bool,
) -> color_eyre::Result<Option<FileDiffMetadata>> {
    let is_git_diff = is_git_diff.unwrap_or_else(|| file_diff_string.contains("diff --git"));
    let is_partial = true;
    let mut last_hunk_end = 0usize;
    let hunks = split_at_line_prefix(file_diff_string, "@@ ");
    let mut current_file: Option<FileDiffMetadata> = None;
    let mut deletion_line_index = 0usize;
    let mut addition_line_index = 0usize;

    for hunk in hunks {
        let mut lines = split_with_newlines(hunk);
        let Some(first_line) = lines.first().copied() else {
            if throw_on_error {
                return Err(eyre!("parsePatchContent: invalid hunk"));
            }
            continue;
        };
        let file_header = parse_patch_hunk_header(first_line);

        if file_header.is_none() || current_file.is_none() {
            if current_file.is_some() {
                if throw_on_error {
                    return Err(eyre!("parsePatchContent: Invalid hunk"));
                }
                continue;
            }

            let mut file = FileDiffMetadata {
                name: String::new(),
                prev_name: None,
                new_object_id: None,
                prev_object_id: None,
                mode: None,
                prev_mode: None,
                change_type: ChangeType::Change,
                hunks: Vec::new(),
                split_line_count: 0,
                unified_line_count: 0,
                is_partial,
                deletion_lines: Vec::new(),
                addition_lines: Vec::new(),
                cache_key: cache_key.clone(),
            };

            for line in &lines {
                if line.starts_with("diff --git") {
                    match parse_git_diff_names(line.trim_end_matches(['\r', '\n'])) {
                        Some((prev_name, name)) => {
                            file.name = name;
                            if file.name != prev_name {
                                file.prev_name = Some(prev_name);
                            }
                        }
                        None if throw_on_error => {
                            return Err(eyre!("parsePatchContent: invalid git diff header"));
                        }
                        None => {}
                    }
                    continue;
                }

                if line.starts_with("---") || line.starts_with("+++") {
                    if let Some((header_type, file_name)) = parse_filename_header(line, is_git_diff)
                    {
                        if header_type == "---" && file_name != "/dev/null" {
                            file.prev_name = Some(file_name.clone());
                            file.name = file_name;
                        } else if header_type == "+++" && file_name != "/dev/null" {
                            file.name = file_name;
                        }
                    }
                } else if is_git_diff {
                    parse_git_file_metadata(line, &mut file);
                }
            }

            current_file = Some(file);
            continue;
        }

        while matches!(lines.last(), Some(&"\n" | &"\r" | &"\r\n" | &"")) {
            lines.pop();
        }

        let file = current_file
            .as_mut()
            .expect("current file should exist after header parsing");
        let file_header = file_header.expect("hunk header should exist");
        let mut addition_lines = 0usize;
        let mut deletion_lines = 0usize;

        deletion_line_index = if is_partial {
            deletion_line_index
        } else {
            file_header.deletion_start.saturating_sub(1)
        };
        addition_line_index = if is_partial {
            addition_line_index
        } else {
            file_header.addition_start.saturating_sub(1)
        };

        let mut hunk_data = Hunk {
            collapsed_before: 0,
            split_line_count: 0,
            split_line_start: 0,
            unified_line_count: 0,
            unified_line_start: 0,
            addition_count: file_header.addition_count,
            addition_start: file_header.addition_start,
            addition_lines,
            addition_line_index,
            deletion_count: file_header.deletion_count,
            deletion_start: file_header.deletion_start,
            deletion_lines,
            deletion_line_index,
            hunk_content: Vec::new(),
            hunk_context: file_header.hunk_context.map(ToOwned::to_owned),
            hunk_specs: trim_line_end(first_line).to_string(),
            no_eof_cr_additions: false,
            no_eof_cr_deletions: false,
        };

        let mut parsed_addition_lines = 0usize;
        let mut parsed_deletion_lines = 0usize;
        let mut current_content_index: Option<usize> = None;
        let mut last_line_type: Option<ParsedRawLineType> = None;

        for raw_line in lines.iter().skip(1).copied() {
            if parsed_addition_lines >= hunk_data.addition_count
                && parsed_deletion_lines >= hunk_data.deletion_count
                && !raw_line.starts_with('\\')
            {
                break;
            }

            let Some(first_char) = raw_line.chars().next() else {
                continue;
            };
            let Some(line_type) = parse_raw_line_type(first_char) else {
                if throw_on_error {
                    return Err(eyre!(
                        "parseLineType: Invalid firstChar: {:?}, full line: {:?}",
                        first_char,
                        raw_line
                    ));
                }
                continue;
            };

            match line_type {
                ParsedRawLineType::Addition => {
                    let line = get_parsed_line_content(raw_line);
                    let index = ensure_change_group(
                        &mut hunk_data.hunk_content,
                        &mut current_content_index,
                        deletion_line_index,
                        addition_line_index,
                    );
                    addition_line_index += 1;
                    parsed_addition_lines += 1;
                    file.addition_lines.push(line);
                    if let HunkContent::Change {
                        additions: group_additions,
                        ..
                    } = &mut hunk_data.hunk_content[index]
                    {
                        *group_additions += 1;
                    }
                    addition_lines += 1;
                    last_line_type = Some(ParsedRawLineType::Addition);
                }
                ParsedRawLineType::Deletion => {
                    let line = get_parsed_line_content(raw_line);
                    let index = ensure_change_group(
                        &mut hunk_data.hunk_content,
                        &mut current_content_index,
                        deletion_line_index,
                        addition_line_index,
                    );
                    deletion_line_index += 1;
                    parsed_deletion_lines += 1;
                    file.deletion_lines.push(line);
                    if let HunkContent::Change {
                        deletions: group_deletions,
                        ..
                    } = &mut hunk_data.hunk_content[index]
                    {
                        *group_deletions += 1;
                    }
                    deletion_lines += 1;
                    last_line_type = Some(ParsedRawLineType::Deletion);
                }
                ParsedRawLineType::Context => {
                    let line = get_parsed_line_content(raw_line);
                    let index = ensure_context_group(
                        &mut hunk_data.hunk_content,
                        &mut current_content_index,
                        deletion_line_index,
                        addition_line_index,
                    );
                    addition_line_index += 1;
                    deletion_line_index += 1;
                    parsed_addition_lines += 1;
                    parsed_deletion_lines += 1;
                    file.deletion_lines.push(line.clone());
                    file.addition_lines.push(line);
                    if let HunkContent::Context { lines, .. } = &mut hunk_data.hunk_content[index] {
                        *lines += 1;
                    }
                    last_line_type = Some(ParsedRawLineType::Context);
                }
                ParsedRawLineType::Metadata => match (current_content_index, last_line_type) {
                    (Some(index), Some(ParsedRawLineType::Context)) => {
                        hunk_data.no_eof_cr_additions = true;
                        hunk_data.no_eof_cr_deletions = true;
                        clean_last_line(&mut file.addition_lines);
                        clean_last_line(&mut file.deletion_lines);
                        current_content_index = Some(index);
                    }
                    (Some(index), Some(ParsedRawLineType::Deletion)) => {
                        hunk_data.no_eof_cr_deletions = true;
                        clean_last_line(&mut file.deletion_lines);
                        current_content_index = Some(index);
                    }
                    (Some(index), Some(ParsedRawLineType::Addition)) => {
                        hunk_data.no_eof_cr_additions = true;
                        clean_last_line(&mut file.addition_lines);
                        current_content_index = Some(index);
                    }
                    _ => {}
                },
            }
        }

        hunk_data.addition_lines = addition_lines;
        hunk_data.deletion_lines = deletion_lines;
        hunk_data.collapsed_before = hunk_data
            .addition_start
            .saturating_sub(1)
            .saturating_sub(last_hunk_end);
        last_hunk_end = hunk_data
            .addition_start
            .saturating_add(hunk_data.addition_count)
            .saturating_sub(1);

        for content in &hunk_data.hunk_content {
            match content {
                HunkContent::Context { lines, .. } => {
                    hunk_data.split_line_count += *lines;
                    hunk_data.unified_line_count += *lines;
                }
                HunkContent::Change {
                    additions,
                    deletions,
                    ..
                } => {
                    hunk_data.split_line_count += (*additions).max(*deletions);
                    hunk_data.unified_line_count += *additions + *deletions;
                }
            }
        }

        hunk_data.split_line_start = file.split_line_count + hunk_data.collapsed_before;
        hunk_data.unified_line_start = file.unified_line_count + hunk_data.collapsed_before;
        file.split_line_count += hunk_data.collapsed_before + hunk_data.split_line_count;
        file.unified_line_count += hunk_data.collapsed_before + hunk_data.unified_line_count;
        file.hunks.push(hunk_data);
    }

    let Some(mut file) = current_file else {
        return Ok(None);
    };

    if !is_git_diff {
        if file
            .prev_name
            .as_ref()
            .is_some_and(|prev| prev != &file.name)
        {
            file.change_type = if file.hunks.is_empty() {
                ChangeType::RenamePure
            } else {
                ChangeType::RenameChanged
            };
        }
    }

    if !matches!(
        file.change_type,
        ChangeType::RenamePure | ChangeType::RenameChanged
    ) {
        file.prev_name = None;
    }

    Ok(Some(file))
}

pub fn parse_diff_from_file(
    old_file: &FileContents,
    new_file: &FileContents,
    options: ParseDiffOptions,
) -> FileDiffMetadata {
    let deletion_lines = split_file_contents_owned(&old_file.contents);
    let addition_lines = split_file_contents_owned(&new_file.contents);
    let context_lines = if options.context_lines == 0 {
        4
    } else {
        options.context_lines
    };
    let ops = compute_full_diff_ops(&deletion_lines, &addition_lines, options.ignore_whitespace);
    let hunks = build_full_diff_hunks(&ops, context_lines);
    let mut file = FileDiffMetadata {
        name: new_file.name.clone(),
        prev_name: (old_file.name != new_file.name).then(|| old_file.name.clone()),
        new_object_id: None,
        prev_object_id: None,
        mode: None,
        prev_mode: None,
        change_type: if old_file.name != new_file.name {
            if hunks.is_empty() {
                ChangeType::RenamePure
            } else {
                ChangeType::RenameChanged
            }
        } else if old_file.contents.is_empty() && !new_file.contents.is_empty() {
            ChangeType::New
        } else if !old_file.contents.is_empty() && new_file.contents.is_empty() {
            ChangeType::Deleted
        } else {
            ChangeType::Change
        },
        hunks,
        split_line_count: 0,
        unified_line_count: 0,
        is_partial: false,
        deletion_lines,
        addition_lines,
        cache_key: old_file
            .cache_key
            .as_ref()
            .zip(new_file.cache_key.as_ref())
            .map(|(old_key, new_key)| format!("{old_key}:{new_key}")),
    };

    apply_full_diff_no_eof_flags(&mut file);
    finalize_full_file_line_counts(&mut file);
    file
}

pub fn clean_last_newline(contents: &str) -> String {
    if let Some(stripped) = contents.strip_suffix("\r\n") {
        stripped.to_string()
    } else if let Some(stripped) = contents.strip_suffix('\n') {
        stripped.to_string()
    } else {
        contents.to_string()
    }
}

pub fn get_line_ending_type(content: &str) -> LineEndingType {
    if content.contains("\r\n") {
        LineEndingType::CRLF
    } else if content.contains('\r') {
        LineEndingType::CR
    } else if content.contains('\n') {
        LineEndingType::LF
    } else {
        LineEndingType::None
    }
}

pub fn parse_line_type(line: &str) -> Option<ParsedLine> {
    let first_char = line.chars().next()?;
    let line_type = match first_char {
        ' ' => HunkLineType::Context,
        '\\' => HunkLineType::Metadata,
        '+' => HunkLineType::Addition,
        '-' => HunkLineType::Deletion,
        _ => return None,
    };
    let processed_line = line.get(first_char.len_utf8()..).unwrap_or_default();
    Some(ParsedLine {
        line: if processed_line.is_empty() {
            "\n".to_string()
        } else {
            processed_line.to_string()
        },
        line_type,
    })
}

pub fn get_icon_for_type(icon_type: DiffIconType) -> &'static str {
    match icon_type {
        DiffIconType::File => "diffs-icon-file-code",
        DiffIconType::Change => "diffs-icon-symbol-modified",
        DiffIconType::New => "diffs-icon-symbol-added",
        DiffIconType::Deleted => "diffs-icon-symbol-deleted",
        DiffIconType::RenamePure | DiffIconType::RenameChanged => "diffs-icon-symbol-moved",
    }
}

pub fn are_files_equal(file_a: Option<&FileContents>, file_b: Option<&FileContents>) -> bool {
    match (file_a, file_b) {
        (None, None) => true,
        (Some(file_a), Some(file_b)) => {
            file_a.cache_key == file_b.cache_key
                && file_a.contents == file_b.contents
                && file_a.name == file_b.name
                && file_a.lang == file_b.lang
        }
        _ => false,
    }
}

pub fn are_diff_targets_equal(
    diff_a: Option<&FileDiffMetadata>,
    diff_b: Option<&FileDiffMetadata>,
) -> bool {
    match (diff_a, diff_b) {
        (None, None) => true,
        (Some(diff_a), Some(diff_b)) if std::ptr::eq(diff_a, diff_b) => true,
        (Some(diff_a), Some(diff_b)) => diff_a
            .cache_key
            .as_ref()
            .is_some_and(|cache_key| Some(cache_key) == diff_b.cache_key.as_ref()),
        _ => false,
    }
}

pub fn are_selections_equal(
    selection_a: Option<&SelectedLineRange>,
    selection_b: Option<&SelectedLineRange>,
) -> bool {
    selection_a == selection_b
}

pub fn are_hunk_data_equal(hunk_a: &HunkData, hunk_b: &HunkData) -> bool {
    hunk_a == hunk_b
}

pub fn are_line_annotations_equal<T: PartialEq>(
    annotation_a: &LineAnnotation<T>,
    annotation_b: &LineAnnotation<T>,
) -> bool {
    annotation_a == annotation_b
}

pub fn are_diff_line_annotations_equal<T: PartialEq>(
    annotation_a: &DiffLineAnnotation<T>,
    annotation_b: &DiffLineAnnotation<T>,
) -> bool {
    annotation_a == annotation_b
}

pub fn get_line_annotation_name(annotation: &impl LineAnnotationName) -> String {
    let side = match annotation.annotation_side() {
        Some(SelectionSide::Deletions) => "deletions-",
        Some(SelectionSide::Additions) => "additions-",
        None => "",
    };
    format!("annotation-{side}{}", annotation.annotation_line_number())
}

pub fn are_objects_equal(
    object_a: Option<&JsonMap<String, JsonValue>>,
    object_b: Option<&JsonMap<String, JsonValue>>,
    omit_keys: &[&str],
) -> bool {
    match (object_a, object_b) {
        (None, None) => true,
        (Some(object_a), Some(object_b)) => {
            let omit_keys: HashSet<&str> = omit_keys.iter().copied().collect();
            for (key, value_a) in object_a {
                if omit_keys.contains(key.as_str()) {
                    continue;
                }
                if object_b.get(key) != Some(value_a) {
                    return false;
                }
            }
            object_b
                .keys()
                .all(|key| omit_keys.contains(key.as_str()) || object_a.contains_key(key))
        }
        _ => false,
    }
}

pub fn are_themes_equal(theme_a: Option<&ThemeSpec>, theme_b: Option<&ThemeSpec>) -> bool {
    theme_a == theme_b
}

pub fn are_pre_properties_equal(
    props_a: Option<&PrePropertiesConfig>,
    props_b: Option<&PrePropertiesConfig>,
) -> bool {
    props_a == props_b
}

pub fn are_file_render_options_equal(
    options_a: &RenderFileOptions,
    options_b: &RenderFileOptions,
) -> bool {
    are_themes_equal(Some(&options_a.theme), Some(&options_b.theme))
        && options_a.use_token_transformer == options_b.use_token_transformer
        && options_a.tokenize_max_line_length == options_b.tokenize_max_line_length
}

pub fn are_diff_render_options_equal(
    options_a: &RenderDiffOptions,
    options_b: &RenderDiffOptions,
) -> bool {
    are_themes_equal(Some(&options_a.theme), Some(&options_b.theme))
        && options_a.use_token_transformer == options_b.use_token_transformer
        && options_a.tokenize_max_line_length == options_b.tokenize_max_line_length
        && options_a.line_diff_type == options_b.line_diff_type
        && options_a.max_line_diff_length == options_b.max_line_diff_length
}

pub fn are_worker_stats_equal(
    stats_a: Option<&WorkerStats>,
    stats_b: Option<&WorkerStats>,
) -> bool {
    stats_a == stats_b
}

pub fn are_merge_conflict_actions_equal(
    action_a: &MergeConflictDiffAction,
    action_b: &MergeConflictDiffAction,
) -> bool {
    action_a.conflict_data == action_b.conflict_data
        && action_a.conflict_index == action_b.conflict_index
        && action_a.conflict == action_b.conflict
}

pub fn get_merge_conflict_line_types(lines: &[String]) -> Vec<MergeConflictLineType> {
    get_merge_conflict_parse_result(lines).line_types
}

pub fn get_merge_conflict_parse_result(lines: &[String]) -> MergeConflictParseResult {
    #[derive(Debug, Clone, Copy)]
    enum MergeConflictStage {
        Current,
        Base,
        Incoming,
    }

    #[derive(Debug, Clone, Copy)]
    struct MergeConflictFrame {
        stage: MergeConflictStage,
        start_line_index: usize,
        base_marker_line_index: Option<usize>,
        separator_line_index: Option<usize>,
    }

    let mut line_types = Vec::with_capacity(lines.len());
    let mut stack: Vec<MergeConflictFrame> = Vec::new();
    let mut regions = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let line = trim_line_ending_for_conflict_marker(line);

        if is_merge_conflict_start_marker(line) {
            stack.push(MergeConflictFrame {
                stage: MergeConflictStage::Current,
                start_line_index: index,
                base_marker_line_index: None,
                separator_line_index: None,
            });
            line_types.push(MergeConflictLineType::MarkerStart);
            continue;
        }

        let Some(frame) = stack.last_mut() else {
            line_types.push(MergeConflictLineType::None);
            continue;
        };

        if is_merge_conflict_base_marker(line) {
            frame.stage = MergeConflictStage::Base;
            frame.base_marker_line_index = Some(index);
            line_types.push(MergeConflictLineType::MarkerBase);
            continue;
        }

        if is_merge_conflict_separator_marker(line) {
            frame.stage = MergeConflictStage::Incoming;
            frame.separator_line_index = Some(index);
            line_types.push(MergeConflictLineType::MarkerSeparator);
            continue;
        }

        if is_merge_conflict_end_marker(line) {
            let completed_frame = stack.pop();
            line_types.push(MergeConflictLineType::MarkerEnd);
            if let Some(completed_frame) = completed_frame {
                if let Some(separator_line_index) = completed_frame.separator_line_index {
                    let conflict_index = regions.len();
                    regions.push(MergeConflictRegion {
                        conflict_index,
                        start_line_index: completed_frame.start_line_index,
                        start_line_number: completed_frame.start_line_index + 1,
                        separator_line_index,
                        separator_line_number: separator_line_index + 1,
                        end_line_index: index,
                        end_line_number: index + 1,
                        base_marker_line_index: completed_frame.base_marker_line_index,
                        base_marker_line_number: completed_frame
                            .base_marker_line_index
                            .map(|line_index| line_index + 1),
                    });
                }
            }
            continue;
        }

        line_types.push(match frame.stage {
            MergeConflictStage::Current => MergeConflictLineType::Current,
            MergeConflictStage::Base => MergeConflictLineType::Base,
            MergeConflictStage::Incoming => MergeConflictLineType::Incoming,
        });
    }

    MergeConflictParseResult {
        line_types,
        regions,
    }
}

pub fn get_merge_conflict_action_line_number(conflict: &MergeConflictRegion) -> usize {
    conflict.start_line_number.saturating_sub(1).max(1)
}

pub fn get_merge_conflict_action_slot_name(input: MergeConflictActionSlotInput) -> String {
    format!(
        "merge-conflict-action-{}-{}-{}",
        input.hunk_index, input.line_index, input.conflict_index
    )
}

pub fn get_hunk_separator_slot_name(column_type: CodeColumnType, hunk_index: usize) -> String {
    let column_type = match column_type {
        CodeColumnType::Unified => "unified",
        CodeColumnType::Additions => "additions",
        CodeColumnType::Deletions => "deletions",
    };
    format!("hunk-separator-{column_type}-{hunk_index}")
}

fn trim_line_ending_for_conflict_marker(line: &str) -> &str {
    if let Some(line) = line.strip_suffix("\r\n") {
        line
    } else if let Some(line) = line.strip_suffix('\n') {
        line
    } else if let Some(line) = line.strip_suffix('\r') {
        line
    } else {
        line
    }
}

fn is_merge_conflict_start_marker(line: &str) -> bool {
    is_repeated_marker_with_optional_label(line, b'<')
}

fn is_merge_conflict_base_marker(line: &str) -> bool {
    is_repeated_marker_with_optional_label(line, b'|')
}

fn is_merge_conflict_end_marker(line: &str) -> bool {
    is_repeated_marker_with_optional_label(line, b'>')
}

fn is_merge_conflict_separator_marker(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.len() >= 7 && bytes.iter().all(|byte| *byte == b'=')
}

fn is_repeated_marker_with_optional_label(line: &str, marker: u8) -> bool {
    let bytes = line.as_bytes();
    let marker_count = bytes.iter().take_while(|byte| **byte == marker).count();
    if marker_count < 7 {
        return false;
    }
    bytes
        .get(marker_count)
        .is_none_or(|byte| byte.is_ascii_whitespace())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeConflictScanStage {
    Current,
    Base,
    Incoming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeConflictContentRole {
    Current,
    Base,
    Incoming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeConflictMarkerType {
    Start,
    Base,
    Separator,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextFlushMode {
    Leading,
    BeforeChange,
    Trailing,
}

#[derive(Debug, Clone)]
struct SyntheticConflictHunkBuilder {
    addition_start: usize,
    deletion_start: usize,
    addition_count: usize,
    deletion_count: usize,
    addition_lines: usize,
    deletion_lines: usize,
    addition_line_index: usize,
    deletion_line_index: usize,
    hunk_content: Vec<HunkContent>,
    context_buffer_addition_start: usize,
    context_buffer_deletion_start: usize,
    context_buffer_count: usize,
    context_buffer_base_conflicts: Vec<(usize, usize)>,
}

#[derive(Debug, Clone)]
struct SyntheticConflictFrame {
    conflict_index: usize,
    stage: MergeConflictScanStage,
    start_line_index: usize,
    base_marker_line_index: Option<usize>,
    separator_line_index: Option<usize>,
    marker_start: String,
    marker_base: Option<String>,
    marker_separator: Option<String>,
}

impl SyntheticConflictFrame {
    fn as_stage_and_conflict_index(&self) -> (MergeConflictScanStage, usize) {
        (self.stage, self.conflict_index)
    }
}

#[derive(Debug, Clone)]
struct SyntheticConflictActionBuilder {
    completed: bool,
    conflict_index: usize,
    hunk_index: Option<usize>,
    start_content_index: Option<usize>,
    end_content_index: Option<usize>,
    end_marker_content_index: Option<usize>,
    current_content_index: Option<usize>,
    base_content_index: Option<usize>,
    incoming_content_index: Option<usize>,
    conflict: MergeConflictRegion,
    marker_lines: MergeConflictMarkerLines,
}

#[derive(Debug, Clone)]
struct SyntheticConflictParseState {
    deletion_lines: Vec<String>,
    addition_lines: Vec<String>,
    current_contents: String,
    incoming_contents: String,
    conflict_stack: Vec<SyntheticConflictFrame>,
    conflict_builders: Vec<Option<SyntheticConflictActionBuilder>>,
    actions: Vec<Option<MergeConflictDiffAction>>,
    hunks: Vec<Hunk>,
    next_conflict_index: usize,
    split_line_count: usize,
    unified_line_count: usize,
    last_hunk_end: usize,
    active_hunk: Option<SyntheticConflictHunkBuilder>,
    max_context_lines: usize,
    max_context_lines2: usize,
}

fn create_resolved_conflict_file(
    file: &FileContents,
    side: &str,
    contents: String,
) -> FileContents {
    FileContents {
        contents,
        cache_key: file
            .cache_key
            .as_ref()
            .map(|cache_key| format!("{cache_key}:merge-conflict-{side}")),
        ..file.clone()
    }
}

fn create_synthetic_conflict_hunk_builder(
    addition_start: usize,
    deletion_start: usize,
) -> SyntheticConflictHunkBuilder {
    SyntheticConflictHunkBuilder {
        addition_start,
        deletion_start,
        addition_count: 0,
        deletion_count: 0,
        addition_lines: 0,
        deletion_lines: 0,
        addition_line_index: addition_start.saturating_sub(1),
        deletion_line_index: deletion_start.saturating_sub(1),
        hunk_content: Vec::new(),
        context_buffer_addition_start: addition_start.saturating_sub(1),
        context_buffer_deletion_start: deletion_start.saturating_sub(1),
        context_buffer_count: 0,
        context_buffer_base_conflicts: Vec::new(),
    }
}

fn ensure_synthetic_conflict_hunk(state: &mut SyntheticConflictParseState) {
    if state.active_hunk.is_none() {
        state.active_hunk = Some(create_synthetic_conflict_hunk_builder(
            state.addition_lines.len() + 1,
            state.deletion_lines.len() + 1,
        ));
    }
}

fn append_synthetic_conflict_change(
    hunk: &mut SyntheticConflictHunkBuilder,
    deletion_line_index: usize,
    addition_line_index: usize,
    deletion: bool,
    addition: bool,
) -> usize {
    if let Some(HunkContent::Change {
        deletions,
        additions,
        ..
    }) = hunk.hunk_content.last_mut()
    {
        if deletion {
            *deletions += 1;
        }
        if addition {
            *additions += 1;
        }
        return hunk.hunk_content.len().saturating_sub(1);
    }
    hunk.hunk_content.push(HunkContent::Change {
        deletions: usize::from(deletion),
        deletion_line_index,
        additions: usize::from(addition),
        addition_line_index,
    });
    hunk.hunk_content.len().saturating_sub(1)
}

fn format_hunk_range_for_header(start: usize, count: usize) -> String {
    if count == 1 {
        start.to_string()
    } else {
        format!("{start},{count}")
    }
}

fn assign_synthetic_conflict_content(
    state: &mut SyntheticConflictParseState,
    conflict_index: usize,
    role: MergeConflictContentRole,
    content_index: usize,
) -> color_eyre::Result<()> {
    let hunk_index = state.hunks.len();
    let builder = state
        .conflict_builders
        .get_mut(conflict_index)
        .and_then(Option::as_mut)
        .ok_or_else(|| {
            eyre!(
                "parseMergeConflictDiffFromFile: failed to locate conflict action {conflict_index}"
            )
        })?;

    if let Some(existing_hunk_index) = builder.hunk_index {
        if existing_hunk_index != hunk_index {
            return Err(eyre!(
                "parseMergeConflictDiffFromFile: conflict {} spans multiple hunks and cannot be anchored",
                conflict_index
            ));
        }
    } else {
        builder.hunk_index = Some(hunk_index);
    }

    builder.start_content_index.get_or_insert(content_index);
    builder.end_content_index = Some(content_index);
    builder.end_marker_content_index = Some(content_index);

    match role {
        MergeConflictContentRole::Current => {
            builder.current_content_index.get_or_insert(content_index);
        }
        MergeConflictContentRole::Base => {
            builder.base_content_index.get_or_insert(content_index);
        }
        MergeConflictContentRole::Incoming => {
            builder.incoming_content_index = Some(content_index);
        }
    }

    Ok(())
}

fn flush_synthetic_conflict_context(
    state: &mut SyntheticConflictParseState,
    mode: ContextFlushMode,
) -> color_eyre::Result<()> {
    let Some(hunk) = state.active_hunk.as_mut() else {
        return Ok(());
    };

    let mut count = hunk.context_buffer_count;
    let mut addition_start = hunk.context_buffer_addition_start;
    let mut deletion_start = hunk.context_buffer_deletion_start;

    if mode == ContextFlushMode::Leading && count > state.max_context_lines {
        let difference = count - state.max_context_lines;
        addition_start += difference;
        deletion_start += difference;
        count = state.max_context_lines;
        hunk.addition_start += difference;
        hunk.deletion_start += difference;
        hunk.addition_line_index += difference;
        hunk.deletion_line_index += difference;
    }

    if mode == ContextFlushMode::Trailing && count > state.max_context_lines {
        count = state.max_context_lines;
    }

    if count == 0 {
        hunk.context_buffer_count = 0;
        hunk.context_buffer_base_conflicts.clear();
        return Ok(());
    }

    let content_index =
        if let Some(HunkContent::Context { lines, .. }) = hunk.hunk_content.last_mut() {
            *lines += count;
            hunk.hunk_content.len().saturating_sub(1)
        } else {
            hunk.hunk_content.push(HunkContent::Context {
                lines: count,
                addition_line_index: addition_start,
                deletion_line_index: deletion_start,
            });
            hunk.hunk_content.len().saturating_sub(1)
        };

    hunk.addition_count += count;
    hunk.deletion_count += count;

    let buffer_start_offset = addition_start.saturating_sub(hunk.context_buffer_addition_start);
    let assignments = if hunk.context_buffer_base_conflicts.is_empty() {
        Vec::new()
    } else {
        hunk.context_buffer_base_conflicts
            .iter()
            .filter_map(|(offset, conflict_index)| {
                (*offset >= buffer_start_offset && *offset < buffer_start_offset + count)
                    .then_some((*conflict_index, content_index))
            })
            .collect::<Vec<_>>()
    };
    hunk.context_buffer_count = 0;
    hunk.context_buffer_base_conflicts.clear();

    for (conflict_index, content_index) in assignments {
        assign_synthetic_conflict_content(
            state,
            conflict_index,
            MergeConflictContentRole::Base,
            content_index,
        )?;
    }

    Ok(())
}

fn finalize_synthetic_conflict_hunk(state: &mut SyntheticConflictParseState) {
    let Some(hunk) = state.active_hunk.take() else {
        return;
    };
    if hunk.hunk_content.is_empty() {
        return;
    }

    let mut hunk_split_line_count = 0;
    let mut hunk_unified_line_count = 0;
    for content in &hunk.hunk_content {
        match content {
            HunkContent::Context { lines, .. } => {
                hunk_split_line_count += *lines;
                hunk_unified_line_count += *lines;
            }
            HunkContent::Change {
                deletions,
                additions,
                ..
            } => {
                hunk_split_line_count += (*deletions).max(*additions);
                hunk_unified_line_count += deletions + additions;
            }
        }
    }

    let collapsed_before = hunk
        .addition_start
        .saturating_sub(1)
        .saturating_sub(state.last_hunk_end);
    let finalized_hunk = Hunk {
        collapsed_before,
        addition_start: hunk.addition_start,
        addition_count: hunk.addition_count,
        addition_lines: hunk.addition_lines,
        addition_line_index: hunk.addition_line_index,
        deletion_start: hunk.deletion_start,
        deletion_count: hunk.deletion_count,
        deletion_lines: hunk.deletion_lines,
        deletion_line_index: hunk.deletion_line_index,
        hunk_content: hunk.hunk_content,
        hunk_context: None,
        hunk_specs: format!(
            "@@ -{} +{} @@\n",
            format_hunk_range_for_header(hunk.deletion_start, hunk.deletion_count),
            format_hunk_range_for_header(hunk.addition_start, hunk.addition_count)
        ),
        split_line_start: state.split_line_count + collapsed_before,
        split_line_count: hunk_split_line_count,
        unified_line_start: state.unified_line_count + collapsed_before,
        unified_line_count: hunk_unified_line_count,
        no_eof_cr_additions: false,
        no_eof_cr_deletions: false,
    };

    state.hunks.push(finalized_hunk);
    state.split_line_count += collapsed_before + hunk_split_line_count;
    state.unified_line_count += collapsed_before + hunk_unified_line_count;
    state.last_hunk_end = hunk.addition_start + hunk.addition_count - 1;
}

fn split_synthetic_conflict_hunk_with_buffered_context(
    state: &mut SyntheticConflictParseState,
) -> color_eyre::Result<()> {
    let Some(hunk) = state.active_hunk.as_ref() else {
        return Ok(());
    };
    let count = hunk.context_buffer_count;
    let omitted_context_line_count = count.saturating_sub(state.max_context_lines2);
    let next_addition_start = hunk.context_buffer_addition_start + count - state.max_context_lines;
    let next_deletion_start = hunk.context_buffer_deletion_start + count - state.max_context_lines;
    let tail_offset = count - state.max_context_lines;
    let next_base_conflicts = hunk
        .context_buffer_base_conflicts
        .iter()
        .filter_map(|(offset, conflict_index)| {
            (*offset >= tail_offset).then_some((*offset - tail_offset, *conflict_index))
        })
        .collect::<Vec<_>>();

    flush_synthetic_conflict_context(state, ContextFlushMode::Trailing)?;

    let (addition_start, deletion_start, emitted_addition_count, emitted_deletion_count) = {
        let hunk = state
            .active_hunk
            .as_ref()
            .expect("active hunk should exist after context flush");
        (
            hunk.addition_start,
            hunk.deletion_start,
            hunk.addition_count,
            hunk.deletion_count,
        )
    };
    finalize_synthetic_conflict_hunk(state);

    let mut next_hunk = create_synthetic_conflict_hunk_builder(
        addition_start + emitted_addition_count + omitted_context_line_count,
        deletion_start + emitted_deletion_count + omitted_context_line_count,
    );
    next_hunk.context_buffer_addition_start = next_addition_start;
    next_hunk.context_buffer_deletion_start = next_deletion_start;
    next_hunk.context_buffer_count = state.max_context_lines;
    next_hunk.context_buffer_base_conflicts = next_base_conflicts;
    state.active_hunk = Some(next_hunk);

    Ok(())
}

fn emit_synthetic_conflict_context_line(
    state: &mut SyntheticConflictParseState,
    line: &str,
    base_conflict_index: Option<usize>,
) {
    let addition_start = state.addition_lines.len();
    let deletion_start = state.deletion_lines.len();
    ensure_synthetic_conflict_hunk(state);

    let hunk = state
        .active_hunk
        .as_mut()
        .expect("active hunk should exist after ensure");
    if hunk.context_buffer_count == 0 {
        hunk.context_buffer_addition_start = addition_start;
        hunk.context_buffer_deletion_start = deletion_start;
    }

    state.addition_lines.push(line.to_string());
    state.deletion_lines.push(line.to_string());
    state.incoming_contents.push_str(line);
    state.current_contents.push_str(line);
    if let Some(conflict_index) = base_conflict_index {
        hunk.context_buffer_base_conflicts
            .push((hunk.context_buffer_count, conflict_index));
    }
    hunk.context_buffer_count += 1;
}

fn emit_synthetic_conflict_change_line(
    state: &mut SyntheticConflictParseState,
    deletion: bool,
    addition: bool,
    line: &str,
    conflict_index: usize,
    role: MergeConflictContentRole,
) -> color_eyre::Result<()> {
    ensure_synthetic_conflict_hunk(state);
    let should_split = state.active_hunk.as_ref().is_some_and(|hunk| {
        !hunk.hunk_content.is_empty() && hunk.context_buffer_count > state.max_context_lines2
    });
    if should_split {
        split_synthetic_conflict_hunk_with_buffered_context(state)?;
    }

    let flush_mode = if state
        .active_hunk
        .as_ref()
        .is_some_and(|hunk| hunk.hunk_content.is_empty())
    {
        ContextFlushMode::Leading
    } else {
        ContextFlushMode::BeforeChange
    };
    flush_synthetic_conflict_context(state, flush_mode)?;

    let addition_line_index = state.addition_lines.len();
    let deletion_line_index = state.deletion_lines.len();
    if addition {
        state.addition_lines.push(line.to_string());
        state.incoming_contents.push_str(line);
    }
    if deletion {
        state.deletion_lines.push(line.to_string());
        state.current_contents.push_str(line);
    }

    let content_index = {
        let hunk = state
            .active_hunk
            .as_mut()
            .expect("active hunk should exist before emitting change");
        let content_index = append_synthetic_conflict_change(
            hunk,
            deletion_line_index,
            addition_line_index,
            deletion,
            addition,
        );
        if addition {
            hunk.addition_count += 1;
            hunk.addition_lines += 1;
        }
        if deletion {
            hunk.deletion_count += 1;
            hunk.deletion_lines += 1;
        }
        content_index
    };

    assign_synthetic_conflict_content(state, conflict_index, role, content_index)
}

fn handle_synthetic_conflict_start_marker(
    state: &mut SyntheticConflictParseState,
    line: &str,
    line_index: usize,
) {
    let conflict_index = state.next_conflict_index;
    state.next_conflict_index += 1;
    state.conflict_stack.push(SyntheticConflictFrame {
        conflict_index,
        stage: MergeConflictScanStage::Current,
        start_line_index: line_index,
        base_marker_line_index: None,
        separator_line_index: None,
        marker_start: line.to_string(),
        marker_base: None,
        marker_separator: None,
    });

    if state.conflict_builders.len() <= conflict_index {
        state
            .conflict_builders
            .resize_with(conflict_index + 1, || None);
    }
    state.conflict_builders[conflict_index] = Some(SyntheticConflictActionBuilder {
        completed: false,
        conflict_index,
        hunk_index: None,
        start_content_index: None,
        end_content_index: None,
        end_marker_content_index: None,
        current_content_index: None,
        base_content_index: None,
        incoming_content_index: None,
        conflict: MergeConflictRegion {
            conflict_index,
            start_line_index: line_index,
            start_line_number: line_index + 1,
            separator_line_index: line_index,
            separator_line_number: line_index + 1,
            end_line_index: line_index,
            end_line_number: line_index + 1,
            base_marker_line_index: None,
            base_marker_line_number: None,
        },
        marker_lines: MergeConflictMarkerLines {
            start: line.to_string(),
            base: None,
            separator: String::new(),
            end: String::new(),
        },
    });
}

fn finalize_synthetic_conflict(
    state: &mut SyntheticConflictParseState,
    frame: SyntheticConflictFrame,
    end_line_index: usize,
    end_marker_line: &str,
) -> color_eyre::Result<()> {
    let Some(separator_line_index) = frame.separator_line_index else {
        return Err(eyre!(
            "parseMergeConflictDiffFromFile: conflict {} is missing a separator marker",
            frame.conflict_index
        ));
    };
    let Some(separator_line) = frame.marker_separator else {
        return Err(eyre!(
            "parseMergeConflictDiffFromFile: conflict {} is missing a separator marker",
            frame.conflict_index
        ));
    };

    let builder = state
        .conflict_builders
        .get_mut(frame.conflict_index)
        .and_then(Option::as_mut)
        .ok_or_else(|| {
            eyre!(
                "parseMergeConflictDiffFromFile: failed to finalize conflict {}",
                frame.conflict_index
            )
        })?;

    builder.marker_lines.start = frame.marker_start;
    builder.marker_lines.base = frame.marker_base;
    builder.marker_lines.separator = separator_line;
    builder.marker_lines.end = end_marker_line.to_string();
    builder.conflict = MergeConflictRegion {
        conflict_index: frame.conflict_index,
        start_line_index: frame.start_line_index,
        start_line_number: frame.start_line_index + 1,
        separator_line_index,
        separator_line_number: separator_line_index + 1,
        end_line_index,
        end_line_number: end_line_index + 1,
        base_marker_line_index: frame.base_marker_line_index,
        base_marker_line_number: frame
            .base_marker_line_index
            .map(|line_index| line_index + 1),
    };

    let fallback_content_index = builder
        .current_content_index
        .or(builder.incoming_content_index);
    builder.current_content_index = builder.current_content_index.or(fallback_content_index);
    builder.incoming_content_index = builder.incoming_content_index.or(fallback_content_index);
    builder.start_content_index = builder.start_content_index.or(fallback_content_index);
    builder.end_content_index = builder.end_content_index.or(fallback_content_index);
    builder.end_marker_content_index = builder.end_marker_content_index.or(fallback_content_index);

    let hunk_index = builder.hunk_index.ok_or_else(|| {
        eyre!(
            "parseMergeConflictDiffFromFile: failed to anchor merge conflict {}",
            frame.conflict_index
        )
    })?;
    let start_content_index = builder.start_content_index.ok_or_else(|| {
        eyre!(
            "parseMergeConflictDiffFromFile: failed to anchor merge conflict {}",
            frame.conflict_index
        )
    })?;
    let end_content_index = builder.end_content_index.ok_or_else(|| {
        eyre!(
            "parseMergeConflictDiffFromFile: failed to anchor merge conflict {}",
            frame.conflict_index
        )
    })?;
    let end_marker_content_index = builder.end_marker_content_index.ok_or_else(|| {
        eyre!(
            "parseMergeConflictDiffFromFile: failed to anchor merge conflict {}",
            frame.conflict_index
        )
    })?;

    let action = MergeConflictDiffAction {
        conflict_data: ProcessFileConflictData {
            hunk_index,
            start_content_index,
            end_content_index,
            current_content_index: builder.current_content_index,
            base_content_index: builder.base_content_index,
            incoming_content_index: builder.incoming_content_index,
            end_marker_content_index,
        },
        conflict: builder.conflict.clone(),
        conflict_index: builder.conflict_index,
        marker_lines: builder.marker_lines.clone(),
    };

    if state.actions.len() <= frame.conflict_index {
        state.actions.resize_with(frame.conflict_index + 1, || None);
    }
    state.actions[frame.conflict_index] = Some(action);
    builder.completed = true;

    Ok(())
}

pub fn build_merge_conflict_marker_rows(
    file_diff: &FileDiffMetadata,
    actions: &[Option<MergeConflictDiffAction>],
) -> Vec<MergeConflictMarkerRow> {
    let mut marker_rows = Vec::new();
    let mut cached_hunk_index = usize::MAX;
    let mut cached_unified_starts = Vec::new();
    for action in actions.iter().flatten() {
        let Some(hunk) = file_diff.hunks.get(action.conflict_data.hunk_index) else {
            continue;
        };
        if cached_hunk_index != action.conflict_data.hunk_index {
            cached_hunk_index = action.conflict_data.hunk_index;
            cached_unified_starts = build_unified_line_starts_for_hunk(hunk);
        }

        let action_line_index = unified_line_start_from_cache(
            &cached_unified_starts,
            action.conflict_data.start_content_index,
        );
        marker_rows.push(create_merge_conflict_marker_row(
            action,
            MergeConflictMarkerRowType::MarkerStart,
            action.conflict_data.start_content_index,
            action.marker_lines.start.clone(),
            action_line_index,
        ));

        if let Some(base_content_index) = action.conflict_data.base_content_index {
            let Some(current_content_index) = action.conflict_data.current_content_index else {
                continue;
            };
            let Some(incoming_content_index) = action.conflict_data.incoming_content_index else {
                continue;
            };
            let Some(base_marker_line) = action.marker_lines.base.clone() else {
                continue;
            };
            let Some(HunkContent::Change { deletions, .. }) =
                hunk.hunk_content.get(current_content_index)
            else {
                continue;
            };
            if !matches!(
                hunk.hunk_content.get(base_content_index),
                Some(HunkContent::Context { .. })
            ) || !matches!(
                hunk.hunk_content.get(incoming_content_index),
                Some(HunkContent::Change { .. })
            ) {
                continue;
            }

            let current_start =
                unified_line_start_from_cache(&cached_unified_starts, current_content_index);
            let incoming_start =
                unified_line_start_from_cache(&cached_unified_starts, incoming_content_index);
            marker_rows.push(create_merge_conflict_marker_row(
                action,
                MergeConflictMarkerRowType::MarkerBase,
                base_content_index,
                base_marker_line,
                current_start + deletions,
            ));
            marker_rows.push(create_merge_conflict_marker_row(
                action,
                MergeConflictMarkerRowType::MarkerSeparator,
                base_content_index,
                action.marker_lines.separator.clone(),
                incoming_start,
            ));
            marker_rows.push(create_merge_conflict_marker_row(
                action,
                MergeConflictMarkerRowType::MarkerEnd,
                action.conflict_data.end_marker_content_index,
                action.marker_lines.end.clone(),
                unified_line_end_from_cache(
                    &cached_unified_starts,
                    action.conflict_data.end_marker_content_index,
                ),
            ));
            continue;
        }

        let Some(current_content_index) = action.conflict_data.current_content_index else {
            continue;
        };
        let Some(HunkContent::Change { deletions, .. }) =
            hunk.hunk_content.get(current_content_index)
        else {
            continue;
        };
        let content_start =
            unified_line_start_from_cache(&cached_unified_starts, current_content_index);
        let separator_line_index = if *deletions > 0 {
            content_start + deletions
        } else {
            action_line_index
        };
        marker_rows.push(create_merge_conflict_marker_row(
            action,
            MergeConflictMarkerRowType::MarkerSeparator,
            current_content_index,
            action.marker_lines.separator.clone(),
            separator_line_index,
        ));
        marker_rows.push(create_merge_conflict_marker_row(
            action,
            MergeConflictMarkerRowType::MarkerEnd,
            action.conflict_data.end_marker_content_index,
            action.marker_lines.end.clone(),
            unified_line_end_from_cache(
                &cached_unified_starts,
                action.conflict_data.end_marker_content_index,
            ),
        ));
    }
    marker_rows
}

pub fn get_merge_conflict_action_anchor(
    action: &MergeConflictDiffAction,
    file_diff: &FileDiffMetadata,
) -> Option<MergeConflictActionAnchor> {
    let hunk = file_diff.hunks.get(action.conflict_data.hunk_index)?;
    Some(MergeConflictActionAnchor {
        hunk_index: action.conflict_data.hunk_index,
        line_index: get_unified_line_start_for_content(
            hunk,
            action.conflict_data.start_content_index,
        ),
    })
}

fn create_merge_conflict_marker_row(
    action: &MergeConflictDiffAction,
    row_type: MergeConflictMarkerRowType,
    content_index: usize,
    line_text: String,
    line_index: usize,
) -> MergeConflictMarkerRow {
    MergeConflictMarkerRow {
        row_type,
        hunk_index: action.conflict_data.hunk_index,
        content_index,
        conflict_index: action.conflict_index,
        line_text,
        line_index,
    }
}

fn build_unified_line_starts_for_hunk(hunk: &Hunk) -> Vec<usize> {
    let mut starts = Vec::with_capacity(hunk.hunk_content.len() + 1);
    let mut line_index = hunk.unified_line_start;
    starts.push(line_index);
    for content in &hunk.hunk_content {
        line_index += match content {
            HunkContent::Context { lines, .. } => *lines,
            HunkContent::Change {
                deletions,
                additions,
                ..
            } => deletions + additions,
        };
        starts.push(line_index);
    }
    starts
}

fn unified_line_start_from_cache(starts: &[usize], content_index: usize) -> usize {
    starts
        .get(content_index)
        .copied()
        .or_else(|| starts.last().copied())
        .unwrap_or(0)
}

fn unified_line_end_from_cache(starts: &[usize], content_index: usize) -> usize {
    let start = unified_line_start_from_cache(starts, content_index);
    let end_exclusive = unified_line_start_from_cache(starts, content_index.saturating_add(1));
    start.max(end_exclusive.saturating_sub(1))
}

fn get_unified_line_start_for_content(hunk: &Hunk, content_index: usize) -> usize {
    let mut line_index = hunk.unified_line_start;
    for content in hunk.hunk_content.iter().take(content_index) {
        line_index += match content {
            HunkContent::Context { lines, .. } => *lines,
            HunkContent::Change {
                deletions,
                additions,
                ..
            } => deletions + additions,
        };
    }
    line_index
}

fn get_synthetic_conflict_marker_type(line: &str) -> Option<MergeConflictMarkerType> {
    let bytes = line.as_bytes();
    if bytes.len() < 7 {
        return None;
    }

    let marker = bytes[0];
    if !matches!(marker, b'<' | b'|' | b'=' | b'>') {
        return None;
    }

    let mut content_end = bytes.len();
    if content_end > 0 && bytes[content_end - 1] == b'\n' {
        content_end -= 1;
    }
    if content_end > 0 && bytes[content_end - 1] == b'\r' {
        content_end -= 1;
    }
    if content_end < 7 {
        return None;
    }

    let mut marker_len = 1usize;
    while marker_len < content_end && bytes[marker_len] == marker {
        marker_len += 1;
    }
    if marker_len < 7 {
        return None;
    }

    if marker == b'=' {
        return (marker_len == content_end).then_some(MergeConflictMarkerType::Separator);
    }

    if marker_len != content_end && !is_merge_conflict_marker_whitespace(bytes[marker_len]) {
        return None;
    }

    match marker {
        b'<' => Some(MergeConflictMarkerType::Start),
        b'|' => Some(MergeConflictMarkerType::Base),
        b'>' => Some(MergeConflictMarkerType::End),
        _ => None,
    }
}

fn is_merge_conflict_marker_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b' ')
}

pub fn get_singular_patch(patch: &str) -> color_eyre::Result<FileDiffMetadata> {
    let parsed_patches = parse_patch_files(patch, None, true)?;
    if parsed_patches.len() != 1 {
        color_eyre::eyre::bail!("PatchDiff: Provided patch must include only 1 patch, with 1 diff");
    }
    let patch = parsed_patches.into_iter().next().unwrap();
    if patch.files.len() != 1 {
        color_eyre::eyre::bail!("FileDiff: Provided patch must contain exactly 1 file diff");
    }
    Ok(patch.files.into_iter().next().unwrap())
}

pub fn trim_patch_context(patch: &str, context_size: usize) -> String {
    let mut lines = Vec::new();
    let mut current_hunk: Option<TrimmedPatchHunk> = None;

    for line in patch.split('\n') {
        if let Some(header) = parse_patch_hunk_header(line) {
            if let Some(mut hunk) = current_hunk.take() {
                if !hunk.hunk_lines.is_empty() {
                    flush_trim_context_lines(
                        &mut hunk,
                        context_size,
                        TrimContextFlushMode::Trailing,
                    );
                    flush_trim_hunk(&hunk, &mut lines);
                }
            }

            current_hunk = Some(TrimmedPatchHunk {
                addition_start: header.addition_start,
                deletion_start: header.deletion_start,
                addition_count: 0,
                deletion_count: 0,
                hunk_lines: Vec::new(),
                context_lines: Vec::new(),
            });
            continue;
        }

        if current_hunk.is_none() {
            lines.push(line.to_string());
            continue;
        }

        let hunk = current_hunk
            .as_mut()
            .expect("hunk should exist after is_none check");
        if line.starts_with(' ') {
            hunk.context_lines.push(line.to_string());
        } else if !line.is_empty() {
            if !hunk.hunk_lines.is_empty()
                && hunk.context_lines.len() > context_size.saturating_mul(2)
            {
                let omitted_context_line_count =
                    hunk.context_lines.len() - context_size.saturating_mul(2);
                let next_context_lines = hunk.context_lines
                    [hunk.context_lines.len().saturating_sub(context_size)..]
                    .to_vec();
                flush_trim_context_lines(hunk, context_size, TrimContextFlushMode::Trailing);
                let emitted_addition_count = hunk.addition_count;
                let emitted_deletion_count = hunk.deletion_count;
                flush_trim_hunk(hunk, &mut lines);

                *hunk = TrimmedPatchHunk {
                    addition_start: hunk.addition_start
                        + emitted_addition_count
                        + omitted_context_line_count,
                    deletion_start: hunk.deletion_start
                        + emitted_deletion_count
                        + omitted_context_line_count,
                    addition_count: 0,
                    deletion_count: 0,
                    hunk_lines: Vec::new(),
                    context_lines: next_context_lines,
                };
            }

            let mode = if hunk.hunk_lines.is_empty() {
                TrimContextFlushMode::Leading
            } else {
                TrimContextFlushMode::BeforeChange
            };
            flush_trim_context_lines(hunk, context_size, mode);
            hunk.hunk_lines.push(line.to_string());
            if line.starts_with('+') {
                hunk.addition_count += 1;
            } else if line.starts_with('-') {
                hunk.deletion_count += 1;
            }
        }
    }

    if let Some(mut hunk) = current_hunk {
        if !hunk.hunk_lines.is_empty() {
            flush_trim_context_lines(&mut hunk, context_size, TrimContextFlushMode::Trailing);
            flush_trim_hunk(&hunk, &mut lines);
        }
    }

    let result = lines.join("\n");
    if patch.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    }
}

pub fn diff_accept_reject_hunk(
    diff: &FileDiffMetadata,
    hunk_index: usize,
    resolution: DiffHunkResolution,
) -> color_eyre::Result<FileDiffMetadata> {
    let hunk = diff
        .hunks
        .get(hunk_index)
        .ok_or_else(|| eyre!("diffAcceptRejectHunk: Invalid hunk index"))?;
    resolve_diff_region(
        diff,
        hunk_index,
        0,
        hunk.hunk_content.len().saturating_sub(1),
        normalize_diff_resolution(resolution),
    )
}

pub fn diff_accept_reject_content(
    diff: &FileDiffMetadata,
    hunk_index: usize,
    content_index: usize,
    resolution: DiffHunkResolution,
) -> color_eyre::Result<FileDiffMetadata> {
    resolve_diff_region(
        diff,
        hunk_index,
        content_index,
        content_index,
        normalize_diff_resolution(resolution),
    )
}

pub fn resolve_conflict(
    diff: &FileDiffMetadata,
    conflict: &ProcessFileConflictData,
    resolution: MergeConflictResolution,
) -> color_eyre::Result<FileDiffMetadata> {
    let mut indexes_to_delete = HashSet::new();
    if let Some(base_content_index) = conflict.base_content_index {
        indexes_to_delete.insert(base_content_index);
    }
    if conflict.end_marker_content_index != conflict.end_content_index {
        indexes_to_delete.insert(conflict.end_marker_content_index);
    }

    resolve_diff_region_with_deleted_indexes(
        diff,
        conflict.hunk_index,
        conflict.start_content_index,
        conflict.end_content_index,
        normalize_merge_conflict_resolution(resolution),
        &indexes_to_delete,
    )
}

pub fn resolve_merge_conflict_contents(
    contents: &str,
    conflict: &MergeConflictRegion,
    resolution: MergeConflictResolution,
) -> String {
    let lines = split_file_contents_preserving_endings(contents);
    let current_end = conflict
        .base_marker_line_index
        .unwrap_or(conflict.separator_line_index);
    let incoming_start = conflict.separator_line_index.saturating_add(1);

    let mut resolved = String::with_capacity(contents.len());
    for line in lines.iter().take(conflict.start_line_index) {
        resolved.push_str(line);
    }

    match resolution {
        MergeConflictResolution::Current => {
            for line in lines
                .iter()
                .take(current_end)
                .skip(conflict.start_line_index.saturating_add(1))
            {
                resolved.push_str(line);
            }
        }
        MergeConflictResolution::Incoming => {
            for line in lines
                .iter()
                .take(conflict.end_line_index)
                .skip(incoming_start)
            {
                resolved.push_str(line);
            }
        }
        MergeConflictResolution::Both => {
            for line in lines
                .iter()
                .take(current_end)
                .skip(conflict.start_line_index.saturating_add(1))
            {
                resolved.push_str(line);
            }
            for line in lines
                .iter()
                .take(conflict.end_line_index)
                .skip(incoming_start)
            {
                resolved.push_str(line);
            }
        }
    }

    for line in lines.iter().skip(conflict.end_line_index.saturating_add(1)) {
        resolved.push_str(line);
    }
    resolved
}

fn split_file_contents_preserving_endings(contents: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    while line_start < contents.len() {
        let Some(relative_newline_index) = contents[line_start..].find('\n') else {
            lines.push(&contents[line_start..]);
            break;
        };
        let line_end = line_start + relative_newline_index + 1;
        lines.push(&contents[line_start..line_end]);
        line_start = line_end;
    }
    lines
}

pub fn parse_merge_conflict_diff_from_file(
    file: &FileContents,
    max_context_lines: usize,
) -> color_eyre::Result<ParseMergeConflictDiffFromFileResult> {
    let max_context_lines = max_context_lines.max(1);
    let estimated_line_count = file.contents.len().saturating_div(32).saturating_add(1);
    let mut state = SyntheticConflictParseState {
        deletion_lines: Vec::with_capacity(estimated_line_count),
        addition_lines: Vec::with_capacity(estimated_line_count),
        current_contents: String::with_capacity(file.contents.len()),
        incoming_contents: String::with_capacity(file.contents.len()),
        conflict_stack: Vec::new(),
        conflict_builders: Vec::new(),
        actions: Vec::new(),
        hunks: Vec::new(),
        next_conflict_index: 0,
        split_line_count: 0,
        unified_line_count: 0,
        last_hunk_end: 0,
        active_hunk: None,
        max_context_lines,
        max_context_lines2: max_context_lines.saturating_mul(2),
    };

    let mut line_start = 0usize;
    let mut line_index = 0usize;
    while line_start < file.contents.len() {
        let relative_newline_index = file.contents[line_start..].find('\n');
        let line_end = relative_newline_index
            .map(|index| line_start + index + 1)
            .unwrap_or(file.contents.len());
        let line = &file.contents[line_start..line_end];
        if state.conflict_stack.is_empty() {
            if line.as_bytes().first() == Some(&b'<')
                && get_synthetic_conflict_marker_type(line) == Some(MergeConflictMarkerType::Start)
            {
                handle_synthetic_conflict_start_marker(&mut state, line, line_index);
                line_start = line_end;
                line_index += 1;
                continue;
            }
            emit_synthetic_conflict_context_line(&mut state, line, None);
            line_start = line_end;
            line_index += 1;
            continue;
        }

        match get_synthetic_conflict_marker_type(line) {
            Some(MergeConflictMarkerType::Start) => {
                handle_synthetic_conflict_start_marker(&mut state, line, line_index);
                line_start = line_end;
                line_index += 1;
                continue;
            }
            Some(MergeConflictMarkerType::Base) => {
                let frame = state.conflict_stack.last_mut().ok_or_else(|| {
                    eyre!("parseMergeConflictDiffFromFile: base marker outside conflict")
                })?;
                frame.stage = MergeConflictScanStage::Base;
                frame.base_marker_line_index = Some(line_index);
                frame.marker_base = Some(line.to_string());
                line_start = line_end;
                line_index += 1;
                continue;
            }
            Some(MergeConflictMarkerType::Separator) => {
                let frame = state.conflict_stack.last_mut().ok_or_else(|| {
                    eyre!("parseMergeConflictDiffFromFile: separator marker outside conflict")
                })?;
                frame.stage = MergeConflictScanStage::Incoming;
                frame.separator_line_index = Some(line_index);
                frame.marker_separator = Some(line.to_string());
                line_start = line_end;
                line_index += 1;
                continue;
            }
            Some(MergeConflictMarkerType::End) => {
                let frame = state.conflict_stack.pop().ok_or_else(|| {
                    eyre!("parseMergeConflictDiffFromFile: end marker outside conflict")
                })?;
                finalize_synthetic_conflict(&mut state, frame, line_index, line)?;
                line_start = line_end;
                line_index += 1;
                continue;
            }
            None => {}
        }

        let (stage, conflict_index) = state
            .conflict_stack
            .last()
            .ok_or_else(|| eyre!("parseMergeConflictDiffFromFile: missing conflict frame"))?
            .as_stage_and_conflict_index();
        match stage {
            MergeConflictScanStage::Current => {
                emit_synthetic_conflict_change_line(
                    &mut state,
                    true,
                    false,
                    line,
                    conflict_index,
                    MergeConflictContentRole::Current,
                )?;
            }
            MergeConflictScanStage::Base => {
                emit_synthetic_conflict_context_line(&mut state, line, Some(conflict_index));
            }
            MergeConflictScanStage::Incoming => {
                emit_synthetic_conflict_change_line(
                    &mut state,
                    false,
                    true,
                    line,
                    conflict_index,
                    MergeConflictContentRole::Incoming,
                )?;
            }
        }
        line_start = line_end;
        line_index += 1;
    }

    if !state.conflict_stack.is_empty() {
        return Err(eyre!(
            "parseMergeConflictDiffFromFile: unfinished merge conflict marker stack"
        ));
    }

    if state
        .active_hunk
        .as_ref()
        .is_some_and(|hunk| !hunk.hunk_content.is_empty())
    {
        flush_synthetic_conflict_context(&mut state, ContextFlushMode::Trailing)?;
        finalize_synthetic_conflict_hunk(&mut state);
    }

    for (conflict_index, builder) in state.conflict_builders.iter().enumerate() {
        if !builder.as_ref().is_some_and(|builder| builder.completed) {
            return Err(eyre!(
                "parseMergeConflictDiffFromFile: failed to build merge conflict action {}",
                conflict_index
            ));
        }
    }

    if !state.hunks.is_empty()
        && !state.addition_lines.is_empty()
        && !state.deletion_lines.is_empty()
    {
        let last_hunk = state
            .hunks
            .last()
            .expect("last hunk should exist after non-empty check");
        let collapsed_after = state
            .addition_lines
            .len()
            .saturating_sub(last_hunk.addition_start + last_hunk.addition_count.saturating_sub(1));
        state.split_line_count += collapsed_after;
        state.unified_line_count += collapsed_after;
    }

    let current_contents = state.current_contents;
    let incoming_contents = state.incoming_contents;
    let current_file = create_resolved_conflict_file(file, "current", current_contents);
    let incoming_file = create_resolved_conflict_file(file, "incoming", incoming_contents);
    let change_type = if incoming_file.contents.is_empty() {
        ChangeType::Deleted
    } else if current_file.contents.is_empty() {
        ChangeType::New
    } else {
        ChangeType::Change
    };

    let mut file_diff = FileDiffMetadata {
        name: file.name.clone(),
        prev_name: None,
        new_object_id: None,
        prev_object_id: None,
        mode: None,
        prev_mode: None,
        change_type,
        split_line_count: state.split_line_count,
        unified_line_count: state.unified_line_count,
        hunks: state.hunks,
        is_partial: false,
        deletion_lines: state.deletion_lines,
        addition_lines: state.addition_lines,
        cache_key: None,
    };
    file_diff.cache_key = file
        .cache_key
        .as_ref()
        .map(|cache_key| format!("{cache_key}:merge-conflict-diff"));

    let marker_rows = build_merge_conflict_marker_rows(&file_diff, &state.actions);

    Ok(ParseMergeConflictDiffFromFileResult {
        file_diff,
        current_file,
        incoming_file,
        actions: state.actions,
        marker_rows,
    })
}

pub fn iterate_over_file<'a, F>(lines: &'a [String], options: FileIterationOptions, mut callback: F)
where
    F: FnMut(FileLine<'a>) -> bool,
{
    if lines.is_empty() {
        return;
    }

    let starting_line = options.starting_line.min(lines.len());
    let requested_total = options.total_lines.unwrap_or(usize::MAX);
    let len = starting_line
        .saturating_add(requested_total)
        .min(lines.len());
    let last_line_index = match lines.last().map(String::as_str) {
        Some("" | "\n" | "\r\n" | "\r") => lines.len().saturating_sub(2),
        Some(_) => lines.len() - 1,
        None => return,
    };

    for line_index in starting_line..len {
        let is_last_line = line_index == last_line_index;
        if callback(FileLine {
            line_index,
            line_number: line_index + 1,
            content: &lines[line_index],
            is_last_line,
        }) || is_last_line
        {
            break;
        }
    }
}

pub fn collect_diff_lines(
    diff: &FileDiffMetadata,
    options: DiffIterationOptions<'_>,
) -> color_eyre::Result<Vec<DiffLine>> {
    let mut lines = Vec::new();
    collect_all_diff_lines(diff, options, &mut lines)?;

    if options.starting_line == 0 && options.total_lines.is_none() {
        return Ok(lines);
    }

    let start = options.starting_line.min(lines.len());
    let end = options
        .total_lines
        .map(|total| start.saturating_add(total).min(lines.len()))
        .unwrap_or(lines.len());
    Ok(lines[start..end].to_vec())
}

pub fn iterate_over_diff<F>(
    diff: &FileDiffMetadata,
    options: DiffIterationOptions<'_>,
    mut callback: F,
) -> color_eyre::Result<()>
where
    F: FnMut(DiffLine) -> bool,
{
    for line in collect_diff_lines(diff, options)? {
        if callback(line) {
            break;
        }
    }
    Ok(())
}

pub fn compute_virtual_file_metrics(metrics: Option<VirtualFileMetrics>) -> VirtualFileMetrics {
    metrics.unwrap_or_default()
}

pub fn are_render_ranges_equal(
    render_range_a: Option<&RenderRange>,
    render_range_b: Option<&RenderRange>,
) -> bool {
    render_range_a == render_range_b
}

pub fn is_default_render_range(render_range: &RenderRange) -> bool {
    render_range.starting_line == 0
        && render_range.total_lines.is_none()
        && render_range.buffer_before == 0
        && render_range.buffer_after == 0
}

pub fn are_virtual_window_specs_equal(
    window_specs_a: Option<&VirtualWindowSpecs>,
    window_specs_b: Option<&VirtualWindowSpecs>,
) -> bool {
    window_specs_a == window_specs_b
}

pub fn create_window_from_scroll_position(
    options: WindowFromScrollPositionOptions,
) -> VirtualWindowSpecs {
    let window_height = options.height + options.overscroll_size * 2.0;
    let effective_height = if options.fit_perfectly {
        options.height + options.fit_perfectly_overscroll * 2.0
    } else {
        window_height
    };
    let scroll_height = options.scroll_height.max(effective_height);

    if window_height >= scroll_height || options.fit_perfectly {
        let top = (options.scroll_top - options.fit_perfectly_overscroll).max(0.0);
        let bottom = (options.scroll_top + effective_height).min(scroll_height);
        return VirtualWindowSpecs {
            top,
            bottom: bottom.max(top),
        };
    }

    let scroll_center = options.scroll_top + options.height / 2.0;
    let mut top = scroll_center - window_height / 2.0;
    let mut bottom = top + window_height;
    if top < 0.0 {
        top = 0.0;
    }
    if bottom > scroll_height {
        bottom = scroll_height;
    }
    top = top.max(0.0).floor();
    VirtualWindowSpecs {
        top,
        bottom: bottom.min(scroll_height).max(top).ceil(),
    }
}

pub fn get_total_line_count_from_hunks(hunks: &[Hunk]) -> usize {
    hunks
        .last()
        .map(|hunk| {
            hunk.addition_start
                .saturating_add(hunk.addition_count)
                .max(hunk.deletion_start.saturating_add(hunk.deletion_count))
        })
        .unwrap_or(0)
}

pub fn get_virtual_file_padding_top(
    metrics: &VirtualFileMetrics,
    disable_file_header: bool,
) -> usize {
    metrics.padding_top.unwrap_or(if disable_file_header {
        metrics.spacing
    } else {
        0
    })
}

pub fn get_virtual_file_padding_bottom(metrics: &VirtualFileMetrics) -> usize {
    metrics.padding_bottom.unwrap_or(metrics.spacing)
}

pub fn get_virtual_file_header_region(
    metrics: &VirtualFileMetrics,
    disable_file_header: bool,
) -> usize {
    let padding_top = get_virtual_file_padding_top(metrics, disable_file_header);
    if disable_file_header {
        padding_top
    } else {
        metrics.diff_header_height + padding_top
    }
}

pub fn get_default_hunk_separator_height(kind: HunkSeparatorKind) -> usize {
    match kind {
        HunkSeparatorKind::Simple => 4,
        HunkSeparatorKind::Metadata
        | HunkSeparatorKind::LineInfo
        | HunkSeparatorKind::LineInfoBasic
        | HunkSeparatorKind::Custom => 32,
    }
}

pub fn get_expanded_region_public(
    is_partial: bool,
    range_size: usize,
    expanded_hunks: Option<ExpandedHunks<'_>>,
    hunk_index: usize,
    collapsed_context_threshold: usize,
) -> ExpandedRegion {
    get_expanded_region(
        is_partial,
        range_size,
        expanded_hunks,
        hunk_index,
        collapsed_context_threshold,
    )
    .into()
}

pub fn get_hunk_separator_height(kind: HunkSeparatorKind, metrics: &VirtualFileMetrics) -> usize {
    metrics
        .hunk_separator_height
        .unwrap_or_else(|| get_default_hunk_separator_height(kind))
}

pub fn get_hunk_separator_gap(kind: HunkSeparatorKind, metrics: &VirtualFileMetrics) -> usize {
    match kind {
        HunkSeparatorKind::Simple
        | HunkSeparatorKind::Metadata
        | HunkSeparatorKind::LineInfoBasic => 0,
        HunkSeparatorKind::LineInfo | HunkSeparatorKind::Custom => metrics.spacing,
    }
}

pub fn has_leading_hunk_separator(
    kind: HunkSeparatorKind,
    hunk_index: usize,
    hunk_specs: Option<&str>,
) -> bool {
    match kind {
        HunkSeparatorKind::Simple => hunk_index > 0,
        HunkSeparatorKind::Metadata => hunk_specs.is_some(),
        HunkSeparatorKind::LineInfo
        | HunkSeparatorKind::LineInfoBasic
        | HunkSeparatorKind::Custom => true,
    }
}

pub fn has_trailing_hunk_separator(kind: HunkSeparatorKind) -> bool {
    !matches!(
        kind,
        HunkSeparatorKind::Simple | HunkSeparatorKind::Metadata
    )
}

pub fn get_leading_hunk_separator_layout(
    kind: HunkSeparatorKind,
    metrics: &VirtualFileMetrics,
    hunk_index: usize,
    hunk_specs: Option<&str>,
) -> Option<HunkSeparatorLayout> {
    if !has_leading_hunk_separator(kind, hunk_index, hunk_specs) {
        return None;
    }

    let height = get_hunk_separator_height(kind, metrics);
    let gap = get_hunk_separator_gap(kind, metrics);
    let gap_before = if hunk_index > 0 { gap } else { 0 };
    let gap_after = gap;
    Some(HunkSeparatorLayout {
        height,
        gap_before,
        gap_after,
        total_height: gap_before + height + gap_after,
    })
}

pub fn get_trailing_hunk_separator_layout(
    kind: HunkSeparatorKind,
    metrics: &VirtualFileMetrics,
) -> Option<HunkSeparatorLayout> {
    if !has_trailing_hunk_separator(kind) {
        return None;
    }

    let height = get_hunk_separator_height(kind, metrics);
    let gap_before = get_hunk_separator_gap(kind, metrics);
    Some(HunkSeparatorLayout {
        height,
        gap_before,
        gap_after: 0,
        total_height: gap_before + height,
    })
}

pub fn compute_estimated_diff_heights(
    file_diff: &FileDiffMetadata,
    options: EstimatedDiffHeightOptions<'_>,
) -> color_eyre::Result<EstimatedDiffHeights> {
    let mut split_height =
        get_virtual_file_header_region(&options.metrics, options.disable_file_header);
    let mut unified_height = split_height;
    let expanded_hunks = if options.expand_unchanged {
        Some(ExpandedHunks::All)
    } else {
        options.expanded_hunks
    };
    let final_hunk_index = file_diff.hunks.len().saturating_sub(1);

    for (hunk_index, hunk) in file_diff.hunks.iter().enumerate() {
        let leading_region = get_expanded_region(
            file_diff.is_partial,
            hunk.collapsed_before,
            expanded_hunks,
            hunk_index,
            options.collapsed_context_threshold,
        );
        let leading_expanded_height =
            (leading_region.from_start + leading_region.from_end) * options.metrics.line_height;
        split_height += leading_expanded_height;
        unified_height += leading_expanded_height;

        if leading_region.collapsed_lines > 0 {
            let separator_height = get_leading_hunk_separator_layout(
                options.hunk_separator_kind,
                &options.metrics,
                hunk_index,
                (!hunk.hunk_specs.is_empty()).then_some(hunk.hunk_specs.as_str()),
            )
            .map(|layout| layout.total_height)
            .unwrap_or(0);
            split_height += separator_height;
            unified_height += separator_height;
        }

        split_height += hunk.split_line_count * options.metrics.line_height;
        unified_height += hunk.unified_line_count * options.metrics.line_height;

        let metadata_counts = no_newline_metadata_line_counts(hunk);
        split_height += metadata_counts.0 * options.metrics.line_height;
        unified_height += metadata_counts.1 * options.metrics.line_height;

        if hunk_index == final_hunk_index && has_final_collapsed_hunk(file_diff) {
            let trailing_region = get_expanded_region(
                file_diff.is_partial,
                get_trailing_range_size(file_diff, hunk)?,
                expanded_hunks,
                file_diff.hunks.len(),
                options.collapsed_context_threshold,
            );
            let trailing_expanded_height = (trailing_region.from_start + trailing_region.from_end)
                * options.metrics.line_height;
            split_height += trailing_expanded_height;
            unified_height += trailing_expanded_height;

            if trailing_region.collapsed_lines > 0 {
                let separator_height = get_trailing_hunk_separator_layout(
                    options.hunk_separator_kind,
                    &options.metrics,
                )
                .map(|layout| layout.total_height)
                .unwrap_or(0);
                split_height += separator_height;
                unified_height += separator_height;
            }
        }
    }

    if !file_diff.hunks.is_empty() {
        let padding_bottom = get_virtual_file_padding_bottom(&options.metrics);
        split_height += padding_bottom;
        unified_height += padding_bottom;
    }

    Ok(EstimatedDiffHeights {
        split_height,
        unified_height,
    })
}

pub async fn load_diff_view(
    repo_root: &Path,
    file: &FileEntry,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    load_diff_view_for_working_tree(repo_root, file, highlight_registry).await
}

pub async fn load_diff_view_for_working_tree(
    repo_root: &Path,
    file: &FileEntry,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    let preview = load_diff_preview_for_working_tree(repo_root, file, true).await?;
    build_diff_view_from_preview_data(&preview, file, highlight_registry)
}

pub async fn load_diff_view_for_commit_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &CommitCompareSelection,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    let preview = load_diff_preview_for_commit_compare(repo_root, file, selection, true).await?;
    build_diff_view_from_preview_data(&preview, file, highlight_registry)
}

pub async fn load_diff_view_for_branch_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &BranchCompareSelection,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    let preview = load_diff_preview_for_branch_compare(repo_root, file, selection, true).await?;
    build_diff_view_from_preview_data(&preview, file, highlight_registry)
}

pub async fn load_diff_preview_for_working_tree(
    repo_root: &Path,
    file: &FileEntry,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    load_file_preview(repo_root, file, include_exact_context).await
}

pub async fn load_diff_preview_for_commit_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &CommitCompareSelection,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    load_commit_preview(repo_root, file, selection, include_exact_context).await
}

pub async fn load_diff_preview_for_branch_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &BranchCompareSelection,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    load_branch_preview(repo_root, file, selection, include_exact_context).await
}

pub fn build_diff_view_from_preview_data(
    preview: &DiffPreviewData,
    file: &FileEntry,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    if let Some(merge_conflict) = &preview.merge_conflict {
        let mut diff_view =
            build_merge_conflict_diff_view(merge_conflict, file.filetype, preview.note.clone());
        if let Some(registry) = highlight_registry {
            diff_view.apply_syntax_highlighting(file.filetype, registry);
        }
        return Ok(diff_view);
    }

    if preview.diff.trim().is_empty() {
        let message = preview
            .note
            .clone()
            .unwrap_or_else(|| "No textual diff available.".to_string());
        return Ok(DiffView::empty(message));
    }

    let (rows, hunks, gaps) = build_diff_rows(&preview.diff, file.filetype);
    let mut diff_view = DiffView {
        rows,
        note: preview.note.clone(),
        hunks,
        gaps,
        gap_expansions: HashMap::new(),
        old_file_source: preview.old_file_source.clone(),
        old_exact_highlighted_lines: None,
        new_file_lines: preview.new_file_lines.clone(),
        new_file_source: preview.new_file_source.clone(),
        new_exact_highlighted_lines: None,
        display_cache: DiffDisplayCache::default(),
    };

    if let Some(registry) = highlight_registry {
        diff_view.apply_syntax_highlighting(file.filetype, registry);
    }

    Ok(diff_view)
}

pub fn build_merge_conflict_diff_view(
    merge_conflict: &ParseMergeConflictDiffFromFileResult,
    _filetype: Option<&'static str>,
    note: Option<String>,
) -> DiffView {
    let mut rows = Vec::new();
    let mut hunks = Vec::new();
    append_file_diff_rows_with_conflicts(
        &merge_conflict.file_diff,
        &merge_conflict.actions,
        &merge_conflict.marker_rows,
        &mut rows,
        &mut hunks,
    );

    DiffView {
        rows,
        note,
        hunks,
        gaps: Vec::new(),
        gap_expansions: HashMap::new(),
        old_file_source: None,
        old_exact_highlighted_lines: None,
        new_file_lines: None,
        new_file_source: None,
        new_exact_highlighted_lines: None,
        display_cache: DiffDisplayCache::default(),
    }
}

#[inline]
pub fn build_diff_view_from_diff_text(diff: &str, filetype: Option<&'static str>) -> DiffView {
    build_diff_view_from_diff_text_with_context(diff, filetype, None, None)
}

#[inline]
pub fn build_diff_view_from_diff_text_with_context(
    diff: &str,
    filetype: Option<&'static str>,
    old_file_lines: Option<Vec<String>>,
    new_file_lines: Option<Vec<String>>,
) -> DiffView {
    if diff.trim().is_empty() {
        return DiffView::empty("No textual diff available.");
    }

    let (rows, hunks, gaps) = build_diff_rows(diff, filetype);
    let old_file_source = old_file_lines.as_deref().map(source_from_lines);
    let new_file_source = new_file_lines.as_deref().map(source_from_lines);
    DiffView {
        rows,
        note: None,
        hunks,
        gaps,
        gap_expansions: HashMap::new(),
        old_file_source,
        old_exact_highlighted_lines: None,
        new_file_lines,
        new_file_source,
        new_exact_highlighted_lines: None,
        display_cache: DiffDisplayCache::default(),
    }
}

#[derive(Debug, Clone)]
pub struct DiffPreviewData {
    diff: String,
    note: Option<String>,
    old_file_source: Option<Arc<str>>,
    new_file_lines: Option<Vec<String>>,
    new_file_source: Option<Arc<str>>,
    merge_conflict: Option<ParseMergeConflictDiffFromFileResult>,
}

impl DiffPreviewData {
    fn from_sources(
        diff: String,
        note: Option<String>,
        old_file_lines: Option<Vec<String>>,
        new_file_lines: Option<Vec<String>>,
    ) -> Self {
        let old_file_source = old_file_lines.as_deref().map(source_from_lines);
        let new_file_source = new_file_lines.as_deref().map(source_from_lines);
        Self {
            diff,
            note,
            old_file_source,
            new_file_lines,
            new_file_source,
            merge_conflict: None,
        }
    }

    fn from_merge_conflict(merge_conflict: ParseMergeConflictDiffFromFileResult) -> Self {
        Self {
            diff: String::new(),
            note: Some("Merge conflict: use 1 current, 2 incoming, 3 both.".to_string()),
            old_file_source: None,
            new_file_lines: None,
            new_file_source: None,
            merge_conflict: Some(merge_conflict),
        }
    }
}

#[derive(Clone, Copy)]
enum PreviewTarget<'a> {
    Revision(&'a str),
    WorkingTree,
}

async fn load_file_preview(
    repo_root: &Path,
    file: &FileEntry,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    if let Some(preview) = load_merge_conflict_preview(repo_root, file).await? {
        return Ok(preview);
    }

    if file.status == "??" {
        load_untracked_preview(repo_root, &file.path, include_exact_context).await
    } else {
        load_tracked_preview(repo_root, &file.path, include_exact_context).await
    }
}

async fn load_merge_conflict_preview(
    repo_root: &Path,
    file: &FileEntry,
) -> color_eyre::Result<Option<DiffPreviewData>> {
    let full_path = repo_root.join(&file.path);
    let bytes = match fs::read(full_path).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    if bytes.contains(&0) {
        return Ok(None);
    }
    let contents = String::from_utf8_lossy(&bytes).into_owned();
    if !(contents.contains("<<<<<<<")
        && contents.contains("=======")
        && contents.contains(">>>>>>>"))
    {
        return Ok(None);
    }

    let parsed = parse_merge_conflict_diff_from_file(
        &FileContents {
            name: file.path.clone(),
            contents,
            lang: file.filetype.map(str::to_string),
            header: None,
            cache_key: Some(format!("{}:{}:merge-conflict", file.path, file.status)),
        },
        6,
    )?;

    if parsed.actions.iter().all(Option::is_none) {
        return Ok(None);
    }

    Ok(Some(DiffPreviewData::from_merge_conflict(parsed)))
}

async fn load_commit_preview(
    repo_root: &Path,
    file: &FileEntry,
    selection: &CommitCompareSelection,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    load_revision_preview(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            selection.base_ref.as_str(),
            selection.commit_hash.as_str(),
            "--",
            file.path.as_str(),
        ],
        Some(PreviewTarget::Revision(selection.base_ref.as_str())),
        Some(PreviewTarget::Revision(selection.commit_hash.as_str())),
        file.path.as_str(),
        include_exact_context,
    )
    .await
}

async fn load_branch_preview(
    repo_root: &Path,
    file: &FileEntry,
    selection: &BranchCompareSelection,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    let diff_range = build_branch_diff_range(selection);
    let merge_base = resolve_branch_compare_base(repo_root, selection).await?;
    load_revision_preview(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            diff_range.as_str(),
            "--",
            file.path.as_str(),
        ],
        Some(PreviewTarget::Revision(merge_base.as_str())),
        Some(PreviewTarget::Revision(selection.source_ref.as_str())),
        file.path.as_str(),
        include_exact_context,
    )
    .await
}

async fn resolve_branch_compare_base(
    repo_root: &Path,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<String> {
    let merge_base = git_output(
        repo_root,
        &[
            "merge-base",
            selection.destination_ref.as_str(),
            selection.source_ref.as_str(),
        ],
    )
    .await?;
    let merge_base = merge_base.trim().to_string();
    if merge_base.is_empty() {
        return Err(eyre!(
            "failed to resolve merge base for {} and {}",
            selection.destination_ref,
            selection.source_ref
        ));
    }
    Ok(merge_base)
}

async fn load_tracked_preview(
    repo_root: &Path,
    file_path: &str,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    load_revision_preview(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            "HEAD",
            "--",
            file_path,
        ],
        Some(PreviewTarget::Revision("HEAD")),
        Some(PreviewTarget::WorkingTree),
        file_path,
        include_exact_context,
    )
    .await
}

async fn load_revision_preview(
    repo_root: &Path,
    diff_args: &[&str],
    old_target: Option<PreviewTarget<'_>>,
    new_target: Option<PreviewTarget<'_>>,
    file_path: &str,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    let diff = git_output(repo_root, diff_args).await?;
    let old_file_lines = if include_exact_context {
        load_preview_target_lines(repo_root, old_target, file_path).await?
    } else {
        None
    };
    let new_file_lines = if include_exact_context || diff_needs_context_lines(&diff) {
        load_preview_target_lines(repo_root, new_target, file_path).await?
    } else {
        None
    };

    Ok(DiffPreviewData::from_sources(
        diff,
        None,
        old_file_lines,
        new_file_lines,
    ))
}

async fn load_untracked_preview(
    repo_root: &Path,
    file_path: &str,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    let full_path = repo_root.join(file_path);
    match fs::metadata(&full_path).await {
        Ok(metadata) if metadata.is_dir() => {
            return Ok(DiffPreviewData::from_sources(
                String::new(),
                Some("Directory or symlinked directory; no preview available.".to_string()),
                None,
                None,
            ));
        }
        Ok(_) => {}
        Err(_) => {
            return Ok(DiffPreviewData::from_sources(
                String::new(),
                Some("Unable to read untracked file content.".to_string()),
                None,
                None,
            ));
        }
    };

    let bytes = match fs::read(&full_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Ok(DiffPreviewData::from_sources(
                String::new(),
                Some("Unable to read untracked file content.".to_string()),
                None,
                None,
            ));
        }
    };

    if bytes.contains(&0) {
        return Ok(DiffPreviewData::from_sources(
            String::new(),
            Some("Binary or non-text file; no preview available.".to_string()),
            None,
            None,
        ));
    }

    let content = String::from_utf8_lossy(&bytes);
    let diff = create_untracked_file_diff(file_path, &content);
    let needs_new_file_context = include_exact_context || diff_needs_context_lines(&diff);
    let normalized_content = Arc::<str>::from(content.replace("\r\n", "\n"));
    let new_file_lines = if needs_new_file_context {
        Some(split_lines_for_context(&content))
    } else {
        None
    };
    let new_file_source = needs_new_file_context.then_some(normalized_content.clone());
    Ok(if diff.trim().is_empty() {
        DiffPreviewData {
            diff,
            note: Some("Untracked empty file; no textual hunk to preview.".to_string()),
            old_file_source: None,
            new_file_lines: Some(split_lines_for_context(&content)),
            new_file_source: Some(normalized_content),
            merge_conflict: None,
        }
    } else {
        DiffPreviewData {
            diff,
            note: None,
            old_file_source: None,
            new_file_lines,
            new_file_source,
            merge_conflict: None,
        }
    })
}

async fn load_preview_target_lines(
    repo_root: &Path,
    target: Option<PreviewTarget<'_>>,
    file_path: &str,
) -> color_eyre::Result<Option<Vec<String>>> {
    match target {
        Some(PreviewTarget::Revision(revision)) => {
            load_revision_file_lines(repo_root, revision, file_path).await
        }
        Some(PreviewTarget::WorkingTree) => {
            load_working_tree_file_lines(repo_root, file_path).await
        }
        None => Ok(None),
    }
}

async fn load_working_tree_file_lines(
    repo_root: &Path,
    file_path: &str,
) -> color_eyre::Result<Option<Vec<String>>> {
    let full_path = repo_root.join(file_path);
    let bytes = match fs::read(full_path).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    if bytes.contains(&0) {
        return Ok(None);
    }

    Ok(Some(split_lines_for_context(&String::from_utf8_lossy(
        &bytes,
    ))))
}

async fn load_revision_file_lines(
    repo_root: &Path,
    revision: &str,
    file_path: &str,
) -> color_eyre::Result<Option<Vec<String>>> {
    let spec = format!("{revision}:{file_path}");
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["show", spec.as_str()])
        .output()
        .await
        .wrap_err_with(|| format!("failed to load {spec}"))?;

    if !output.status.success() {
        return Ok(None);
    }
    if output.stdout.contains(&0) {
        return Ok(None);
    }

    Ok(Some(split_lines_for_context(&String::from_utf8_lossy(
        &output.stdout,
    ))))
}

fn split_lines_for_context(content: &str) -> Vec<String> {
    let normalized = content.replace("\r\n", "\n");
    let mut lines = normalized
        .split('\n')
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if normalized.ends_with('\n') {
        let _ = lines.pop();
    }
    lines
}

fn source_from_lines(lines: &[String]) -> Arc<str> {
    let source_len = lines.iter().map(|line| line.len()).sum::<usize>();
    let mut source = String::with_capacity(source_len + lines.len().saturating_sub(1));
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            source.push('\n');
        }
        source.push_str(line);
    }
    Arc::<str>::from(source)
}

fn diff_needs_context_lines(diff: &str) -> bool {
    let mut hunk_count = 0usize;
    for line in diff.lines() {
        if line.starts_with("@@ -") {
            hunk_count += 1;
            if hunk_count > 1 {
                return true;
            }
        }
    }
    false
}

fn create_untracked_file_diff(input_path: &str, content: &str) -> String {
    let normalized = content.replace("\r\n", "\n");
    if normalized.is_empty() {
        return String::new();
    }

    let has_trailing_newline = normalized.ends_with('\n');
    let mut lines: Vec<&str> = normalized.split('\n').collect();
    if has_trailing_newline {
        let _ = lines.pop();
    }

    let line_count = lines.len();
    let hunk_header = format!("@@ -0,0 +1,{} @@", line_count);
    let mut diff_lines = vec![
        format!("diff --git a/{input_path} b/{input_path}"),
        "new file mode 100644".to_string(),
        "index 0000000..1111111".to_string(),
        "--- /dev/null".to_string(),
        format!("+++ b/{input_path}"),
        hunk_header,
    ];
    diff_lines.extend(lines.into_iter().map(|line| format!("+{}", line)));
    if !has_trailing_newline {
        diff_lines.push("\\ No newline at end of file".to_string());
    }
    diff_lines.push(String::new());
    diff_lines.join("\n")
}

fn has_commit_metadata_boundary(data: &str) -> bool {
    data.starts_with("From ") || data.contains("\nFrom ")
}

fn is_git_diff_patch(data: &str) -> bool {
    data.starts_with("diff --git") || data.contains("\ndiff --git")
}

fn split_at_line_prefix<'a>(contents: &'a str, prefix: &str) -> Vec<&'a str> {
    if contents.is_empty() {
        return vec![""];
    }

    let first_boundary_index = if contents.starts_with(prefix) {
        Some(0)
    } else {
        find_line_prefix_index(contents, prefix, 0)
    };
    let Some(first_boundary_index) = first_boundary_index else {
        return vec![contents];
    };

    let mut parts = Vec::new();
    if first_boundary_index > 0 {
        parts.push(&contents[..first_boundary_index]);
    }

    let mut start_index = first_boundary_index;
    while let Some(next_boundary_index) =
        find_line_prefix_index(contents, prefix, start_index.saturating_add(1))
    {
        parts.push(&contents[start_index..next_boundary_index]);
        start_index = next_boundary_index;
    }
    parts.push(&contents[start_index..]);
    parts
}

fn find_line_prefix_index(contents: &str, prefix: &str, from_index: usize) -> Option<usize> {
    if from_index == 0 && contents.starts_with(prefix) {
        return Some(0);
    }

    let newline_prefix = format!("\n{prefix}");
    contents[from_index..]
        .find(&newline_prefix)
        .map(|index| from_index + index + 1)
}

fn split_at_unified_file_break(contents: &str) -> Vec<&str> {
    split_at_line_boundaries(contents, is_unified_file_break)
}

fn split_at_line_boundaries<'a>(
    contents: &'a str,
    is_boundary: impl Fn(&str) -> bool,
) -> Vec<&'a str> {
    if contents.is_empty() {
        return vec![""];
    }

    let mut boundaries = Vec::new();
    let mut line_start = 0usize;
    loop {
        let line_end = contents[line_start..]
            .find('\n')
            .map(|offset| line_start + offset + 1)
            .unwrap_or(contents.len());
        if is_boundary(&contents[line_start..line_end]) {
            boundaries.push(line_start);
        }
        if line_end == contents.len() {
            break;
        }
        line_start = line_end;
    }

    let Some(&first_boundary) = boundaries.first() else {
        return vec![contents];
    };

    let mut parts = Vec::new();
    if first_boundary > 0 {
        parts.push(&contents[..first_boundary]);
    }
    for pair in boundaries.windows(2) {
        parts.push(&contents[pair[0]..pair[1]]);
    }
    parts.push(&contents[*boundaries.last().unwrap()..]);
    parts
}

fn is_unified_file_break(line: &str) -> bool {
    let trimmed_newline = line.trim_end_matches(['\r', '\n']);
    let Some(rest) = trimmed_newline.strip_prefix("---") else {
        return false;
    };
    let mut chars = rest.chars();
    chars.next().is_some_and(char::is_whitespace)
        && chars.next().is_some_and(|ch| !ch.is_whitespace())
}

fn split_with_newlines(contents: &str) -> Vec<&str> {
    if contents.is_empty() {
        return vec![""];
    }

    let mut lines = Vec::new();
    let mut start_index = 0usize;
    for (index, ch) in contents.char_indices() {
        if ch == '\n' {
            lines.push(&contents[start_index..=index]);
            start_index = index + 1;
        }
    }
    if start_index < contents.len() {
        lines.push(&contents[start_index..]);
    }
    lines
}

fn split_file_contents_owned(contents: &str) -> Vec<String> {
    if contents.is_empty() {
        return Vec::new();
    }
    split_with_newlines(contents)
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
}

fn compute_full_diff_ops(
    old_lines: &[String],
    new_lines: &[String],
    ignore_whitespace: bool,
) -> Vec<FullDiffOp> {
    let anchors = unique_ordered_line_anchors(old_lines, new_lines, ignore_whitespace);
    if anchors.is_empty() {
        return compute_myers_full_diff_ops(old_lines, new_lines, 0, 0, ignore_whitespace);
    }

    let mut ops = Vec::with_capacity(old_lines.len() + new_lines.len());
    let mut previous_old_index = 0usize;
    let mut previous_new_index = 0usize;
    for (old_index, new_index) in anchors {
        ops.extend(compute_myers_full_diff_ops(
            &old_lines[previous_old_index..old_index],
            &new_lines[previous_new_index..new_index],
            previous_old_index,
            previous_new_index,
            ignore_whitespace,
        ));
        ops.push(FullDiffOp::Equal {
            old_index,
            new_index,
        });
        previous_old_index = old_index + 1;
        previous_new_index = new_index + 1;
    }
    ops.extend(compute_myers_full_diff_ops(
        &old_lines[previous_old_index..],
        &new_lines[previous_new_index..],
        previous_old_index,
        previous_new_index,
        ignore_whitespace,
    ));
    ops
}

fn compute_myers_full_diff_ops(
    old_lines: &[String],
    new_lines: &[String],
    old_base_index: usize,
    new_base_index: usize,
    ignore_whitespace: bool,
) -> Vec<FullDiffOp> {
    let old_len = old_lines.len();
    let new_len = new_lines.len();
    if old_len == 0 {
        return (0..new_len)
            .map(|new_index| FullDiffOp::Insert {
                old_index: old_base_index,
                new_index: new_base_index + new_index,
            })
            .collect();
    }
    if new_len == 0 {
        return (0..old_len)
            .map(|old_index| FullDiffOp::Delete {
                old_index: old_base_index + old_index,
                new_index: new_base_index,
            })
            .collect();
    }

    let max_distance = old_len + new_len;
    let offset = max_distance as isize;
    let vector_len = max_distance * 2 + 3;
    let mut frontier = vec![-1isize; vector_len];
    frontier[(offset + 1) as usize] = 0;
    let mut trace = Vec::new();

    for distance in 0..=max_distance {
        let mut next_frontier = frontier.clone();
        let distance = distance as isize;
        let mut diagonal = -distance;
        while diagonal <= distance {
            let mut x = if diagonal == -distance {
                frontier[(offset + diagonal + 1) as usize]
            } else if diagonal != distance
                && frontier[(offset + diagonal - 1) as usize]
                    < frontier[(offset + diagonal + 1) as usize]
            {
                frontier[(offset + diagonal + 1) as usize]
            } else {
                frontier[(offset + diagonal - 1) as usize] + 1
            };
            let mut y = x - diagonal;

            while x >= 0
                && y >= 0
                && (x as usize) < old_len
                && (y as usize) < new_len
                && diff_lines_equal(
                    &old_lines[x as usize],
                    &new_lines[y as usize],
                    ignore_whitespace,
                )
            {
                x += 1;
                y += 1;
            }

            next_frontier[(offset + diagonal) as usize] = x;
            if x as usize >= old_len && y as usize >= new_len {
                trace.push(next_frontier);
                return backtrack_full_diff_ops(
                    &trace,
                    old_len,
                    new_len,
                    old_base_index,
                    new_base_index,
                    offset,
                );
            }
            diagonal += 2;
        }

        trace.push(next_frontier.clone());
        frontier = next_frontier;
    }

    Vec::new()
}

fn unique_ordered_line_anchors(
    old_lines: &[String],
    new_lines: &[String],
    ignore_whitespace: bool,
) -> Vec<(usize, usize)> {
    let mut old_occurrences: HashMap<String, (usize, usize)> = HashMap::new();
    for (index, line) in old_lines.iter().enumerate() {
        let entry = old_occurrences
            .entry(diff_line_key(line, ignore_whitespace))
            .or_insert((0, index));
        entry.0 += 1;
        entry.1 = index;
    }

    let mut new_occurrences: HashMap<String, (usize, usize)> = HashMap::new();
    for (index, line) in new_lines.iter().enumerate() {
        let entry = new_occurrences
            .entry(diff_line_key(line, ignore_whitespace))
            .or_insert((0, index));
        entry.0 += 1;
        entry.1 = index;
    }

    let mut candidates = old_occurrences
        .into_iter()
        .filter_map(|(line, (old_count, old_index))| {
            if old_count != 1 {
                return None;
            }
            let (new_count, new_index) = new_occurrences.get(&line).copied()?;
            (new_count == 1).then_some((old_index, new_index))
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable_by_key(|(old_index, new_index)| (*old_index, *new_index));
    longest_increasing_new_index_subsequence(&candidates)
}

fn diff_line_key(line: &str, ignore_whitespace: bool) -> String {
    if ignore_whitespace {
        line.trim().to_string()
    } else {
        line.to_string()
    }
}

fn longest_increasing_new_index_subsequence(candidates: &[(usize, usize)]) -> Vec<(usize, usize)> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut tails: Vec<usize> = Vec::new();
    let mut previous: Vec<Option<usize>> = vec![None; candidates.len()];
    for (candidate_index, &(_, new_index)) in candidates.iter().enumerate() {
        let position = tails
            .binary_search_by(|&tail_index| candidates[tail_index].1.cmp(&new_index))
            .unwrap_or_else(|position| position);
        if position > 0 {
            previous[candidate_index] = Some(tails[position - 1]);
        }
        if position == tails.len() {
            tails.push(candidate_index);
        } else {
            tails[position] = candidate_index;
        }
    }

    let mut result = Vec::with_capacity(tails.len());
    let mut current = tails.last().copied();
    while let Some(index) = current {
        result.push(candidates[index]);
        current = previous[index];
    }
    result.reverse();
    result
}

fn backtrack_full_diff_ops(
    trace: &[Vec<isize>],
    old_len: usize,
    new_len: usize,
    old_base_index: usize,
    new_base_index: usize,
    offset: isize,
) -> Vec<FullDiffOp> {
    let mut ops = Vec::with_capacity(old_len + new_len);
    let mut x = old_len as isize;
    let mut y = new_len as isize;

    for distance in (1..trace.len()).rev() {
        let previous_frontier = &trace[distance - 1];
        let diagonal = x - y;
        let distance = distance as isize;
        let previous_diagonal = if diagonal == -distance
            || (diagonal != distance
                && previous_frontier[(offset + diagonal - 1) as usize]
                    < previous_frontier[(offset + diagonal + 1) as usize])
        {
            diagonal + 1
        } else {
            diagonal - 1
        };

        let previous_x = previous_frontier[(offset + previous_diagonal) as usize];
        let previous_y = previous_x - previous_diagonal;

        while x > previous_x && y > previous_y {
            x -= 1;
            y -= 1;
            ops.push(FullDiffOp::Equal {
                old_index: old_base_index + x as usize,
                new_index: new_base_index + y as usize,
            });
        }

        if x == previous_x {
            y -= 1;
            ops.push(FullDiffOp::Insert {
                old_index: old_base_index + x as usize,
                new_index: new_base_index + y as usize,
            });
        } else {
            x -= 1;
            ops.push(FullDiffOp::Delete {
                old_index: old_base_index + x as usize,
                new_index: new_base_index + y as usize,
            });
        }
    }

    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        ops.push(FullDiffOp::Equal {
            old_index: old_base_index + x as usize,
            new_index: new_base_index + y as usize,
        });
    }
    while x > 0 {
        x -= 1;
        ops.push(FullDiffOp::Delete {
            old_index: old_base_index + x as usize,
            new_index: new_base_index,
        });
    }
    while y > 0 {
        y -= 1;
        ops.push(FullDiffOp::Insert {
            old_index: old_base_index,
            new_index: new_base_index + y as usize,
        });
    }

    ops.reverse();
    ops
}

fn diff_lines_equal(old_line: &str, new_line: &str, ignore_whitespace: bool) -> bool {
    if ignore_whitespace {
        old_line.trim() == new_line.trim()
    } else {
        old_line == new_line
    }
}

fn build_full_diff_hunks(ops: &[FullDiffOp], context_lines: usize) -> Vec<Hunk> {
    let mut changed_ranges = Vec::new();
    let mut index = 0usize;
    while index < ops.len() {
        if matches!(ops[index], FullDiffOp::Equal { .. }) {
            index += 1;
            continue;
        }
        let start = index;
        while index < ops.len() && !matches!(ops[index], FullDiffOp::Equal { .. }) {
            index += 1;
        }
        changed_ranges.push((start, index));
    }

    if changed_ranges.is_empty() {
        return Vec::new();
    }

    let mut expanded_ranges: Vec<(usize, usize)> = Vec::new();
    for (start, end) in changed_ranges {
        let expanded_start = start.saturating_sub(context_lines);
        let expanded_end = (end + context_lines).min(ops.len());
        if let Some((_, previous_end)) = expanded_ranges.last_mut() {
            if expanded_start <= *previous_end {
                *previous_end = (*previous_end).max(expanded_end);
                continue;
            }
        }
        expanded_ranges.push((expanded_start, expanded_end));
    }

    expanded_ranges
        .into_iter()
        .map(|(start, end)| build_full_diff_hunk(&ops[start..end]))
        .collect()
}

fn build_full_diff_hunk(ops: &[FullDiffOp]) -> Hunk {
    let old_start_index = ops.iter().find_map(full_diff_old_index).unwrap_or(0);
    let new_start_index = ops.iter().find_map(full_diff_new_index).unwrap_or(0);
    let mut hunk = Hunk {
        collapsed_before: 0,
        split_line_count: 0,
        split_line_start: 0,
        unified_line_count: 0,
        unified_line_start: 0,
        addition_count: 0,
        addition_start: full_diff_start_line_number(new_start_index, ops, true),
        addition_lines: 0,
        addition_line_index: new_start_index,
        deletion_count: 0,
        deletion_start: full_diff_start_line_number(old_start_index, ops, false),
        deletion_lines: 0,
        deletion_line_index: old_start_index,
        hunk_content: Vec::new(),
        hunk_context: None,
        hunk_specs: String::new(),
        no_eof_cr_additions: false,
        no_eof_cr_deletions: false,
    };

    let mut current_content_index = None;
    for op in ops {
        match *op {
            FullDiffOp::Equal {
                old_index,
                new_index,
            } => {
                let index = ensure_context_group(
                    &mut hunk.hunk_content,
                    &mut current_content_index,
                    old_index,
                    new_index,
                );
                if let HunkContent::Context { lines, .. } = &mut hunk.hunk_content[index] {
                    *lines += 1;
                }
                hunk.addition_count += 1;
                hunk.deletion_count += 1;
            }
            FullDiffOp::Delete {
                old_index,
                new_index,
            } => {
                let index = ensure_change_group(
                    &mut hunk.hunk_content,
                    &mut current_content_index,
                    old_index,
                    new_index,
                );
                if let HunkContent::Change { deletions, .. } = &mut hunk.hunk_content[index] {
                    *deletions += 1;
                }
                hunk.deletion_count += 1;
                hunk.deletion_lines += 1;
            }
            FullDiffOp::Insert {
                old_index,
                new_index,
            } => {
                let index = ensure_change_group(
                    &mut hunk.hunk_content,
                    &mut current_content_index,
                    old_index,
                    new_index,
                );
                if let HunkContent::Change { additions, .. } = &mut hunk.hunk_content[index] {
                    *additions += 1;
                }
                hunk.addition_count += 1;
                hunk.addition_lines += 1;
            }
        }
    }

    hunk.hunk_specs = format!(
        "@@ -{},{} +{},{} @@",
        hunk.deletion_start, hunk.deletion_count, hunk.addition_start, hunk.addition_count
    );
    for content in &hunk.hunk_content {
        match content {
            HunkContent::Context { lines, .. } => {
                hunk.split_line_count += *lines;
                hunk.unified_line_count += *lines;
            }
            HunkContent::Change {
                additions,
                deletions,
                ..
            } => {
                hunk.split_line_count += (*additions).max(*deletions);
                hunk.unified_line_count += *additions + *deletions;
            }
        }
    }
    hunk
}

fn collect_all_diff_lines(
    diff: &FileDiffMetadata,
    options: DiffIterationOptions<'_>,
    lines: &mut Vec<DiffLine>,
) -> color_eyre::Result<()> {
    let Some(final_hunk_index) = diff.hunks.len().checked_sub(1) else {
        return Ok(());
    };

    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
        let leading_region = get_expanded_region(
            diff.is_partial,
            hunk.collapsed_before,
            options.expanded_hunks,
            hunk_index,
            options.collapsed_context_threshold,
        );
        let trailing_region = if hunk_index == final_hunk_index && has_final_collapsed_hunk(diff) {
            Some(get_expanded_region(
                diff.is_partial,
                get_trailing_range_size(diff, hunk)?,
                options.expanded_hunks,
                diff.hunks.len(),
                options.collapsed_context_threshold,
            ))
        } else {
            None
        };
        let mut pending_collapsed = leading_region.collapsed_lines;

        emit_expanded_region_start(lines, hunk_index, hunk, &leading_region, options.diff_style);
        emit_expanded_region_end(
            lines,
            hunk_index,
            hunk,
            &leading_region,
            options.diff_style,
            &mut pending_collapsed,
        );

        let last_content_index = hunk.hunk_content.len().saturating_sub(1);
        for (content_index, content) in hunk.hunk_content.iter().enumerate() {
            let is_last_content = content_index == last_content_index;
            let collapsed_before = take_pending_collapsed(&mut pending_collapsed);
            let collapsed_after = if is_last_content {
                trailing_region
                    .filter(|region| region.from_start + region.from_end == 0)
                    .map(|region| region.collapsed_lines)
                    .unwrap_or(0)
            } else {
                0
            };

            match *content {
                HunkContent::Context {
                    lines: context_lines,
                    addition_line_index,
                    deletion_line_index,
                } => emit_context_diff_lines(
                    lines,
                    hunk_index,
                    hunk,
                    options.diff_style,
                    context_lines,
                    addition_line_index,
                    deletion_line_index,
                    is_last_content,
                    collapsed_before,
                    collapsed_after,
                ),
                HunkContent::Change {
                    deletions,
                    deletion_line_index,
                    additions,
                    addition_line_index,
                } => emit_change_diff_lines(
                    lines,
                    hunk_index,
                    hunk,
                    options.diff_style,
                    deletions,
                    deletion_line_index,
                    additions,
                    addition_line_index,
                    is_last_content,
                    collapsed_before,
                    collapsed_after,
                ),
            }
        }

        if let Some(trailing_region) = trailing_region {
            emit_trailing_expanded_region(lines, diff, hunk, &trailing_region, options.diff_style)?;
        }
    }

    Ok(())
}

fn resolve_diff_region(
    diff: &FileDiffMetadata,
    hunk_index: usize,
    start_content_index: usize,
    end_content_index: usize,
    resolution: NormalizedDiffResolution,
) -> color_eyre::Result<FileDiffMetadata> {
    resolve_diff_region_with_deleted_indexes(
        diff,
        hunk_index,
        start_content_index,
        end_content_index,
        resolution,
        &HashSet::new(),
    )
}

fn resolve_diff_region_with_deleted_indexes(
    diff: &FileDiffMetadata,
    hunk_index: usize,
    start_content_index: usize,
    end_content_index: usize,
    resolution: NormalizedDiffResolution,
    indexes_to_delete: &HashSet<usize>,
) -> color_eyre::Result<FileDiffMetadata> {
    let current_hunk = diff
        .hunks
        .get(hunk_index)
        .ok_or_else(|| eyre!("resolveRegion: Invalid hunk index: {hunk_index}"))?;
    if start_content_index > end_content_index
        || end_content_index >= current_hunk.hunk_content.len()
    {
        return Err(eyre!(
            "resolveRegion: Invalid content range, {start_content_index}, {end_content_index}"
        ));
    }

    let mut resolved_diff = FileDiffMetadata {
        hunks: Vec::with_capacity(diff.hunks.len()),
        deletion_lines: Vec::new(),
        addition_lines: Vec::new(),
        split_line_count: 0,
        unified_line_count: 0,
        cache_key: diff.cache_key.as_ref().map(|cache_key| {
            format!(
                "{cache_key}:{}-{hunk_index}:{start_content_index}-{end_content_index}",
                resolution_cache_key_prefix(resolution)
            )
        }),
        ..diff.clone()
    };

    let mut cursor = ResolveCursor {
        next_addition_start: 1,
        next_deletion_start: 1,
        ..ResolveCursor::default()
    };
    let updates_eof_state = hunk_index == diff.hunks.len().saturating_sub(1)
        && end_content_index == current_hunk.hunk_content.len().saturating_sub(1);
    let should_process_collapsed_context = !diff.is_partial;

    for (index, hunk) in diff.hunks.iter().enumerate() {
        process_resolved_collapsed_context(
            diff,
            &mut resolved_diff,
            &mut cursor,
            hunk.deletion_line_index
                .saturating_sub(hunk.collapsed_before),
            hunk.addition_line_index
                .saturating_sub(hunk.collapsed_before),
            hunk.collapsed_before,
            should_process_collapsed_context,
        )?;

        let mut new_hunk = Hunk {
            hunk_content: Vec::new(),
            addition_start: cursor.next_addition_start,
            deletion_start: cursor.next_deletion_start,
            addition_line_index: cursor.next_addition_line_index,
            deletion_line_index: cursor.next_deletion_line_index,
            addition_count: 0,
            deletion_count: 0,
            deletion_lines: 0,
            addition_lines: 0,
            split_line_start: cursor.split_line_count,
            unified_line_start: cursor.unified_line_count,
            split_line_count: 0,
            unified_line_count: 0,
            ..hunk.clone()
        };

        for (content_index, content) in hunk.hunk_content.iter().enumerate() {
            if index != hunk_index
                || content_index < start_content_index
                || content_index > end_content_index
            {
                push_content_lines_to_diff(
                    content,
                    &mut resolved_diff,
                    &diff.deletion_lines,
                    &diff.addition_lines,
                )?;
                let new_content = reindex_hunk_content(
                    content,
                    cursor.next_deletion_line_index,
                    cursor.next_addition_line_index,
                );
                new_hunk.hunk_content.push(new_content.clone());
                advance_resolve_cursor(&new_content, &mut cursor, &mut new_hunk);
            } else if indexes_to_delete.contains(&content_index) {
                new_hunk.hunk_content.push(HunkContent::Context {
                    lines: 0,
                    deletion_line_index: cursor.next_deletion_line_index,
                    addition_line_index: cursor.next_addition_line_index,
                });
            } else if let HunkContent::Context { lines, .. } = content {
                push_content_lines_to_diff(
                    content,
                    &mut resolved_diff,
                    &diff.deletion_lines,
                    &diff.addition_lines,
                )?;
                let new_content = HunkContent::Context {
                    lines: *lines,
                    deletion_line_index: cursor.next_deletion_line_index,
                    addition_line_index: cursor.next_addition_line_index,
                };
                new_hunk.hunk_content.push(new_content.clone());
                advance_resolve_cursor(&new_content, &mut cursor, &mut new_hunk);
            } else if let HunkContent::Change {
                deletions,
                deletion_line_index,
                additions,
                addition_line_index,
            } = *content
            {
                push_resolve_lines_to_diff(
                    resolution,
                    deletions,
                    deletion_line_index,
                    additions,
                    addition_line_index,
                    &mut resolved_diff,
                    &diff.deletion_lines,
                    &diff.addition_lines,
                )?;
                let lines = match resolution {
                    NormalizedDiffResolution::Deletions => deletions,
                    NormalizedDiffResolution::Additions => additions,
                    NormalizedDiffResolution::Both => deletions + additions,
                };
                let new_content = HunkContent::Context {
                    lines,
                    deletion_line_index: cursor.next_deletion_line_index,
                    addition_line_index: cursor.next_addition_line_index,
                };
                new_hunk.hunk_content.push(new_content.clone());
                advance_resolve_cursor(&new_content, &mut cursor, &mut new_hunk);
            }
        }

        if index == hunk_index && updates_eof_state {
            let no_eof_cr = if resolution == NormalizedDiffResolution::Deletions {
                hunk.no_eof_cr_deletions
            } else {
                hunk.no_eof_cr_additions
            };
            new_hunk.no_eof_cr_additions = no_eof_cr;
            new_hunk.no_eof_cr_deletions = no_eof_cr;
        }

        resolved_diff.hunks.push(new_hunk);
    }

    if let Some(final_hunk) = diff.hunks.last().filter(|_| !diff.is_partial) {
        let deletion_start = final_hunk.deletion_line_index + final_hunk.deletion_count;
        let addition_start = final_hunk.addition_line_index + final_hunk.addition_count;
        let line_count = diff
            .deletion_lines
            .len()
            .saturating_sub(deletion_start)
            .min(diff.addition_lines.len().saturating_sub(addition_start));
        push_resolved_collapsed_context_lines(
            &mut resolved_diff,
            &diff.deletion_lines,
            &diff.addition_lines,
            deletion_start,
            addition_start,
            line_count,
        )?;
    }

    resolved_diff.split_line_count = cursor.split_line_count;
    resolved_diff.unified_line_count = cursor.unified_line_count;
    Ok(resolved_diff)
}

fn normalize_diff_resolution(resolution: DiffHunkResolution) -> NormalizedDiffResolution {
    match resolution {
        DiffHunkResolution::Accept
        | DiffHunkResolution::Incoming
        | DiffHunkResolution::Additions => NormalizedDiffResolution::Additions,
        DiffHunkResolution::Reject
        | DiffHunkResolution::Current
        | DiffHunkResolution::Deletions => NormalizedDiffResolution::Deletions,
        DiffHunkResolution::Both => NormalizedDiffResolution::Both,
    }
}

fn normalize_merge_conflict_resolution(
    resolution: MergeConflictResolution,
) -> NormalizedDiffResolution {
    match resolution {
        MergeConflictResolution::Current => NormalizedDiffResolution::Deletions,
        MergeConflictResolution::Incoming => NormalizedDiffResolution::Additions,
        MergeConflictResolution::Both => NormalizedDiffResolution::Both,
    }
}

fn resolution_cache_key_prefix(resolution: NormalizedDiffResolution) -> char {
    match resolution {
        NormalizedDiffResolution::Deletions => 'd',
        NormalizedDiffResolution::Additions => 'a',
        NormalizedDiffResolution::Both => 'b',
    }
}

fn process_resolved_collapsed_context(
    source_diff: &FileDiffMetadata,
    resolved_diff: &mut FileDiffMetadata,
    cursor: &mut ResolveCursor,
    deletion_line_index: usize,
    addition_line_index: usize,
    line_count: usize,
    should_process_content: bool,
) -> color_eyre::Result<()> {
    if line_count == 0 {
        return Ok(());
    }

    if should_process_content {
        push_resolved_collapsed_context_lines(
            resolved_diff,
            &source_diff.deletion_lines,
            &source_diff.addition_lines,
            deletion_line_index,
            addition_line_index,
            line_count,
        )?;
        cursor.next_addition_line_index += line_count;
        cursor.next_deletion_line_index += line_count;
    }

    cursor.next_addition_start += line_count;
    cursor.next_deletion_start += line_count;
    cursor.split_line_count += line_count;
    cursor.unified_line_count += line_count;
    Ok(())
}

fn push_resolved_collapsed_context_lines(
    diff: &mut FileDiffMetadata,
    deletion_lines: &[String],
    addition_lines: &[String],
    deletion_line_index: usize,
    addition_line_index: usize,
    line_count: usize,
) -> color_eyre::Result<()> {
    for index in 0..line_count {
        let deletion_line = deletion_lines
            .get(deletion_line_index + index)
            .ok_or_else(|| eyre!("pushCollapsedContextLines: missing collapsed context line"))?;
        let addition_line = addition_lines
            .get(addition_line_index + index)
            .ok_or_else(|| eyre!("pushCollapsedContextLines: missing collapsed context line"))?;
        diff.deletion_lines.push(deletion_line.clone());
        diff.addition_lines.push(addition_line.clone());
    }
    Ok(())
}

fn push_content_lines_to_diff(
    content: &HunkContent,
    diff: &mut FileDiffMetadata,
    deletion_lines: &[String],
    addition_lines: &[String],
) -> color_eyre::Result<()> {
    match *content {
        HunkContent::Context {
            lines,
            addition_line_index,
            ..
        } => {
            for index in 0..lines {
                let line = addition_lines
                    .get(addition_line_index + index)
                    .ok_or_else(|| eyre!("pushContentLinesToDiff: Context line does not exist"))?;
                diff.deletion_lines.push(line.clone());
                diff.addition_lines.push(line.clone());
            }
        }
        HunkContent::Change {
            deletions,
            deletion_line_index,
            additions,
            addition_line_index,
        } => {
            for index in 0..deletions.max(additions) {
                if index < deletions {
                    let line =
                        deletion_lines
                            .get(deletion_line_index + index)
                            .ok_or_else(|| {
                                eyre!("pushContentLinesToDiff: Deletion line does not exist")
                            })?;
                    diff.deletion_lines.push(line.clone());
                }
                if index < additions {
                    let line =
                        addition_lines
                            .get(addition_line_index + index)
                            .ok_or_else(|| {
                                eyre!("pushContentLinesToDiff: Addition line does not exist")
                            })?;
                    diff.addition_lines.push(line.clone());
                }
            }
        }
    }
    Ok(())
}

fn push_resolve_lines_to_diff(
    resolution: NormalizedDiffResolution,
    deletions: usize,
    deletion_line_index: usize,
    additions: usize,
    addition_line_index: usize,
    diff: &mut FileDiffMetadata,
    deletion_lines: &[String],
    addition_lines: &[String],
) -> color_eyre::Result<()> {
    if matches!(
        resolution,
        NormalizedDiffResolution::Deletions | NormalizedDiffResolution::Both
    ) {
        for index in 0..deletions {
            let line = deletion_lines
                .get(deletion_line_index + index)
                .ok_or_else(|| eyre!("pushResolveLinesToDiff: Deletion line does not exist"))?;
            diff.deletion_lines.push(line.clone());
            diff.addition_lines.push(line.clone());
        }
    }
    if matches!(
        resolution,
        NormalizedDiffResolution::Additions | NormalizedDiffResolution::Both
    ) {
        for index in 0..additions {
            let line = addition_lines
                .get(addition_line_index + index)
                .ok_or_else(|| eyre!("pushResolveLinesToDiff: Addition line does not exist"))?;
            diff.deletion_lines.push(line.clone());
            diff.addition_lines.push(line.clone());
        }
    }
    Ok(())
}

fn reindex_hunk_content(
    content: &HunkContent,
    deletion_line_index: usize,
    addition_line_index: usize,
) -> HunkContent {
    match *content {
        HunkContent::Context { lines, .. } => HunkContent::Context {
            lines,
            deletion_line_index,
            addition_line_index,
        },
        HunkContent::Change {
            deletions,
            additions,
            ..
        } => HunkContent::Change {
            deletions,
            deletion_line_index,
            additions,
            addition_line_index,
        },
    }
}

fn advance_resolve_cursor(content: &HunkContent, cursor: &mut ResolveCursor, hunk: &mut Hunk) {
    match *content {
        HunkContent::Context { lines, .. } => {
            cursor.next_addition_line_index += lines;
            cursor.next_deletion_line_index += lines;
            cursor.next_addition_start += lines;
            cursor.next_deletion_start += lines;
            cursor.split_line_count += lines;
            cursor.unified_line_count += lines;

            hunk.addition_count += lines;
            hunk.deletion_count += lines;
            hunk.split_line_count += lines;
            hunk.unified_line_count += lines;
        }
        HunkContent::Change {
            deletions,
            additions,
            ..
        } => {
            cursor.next_addition_line_index += additions;
            cursor.next_deletion_line_index += deletions;
            cursor.next_addition_start += additions;
            cursor.next_deletion_start += deletions;
            cursor.split_line_count += deletions.max(additions);
            cursor.unified_line_count += deletions + additions;

            hunk.deletion_count += deletions;
            hunk.deletion_lines += deletions;
            hunk.addition_count += additions;
            hunk.addition_lines += additions;
            hunk.split_line_count += deletions.max(additions);
            hunk.unified_line_count += deletions + additions;
        }
    }
}

fn emit_expanded_region_start(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    hunk: &Hunk,
    region: &ExpandedRegionResult,
    diff_style: DiffStyle,
) {
    if region.from_start == 0 {
        return;
    }

    let unified_line_index = hunk.unified_line_start.saturating_sub(region.range_size);
    let split_line_index = hunk.split_line_start.saturating_sub(region.range_size);
    let deletion_line_index = hunk.deletion_line_index.saturating_sub(region.range_size);
    let addition_line_index = hunk.addition_line_index.saturating_sub(region.range_size);
    let deletion_line_number = hunk.deletion_start.saturating_sub(region.range_size);
    let addition_line_number = hunk.addition_start.saturating_sub(region.range_size);

    for index in 0..region.from_start {
        push_context_expanded_line(
            lines,
            hunk_index,
            true,
            0,
            0,
            diff_style,
            LinePairIndices {
                unified_line_index: unified_line_index + index,
                split_line_index: split_line_index + index,
                deletion_line_index: deletion_line_index + index,
                addition_line_index: addition_line_index + index,
                deletion_line_number: deletion_line_number + index,
                addition_line_number: addition_line_number + index,
            },
        );
    }
}

fn emit_expanded_region_end(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    hunk: &Hunk,
    region: &ExpandedRegionResult,
    diff_style: DiffStyle,
    pending_collapsed: &mut usize,
) {
    if region.from_end == 0 {
        return;
    }

    let unified_line_index = hunk.unified_line_start.saturating_sub(region.from_end);
    let split_line_index = hunk.split_line_start.saturating_sub(region.from_end);
    let deletion_line_index = hunk.deletion_line_index.saturating_sub(region.from_end);
    let addition_line_index = hunk.addition_line_index.saturating_sub(region.from_end);
    let deletion_line_number = hunk.deletion_start.saturating_sub(region.from_end);
    let addition_line_number = hunk.addition_start.saturating_sub(region.from_end);

    for index in 0..region.from_end {
        push_context_expanded_line(
            lines,
            hunk_index,
            true,
            if index == 0 {
                take_pending_collapsed(pending_collapsed)
            } else {
                0
            },
            0,
            diff_style,
            LinePairIndices {
                unified_line_index: unified_line_index + index,
                split_line_index: split_line_index + index,
                deletion_line_index: deletion_line_index + index,
                addition_line_index: addition_line_index + index,
                deletion_line_number: deletion_line_number + index,
                addition_line_number: addition_line_number + index,
            },
        );
    }
}

fn emit_context_diff_lines(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    hunk: &Hunk,
    diff_style: DiffStyle,
    context_lines: usize,
    addition_line_index: usize,
    deletion_line_index: usize,
    is_last_content: bool,
    collapsed_before: usize,
    collapsed_after: usize,
) {
    let unified_offset =
        unified_offset_before_content(hunk, addition_line_index, deletion_line_index);
    let split_offset = split_offset_before_content(hunk, addition_line_index, deletion_line_index);
    for index in 0..context_lines {
        let is_last_line = is_last_content && index == context_lines.saturating_sub(1);
        push_context_line(
            lines,
            hunk_index,
            collapsed_before_for_index(collapsed_before, index),
            collapsed_after_for_index(collapsed_after, index, context_lines),
            diff_style,
            LinePairIndices {
                unified_line_index: hunk.unified_line_start + unified_offset + index,
                split_line_index: hunk.split_line_start + split_offset + index,
                deletion_line_index: deletion_line_index + index,
                addition_line_index: addition_line_index + index,
                deletion_line_number: hunk.deletion_start + deletion_line_index
                    - hunk.deletion_line_index
                    + index,
                addition_line_number: hunk.addition_start + addition_line_index
                    - hunk.addition_line_index
                    + index,
            },
            is_last_line && hunk.no_eof_cr_deletions,
            is_last_line && hunk.no_eof_cr_additions,
        );
    }
}

fn emit_change_diff_lines(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    hunk: &Hunk,
    diff_style: DiffStyle,
    deletions: usize,
    deletion_line_index: usize,
    additions: usize,
    addition_line_index: usize,
    is_last_content: bool,
    collapsed_before: usize,
    collapsed_after: usize,
) {
    let split_count = deletions.max(additions);
    let unified_start = hunk.unified_line_start
        + unified_offset_before_content(hunk, addition_line_index, deletion_line_index);
    let split_start = hunk.split_line_start
        + split_offset_before_content(hunk, addition_line_index, deletion_line_index);
    let deletion_line_number = hunk.deletion_start + deletion_line_index - hunk.deletion_line_index;
    let addition_line_number = hunk.addition_start + addition_line_index - hunk.addition_line_index;

    if diff_style == DiffStyle::Unified {
        for index in 0..deletions {
            lines.push(DiffLine {
                hunk_index,
                has_hunk: true,
                collapsed_before: collapsed_before_for_index(collapsed_before, index),
                collapsed_after: if additions == 0 {
                    collapsed_after_for_index(collapsed_after, index, deletions)
                } else {
                    0
                },
                line_type: DiffLineType::Change,
                deletion_line: Some(DiffLineMetadata {
                    unified_line_index: unified_start + index,
                    split_line_index: split_start + index,
                    line_index: deletion_line_index + index,
                    line_number: deletion_line_number + index,
                    no_eof_cr: is_last_content
                        && index == deletions.saturating_sub(1)
                        && hunk.no_eof_cr_deletions,
                }),
                addition_line: None,
            });
        }
        for index in 0..additions {
            lines.push(DiffLine {
                hunk_index,
                has_hunk: true,
                collapsed_before: collapsed_before_for_index(collapsed_before, deletions + index),
                collapsed_after: collapsed_after_for_index(collapsed_after, index, additions),
                line_type: DiffLineType::Change,
                deletion_line: None,
                addition_line: Some(DiffLineMetadata {
                    unified_line_index: unified_start + deletions + index,
                    split_line_index: split_start + index,
                    line_index: addition_line_index + index,
                    line_number: addition_line_number + index,
                    no_eof_cr: is_last_content
                        && index == additions.saturating_sub(1)
                        && hunk.no_eof_cr_additions,
                }),
            });
        }
        return;
    }

    for index in 0..split_count {
        let deletion_line = (index < deletions).then(|| DiffLineMetadata {
            unified_line_index: unified_start + index,
            split_line_index: split_start + index,
            line_index: deletion_line_index + index,
            line_number: deletion_line_number + index,
            no_eof_cr: is_last_content
                && index == split_count.saturating_sub(1)
                && hunk.no_eof_cr_deletions,
        });
        let addition_line = (index < additions).then(|| DiffLineMetadata {
            unified_line_index: unified_start + deletions + index,
            split_line_index: split_start + index,
            line_index: addition_line_index + index,
            line_number: addition_line_number + index,
            no_eof_cr: is_last_content
                && index == split_count.saturating_sub(1)
                && hunk.no_eof_cr_additions,
        });
        lines.push(DiffLine {
            hunk_index,
            has_hunk: true,
            collapsed_before: collapsed_before_for_index(collapsed_before, index),
            collapsed_after: collapsed_after_for_index(collapsed_after, index, split_count),
            line_type: DiffLineType::Change,
            deletion_line,
            addition_line,
        });
    }
}

fn emit_trailing_expanded_region(
    lines: &mut Vec<DiffLine>,
    diff: &FileDiffMetadata,
    hunk: &Hunk,
    region: &ExpandedRegionResult,
    diff_style: DiffStyle,
) -> color_eyre::Result<()> {
    let len = region.from_start + region.from_end;
    if len == 0 {
        return Ok(());
    }

    let deletion_start = hunk.deletion_line_index + hunk.deletion_count;
    let addition_start = hunk.addition_line_index + hunk.addition_count;
    if deletion_start + len > diff.deletion_lines.len()
        || addition_start + len > diff.addition_lines.len()
    {
        return Err(eyre!(
            "iterateOverDiff: trailing context out of bounds for {}",
            diff.name
        ));
    }

    for index in 0..len {
        push_context_expanded_line(
            lines,
            diff.hunks.len(),
            false,
            0,
            if index == len - 1 {
                region.collapsed_lines
            } else {
                0
            },
            diff_style,
            LinePairIndices {
                unified_line_index: hunk.unified_line_start + hunk.unified_line_count + index,
                split_line_index: hunk.split_line_start + hunk.split_line_count + index,
                deletion_line_index: deletion_start + index,
                addition_line_index: addition_start + index,
                deletion_line_number: hunk.deletion_start + hunk.deletion_count + index,
                addition_line_number: hunk.addition_start + hunk.addition_count + index,
            },
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct LinePairIndices {
    unified_line_index: usize,
    split_line_index: usize,
    deletion_line_index: usize,
    addition_line_index: usize,
    deletion_line_number: usize,
    addition_line_number: usize,
}

fn push_context_line(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    collapsed_before: usize,
    collapsed_after: usize,
    diff_style: DiffStyle,
    indices: LinePairIndices,
    no_eof_cr_deletions: bool,
    no_eof_cr_additions: bool,
) {
    push_paired_line(
        lines,
        hunk_index,
        true,
        DiffLineType::Context,
        collapsed_before,
        collapsed_after,
        diff_style,
        indices,
        no_eof_cr_deletions,
        no_eof_cr_additions,
    );
}

fn push_context_expanded_line(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    has_hunk: bool,
    collapsed_before: usize,
    collapsed_after: usize,
    diff_style: DiffStyle,
    indices: LinePairIndices,
) {
    push_paired_line(
        lines,
        hunk_index,
        has_hunk,
        DiffLineType::ContextExpanded,
        collapsed_before,
        collapsed_after,
        diff_style,
        indices,
        false,
        false,
    );
}

fn push_paired_line(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    has_hunk: bool,
    line_type: DiffLineType,
    collapsed_before: usize,
    collapsed_after: usize,
    _diff_style: DiffStyle,
    indices: LinePairIndices,
    no_eof_cr_deletions: bool,
    no_eof_cr_additions: bool,
) {
    lines.push(DiffLine {
        hunk_index,
        has_hunk,
        collapsed_before,
        collapsed_after,
        line_type,
        deletion_line: Some(DiffLineMetadata {
            unified_line_index: indices.unified_line_index,
            split_line_index: indices.split_line_index,
            line_index: indices.deletion_line_index,
            line_number: indices.deletion_line_number,
            no_eof_cr: no_eof_cr_deletions,
        }),
        addition_line: Some(DiffLineMetadata {
            unified_line_index: indices.unified_line_index,
            split_line_index: indices.split_line_index,
            line_index: indices.addition_line_index,
            line_number: indices.addition_line_number,
            no_eof_cr: no_eof_cr_additions,
        }),
    });
}

fn get_expanded_region(
    is_partial: bool,
    range_size: usize,
    expanded_hunks: Option<ExpandedHunks<'_>>,
    hunk_index: usize,
    collapsed_context_threshold: usize,
) -> ExpandedRegionResult {
    if range_size == 0 || is_partial {
        return ExpandedRegionResult {
            from_start: 0,
            from_end: 0,
            range_size,
            collapsed_lines: range_size,
        };
    }

    if expanded_hunks == Some(ExpandedHunks::All) || range_size <= collapsed_context_threshold {
        return ExpandedRegionResult {
            from_start: range_size,
            from_end: 0,
            range_size,
            collapsed_lines: 0,
        };
    }

    let region = match expanded_hunks {
        Some(ExpandedHunks::Regions(regions)) => regions.get(&hunk_index).copied(),
        _ => None,
    };
    let from_start = region
        .map(|region| region.from_start.min(range_size))
        .unwrap_or(0);
    let from_end = region
        .map(|region| region.from_end.min(range_size))
        .unwrap_or(0);
    let expanded_count = from_start + from_end;
    if expanded_count >= range_size {
        return ExpandedRegionResult {
            from_start: range_size,
            from_end: 0,
            range_size,
            collapsed_lines: 0,
        };
    }

    ExpandedRegionResult {
        from_start,
        from_end,
        range_size,
        collapsed_lines: range_size - expanded_count,
    }
}

fn has_final_collapsed_hunk(diff: &FileDiffMetadata) -> bool {
    let Some(last_hunk) = diff.hunks.last() else {
        return false;
    };
    if diff.is_partial || diff.addition_lines.is_empty() || diff.deletion_lines.is_empty() {
        return false;
    }
    last_hunk.addition_line_index + last_hunk.addition_count < diff.addition_lines.len()
        || last_hunk.deletion_line_index + last_hunk.deletion_count < diff.deletion_lines.len()
}

fn no_newline_metadata_line_counts(hunk: &Hunk) -> (usize, usize) {
    if !hunk.no_eof_cr_additions && !hunk.no_eof_cr_deletions {
        return (0, 0);
    }

    let Some(last_content) = hunk.hunk_content.last() else {
        return (0, 0);
    };

    match *last_content {
        HunkContent::Context { lines, .. } => {
            let metadata_rows = usize::from(lines > 0);
            (metadata_rows, metadata_rows)
        }
        HunkContent::Change {
            deletions,
            additions,
            ..
        } => {
            let unified = usize::from(deletions > 0 && hunk.no_eof_cr_deletions)
                + usize::from(additions > 0 && hunk.no_eof_cr_additions);
            let split = usize::from(
                (deletions > 0 && hunk.no_eof_cr_deletions)
                    || (additions > 0 && hunk.no_eof_cr_additions),
            );
            (split, unified)
        }
    }
}

fn get_trailing_range_size(diff: &FileDiffMetadata, hunk: &Hunk) -> color_eyre::Result<usize> {
    let addition_remaining = diff
        .addition_lines
        .len()
        .saturating_sub(hunk.addition_line_index + hunk.addition_count);
    let deletion_remaining = diff
        .deletion_lines
        .len()
        .saturating_sub(hunk.deletion_line_index + hunk.deletion_count);
    if addition_remaining != deletion_remaining {
        return Err(eyre!(
            "iterateOverDiff: trailing context mismatch (additions={}, deletions={}) for {}",
            addition_remaining,
            deletion_remaining,
            diff.name
        ));
    }
    Ok(addition_remaining.min(deletion_remaining))
}

fn take_pending_collapsed(value: &mut usize) -> usize {
    let pending = *value;
    *value = 0;
    pending
}

fn collapsed_before_for_index(collapsed_before: usize, index: usize) -> usize {
    if index == 0 { collapsed_before } else { 0 }
}

fn collapsed_after_for_index(collapsed_after: usize, index: usize, len: usize) -> usize {
    if len > 0 && index == len - 1 {
        collapsed_after
    } else {
        0
    }
}

fn unified_offset_before_content(
    hunk: &Hunk,
    addition_line_index: usize,
    deletion_line_index: usize,
) -> usize {
    let mut offset = 0usize;
    for content in &hunk.hunk_content {
        if hunk_content_starts_at(content, addition_line_index, deletion_line_index) {
            break;
        }
        match *content {
            HunkContent::Context { lines, .. } => offset += lines,
            HunkContent::Change {
                additions,
                deletions,
                ..
            } => offset += additions + deletions,
        }
    }
    offset
}

fn split_offset_before_content(
    hunk: &Hunk,
    addition_line_index: usize,
    deletion_line_index: usize,
) -> usize {
    let mut offset = 0usize;
    for content in &hunk.hunk_content {
        if hunk_content_starts_at(content, addition_line_index, deletion_line_index) {
            break;
        }
        match *content {
            HunkContent::Context { lines, .. } => offset += lines,
            HunkContent::Change {
                additions,
                deletions,
                ..
            } => offset += additions.max(deletions),
        }
    }
    offset
}

fn hunk_content_starts_at(
    content: &HunkContent,
    addition_line_index: usize,
    deletion_line_index: usize,
) -> bool {
    match *content {
        HunkContent::Context {
            addition_line_index: content_addition_line_index,
            deletion_line_index: content_deletion_line_index,
            ..
        }
        | HunkContent::Change {
            addition_line_index: content_addition_line_index,
            deletion_line_index: content_deletion_line_index,
            ..
        } => {
            content_addition_line_index == addition_line_index
                && content_deletion_line_index == deletion_line_index
        }
    }
}

fn full_diff_old_index(op: &FullDiffOp) -> Option<usize> {
    match *op {
        FullDiffOp::Equal { old_index, .. } | FullDiffOp::Delete { old_index, .. } => {
            Some(old_index)
        }
        FullDiffOp::Insert { .. } => None,
    }
}

fn full_diff_new_index(op: &FullDiffOp) -> Option<usize> {
    match *op {
        FullDiffOp::Equal { new_index, .. } | FullDiffOp::Insert { new_index, .. } => {
            Some(new_index)
        }
        FullDiffOp::Delete { .. } => None,
    }
}

fn full_diff_start_line_number(
    start_index: usize,
    ops: &[FullDiffOp],
    addition_side: bool,
) -> usize {
    let side_has_lines = ops.iter().any(|op| {
        if addition_side {
            matches!(op, FullDiffOp::Equal { .. } | FullDiffOp::Insert { .. })
        } else {
            matches!(op, FullDiffOp::Equal { .. } | FullDiffOp::Delete { .. })
        }
    });
    if side_has_lines {
        start_index + 1
    } else {
        start_index
    }
}

fn finalize_full_file_line_counts(file: &mut FileDiffMetadata) {
    let mut last_hunk_end = 0usize;
    for hunk in &mut file.hunks {
        hunk.collapsed_before = hunk
            .addition_start
            .saturating_sub(1)
            .saturating_sub(last_hunk_end);
        hunk.split_line_start = file.split_line_count + hunk.collapsed_before;
        hunk.unified_line_start = file.unified_line_count + hunk.collapsed_before;
        file.split_line_count += hunk.collapsed_before + hunk.split_line_count;
        file.unified_line_count += hunk.collapsed_before + hunk.unified_line_count;
        last_hunk_end = hunk
            .addition_start
            .saturating_add(hunk.addition_count)
            .saturating_sub(1);
    }

    if let Some(last_hunk) = file.hunks.last() {
        if !file.addition_lines.is_empty() && !file.deletion_lines.is_empty() {
            let last_hunk_end = last_hunk
                .addition_start
                .saturating_add(last_hunk.addition_count)
                .saturating_sub(1);
            let collapsed_after = file.addition_lines.len().saturating_sub(last_hunk_end);
            file.split_line_count += collapsed_after;
            file.unified_line_count += collapsed_after;
        }
    }
}

fn apply_full_diff_no_eof_flags(file: &mut FileDiffMetadata) {
    let deletion_has_no_eof_cr = file
        .deletion_lines
        .last()
        .is_some_and(|line| !line.ends_with('\n'));
    let addition_has_no_eof_cr = file
        .addition_lines
        .last()
        .is_some_and(|line| !line.ends_with('\n'));

    for hunk in &mut file.hunks {
        if deletion_has_no_eof_cr
            && hunk.deletion_line_index + hunk.deletion_count == file.deletion_lines.len()
        {
            hunk.no_eof_cr_deletions = true;
        }
        if addition_has_no_eof_cr
            && hunk.addition_line_index + hunk.addition_count == file.addition_lines.len()
        {
            hunk.no_eof_cr_additions = true;
        }
    }
}

fn parse_patch_hunk_header(line: &str) -> Option<PatchHunkHeader<'_>> {
    let line = line.strip_prefix("@@ -")?;
    let mut index = 0usize;
    let deletion_start = read_decimal(line, &mut index)?;

    let mut deletion_count = 1usize;
    if line.as_bytes().get(index) == Some(&b',') {
        index += 1;
        deletion_count = read_decimal(line, &mut index)?;
    }

    if line.as_bytes().get(index) != Some(&b' ') || line.as_bytes().get(index + 1) != Some(&b'+') {
        return None;
    }
    index += 2;

    let addition_start = read_decimal(line, &mut index)?;
    let mut addition_count = 1usize;
    if line.as_bytes().get(index) == Some(&b',') {
        index += 1;
        addition_count = read_decimal(line, &mut index)?;
    }

    if line.as_bytes().get(index) != Some(&b' ')
        || line.as_bytes().get(index + 1) != Some(&b'@')
        || line.as_bytes().get(index + 2) != Some(&b'@')
    {
        return None;
    }

    let context_start_index = index + 3;
    let hunk_context = if line.as_bytes().get(context_start_index) == Some(&b' ') {
        Some(trim_line_end(&line[context_start_index + 1..]))
    } else {
        None
    };

    Some(PatchHunkHeader {
        addition_count,
        addition_start,
        deletion_count,
        deletion_start,
        hunk_context,
    })
}

fn flush_trim_context_lines(
    hunk: &mut TrimmedPatchHunk,
    context_size: usize,
    mode: TrimContextFlushMode,
) {
    if mode == TrimContextFlushMode::Leading && hunk.context_lines.len() > context_size {
        let difference = hunk.context_lines.len() - context_size;
        hunk.context_lines.drain(0..difference);
        hunk.addition_start += difference;
        hunk.deletion_start += difference;
    }

    if mode == TrimContextFlushMode::Trailing && hunk.context_lines.len() > context_size {
        hunk.context_lines.truncate(context_size);
    }

    if !hunk.context_lines.is_empty() {
        hunk.addition_count += hunk.context_lines.len();
        hunk.deletion_count += hunk.context_lines.len();
        hunk.hunk_lines.append(&mut hunk.context_lines);
    }
}

fn flush_trim_hunk(hunk: &TrimmedPatchHunk, lines: &mut Vec<String>) {
    lines.push(format!(
        "@@ -{} +{} @@",
        format_trim_hunk_range(hunk.deletion_start, hunk.deletion_count),
        format_trim_hunk_range(hunk.addition_start, hunk.addition_count)
    ));
    lines.extend(hunk.hunk_lines.iter().cloned());
}

fn format_trim_hunk_range(start: usize, count: usize) -> String {
    if count == 1 {
        start.to_string()
    } else {
        format!("{start},{count}")
    }
}

fn read_decimal(value: &str, index: &mut usize) -> Option<usize> {
    let start = *index;
    let mut parsed = 0usize;
    for byte in value.as_bytes().iter().skip(start).copied() {
        if !byte.is_ascii_digit() {
            break;
        }
        parsed = parsed * 10 + usize::from(byte - b'0');
        *index += 1;
    }
    (*index != start).then_some(parsed)
}

fn trim_line_end(value: &str) -> &str {
    value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(value)
}

fn parse_raw_line_type(first_char: char) -> Option<ParsedRawLineType> {
    match first_char {
        ' ' => Some(ParsedRawLineType::Context),
        '+' => Some(ParsedRawLineType::Addition),
        '-' => Some(ParsedRawLineType::Deletion),
        '\\' => Some(ParsedRawLineType::Metadata),
        _ => None,
    }
}

fn get_parsed_line_content(raw_line: &str) -> String {
    let processed = raw_line.get(1..).unwrap_or("");
    if processed.is_empty() {
        "\n".to_string()
    } else {
        processed.to_string()
    }
}

fn clean_last_line(lines: &mut [String]) {
    if let Some(line) = lines.last_mut() {
        if line.ends_with("\r\n") {
            line.truncate(line.len() - 2);
        } else if line.ends_with('\n') {
            line.truncate(line.len() - 1);
        }
    }
}

fn ensure_change_group(
    content: &mut Vec<HunkContent>,
    current_content_index: &mut Option<usize>,
    deletion_line_index: usize,
    addition_line_index: usize,
) -> usize {
    if let Some(index) = *current_content_index {
        if matches!(content.get(index), Some(HunkContent::Change { .. })) {
            return index;
        }
    }

    content.push(HunkContent::Change {
        deletions: 0,
        deletion_line_index,
        additions: 0,
        addition_line_index,
    });
    let index = content.len() - 1;
    *current_content_index = Some(index);
    index
}

fn ensure_context_group(
    content: &mut Vec<HunkContent>,
    current_content_index: &mut Option<usize>,
    deletion_line_index: usize,
    addition_line_index: usize,
) -> usize {
    if let Some(index) = *current_content_index {
        if matches!(content.get(index), Some(HunkContent::Context { .. })) {
            return index;
        }
    }

    content.push(HunkContent::Context {
        lines: 0,
        addition_line_index,
        deletion_line_index,
    });
    let index = content.len() - 1;
    *current_content_index = Some(index);
    index
}

fn parse_git_file_metadata(line: &str, file: &mut FileDiffMetadata) {
    let line = trim_line_end(line);
    if let Some(mode) = line.strip_prefix("new mode ") {
        file.mode = Some(mode.trim().to_string());
    }
    if let Some(mode) = line.strip_prefix("old mode ") {
        file.prev_mode = Some(mode.trim().to_string());
    }
    if let Some(mode) = line.strip_prefix("new file mode") {
        file.change_type = ChangeType::New;
        file.mode = Some(mode.trim().to_string());
    }
    if let Some(mode) = line.strip_prefix("deleted file mode") {
        file.change_type = ChangeType::Deleted;
        file.mode = Some(mode.trim().to_string());
    }
    if let Some(rest) = line.strip_prefix("similarity index") {
        file.change_type = if rest.trim() == "100%" {
            ChangeType::RenamePure
        } else {
            ChangeType::RenameChanged
        };
    }
    if let Some(rest) = line.strip_prefix("index ") {
        let mut parts = rest.split_whitespace();
        if let Some(ids) = parts.next() {
            if let Some((prev_object_id, new_object_id)) = ids.split_once("..") {
                file.prev_object_id = Some(prev_object_id.to_string());
                file.new_object_id = Some(new_object_id.to_string());
            }
        }
        if let Some(mode) = parts.next() {
            file.mode = Some(mode.to_string());
        }
    }
    if let Some(prev_name) = line.strip_prefix("rename from ") {
        file.prev_name = Some(prev_name.trim().to_string());
    }
    if let Some(name) = line.strip_prefix("rename to ") {
        file.name = name.trim().to_string();
    }
}

fn parse_filename_header(line: &str, is_git_diff: bool) -> Option<(&'static str, String)> {
    let line = trim_line_end(line);
    let header_type = if line.starts_with("---") {
        "---"
    } else if line.starts_with("+++") {
        "+++"
    } else {
        return None;
    };
    let rest = line.get(3..)?.trim_start();
    let file_name = rest
        .split('\t')
        .next()
        .unwrap_or(rest)
        .split('\r')
        .next()
        .unwrap_or(rest)
        .split('\n')
        .next()
        .unwrap_or(rest)
        .trim();
    let file_name = if is_git_diff {
        strip_git_side_prefix(file_name).unwrap_or(file_name)
    } else {
        file_name
    };
    Some((header_type, file_name.to_string()))
}

fn parse_git_diff_names(line: &str) -> Option<(String, String)> {
    let mut rest = line.strip_prefix("diff --git ")?;
    let prev = parse_git_header_path(&mut rest)?;
    rest = rest.trim_start();
    let next = parse_git_header_path(&mut rest)?;
    Some((prev, next))
}

fn parse_git_header_path(rest: &mut &str) -> Option<String> {
    let value = rest.trim_start();
    if let Some(after_quote) = value.strip_prefix('"') {
        for (index, ch) in after_quote.char_indices() {
            if ch == '"' {
                let path = &after_quote[..index];
                *rest = &after_quote[index + ch.len_utf8()..];
                return strip_git_side_prefix(path).map(ToOwned::to_owned);
            }
        }
        None
    } else {
        let end = value.find(char::is_whitespace).unwrap_or(value.len());
        let path = &value[..end];
        *rest = &value[end..];
        strip_git_side_prefix(path).map(ToOwned::to_owned)
    }
}

fn strip_git_side_prefix(path: &str) -> Option<&str> {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .or_else(|| (path == "/dev/null").then_some(path))
}

fn line_without_ending(line: &str) -> &str {
    trim_line_end(line)
}

#[inline]
fn build_diff_rows(
    diff: &str,
    _filetype: Option<&'static str>,
) -> (Vec<DiffRow>, Vec<DiffHunkBlock>, Vec<DiffHunkGap>) {
    let parsed = parse_patch_files(diff, None, false).unwrap_or_default();
    let mut rows = Vec::new();
    let mut hunks = Vec::new();

    for patch in parsed {
        for file in patch.files {
            append_file_diff_rows(&file, &mut rows, &mut hunks);
        }
    }
    if rows.is_empty() && diff.trim_start().starts_with("@@ -") {
        let synthetic = format!("--- a/file\n+++ b/file\n{diff}");
        if let Ok(Some(file)) = process_file(&synthetic, None, Some(false), false) {
            append_file_diff_rows(&file, &mut rows, &mut hunks);
        }
    }

    let mut gaps = Vec::new();
    for (gap_index, pair) in hunks.windows(2).enumerate() {
        let previous = &pair[0];
        let next = &pair[1];
        let new_start = previous.new_start.saturating_add(previous.new_count);
        let new_count = next.new_start.saturating_sub(new_start);
        if new_count == 0 {
            continue;
        }
        gaps.push(DiffHunkGap {
            gap_index,
            new_start,
            new_count,
        });
    }

    (rows, hunks, gaps)
}

fn append_file_diff_rows(
    file: &FileDiffMetadata,
    rows: &mut Vec<DiffRow>,
    hunks: &mut Vec<DiffHunkBlock>,
) {
    append_file_diff_rows_with_conflicts(file, &[], &[], rows, hunks);
}

fn append_file_diff_rows_with_conflicts(
    file: &FileDiffMetadata,
    actions: &[Option<MergeConflictDiffAction>],
    marker_rows: &[MergeConflictMarkerRow],
    rows: &mut Vec<DiffRow>,
    hunks: &mut Vec<DiffHunkBlock>,
) {
    for hunk in &file.hunks {
        let hunk_index = hunks.len();
        let chrome = build_conflict_chrome_rows(actions, marker_rows, hunk_index);
        let row_start = rows.len();
        let mut old_line = hunk.deletion_start;
        let mut new_line = hunk.addition_start;

        for (content_index, content) in hunk.hunk_content.iter().enumerate() {
            append_chrome_rows(rows, chrome.before.get(&content_index));
            let conflict_index =
                conflict_index_for_hunk_content(actions, hunk_index, content_index);
            match content {
                HunkContent::Context {
                    lines,
                    addition_line_index,
                    ..
                } => {
                    for offset in 0..*lines {
                        let text = file
                            .addition_lines
                            .get(addition_line_index + offset)
                            .map(|line| line_without_ending(line))
                            .unwrap_or("");
                        rows.push(render_diff_row(
                            Some(old_line),
                            Some(new_line),
                            text,
                            DiffLineKind::Context,
                            conflict_index,
                        ));
                        old_line += 1;
                        new_line += 1;
                    }
                }
                HunkContent::Change {
                    deletions,
                    deletion_line_index,
                    additions,
                    addition_line_index,
                } => {
                    for offset in 0..*deletions {
                        let text = file
                            .deletion_lines
                            .get(deletion_line_index + offset)
                            .map(|line| line_without_ending(line))
                            .unwrap_or("");
                        rows.push(render_diff_row(
                            Some(old_line),
                            None,
                            text,
                            DiffLineKind::Removed,
                            conflict_index,
                        ));
                        old_line += 1;
                    }
                    append_chrome_rows(rows, chrome.between_change_sides.get(&content_index));
                    for offset in 0..*additions {
                        let text = file
                            .addition_lines
                            .get(addition_line_index + offset)
                            .map(|line| line_without_ending(line))
                            .unwrap_or("");
                        rows.push(render_diff_row(
                            None,
                            Some(new_line),
                            text,
                            DiffLineKind::Added,
                            conflict_index,
                        ));
                        new_line += 1;
                    }
                }
            }
            append_chrome_rows(rows, chrome.after.get(&content_index));
        }

        hunks.push(DiffHunkBlock {
            new_start: hunk.addition_start,
            new_count: hunk.addition_count,
            row_start,
            row_end: rows.len(),
        });
    }
}

#[derive(Debug, Default)]
struct ConflictChromeRows {
    before: HashMap<usize, Vec<DiffRow>>,
    between_change_sides: HashMap<usize, Vec<DiffRow>>,
    after: HashMap<usize, Vec<DiffRow>>,
}

fn build_conflict_chrome_rows(
    actions: &[Option<MergeConflictDiffAction>],
    _marker_rows: &[MergeConflictMarkerRow],
    hunk_index: usize,
) -> ConflictChromeRows {
    let mut chrome = ConflictChromeRows::default();
    for action in actions.iter().flatten() {
        if action.conflict_data.hunk_index != hunk_index {
            continue;
        }

        push_chrome_row(
            &mut chrome.before,
            action.conflict_data.start_content_index,
            render_conflict_action_row(action.conflict_index),
        );
        push_chrome_row(
            &mut chrome.before,
            action.conflict_data.start_content_index,
            render_conflict_marker_row(
                action.conflict_index,
                MergeConflictMarkerRowType::MarkerStart,
                action.marker_lines.start.as_str(),
            ),
        );

        if let (Some(base_content_index), Some(base_marker)) = (
            action.conflict_data.base_content_index,
            action.marker_lines.base.as_deref(),
        ) {
            push_chrome_row(
                &mut chrome.before,
                base_content_index,
                render_conflict_marker_row(
                    action.conflict_index,
                    MergeConflictMarkerRowType::MarkerBase,
                    base_marker,
                ),
            );
            push_chrome_row(
                &mut chrome.after,
                base_content_index,
                render_conflict_marker_row(
                    action.conflict_index,
                    MergeConflictMarkerRowType::MarkerSeparator,
                    action.marker_lines.separator.as_str(),
                ),
            );
        } else {
            let separator_content_index = action
                .conflict_data
                .current_content_index
                .unwrap_or(action.conflict_data.start_content_index);
            push_chrome_row(
                &mut chrome.between_change_sides,
                separator_content_index,
                render_conflict_marker_row(
                    action.conflict_index,
                    MergeConflictMarkerRowType::MarkerSeparator,
                    action.marker_lines.separator.as_str(),
                ),
            );
        }

        push_chrome_row(
            &mut chrome.after,
            action.conflict_data.end_marker_content_index,
            render_conflict_marker_row(
                action.conflict_index,
                MergeConflictMarkerRowType::MarkerEnd,
                action.marker_lines.end.as_str(),
            ),
        );
    }

    chrome
}

fn push_chrome_row(map: &mut HashMap<usize, Vec<DiffRow>>, content_index: usize, row: DiffRow) {
    map.entry(content_index).or_default().push(row);
}

fn append_chrome_rows(rows: &mut Vec<DiffRow>, chrome_rows: Option<&Vec<DiffRow>>) {
    if let Some(chrome_rows) = chrome_rows {
        rows.extend(chrome_rows.iter().cloned());
    }
}

fn render_conflict_action_row(conflict_index: usize) -> DiffRow {
    render_diff_row(
        None,
        None,
        "1 Accept current change | 2 Accept incoming change | 3 Accept both",
        DiffLineKind::ConflictAction,
        Some(conflict_index),
    )
}

fn render_conflict_marker_row(
    conflict_index: usize,
    row_type: MergeConflictMarkerRowType,
    line: &str,
) -> DiffRow {
    let label = match row_type {
        MergeConflictMarkerRowType::MarkerStart => "Current Change",
        MergeConflictMarkerRowType::MarkerBase => "Base",
        MergeConflictMarkerRowType::MarkerSeparator => "Incoming Change",
        MergeConflictMarkerRowType::MarkerEnd => "",
    };
    let line = line_without_ending(line);
    let text = if label.is_empty() {
        line.to_string()
    } else {
        format!("{line} ({label})")
    };
    render_diff_row(
        None,
        None,
        &text,
        DiffLineKind::ConflictMarker(row_type),
        Some(conflict_index),
    )
}

fn conflict_index_for_hunk_content(
    actions: &[Option<MergeConflictDiffAction>],
    hunk_index: usize,
    content_index: usize,
) -> Option<usize> {
    actions.iter().flatten().find_map(|action| {
        (action.conflict_data.hunk_index == hunk_index
            && content_index >= action.conflict_data.start_content_index
            && content_index <= action.conflict_data.end_content_index)
            .then_some(action.conflict_index)
    })
}

#[inline]
fn render_diff_row(
    old_line: Option<usize>,
    new_line: Option<usize>,
    content: &str,
    kind: DiffLineKind,
    conflict_index: Option<usize>,
) -> DiffRow {
    DiffRow {
        kind,
        old_line,
        new_line,
        conflict_index,
        text: content.to_string(),
        syntax: DiffRowSyntax::default(),
    }
}

fn resolve_split_target_line(left: Option<&DiffRow>, right: Option<&DiffRow>) -> Option<usize> {
    right
        .and_then(|row| row.new_line)
        .or_else(|| left.and_then(|row| row.old_line))
}

#[derive(Debug, Clone)]
struct RenderedDisplayLine {
    line: Line<'static>,
    selection: DisplaySelectionLine,
}

#[derive(Debug, Clone)]
struct WrappedLineContent {
    spans: Vec<Span<'static>>,
    text: String,
    content_width: usize,
}

fn render_unified_code_lines(row: &DiffRow, width: usize) -> Vec<RenderedDisplayLine> {
    let base_style = base_style(row.kind);
    let sign_style = match row.kind {
        DiffLineKind::Context => ui::context_sign_style(),
        DiffLineKind::Added => ui::added_sign_style(),
        DiffLineKind::Removed => ui::removed_sign_style(),
        DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => ui::diff_hunk_style(),
    };
    let marker = match row.kind {
        DiffLineKind::Context => ' ',
        DiffLineKind::Added => '+',
        DiffLineKind::Removed => '-',
        DiffLineKind::ConflictAction => ' ',
        DiffLineKind::ConflictMarker(_) => '!',
    };
    let unified_line_number = match row.kind {
        DiffLineKind::Added | DiffLineKind::Context => row.new_line,
        DiffLineKind::Removed => row.old_line,
        DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => None,
    };

    let prefix = vec![
        Span::styled(
            format_line_number(unified_line_number),
            base_style.patch(ui::line_number_style()),
        ),
        Span::styled(format!("{marker} "), sign_style),
    ];
    let continuation_prefix = vec![
        Span::styled(
            " ".repeat(format_line_number(None).width()),
            base_style.patch(ui::line_number_style()),
        ),
        Span::styled("  ", sign_style),
    ];
    let content = render_row_content(row.unified_content(), &row.text, base_style);
    let prefix_width = spans_width(&prefix);
    wrap_prefixed_spans_to_lines(prefix, continuation_prefix, content, width, base_style)
        .into_iter()
        .map(|wrapped| RenderedDisplayLine {
            line: Line::from(wrapped.spans).style(base_style),
            selection: DisplaySelectionLine {
                unified: Some(DisplaySelectionSegment {
                    start_column: prefix_width,
                    content_width: wrapped.content_width,
                    text: wrapped.text,
                }),
                ..DisplaySelectionLine::default()
            },
        })
        .collect()
}

fn render_split_pair_lines(
    left: Option<&DiffRow>,
    right: Option<&DiffRow>,
    side_width: usize,
) -> Vec<RenderedDisplayLine> {
    let left_lines = render_split_side_lines(left, true, side_width);
    let right_lines = render_split_side_lines(right, false, side_width);
    let line_count = left_lines.len().max(right_lines.len());
    let gap = Span::styled("   ", ui::diff_context_style());

    (0..line_count)
        .map(|index| {
            let left_line = left_lines
                .get(index)
                .cloned()
                .unwrap_or_else(|| blank_split_side(side_width));
            let right_line = right_lines
                .get(index)
                .cloned()
                .unwrap_or_else(|| blank_split_side(side_width));
            let mut spans = Vec::new();
            spans.extend(left_line.spans.clone());
            spans.push(gap.clone());
            spans.extend(right_line.spans.clone());
            RenderedDisplayLine {
                line: Line::from(spans),
                selection: DisplaySelectionLine {
                    left: Some(DisplaySelectionSegment {
                        start_column: left_line.start_column,
                        content_width: left_line.content_width,
                        text: left_line.text,
                    }),
                    right: Some(DisplaySelectionSegment {
                        start_column: side_width + 3 + right_line.start_column,
                        content_width: right_line.content_width,
                        text: right_line.text,
                    }),
                    ..DisplaySelectionLine::default()
                },
            }
        })
        .collect()
}

fn render_split_hunk_rows(
    rows: &[DiffRow],
    row_index_offset: usize,
    side_width: usize,
) -> Vec<(
    Line<'static>,
    Option<DisplayNavTarget>,
    DisplayRowRefs,
    DisplaySelectionLine,
)> {
    let mut rendered = Vec::with_capacity(rows.len());
    let mut pending_removed: Vec<(usize, &DiffRow)> = Vec::new();
    let mut pending_added: Vec<(usize, &DiffRow)> = Vec::new();

    let flush_pending = |rendered: &mut Vec<(
        Line<'static>,
        Option<DisplayNavTarget>,
        DisplayRowRefs,
        DisplaySelectionLine,
    )>,
                         removed: &mut Vec<(usize, &DiffRow)>,
                         added: &mut Vec<(usize, &DiffRow)>| {
        let row_count = removed.len().max(added.len());
        for index in 0..row_count {
            let left = removed.get(index).copied();
            let right = added.get(index).copied();
            let target_line =
                resolve_split_target_line(left.map(|(_, row)| row), right.map(|(_, row)| row));
            let row_refs = DisplayRowRefs {
                left: left.map(|(row_index, _)| row_index),
                right: right.map(|(row_index, _)| row_index),
            };
            for rendered_line in render_split_pair_lines(
                left.map(|(_, row)| row),
                right.map(|(_, row)| row),
                side_width,
            ) {
                rendered.push((
                    rendered_line.line,
                    target_line.map(DisplayNavTarget::Line),
                    row_refs,
                    rendered_line.selection,
                ));
            }
        }
        removed.clear();
        added.clear();
    };

    for (row_offset, row) in rows.iter().enumerate() {
        let row_index = row_index_offset + row_offset;
        match row.kind {
            DiffLineKind::Removed => pending_removed.push((row_index, row)),
            DiffLineKind::Added => pending_added.push((row_index, row)),
            DiffLineKind::Context => {
                flush_pending(&mut rendered, &mut pending_removed, &mut pending_added);
                let target_line = resolve_split_target_line(Some(row), Some(row));
                let row_refs = DisplayRowRefs {
                    left: Some(row_index),
                    right: Some(row_index),
                };
                for rendered_line in render_split_pair_lines(Some(row), Some(row), side_width) {
                    rendered.push((
                        rendered_line.line,
                        target_line.map(DisplayNavTarget::Line),
                        row_refs,
                        rendered_line.selection,
                    ));
                }
            }
            DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => {
                flush_pending(&mut rendered, &mut pending_removed, &mut pending_added);
                let row_refs = DisplayRowRefs {
                    left: Some(row_index),
                    right: Some(row_index),
                };
                for rendered_line in render_unified_code_lines(row, side_width * 2 + 3) {
                    rendered.push((
                        rendered_line.line,
                        row.conflict_index.map(DisplayNavTarget::Conflict),
                        row_refs,
                        rendered_line.selection,
                    ));
                }
            }
        }
    }

    flush_pending(&mut rendered, &mut pending_removed, &mut pending_added);
    rendered
}

fn render_expand_gap_line(
    width: usize,
    _remaining: usize,
    _has_expansion: bool,
    direction: GapExpandDirection,
) -> Line<'static> {
    let hint_style = ui::diff_hunk_style();
    let action_style = ui::diff_hunk_style().add_modifier(Modifier::BOLD);
    let label = match direction {
        GapExpandDirection::Down => "↑↑",
        GapExpandDirection::Up => "↓↓",
    };
    let side_padding = 1;
    let trailing_padding = width
        .saturating_sub(side_padding)
        .saturating_sub(label.width());
    let mut spans = vec![
        Span::styled(" ".repeat(side_padding), hint_style),
        Span::styled(label.to_string(), action_style),
        Span::styled(" ".repeat(trailing_padding), hint_style),
    ];
    spans = fit_spans_to_width(spans, width.max(1), hint_style);
    Line::from(spans).style(ui::diff_hunk_style())
}

fn render_expanded_context_lines(
    line_number: usize,
    text: &str,
    highlighted_content: Option<Vec<SyntaxToken>>,
    width: usize,
    split: bool,
) -> Vec<RenderedDisplayLine> {
    let row = DiffRow {
        kind: DiffLineKind::Context,
        old_line: Some(line_number),
        new_line: Some(line_number),
        conflict_index: None,
        text: text.to_string(),
        syntax: DiffRowSyntax {
            left: highlighted_content.clone(),
            right: highlighted_content,
        },
    };
    if split {
        let total_width = width.saturating_sub(1);
        let gutter_width = 3;
        let side_width = total_width.saturating_sub(gutter_width) / 2;
        render_split_pair_lines(Some(&row), Some(&row), side_width)
    } else {
        render_unified_code_lines(&row, width)
    }
}

#[derive(Debug, Clone)]
struct WrappedSideLine {
    spans: Vec<Span<'static>>,
    start_column: usize,
    content_width: usize,
    text: String,
}

fn render_split_side_lines(
    row: Option<&DiffRow>,
    left_side: bool,
    width: usize,
) -> Vec<WrappedSideLine> {
    let Some(row) = row else {
        return vec![blank_split_side(width)];
    };

    let line_number = if left_side {
        row.old_line
    } else {
        row.new_line
    };
    let base_style = base_style(row.kind);
    let prefix = vec![Span::styled(
        format_line_number(line_number),
        base_style.patch(ui::line_number_style()),
    )];
    let continuation_prefix = vec![Span::styled(
        " ".repeat(format_line_number(None).width()),
        base_style.patch(ui::line_number_style()),
    )];
    let content = render_row_content(row.side_content(left_side), &row.text, base_style);
    let prefix_width = spans_width(&prefix);
    wrap_prefixed_spans_to_lines(prefix, continuation_prefix, content, width + 1, base_style)
        .into_iter()
        .map(|wrapped| WrappedSideLine {
            spans: wrapped.spans,
            start_column: prefix_width,
            content_width: wrapped.content_width,
            text: wrapped.text,
        })
        .collect()
}

fn blank_split_side(width: usize) -> WrappedSideLine {
    WrappedSideLine {
        spans: vec![Span::styled(" ".repeat(width), ui::diff_context_style())],
        start_column: 0,
        content_width: width,
        text: String::new(),
    }
}

fn render_row_content(
    syntax_tokens: Option<&[SyntaxToken]>,
    text: &str,
    fallback: Style,
) -> Vec<Span<'static>> {
    let has_tabs = text.as_bytes().contains(&b'\t');

    let raw_spans = match syntax_tokens {
        Some(tokens) if !tokens.is_empty() => tokens
            .iter()
            .map(|token| {
                let style = token
                    .highlight_name
                    .map(|name| ui::syntax_style(name, fallback))
                    .unwrap_or(fallback);
                let content = text
                    .get(token.start..token.end)
                    .map(str::to_string)
                    .unwrap_or_default();
                Span::styled(content, style)
            })
            .collect(),
        _ => vec![Span::styled(text.to_string(), fallback)],
    };

    if has_tabs {
        expand_tabs_in_spans(raw_spans)
    } else {
        raw_spans
    }
}

fn expand_tabs_in_spans(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    let mut expanded = Vec::with_capacity(spans.len());
    let mut visual_column = 0usize;

    for span in spans {
        let style = span.style;
        let mut chunk = String::new();

        for ch in span.content.chars() {
            if ch == '\t' {
                if !chunk.is_empty() {
                    expanded.push(Span::styled(std::mem::take(&mut chunk), style));
                }

                let tab_width = tab_display_width(visual_column);
                if tab_width > 0 {
                    expanded.push(Span::styled(" ".repeat(tab_width), style));
                    visual_column += tab_width;
                }
                continue;
            }

            let Some(ch_width) = UnicodeWidthChar::width(ch) else {
                continue;
            };
            if ch_width == 0 {
                continue;
            }

            chunk.push(ch);
            visual_column += ch_width;
        }

        if !chunk.is_empty() {
            expanded.push(Span::styled(chunk, style));
        }
    }

    expanded
}

fn tab_display_width(visual_column: usize) -> usize {
    let offset = visual_column % DIFF_TAB_WIDTH;
    if offset == 0 {
        DIFF_TAB_WIDTH
    } else {
        DIFF_TAB_WIDTH - offset
    }
}

fn fit_spans_to_width(
    spans: Vec<Span<'static>>,
    width: usize,
    pad_style: Style,
) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }

    let mut fitted = Vec::new();
    let mut current_width = 0usize;

    for span in spans {
        if current_width >= width {
            break;
        }

        let content = span.content.as_ref();
        let remaining = width.saturating_sub(current_width);
        let content_width = UnicodeWidthStr::width(content);

        if content_width <= remaining {
            current_width += content_width;
            fitted.push(span);
            continue;
        }

        let truncated = truncate_to_width(content, remaining);
        current_width += UnicodeWidthStr::width(truncated.as_str());
        fitted.push(Span::styled(truncated, span.style));
        break;
    }

    if current_width < width {
        fitted.push(Span::styled(" ".repeat(width - current_width), pad_style));
    }

    fitted
}

fn wrap_prefixed_spans_to_lines(
    prefix: Vec<Span<'static>>,
    continuation_prefix: Vec<Span<'static>>,
    content: Vec<Span<'static>>,
    width: usize,
    pad_style: Style,
) -> Vec<WrappedLineContent> {
    let target_width = width.saturating_sub(1);
    let prefix_width = spans_width(&prefix);
    let continuation_prefix_width = spans_width(&continuation_prefix);
    let content_width = target_width.saturating_sub(prefix_width);
    let continuation_content_width = target_width.saturating_sub(continuation_prefix_width);

    if target_width == 0 || content_width == 0 || continuation_content_width == 0 {
        let mut spans = prefix;
        spans.extend(content);
        return wrap_spans_to_width(spans, target_width.max(1), pad_style)
            .into_iter()
            .map(|line| WrappedLineContent {
                text: line_text(&line),
                content_width: spans_width(&line),
                spans: line,
            })
            .collect();
    }

    let wrapped_content = wrap_spans_to_width(content, content_width, pad_style);
    wrapped_content
        .into_iter()
        .enumerate()
        .map(|(index, line_content)| {
            let text = line_text(&line_content);
            let mut spans = if index == 0 {
                prefix.clone()
            } else {
                continuation_prefix.clone()
            };
            spans.extend(line_content);
            WrappedLineContent {
                text,
                content_width: if index == 0 {
                    content_width
                } else {
                    continuation_content_width
                },
                spans,
            }
        })
        .collect()
}

fn line_text(spans: &[Span<'static>]) -> String {
    spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
        .trim_end_matches(' ')
        .to_string()
}

fn wrap_spans_to_width(
    spans: Vec<Span<'static>>,
    width: usize,
    pad_style: Style,
) -> Vec<Vec<Span<'static>>> {
    if width == 0 {
        return vec![Vec::new()];
    }

    let mut queue = VecDeque::from(spans);
    let mut wrapped = Vec::new();

    while !queue.is_empty() {
        let mut line = Vec::new();
        let mut current_width = 0usize;

        while current_width < width {
            let Some(span) = queue.pop_front() else {
                break;
            };

            let content = span.content.into_owned();
            let remaining = width.saturating_sub(current_width);
            let content_width = UnicodeWidthStr::width(content.as_str());

            if content_width <= remaining {
                current_width += content_width;
                line.push(Span::styled(content, span.style));
                continue;
            }

            let (head, tail) = split_string_at_width(&content, remaining);
            if !head.is_empty() {
                current_width += UnicodeWidthStr::width(head.as_str());
                line.push(Span::styled(head, span.style));
            }
            if !tail.is_empty() {
                queue.push_front(Span::styled(tail, span.style));
            }
            break;
        }

        if current_width < width {
            line.push(Span::styled(" ".repeat(width - current_width), pad_style));
        }
        wrapped.push(line);
    }

    if wrapped.is_empty() {
        wrapped.push(vec![Span::styled(" ".repeat(width), pad_style)]);
    }

    wrapped
}

fn spans_width(spans: &[Span<'static>]) -> usize {
    spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

fn normalize_selection_points(
    anchor: DiffSelectionPoint,
    head: DiffSelectionPoint,
) -> (DiffSelectionPoint, DiffSelectionPoint) {
    if (anchor.display_index, anchor.column) <= (head.display_index, head.column) {
        (anchor, head)
    } else {
        (head, anchor)
    }
}

fn split_string_at_width(content: &str, width: usize) -> (String, String) {
    let mut head = String::new();
    let mut used = 0usize;
    let mut split_at = content.len();

    for (index, ch) in content.char_indices() {
        let Some(ch_width) = UnicodeWidthChar::width(ch) else {
            continue;
        };
        if ch_width == 0 {
            continue;
        }
        if used + ch_width > width {
            split_at = index;
            return (head, content[split_at..].to_string());
        }
        used += ch_width;
        head.push(ch);
        split_at = index + ch.len_utf8();
    }

    (head, content[split_at..].to_string())
}

fn slice_string_by_width(content: &str, start: usize, end: usize) -> String {
    let mut result = String::new();
    let mut used = 0usize;

    for ch in content.chars() {
        let Some(ch_width) = UnicodeWidthChar::width(ch) else {
            continue;
        };
        if ch_width == 0 {
            continue;
        }

        let next_width = used + ch_width;
        if next_width <= start {
            used = next_width;
            continue;
        }
        if used >= end {
            break;
        }

        result.push(ch);
        used = next_width;
    }

    result
}

fn truncate_to_width(content: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0usize;
    for ch in content.chars() {
        let ch_width = UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]));
        if used + ch_width > width {
            break;
        }
        used += ch_width;
        result.push(ch);
    }
    result
}

fn format_line_number(line: Option<usize>) -> String {
    format!(
        "{:>4} ",
        line.map_or(String::new(), |line| line.to_string())
    )
}

fn base_style(kind: DiffLineKind) -> Style {
    match kind {
        DiffLineKind::Context => ui::diff_context_style(),
        DiffLineKind::Added => ui::diff_added_style(),
        DiffLineKind::Removed => ui::diff_removed_style(),
        DiffLineKind::ConflictAction => ui::diff_hunk_style(),
        DiffLineKind::ConflictMarker(MergeConflictMarkerRowType::MarkerStart)
        | DiffLineKind::ConflictMarker(MergeConflictMarkerRowType::MarkerBase) => {
            ui::diff_removed_style()
        }
        DiffLineKind::ConflictMarker(MergeConflictMarkerRowType::MarkerSeparator)
        | DiffLineKind::ConflictMarker(MergeConflictMarkerRowType::MarkerEnd) => {
            ui::diff_added_style()
        }
    }
}

fn collect_display_highlight_windows(
    row_refs: &[DisplayRowRefs],
    row_count: usize,
) -> (Option<(usize, usize)>, Option<(usize, usize)>) {
    let mut left_min = None;
    let mut left_max = None;
    let mut right_min = None;
    let mut right_max = None;

    for refs in row_refs {
        if let Some(row_index) = refs.left {
            left_min = Some(left_min.map_or(row_index, |current: usize| current.min(row_index)));
            left_max = Some(left_max.map_or(row_index, |current: usize| current.max(row_index)));
        }
        if let Some(row_index) = refs.right {
            right_min = Some(right_min.map_or(row_index, |current: usize| current.min(row_index)));
            right_max = Some(right_max.map_or(row_index, |current: usize| current.max(row_index)));
        }
    }

    (
        expand_row_window(left_min.zip(left_max), row_count),
        expand_row_window(right_min.zip(right_max), row_count),
    )
}

fn expand_row_window(window: Option<(usize, usize)>, row_count: usize) -> Option<(usize, usize)> {
    let (start, end) = window?;
    if row_count == 0 {
        return None;
    }

    Some((
        start.saturating_sub(VIEWPORT_HIGHLIGHT_PADDING_ROWS),
        end.saturating_add(VIEWPORT_HIGHLIGHT_PADDING_ROWS)
            .min(row_count.saturating_sub(1)),
    ))
}

#[cfg(test)]
mod tests {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        text::Text,
        widgets::{Paragraph, Widget},
    };

    use super::*;

    fn render_lines_to_strings(lines: Vec<Line<'static>>, width: u16) -> Vec<String> {
        let area = Rect::new(0, 0, width, lines.len() as u16);
        let mut buffer = Buffer::empty(area);
        Paragraph::new(Text::from(lines)).render(area, &mut buffer);
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn verify_file_hunk_values(file: &FileDiffMetadata) -> Result<(), String> {
        let mut current_split_line_total = 0usize;
        let mut current_unified_line_total = 0usize;
        let mut last_hunk_addition_end = 0usize;

        for (hunk_index, hunk) in file.hunks.iter().enumerate() {
            let mut context_lines = 0usize;
            let mut addition_lines = 0usize;
            let mut deletion_lines = 0usize;
            let mut expected_split_line_count = 0usize;
            let mut expected_unified_line_count = 0usize;

            for content in &hunk.hunk_content {
                match content {
                    HunkContent::Context { lines, .. } => {
                        context_lines += *lines;
                        expected_split_line_count += *lines;
                        expected_unified_line_count += *lines;
                    }
                    HunkContent::Change {
                        additions,
                        deletions,
                        ..
                    } => {
                        addition_lines += *additions;
                        deletion_lines += *deletions;
                        expected_split_line_count += (*additions).max(*deletions);
                        expected_unified_line_count += *additions + *deletions;
                    }
                }
            }

            let prefix = format!("hunks[{hunk_index}]");
            if hunk.addition_count != addition_lines + context_lines {
                return Err(format!(
                    "{prefix}: addition_count {} != additions + context {}",
                    hunk.addition_count,
                    addition_lines + context_lines
                ));
            }
            if hunk.deletion_count != deletion_lines + context_lines {
                return Err(format!(
                    "{prefix}: deletion_count {} != deletions + context {}",
                    hunk.deletion_count,
                    deletion_lines + context_lines
                ));
            }
            if hunk.addition_lines != addition_lines {
                return Err(format!(
                    "{prefix}: addition_lines {} != counted additions {}",
                    hunk.addition_lines, addition_lines
                ));
            }
            if hunk.deletion_lines != deletion_lines {
                return Err(format!(
                    "{prefix}: deletion_lines {} != counted deletions {}",
                    hunk.deletion_lines, deletion_lines
                ));
            }
            if hunk.split_line_count != expected_split_line_count {
                return Err(format!(
                    "{prefix}: split_line_count {} != expected {}",
                    hunk.split_line_count, expected_split_line_count
                ));
            }
            if hunk.unified_line_count != expected_unified_line_count {
                return Err(format!(
                    "{prefix}: unified_line_count {} != expected {}",
                    hunk.unified_line_count, expected_unified_line_count
                ));
            }

            let expected_collapsed_before = hunk
                .addition_start
                .saturating_sub(1 + last_hunk_addition_end);
            if hunk.collapsed_before != expected_collapsed_before {
                return Err(format!(
                    "{prefix}: collapsed_before {} != expected {}",
                    hunk.collapsed_before, expected_collapsed_before
                ));
            }
            if hunk.split_line_start != current_split_line_total + hunk.collapsed_before {
                return Err(format!(
                    "{prefix}: split_line_start {} != expected {}",
                    hunk.split_line_start,
                    current_split_line_total + hunk.collapsed_before
                ));
            }
            if hunk.unified_line_start != current_unified_line_total + hunk.collapsed_before {
                return Err(format!(
                    "{prefix}: unified_line_start {} != expected {}",
                    hunk.unified_line_start,
                    current_unified_line_total + hunk.collapsed_before
                ));
            }

            current_split_line_total = hunk.split_line_start + hunk.split_line_count;
            current_unified_line_total = hunk.unified_line_start + hunk.unified_line_count;
            last_hunk_addition_end = hunk
                .addition_start
                .saturating_add(hunk.addition_count)
                .saturating_sub(1);
        }

        Ok(())
    }

    fn apply_full_diff_ops(old_lines: &[String], new_lines: &[String], ops: &[FullDiffOp]) {
        let mut reconstructed_old = Vec::new();
        let mut reconstructed_new = Vec::new();

        for op in ops {
            match *op {
                FullDiffOp::Equal {
                    old_index,
                    new_index,
                } => {
                    reconstructed_old.push(old_lines[old_index].clone());
                    reconstructed_new.push(new_lines[new_index].clone());
                }
                FullDiffOp::Delete { old_index, .. } => {
                    reconstructed_old.push(old_lines[old_index].clone());
                }
                FullDiffOp::Insert { new_index, .. } => {
                    reconstructed_new.push(new_lines[new_index].clone());
                }
            }
        }

        assert_eq!(reconstructed_old, old_lines);
        assert_eq!(reconstructed_new, new_lines);
    }

    #[test]
    fn full_diff_ops_reconstruct_repeated_lines_and_boundaries() {
        let old_lines = ["alpha\n", "repeat\n", "repeat\n", "remove\n", "tail\n"]
            .into_iter()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let new_lines = [
            "start\n", "alpha\n", "repeat\n", "insert\n", "repeat\n", "tail\n",
        ]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

        let ops = compute_full_diff_ops(&old_lines, &new_lines, false);

        apply_full_diff_ops(&old_lines, &new_lines, &ops);
        assert_eq!(
            ops.iter()
                .filter(|op| !matches!(op, FullDiffOp::Equal { .. }))
                .count(),
            3
        );
        assert!(matches!(ops.first(), Some(FullDiffOp::Insert { .. })));
        assert!(
            ops.iter()
                .any(|op| matches!(op, FullDiffOp::Delete { old_index: 3, .. }))
        );
    }

    #[test]
    fn parse_patch_files_matches_pierre_hunk_metadata_shape() {
        let patch = "\
From 1111111111111111111111111111111111111111 Mon Sep 17 00:00:00 2001
Subject: parser fixture

diff --git a/src/main.rs b/src/main.rs
index 1111111..2222222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@ fn main
 context
-old
+new
+added
 tail
\\ No newline at end of file
";

        let parsed = parse_patch_files(patch, Some("fixture"), true).unwrap();
        assert_eq!(parsed.len(), 1);
        assert!(
            parsed[0]
                .patch_metadata
                .as_deref()
                .unwrap()
                .contains("Subject: parser fixture")
        );

        let file = &parsed[0].files[0];
        assert_eq!(file.name, "src/main.rs");
        assert_eq!(file.change_type, ChangeType::Change);
        assert_eq!(file.prev_object_id.as_deref(), Some("1111111"));
        assert_eq!(file.new_object_id.as_deref(), Some("2222222"));
        assert_eq!(file.mode.as_deref(), Some("100644"));
        assert_eq!(file.cache_key.as_deref(), Some("fixture-0-0"));
        assert_eq!(file.deletion_lines, vec!["context\n", "old\n", "tail"]);
        assert_eq!(
            file.addition_lines,
            vec!["context\n", "new\n", "added\n", "tail"]
        );

        let hunk = &file.hunks[0];
        assert_eq!(hunk.hunk_context.as_deref(), Some("fn main"));
        assert_eq!(hunk.addition_start, 1);
        assert_eq!(hunk.addition_count, 4);
        assert_eq!(hunk.deletion_start, 1);
        assert_eq!(hunk.deletion_count, 3);
        assert_eq!(hunk.addition_lines, 2);
        assert_eq!(hunk.deletion_lines, 1);
        assert_eq!(hunk.split_line_count, 4);
        assert_eq!(hunk.unified_line_count, 5);
        assert!(hunk.no_eof_cr_additions);
        assert!(hunk.no_eof_cr_deletions);
        assert_eq!(
            hunk.hunk_content,
            vec![
                HunkContent::Context {
                    lines: 1,
                    addition_line_index: 0,
                    deletion_line_index: 0,
                },
                HunkContent::Change {
                    deletions: 1,
                    deletion_line_index: 1,
                    additions: 2,
                    addition_line_index: 1,
                },
                HunkContent::Context {
                    lines: 1,
                    addition_line_index: 3,
                    deletion_line_index: 2,
                },
            ]
        );
    }

    #[test]
    fn parse_patch_files_preserves_pure_rename_metadata() {
        let patch = "\
diff --git \"a/old name.txt\" \"b/new name.txt\"
similarity index 100%
rename from old name.txt
rename to new name.txt
";

        let parsed = parse_patch_files(patch, None, true).unwrap();
        let file = &parsed[0].files[0];

        assert_eq!(file.name, "new name.txt");
        assert_eq!(file.prev_name.as_deref(), Some("old name.txt"));
        assert_eq!(file.change_type, ChangeType::RenamePure);
        assert!(file.hunks.is_empty());
    }

    #[test]
    fn parse_patch_files_matches_pierre_file_patch_fixture_summary() {
        let patch = "\
diff --git a//Users/shijie.rao/code/openai/project/oai-airflow-shared/oai_airflow_shared/applied_data_platform/lodestone/lodestone_config_notifications.yaml b//Users/shijie.rao/code/openai/project/oai-airflow-shared/oai_airflow_shared/applied_data_platform/lodestone/lodestone_config_notifications.yaml
--- a//Users/shijie.rao/code/openai/project/oai-airflow-shared/oai_airflow_shared/applied_data_platform/lodestone/lodestone_config_notifications.yaml
+++ b//Users/shijie.rao/code/openai/project/oai-airflow-shared/oai_airflow_shared/applied_data_platform/lodestone/lodestone_config_notifications.yaml
@@ -3720,32 +3720 @@
 
-
-# Codex 2025 holiday campaign
-- airflow:
-    environment: prod
-    dag:
-      start_date: \"2025-12-25T00:00:00Z\"
-      schedule: \"@daily\"
-      audience: INTERNAL_APPLIED
-      urgency: MEDIUM
-      notification:
-        email: shijie.rao@openai.com
-        pagerduty: pagerduty-chatgpt-growth-retention-oncall
-      airflow_dataset_sensors:
-        - fully_qualified_table_name: analytics.scratch.shijie_codex_2025_holiday_campaign_user_id
-  databricks_source:
-    spark_sql: |
-      SELECT DISTINCT
-        user_id
-      FROM
-        analytics.scratch.shijie_codex_2025_holiday_campaign_user_id
-  azure_blob_storage_stage:
-    storage_account: oailodestoneprod
-    container: notifications
-  rockset_sink:
-    workspace: campaigns
-    collection_alias: codex_2025_holiday
-    deployments:
-      - deployment_rrn: rrn:rsd:rs6:c74bab26-bcfd-4e9b-82f8-1417bea02b8d
-        assumed_role_rrn: rrn:role:rs6:68fb7059-b1d7-46f6-bd4e-0d11088735f9
-    shard_count_minimum: 4
-  owner: growth
";

        let parsed = parse_patch_files(patch, Some("file-patch"), true).unwrap();
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].patch_metadata.is_none());

        let file = &parsed[0].files[0];
        assert_eq!(
            file.name,
            "/Users/shijie.rao/code/openai/project/oai-airflow-shared/oai_airflow_shared/applied_data_platform/lodestone/lodestone_config_notifications.yaml"
        );
        assert_eq!(file.cache_key.as_deref(), Some("file-patch-0-0"));
        assert_eq!(file.change_type, ChangeType::Change);
        assert_eq!(file.addition_lines, vec!["\n"]);
        assert_eq!(file.deletion_lines.len(), 32);
        assert_eq!(file.deletion_lines[0], "\n");
        assert_eq!(file.deletion_lines[1], "\n");
        assert_eq!(file.deletion_lines[2], "# Codex 2025 holiday campaign\n");
        assert_eq!(file.deletion_lines[31], "  owner: growth\n");
        assert_eq!(file.split_line_count, 3751);
        assert_eq!(file.unified_line_count, 3751);

        let hunk = &file.hunks[0];
        assert_eq!(hunk.addition_start, 3720);
        assert_eq!(hunk.addition_count, 1);
        assert_eq!(hunk.addition_lines, 0);
        assert_eq!(hunk.deletion_start, 3720);
        assert_eq!(hunk.deletion_count, 32);
        assert_eq!(hunk.deletion_lines, 31);
        assert_eq!(hunk.collapsed_before, 3719);
        assert_eq!(hunk.split_line_start, 3719);
        assert_eq!(hunk.unified_line_start, 3719);
        assert_eq!(hunk.split_line_count, 32);
        assert_eq!(hunk.unified_line_count, 32);
        assert_eq!(
            hunk.hunk_content,
            vec![
                HunkContent::Context {
                    lines: 1,
                    addition_line_index: 0,
                    deletion_line_index: 0,
                },
                HunkContent::Change {
                    deletions: 31,
                    deletion_line_index: 1,
                    additions: 0,
                    addition_line_index: 1,
                },
            ]
        );
        verify_file_hunk_values(file).unwrap();
    }

    #[test]
    fn parse_patch_files_ignores_format_patch_version_trailer() {
        let patch = "\
From 02a2e4e6806f7e8f3adf685fde57cc773196f206 Mon Sep 17 00:00:00 2001
From: \"Patch Fixture\" <patch.fixture@example.invalid>
Date: Tue, 5 May 2026 15:45:50 -0600
Subject: [PATCH] example patch with version trailer

---
 file.txt | 1 +
 1 file changed, 1 insertion(+)

diff --git a/file.txt b/file.txt
index 626799f..8c1202a 100644
--- a/file.txt
+++ b/file.txt
@@ -1,2 +1,3 @@
 line one
+line two
 line three
-- 
2.52.0

";

        let parsed = parse_patch_files(patch, None, true).unwrap();
        let file = &parsed[0].files[0];
        let hunk = &file.hunks[0];

        assert_eq!(hunk.addition_lines, 1);
        assert_eq!(hunk.deletion_lines, 0);
        assert_eq!(
            file.addition_lines,
            vec!["line one\n", "line two\n", "line three\n"]
        );
        assert_eq!(file.deletion_lines, vec!["line one\n", "line three\n"]);
        verify_file_hunk_values(file).unwrap();
    }

    #[test]
    fn parse_patch_files_preserves_final_blank_context_line() {
        let patch = "\
--- a/example.js
+++ b/example.js
@@ -1,4 +1,3 @@
 keep
-remove a
-remove b
+add
 
";

        let parsed = parse_patch_files(patch, None, true).unwrap();
        let file = &parsed[0].files[0];

        assert_eq!(file.addition_lines, vec!["keep\n", "add\n", "\n"]);
        assert_eq!(
            file.deletion_lines,
            vec!["keep\n", "remove a\n", "remove b\n", "\n"]
        );
        verify_file_hunk_values(file).unwrap();
    }

    #[test]
    fn parse_patch_files_salvages_malformed_bare_newline_in_hunk() {
        let patch = "\
diff --git a/malformed.txt b/malformed.txt
index 1111111..2222222 100644
--- a/malformed.txt
+++ b/malformed.txt
@@ -1,3 +1,2 @@
-old one

 old two
+new two
";

        let parsed = parse_patch_files(patch, None, false).unwrap();
        let hunk = &parsed[0].files[0].hunks[0];

        assert_eq!(hunk.deletion_count, 3);
        assert_eq!(hunk.deletion_lines, 1);
        assert_eq!(hunk.addition_count, 2);
        assert_eq!(hunk.addition_lines, 1);
    }

    #[test]
    fn parse_patch_files_preserves_bom_characters_in_hunk_lines() {
        let patch = [
            "diff --git a/bom.txt b/bom.txt\n",
            "index 1111111..2222222 100644\n",
            "--- a/bom.txt\n",
            "+++ b/bom.txt\n",
            "@@ -1 +1 @@\n",
            "-\u{FEFF}old\n",
            "+\u{FEFF}new\n",
        ]
        .join("");

        let parsed = parse_patch_files(&patch, None, true).unwrap();
        let file = &parsed[0].files[0];

        assert_eq!(file.deletion_lines[0], "\u{FEFF}old\n");
        assert_eq!(file.addition_lines[0], "\u{FEFF}new\n");
    }

    #[test]
    fn parse_patch_files_preserves_quoted_git_header_backslash_escapes() {
        let old_name =
            "test/integration/image-optimizer/app/public/\\303\\244\\303\\266\\303\\274.png";
        let new_name = "test/e2e/image-optimizer/app/public/\\303\\244\\303\\266\\303\\274.png";
        let patch = format!(
            "diff --git \"a/{old_name}\" \"b/{new_name}\"\n\
similarity index 100%\n"
        );

        let file = process_file(&patch, None, Some(true), true)
            .unwrap()
            .unwrap();

        assert_eq!(file.name, new_name);
        assert_eq!(file.prev_name.as_deref(), Some(old_name));
        assert_eq!(file.change_type, ChangeType::RenamePure);
    }

    #[test]
    fn parse_diff_from_file_returns_full_file_metadata_and_valid_hunks() {
        let old_file = FileContents {
            name: "example.ts".to_string(),
            contents: "one\nold\nshared\n".to_string(),
            lang: None,
            header: None,
            cache_key: Some("old-key".to_string()),
        };
        let new_file = FileContents {
            name: "example.ts".to_string(),
            contents: "one\nnew\nshared\nadded\n".to_string(),
            lang: None,
            header: None,
            cache_key: Some("new-key".to_string()),
        };

        let file = parse_diff_from_file(&old_file, &new_file, ParseDiffOptions::default());

        assert_eq!(file.name, "example.ts");
        assert_eq!(file.change_type, ChangeType::Change);
        assert!(!file.is_partial);
        assert_eq!(file.cache_key.as_deref(), Some("old-key:new-key"));
        assert_eq!(file.deletion_lines, vec!["one\n", "old\n", "shared\n"]);
        assert_eq!(
            file.addition_lines,
            vec!["one\n", "new\n", "shared\n", "added\n"]
        );
        assert_eq!(file.hunks.len(), 1);
        assert_eq!(
            file.hunks[0].hunk_content,
            vec![
                HunkContent::Context {
                    lines: 1,
                    addition_line_index: 0,
                    deletion_line_index: 0,
                },
                HunkContent::Change {
                    deletions: 1,
                    deletion_line_index: 1,
                    additions: 1,
                    addition_line_index: 1,
                },
                HunkContent::Context {
                    lines: 1,
                    addition_line_index: 2,
                    deletion_line_index: 2,
                },
                HunkContent::Change {
                    deletions: 0,
                    deletion_line_index: 3,
                    additions: 1,
                    addition_line_index: 3,
                },
            ]
        );
        verify_file_hunk_values(&file).unwrap();
    }

    #[test]
    fn parse_diff_from_file_can_ignore_whitespace_only_changes() {
        let old_file = FileContents {
            name: "test.txt".to_string(),
            contents: "hello world\nfoo bar\n".to_string(),
            lang: None,
            header: None,
            cache_key: None,
        };
        let new_file = FileContents {
            name: "test.txt".to_string(),
            contents: "  hello world\nfoo bar\n".to_string(),
            lang: None,
            header: None,
            cache_key: None,
        };

        let with_whitespace =
            parse_diff_from_file(&old_file, &new_file, ParseDiffOptions::default());
        assert!(!with_whitespace.hunks.is_empty());

        let without_whitespace = parse_diff_from_file(
            &old_file,
            &new_file,
            ParseDiffOptions {
                ignore_whitespace: true,
                ..ParseDiffOptions::default()
            },
        );
        assert!(without_whitespace.hunks.is_empty());
        assert_eq!(without_whitespace.change_type, ChangeType::Change);
    }

    #[test]
    fn parse_diff_from_file_handles_unchanged_and_empty_files() {
        let unchanged = FileContents {
            name: "same.txt".to_string(),
            contents: "abc".to_string(),
            lang: None,
            header: None,
            cache_key: None,
        };

        let file = parse_diff_from_file(&unchanged, &unchanged, ParseDiffOptions::default());
        assert_eq!(file.change_type, ChangeType::Change);
        assert!(file.hunks.is_empty());
        assert_eq!(file.deletion_lines, vec!["abc"]);
        assert_eq!(file.addition_lines, vec!["abc"]);

        let empty = FileContents {
            name: "empty.txt".to_string(),
            contents: String::new(),
            lang: None,
            header: None,
            cache_key: None,
        };
        let empty_diff = parse_diff_from_file(&empty, &empty, ParseDiffOptions::default());
        assert_eq!(empty_diff.change_type, ChangeType::Change);
        assert!(empty_diff.hunks.is_empty());
        assert!(empty_diff.deletion_lines.is_empty());
        assert!(empty_diff.addition_lines.is_empty());
    }

    fn build_context(count: usize, label: &str) -> Vec<String> {
        (1..=count)
            .map(|index| format!(" {label}-{index}"))
            .collect()
    }

    fn create_resolution_fixture() -> FileDiffMetadata {
        let old_contents = [
            "line 01 stable",
            "line 02 add anchor",
            "line 03 stable",
            "line 04 stable",
            "line 05 stable",
            "line 06 delete me",
            "line 07 stable",
            "line 08 stable",
            "line 09 stable",
            "line 10 replace old",
            "line 11 stable",
            "line 12 stable",
            "line 13 stable",
            "line 14 mix old a",
            "line 15 mix shared",
            "line 16 mix old b",
            "line 17 stable",
            "",
        ]
        .join("\n");
        let new_contents = [
            "line 01 stable",
            "line 02 add anchor",
            "line 02.1 add first",
            "line 02.2 add second",
            "line 03 stable",
            "line 04 stable",
            "line 05 stable",
            "line 07 stable",
            "line 08 stable",
            "line 09 stable",
            "line 10 replace new",
            "line 11 stable",
            "line 12 stable",
            "line 13 stable",
            "line 14 mix new a",
            "line 15 mix shared",
            "line 16 mix new b",
            "line 17 stable",
            "",
        ]
        .join("\n");

        parse_diff_from_file(
            &FileContents {
                name: "example.ts".to_string(),
                contents: old_contents,
                lang: None,
                header: None,
                cache_key: Some("old-key".to_string()),
            },
            &FileContents {
                name: "example.ts".to_string(),
                contents: new_contents,
                lang: None,
                header: None,
                cache_key: Some("new-key".to_string()),
            },
            ParseDiffOptions {
                context_lines: 1,
                ..ParseDiffOptions::default()
            },
        )
    }

    fn hunk_lines(file: &FileDiffMetadata, hunk_index: usize) -> Vec<String> {
        let hunk = &file.hunks[hunk_index];
        file.addition_lines
            [hunk.addition_line_index..hunk.addition_line_index + hunk.addition_count]
            .to_vec()
    }

    fn expected_resolved_hunk_lines(
        file: &FileDiffMetadata,
        hunk_index: usize,
        resolution: DiffHunkResolution,
    ) -> Vec<String> {
        let hunk = &file.hunks[hunk_index];
        let mut lines = Vec::new();
        for content in &hunk.hunk_content {
            match *content {
                HunkContent::Context {
                    lines: count,
                    addition_line_index,
                    ..
                } => lines.extend_from_slice(
                    &file.addition_lines[addition_line_index..addition_line_index + count],
                ),
                HunkContent::Change {
                    deletions,
                    deletion_line_index,
                    additions,
                    addition_line_index,
                } => match normalize_diff_resolution(resolution) {
                    NormalizedDiffResolution::Deletions => lines.extend_from_slice(
                        &file.deletion_lines[deletion_line_index..deletion_line_index + deletions],
                    ),
                    NormalizedDiffResolution::Additions => lines.extend_from_slice(
                        &file.addition_lines[addition_line_index..addition_line_index + additions],
                    ),
                    NormalizedDiffResolution::Both => {
                        lines.extend_from_slice(
                            &file.deletion_lines
                                [deletion_line_index..deletion_line_index + deletions],
                        );
                        lines.extend_from_slice(
                            &file.addition_lines
                                [addition_line_index..addition_line_index + additions],
                        );
                    }
                },
            }
        }
        lines
    }

    fn assert_resolved_hunk(file: &FileDiffMetadata, hunk_index: usize, expected_lines: &[String]) {
        let hunk = &file.hunks[hunk_index];
        assert!(
            hunk.hunk_content
                .iter()
                .all(|content| matches!(content, HunkContent::Context { .. }))
        );
        assert_eq!(hunk.addition_lines, 0);
        assert_eq!(hunk.deletion_lines, 0);
        assert_eq!(hunk.addition_count, expected_lines.len());
        assert_eq!(hunk.deletion_count, expected_lines.len());
        assert_eq!(
            &file.addition_lines
                [hunk.addition_line_index..hunk.addition_line_index + expected_lines.len()],
            expected_lines
        );
        assert_eq!(
            &file.deletion_lines
                [hunk.deletion_line_index..hunk.deletion_line_index + expected_lines.len()],
            expected_lines
        );
        verify_file_hunk_values(file).unwrap();
    }

    fn virtual_metrics_fixture() -> VirtualFileMetrics {
        VirtualFileMetrics {
            hunk_line_count: 2,
            line_height: 10,
            diff_header_height: 30,
            spacing: 4,
            padding_top: None,
            padding_bottom: None,
            hunk_separator_height: None,
        }
    }

    fn create_two_hunk_diff() -> FileDiffMetadata {
        let old_lines = (1..=140).map(|index| index.to_string()).collect::<Vec<_>>();
        let new_lines = old_lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                if index == 39 {
                    "changed-40".to_string()
                } else if index == 99 {
                    "changed-100".to_string()
                } else {
                    line.clone()
                }
            })
            .collect::<Vec<_>>();

        parse_diff_from_file(
            &FileContents {
                name: "two-hunks.ts".to_string(),
                contents: format!("{}\n", old_lines.join("\n")),
                lang: None,
                header: None,
                cache_key: None,
            },
            &FileContents {
                name: "two-hunks.ts".to_string(),
                contents: format!("{}\n", new_lines.join("\n")),
                lang: None,
                header: None,
                cache_key: None,
            },
            ParseDiffOptions::default(),
        )
    }

    fn compute_height_for_test(
        file_diff: &FileDiffMetadata,
        options: EstimatedDiffHeightOptions<'_>,
    ) -> EstimatedDiffHeights {
        compute_estimated_diff_heights(file_diff, options).unwrap()
    }

    #[test]
    fn trim_patch_context_matches_pierre_large_context_split() {
        let hunk1_before = build_context(40, "h1-before");
        let hunk1_after = build_context(40, "h1-after");
        let hunk2_before = build_context(40, "h2-before");
        let hunk2_middle = build_context(36, "h2-middle");
        let hunk2_after = build_context(40, "h2-after");

        let patch = [
            vec![
                "diff --git a/file.txt b/file.txt".to_string(),
                "--- a/file.txt".to_string(),
                "+++ b/file.txt".to_string(),
                "@@ -1,82 +1,84 @@".to_string(),
            ],
            hunk1_before.clone(),
            vec![
                "-old-1".to_string(),
                "-old-2".to_string(),
                "+new-1".to_string(),
                "+new-2".to_string(),
                "+new-3".to_string(),
                "+new-4".to_string(),
            ],
            hunk1_after.clone(),
            vec!["@@ -200,118 +200,117 @@".to_string()],
            hunk2_before.clone(),
            vec!["+only-add".to_string()],
            hunk2_middle.clone(),
            vec!["-old-3".to_string(), "-old-4".to_string()],
            hunk2_after.clone(),
        ]
        .concat()
        .join("\n");

        let expected = [
            vec![
                "diff --git a/file.txt b/file.txt".to_string(),
                "--- a/file.txt".to_string(),
                "+++ b/file.txt".to_string(),
                "@@ -31,22 +31,24 @@".to_string(),
            ],
            hunk1_before[30..].to_vec(),
            vec![
                "-old-1".to_string(),
                "-old-2".to_string(),
                "+new-1".to_string(),
                "+new-2".to_string(),
                "+new-3".to_string(),
                "+new-4".to_string(),
            ],
            hunk1_after[..10].to_vec(),
            vec!["@@ -230,20 +230,21 @@".to_string()],
            hunk2_before[30..].to_vec(),
            vec!["+only-add".to_string()],
            hunk2_middle[..10].to_vec(),
            vec!["@@ -266,22 +267,20 @@".to_string()],
            hunk2_middle[26..].to_vec(),
            vec!["-old-3".to_string(), "-old-4".to_string()],
            hunk2_after[..10].to_vec(),
        ]
        .concat()
        .join("\n");

        assert_eq!(trim_patch_context(&patch, 10), expected);
    }

    #[test]
    fn trim_patch_context_omits_single_line_counts_and_drops_context_only_hunks() {
        let patch = [
            "diff --git a/a.txt b/a.txt",
            "--- a/a.txt",
            "+++ b/a.txt",
            "@@ -1,0 +1,1 @@",
            "+hello",
        ]
        .join("\n");

        assert_eq!(
            trim_patch_context(&patch, 0),
            [
                "diff --git a/a.txt b/a.txt",
                "--- a/a.txt",
                "+++ b/a.txt",
                "@@ -1,0 +1 @@",
                "+hello",
            ]
            .join("\n")
        );

        let context_only = [
            "diff --git a/empty.txt b/empty.txt",
            "--- a/empty.txt",
            "+++ b/empty.txt",
            "@@ -1,4 +1,4 @@",
            " one",
            " two",
            " three",
            " four",
        ]
        .join("\n");

        assert_eq!(
            trim_patch_context(&context_only, 10),
            [
                "diff --git a/empty.txt b/empty.txt",
                "--- a/empty.txt",
                "+++ b/empty.txt",
            ]
            .join("\n")
        );
    }

    #[test]
    fn simple_diff_utils_match_pierre_edge_cases() {
        assert_eq!(clean_last_newline("alpha\n"), "alpha");
        assert_eq!(clean_last_newline("alpha\r\n"), "alpha");
        assert_eq!(clean_last_newline("alpha\r"), "alpha\r");
        assert_eq!(clean_last_newline("alpha\n\n"), "alpha\n");

        assert_eq!(get_line_ending_type("a\r\nb\n"), LineEndingType::CRLF);
        assert_eq!(get_line_ending_type("a\rb"), LineEndingType::CR);
        assert_eq!(get_line_ending_type("a\nb"), LineEndingType::LF);
        assert_eq!(get_line_ending_type("ab"), LineEndingType::None);

        assert_eq!(
            parse_line_type("+added"),
            Some(ParsedLine {
                line: "added".to_string(),
                line_type: HunkLineType::Addition,
            })
        );
        assert_eq!(
            parse_line_type("-"),
            Some(ParsedLine {
                line: "\n".to_string(),
                line_type: HunkLineType::Deletion,
            })
        );
        assert_eq!(
            parse_line_type("\\ No newline at end of file"),
            Some(ParsedLine {
                line: " No newline at end of file".to_string(),
                line_type: HunkLineType::Metadata,
            })
        );
        assert_eq!(parse_line_type("x invalid"), None);
        assert_eq!(parse_line_type(""), None);

        assert_eq!(
            get_icon_for_type(DiffIconType::File),
            "diffs-icon-file-code"
        );
        assert_eq!(
            get_icon_for_type(DiffIconType::from(ChangeType::Change)),
            "diffs-icon-symbol-modified"
        );
        assert_eq!(
            get_icon_for_type(DiffIconType::from(ChangeType::New)),
            "diffs-icon-symbol-added"
        );
        assert_eq!(
            get_icon_for_type(DiffIconType::from(ChangeType::Deleted)),
            "diffs-icon-symbol-deleted"
        );
        assert_eq!(
            get_icon_for_type(DiffIconType::from(ChangeType::RenamePure)),
            "diffs-icon-symbol-moved"
        );
        assert_eq!(
            get_icon_for_type(DiffIconType::from(ChangeType::RenameChanged)),
            "diffs-icon-symbol-moved"
        );
    }

    #[test]
    fn untracked_file_diff_preserves_missing_final_newline_metadata() {
        let diff = create_untracked_file_diff("new/no-newline.rs", "fn added() {}");

        assert!(diff.contains("\\ No newline at end of file"));
        let parsed = parse_patch_files(&diff, None, true).unwrap();
        let file = &parsed[0].files[0];
        assert_eq!(file.name, "new/no-newline.rs");
        assert_eq!(file.change_type, ChangeType::New);
        assert_eq!(file.addition_lines, vec!["fn added() {}".to_string()]);
        assert!(file.hunks[0].no_eof_cr_additions);
        assert!(!file.hunks[0].no_eof_cr_deletions);
    }

    #[test]
    fn untracked_file_diff_omits_missing_final_newline_metadata_when_present() {
        let diff = create_untracked_file_diff("new/with-newline.rs", "fn added() {}\n");

        assert!(!diff.contains("\\ No newline at end of file"));
        let parsed = parse_patch_files(&diff, None, true).unwrap();
        let file = &parsed[0].files[0];
        assert_eq!(file.name, "new/with-newline.rs");
        assert_eq!(file.change_type, ChangeType::New);
        assert_eq!(file.addition_lines, vec!["fn added() {}\n".to_string()]);
        assert!(!file.hunks[0].no_eof_cr_additions);
        assert!(!file.hunks[0].no_eof_cr_deletions);
    }

    #[test]
    fn are_files_equal_matches_pierre_cache_identity() {
        let base = FileContents {
            name: "src/main.rs".to_string(),
            contents: "fn main() {}\n".to_string(),
            lang: Some("rust".to_string()),
            header: Some("header-a".to_string()),
            cache_key: Some("cache-a".to_string()),
        };
        let different_header = FileContents {
            header: Some("header-b".to_string()),
            ..base.clone()
        };
        let different_cache_key = FileContents {
            cache_key: Some("cache-b".to_string()),
            ..base.clone()
        };
        let different_lang = FileContents {
            lang: Some("typescript".to_string()),
            ..base.clone()
        };

        assert!(are_files_equal(None, None));
        assert!(!are_files_equal(Some(&base), None));
        assert!(are_files_equal(Some(&base), Some(&different_header)));
        assert!(!are_files_equal(Some(&base), Some(&different_cache_key)));
        assert!(!are_files_equal(Some(&base), Some(&different_lang)));
    }

    #[test]
    fn data_equality_helpers_match_pierre_cache_semantics() {
        let diff_without_cache = parse_diff_from_file(
            &FileContents {
                name: "a.txt".to_string(),
                contents: "old\n".to_string(),
                lang: None,
                header: None,
                cache_key: None,
            },
            &FileContents {
                name: "a.txt".to_string(),
                contents: "new\n".to_string(),
                lang: None,
                header: None,
                cache_key: None,
            },
            ParseDiffOptions::default(),
        );
        let diff_clone_without_cache = diff_without_cache.clone();
        assert!(are_diff_targets_equal(
            Some(&diff_without_cache),
            Some(&diff_without_cache)
        ));
        assert!(!are_diff_targets_equal(
            Some(&diff_without_cache),
            Some(&diff_clone_without_cache)
        ));

        let mut diff_with_cache = diff_without_cache.clone();
        diff_with_cache.cache_key = Some("same-cache".to_string());
        let mut matching_cache = diff_clone_without_cache.clone();
        matching_cache.cache_key = Some("same-cache".to_string());
        let mut different_cache = diff_clone_without_cache.clone();
        different_cache.cache_key = Some("different-cache".to_string());

        assert!(are_diff_targets_equal(
            Some(&diff_with_cache),
            Some(&matching_cache)
        ));
        assert!(!are_diff_targets_equal(
            Some(&diff_with_cache),
            Some(&different_cache)
        ));
        assert!(are_diff_targets_equal(None, None));
        assert!(!are_diff_targets_equal(Some(&diff_with_cache), None));

        let selection = SelectedLineRange {
            start: 3,
            side: Some(SelectionSide::Deletions),
            end: 8,
            end_side: Some(SelectionSide::Additions),
        };
        let same_selection = selection;
        let shifted_selection = SelectedLineRange {
            start: 4,
            ..selection
        };
        assert!(are_selections_equal(None, None));
        assert!(are_selections_equal(
            Some(&selection),
            Some(&same_selection)
        ));
        assert!(!are_selections_equal(
            Some(&selection),
            Some(&shifted_selection)
        ));
        assert!(!are_selections_equal(Some(&selection), None));

        let hunk = HunkData {
            slot_name: "hunk-1".to_string(),
            hunk_index: 1,
            lines: 20,
            column_type: CodeColumnType::Unified,
            expandable: Some(HunkDataExpandable {
                chunked: true,
                up: false,
                down: true,
            }),
        };
        let same_hunk = hunk.clone();
        let different_expandable = HunkData {
            expandable: Some(HunkDataExpandable {
                chunked: true,
                up: true,
                down: true,
            }),
            ..hunk.clone()
        };
        assert!(are_hunk_data_equal(&hunk, &same_hunk));
        assert!(!are_hunk_data_equal(&hunk, &different_expandable));

        let conflict_result = parse_merge_conflict_diff_from_file(
            &FileContents {
                name: "conflict.rs".to_string(),
                contents: [
                    "before",
                    "<<<<<<< HEAD",
                    "ours",
                    "=======",
                    "theirs",
                    ">>>>>>> topic",
                    "after",
                    "",
                ]
                .join("\n"),
                lang: None,
                header: None,
                cache_key: None,
            },
            6,
        )
        .unwrap();
        let action = conflict_result.actions[0].as_ref().unwrap();
        let mut same_action_with_different_marker_text = action.clone();
        same_action_with_different_marker_text.marker_lines.start = "<<<<<<< other\n".to_string();
        assert!(are_merge_conflict_actions_equal(
            action,
            &same_action_with_different_marker_text
        ));

        let mut different_action = action.clone();
        different_action.conflict_data.end_content_index += 1;
        assert!(!are_merge_conflict_actions_equal(action, &different_action));
    }

    #[test]
    fn utility_equality_helpers_match_pierre_non_dom_semantics() {
        let line_annotation = LineAnnotation {
            line_number: 12,
            metadata: Some(serde_json::json!({ "id": "a" })),
        };
        let same_line_annotation = line_annotation.clone();
        let different_line_annotation = LineAnnotation {
            metadata: Some(serde_json::json!({ "id": "b" })),
            ..line_annotation.clone()
        };
        assert!(are_line_annotations_equal(
            &line_annotation,
            &same_line_annotation
        ));
        assert!(!are_line_annotations_equal(
            &line_annotation,
            &different_line_annotation
        ));
        assert_eq!(get_line_annotation_name(&line_annotation), "annotation-12");

        let diff_annotation = DiffLineAnnotation {
            side: SelectionSide::Additions,
            line_number: 7,
            metadata: Some(serde_json::json!("meta")),
        };
        let same_diff_annotation = diff_annotation.clone();
        let different_side_annotation = DiffLineAnnotation {
            side: SelectionSide::Deletions,
            ..diff_annotation.clone()
        };
        assert!(are_diff_line_annotations_equal(
            &diff_annotation,
            &same_diff_annotation
        ));
        assert!(!are_diff_line_annotations_equal(
            &diff_annotation,
            &different_side_annotation
        ));
        assert_eq!(
            get_line_annotation_name(&diff_annotation),
            "annotation-additions-7"
        );

        let object_a = serde_json::json!({
            "theme": "ignored",
            "same": 1,
            "flag": true
        })
        .as_object()
        .unwrap()
        .clone();
        let object_b = serde_json::json!({
            "theme": "also-ignored",
            "same": 1,
            "flag": true
        })
        .as_object()
        .unwrap()
        .clone();
        let object_with_extra_key = serde_json::json!({
            "theme": "ignored",
            "same": 1,
            "flag": true,
            "extra": "nope"
        })
        .as_object()
        .unwrap()
        .clone();
        assert!(are_objects_equal(
            Some(&object_a),
            Some(&object_b),
            &["theme"]
        ));
        assert!(!are_objects_equal(
            Some(&object_a),
            Some(&object_with_extra_key),
            &["theme"]
        ));
        assert!(are_objects_equal(None, None, &[]));
        assert!(!are_objects_equal(Some(&object_a), None, &[]));

        let theme = ThemeSpec::Pair {
            dark: "pierre-dark".to_string(),
            light: "pierre-light".to_string(),
        };
        let same_theme = theme.clone();
        let different_theme = ThemeSpec::Name("pierre-dark".to_string());
        assert!(are_themes_equal(Some(&theme), Some(&same_theme)));
        assert!(!are_themes_equal(Some(&theme), Some(&different_theme)));
        assert!(are_themes_equal(None, None));

        let file_options = RenderFileOptions {
            theme: theme.clone(),
            use_token_transformer: true,
            tokenize_max_line_length: 1_000,
        };
        let same_file_options = file_options.clone();
        let different_file_options = RenderFileOptions {
            tokenize_max_line_length: 2_000,
            ..file_options.clone()
        };
        assert!(are_file_render_options_equal(
            &file_options,
            &same_file_options
        ));
        assert!(!are_file_render_options_equal(
            &file_options,
            &different_file_options
        ));

        let diff_options = RenderDiffOptions {
            theme: theme.clone(),
            use_token_transformer: true,
            tokenize_max_line_length: 1_000,
            line_diff_type: LineDiffType::WordAlt,
            max_line_diff_length: 500,
        };
        let same_diff_options = diff_options.clone();
        let different_diff_options = RenderDiffOptions {
            line_diff_type: LineDiffType::Char,
            ..diff_options.clone()
        };
        assert!(are_diff_render_options_equal(
            &diff_options,
            &same_diff_options
        ));
        assert!(!are_diff_render_options_equal(
            &diff_options,
            &different_diff_options
        ));

        let pre_props = PrePropertiesConfig {
            node_type: PreNodeType::Diff,
            diff_indicators: DiffIndicators::Bars,
            disable_background: false,
            disable_line_numbers: false,
            overflow: CodeOverflow::Scroll,
            split: true,
            total_lines: 42,
            custom_properties: Some(object_a.clone()),
        };
        let same_pre_props = pre_props.clone();
        let different_pre_props = PrePropertiesConfig {
            split: false,
            ..pre_props.clone()
        };
        assert!(are_pre_properties_equal(
            Some(&pre_props),
            Some(&same_pre_props)
        ));
        assert!(!are_pre_properties_equal(
            Some(&pre_props),
            Some(&different_pre_props)
        ));
        assert!(are_pre_properties_equal(None, None));

        let stats = WorkerStats {
            busy_workers: 1,
            diff_cache_size: 2,
            file_cache_size: 3,
            manager_state: "ready".to_string(),
            active_tasks: 4,
            queued_tasks: 5,
            theme_subscribers: 6,
            total_workers: 7,
            workers_failed: 8,
        };
        let same_stats = stats.clone();
        let different_stats = WorkerStats {
            queued_tasks: 9,
            ..stats.clone()
        };
        assert!(are_worker_stats_equal(Some(&stats), Some(&same_stats)));
        assert!(!are_worker_stats_equal(
            Some(&stats),
            Some(&different_stats)
        ));
        assert!(are_worker_stats_equal(None, None));
    }

    #[test]
    fn merge_conflict_line_types_match_pierre_stack_parser() {
        let lines = split_file_contents_owned("const a = 1;\nconst b = 2;\n");
        assert_eq!(
            get_merge_conflict_line_types(&lines),
            vec![MergeConflictLineType::None, MergeConflictLineType::None]
        );

        let lines = split_file_contents_owned(
            &[
                "before",
                "<<<<<<< HEAD",
                "ours",
                "||||||| base",
                "base",
                "=======",
                "theirs",
                ">>>>>>> feature",
                "after",
            ]
            .join("\n"),
        );
        let result = get_merge_conflict_parse_result(&lines);
        assert_eq!(
            result.line_types,
            vec![
                MergeConflictLineType::None,
                MergeConflictLineType::MarkerStart,
                MergeConflictLineType::Current,
                MergeConflictLineType::MarkerBase,
                MergeConflictLineType::Base,
                MergeConflictLineType::MarkerSeparator,
                MergeConflictLineType::Incoming,
                MergeConflictLineType::MarkerEnd,
                MergeConflictLineType::None,
            ]
        );
        assert_eq!(
            result.regions,
            vec![MergeConflictRegion {
                conflict_index: 0,
                start_line_index: 1,
                start_line_number: 2,
                separator_line_index: 5,
                separator_line_number: 6,
                end_line_index: 7,
                end_line_number: 8,
                base_marker_line_index: Some(3),
                base_marker_line_number: Some(4),
            }]
        );
        assert_eq!(get_merge_conflict_action_line_number(&result.regions[0]), 1);

        let nested = split_file_contents_owned(
            &[
                "<<<<<<< HEAD",
                "outer ours",
                "<<<<<<< HEAD",
                "inner ours",
                "=======",
                "inner theirs",
                ">>>>>>> topic",
                "=======",
                "outer theirs",
                ">>>>>>> main",
            ]
            .join("\n"),
        );
        assert_eq!(
            get_merge_conflict_line_types(&nested),
            vec![
                MergeConflictLineType::MarkerStart,
                MergeConflictLineType::Current,
                MergeConflictLineType::MarkerStart,
                MergeConflictLineType::Current,
                MergeConflictLineType::MarkerSeparator,
                MergeConflictLineType::Incoming,
                MergeConflictLineType::MarkerEnd,
                MergeConflictLineType::MarkerSeparator,
                MergeConflictLineType::Incoming,
                MergeConflictLineType::MarkerEnd,
            ]
        );
    }

    #[test]
    fn merge_conflict_marker_helpers_match_pierre_edges() {
        let lines = vec![
            "<<<<<<<HEAD\n".to_string(),
            "<<<<<<< HEAD\r\n".to_string(),
            "======= trailing label".to_string(),
            "=======".to_string(),
            ">>>>>>> branch\r".to_string(),
        ];
        assert_eq!(
            get_merge_conflict_line_types(&lines),
            vec![
                MergeConflictLineType::None,
                MergeConflictLineType::MarkerStart,
                MergeConflictLineType::Current,
                MergeConflictLineType::MarkerSeparator,
                MergeConflictLineType::MarkerEnd,
            ]
        );

        assert_eq!(
            get_merge_conflict_action_slot_name(MergeConflictActionSlotInput {
                hunk_index: 2,
                line_index: 17,
                conflict_index: 4,
            }),
            "merge-conflict-action-2-17-4"
        );
        assert_eq!(
            get_merge_conflict_action_line_number(&MergeConflictRegion {
                conflict_index: 0,
                start_line_index: 0,
                start_line_number: 1,
                separator_line_index: 2,
                separator_line_number: 3,
                end_line_index: 4,
                end_line_number: 5,
                base_marker_line_index: None,
                base_marker_line_number: None,
            }),
            1
        );
        assert_eq!(
            get_hunk_separator_slot_name(CodeColumnType::Unified, 3),
            "hunk-separator-unified-3"
        );
        assert_eq!(
            get_hunk_separator_slot_name(CodeColumnType::Additions, 4),
            "hunk-separator-additions-4"
        );
        assert_eq!(
            get_hunk_separator_slot_name(CodeColumnType::Deletions, 5),
            "hunk-separator-deletions-5"
        );
    }

    fn create_merge_conflict_resolution_fixture() -> (FileDiffMetadata, ProcessFileConflictData) {
        let hunk = Hunk {
            collapsed_before: 0,
            split_line_count: 3,
            split_line_start: 0,
            unified_line_count: 3,
            unified_line_start: 0,
            addition_count: 2,
            addition_start: 1,
            addition_lines: 2,
            addition_line_index: 0,
            deletion_count: 2,
            deletion_start: 1,
            deletion_lines: 2,
            deletion_line_index: 0,
            hunk_content: vec![
                HunkContent::Change {
                    deletions: 1,
                    deletion_line_index: 0,
                    additions: 0,
                    addition_line_index: 0,
                },
                HunkContent::Context {
                    lines: 1,
                    addition_line_index: 0,
                    deletion_line_index: 1,
                },
                HunkContent::Change {
                    deletions: 0,
                    deletion_line_index: 2,
                    additions: 1,
                    addition_line_index: 1,
                },
            ],
            hunk_context: None,
            hunk_specs: "@@ -1,2 +1,2 @@\n".to_string(),
            no_eof_cr_additions: false,
            no_eof_cr_deletions: false,
        };

        (
            FileDiffMetadata {
                name: "conflict.txt".to_string(),
                prev_name: None,
                new_object_id: None,
                prev_object_id: None,
                mode: None,
                prev_mode: None,
                change_type: ChangeType::Change,
                hunks: vec![hunk],
                split_line_count: 3,
                unified_line_count: 3,
                is_partial: false,
                deletion_lines: vec!["ours\n".to_string(), "base\n".to_string()],
                addition_lines: vec!["base\n".to_string(), "theirs\n".to_string()],
                cache_key: Some("conflict-key".to_string()),
            },
            ProcessFileConflictData {
                hunk_index: 0,
                start_content_index: 0,
                end_content_index: 2,
                current_content_index: Some(0),
                base_content_index: Some(1),
                incoming_content_index: Some(2),
                end_marker_content_index: 2,
            },
        )
    }

    #[test]
    fn resolve_conflict_strips_base_context_and_resolves_selected_side() {
        let (diff, conflict) = create_merge_conflict_resolution_fixture();

        let incoming = resolve_conflict(&diff, &conflict, MergeConflictResolution::Incoming)
            .expect("incoming conflict should resolve");
        assert_eq!(incoming.cache_key.as_deref(), Some("conflict-key:a-0:0-2"));
        assert_eq!(incoming.deletion_lines, vec!["theirs\n".to_string()]);
        assert_eq!(incoming.addition_lines, vec!["theirs\n".to_string()]);
        assert_eq!(
            incoming.hunks[0].hunk_content,
            vec![
                HunkContent::Context {
                    lines: 0,
                    deletion_line_index: 0,
                    addition_line_index: 0,
                },
                HunkContent::Context {
                    lines: 0,
                    deletion_line_index: 0,
                    addition_line_index: 0,
                },
                HunkContent::Context {
                    lines: 1,
                    deletion_line_index: 0,
                    addition_line_index: 0,
                },
            ]
        );
        assert_eq!(incoming.hunks[0].deletion_count, 1);
        assert_eq!(incoming.hunks[0].addition_count, 1);
        assert_eq!(incoming.hunks[0].split_line_count, 1);
        assert_eq!(incoming.hunks[0].unified_line_count, 1);

        let current = resolve_conflict(&diff, &conflict, MergeConflictResolution::Current)
            .expect("current conflict should resolve");
        assert_eq!(current.cache_key.as_deref(), Some("conflict-key:d-0:0-2"));
        assert_eq!(current.deletion_lines, vec!["ours\n".to_string()]);
        assert_eq!(current.addition_lines, vec!["ours\n".to_string()]);

        let both = resolve_conflict(&diff, &conflict, MergeConflictResolution::Both)
            .expect("both conflict should resolve");
        assert_eq!(both.cache_key.as_deref(), Some("conflict-key:b-0:0-2"));
        assert_eq!(
            both.deletion_lines,
            vec!["ours\n".to_string(), "theirs\n".to_string()]
        );
        assert_eq!(
            both.addition_lines,
            vec!["ours\n".to_string(), "theirs\n".to_string()]
        );
        assert_eq!(both.hunks[0].deletion_count, 2);
        assert_eq!(both.hunks[0].addition_count, 2);
        verify_file_hunk_values(&both).unwrap();
    }

    #[test]
    fn resolve_merge_conflict_contents_replaces_only_selected_marker_block() {
        let contents = [
            "before\n",
            "<<<<<<< HEAD\n",
            "ours\n",
            "||||||| base\n",
            "base\n",
            "=======\n",
            "theirs\n",
            ">>>>>>> feature\n",
            "middle\n",
            "<<<<<<< HEAD\n",
            "second ours\n",
            "=======\n",
            "second theirs\n",
            ">>>>>>> feature\n",
            "after\n",
        ]
        .concat();
        let parsed = parse_merge_conflict_diff_from_file(
            &FileContents {
                name: "conflict.txt".to_string(),
                contents: contents.clone(),
                lang: None,
                header: None,
                cache_key: None,
            },
            6,
        )
        .unwrap();
        let first = parsed.actions[0].as_ref().unwrap();
        let incoming = resolve_merge_conflict_contents(
            &contents,
            &first.conflict,
            MergeConflictResolution::Incoming,
        );

        assert_eq!(
            incoming,
            [
                "before\n",
                "theirs\n",
                "middle\n",
                "<<<<<<< HEAD\n",
                "second ours\n",
                "=======\n",
                "second theirs\n",
                ">>>>>>> feature\n",
                "after\n",
            ]
            .concat()
        );
    }

    #[test]
    fn merge_conflict_diff_view_maps_selected_rows_to_conflict_index() {
        let parsed = parse_merge_conflict_diff_from_file(
            &FileContents {
                name: "conflict.txt".to_string(),
                contents: [
                    "before\n",
                    "<<<<<<< HEAD\n",
                    "ours\n",
                    "=======\n",
                    "theirs\n",
                    ">>>>>>> feature\n",
                    "after\n",
                ]
                .concat(),
                lang: None,
                header: None,
                cache_key: None,
            },
            6,
        )
        .unwrap();
        let mut view = build_merge_conflict_diff_view(&parsed, None, None);
        let rendered = view.rendered_lines(DiffViewMode::Unified, 120).to_vec();
        let rows = render_lines_to_strings(rendered, 120);

        assert!(
            rows.iter()
                .any(|row| row.contains("1 Accept current change"))
        );
        assert!(rows.iter().any(|row| row.contains("<<<<<<< HEAD")));
        assert!(rows.iter().any(|row| row.contains("Current Change")));
        assert!(rows.iter().any(|row| row.contains("Incoming Change")));
        assert_eq!(
            view.selected_conflict_index(DiffViewMode::Unified, 120, 2),
            Some(0)
        );
        assert_eq!(
            view.selected_conflict_index(DiffViewMode::Unified, 120, 4),
            Some(0)
        );
    }

    #[test]
    fn parse_merge_conflict_diff_from_file_creates_current_incoming_diff() {
        let result = parse_merge_conflict_diff_from_file(
            &FileContents {
                name: "session.ts".to_string(),
                contents: [
                    "const start = true;",
                    "<<<<<<< HEAD",
                    "const ttl = 12;",
                    "=======",
                    "const ttl = 24;",
                    ">>>>>>> feature",
                    "const end = true;",
                    "",
                ]
                .join("\n"),
                lang: None,
                header: None,
                cache_key: Some("session-cache".to_string()),
            },
            6,
        )
        .unwrap();

        assert!(result.current_file.contents.contains("const ttl = 12;\n"));
        assert!(!result.current_file.contents.contains("<<<<<<< HEAD\n"));
        assert!(!result.current_file.contents.contains("const ttl = 24;\n"));
        assert!(result.incoming_file.contents.contains("const ttl = 24;\n"));
        assert!(!result.incoming_file.contents.contains("const ttl = 12;\n"));
        assert_eq!(
            result.current_file.cache_key.as_deref(),
            Some("session-cache:merge-conflict-current")
        );
        assert_eq!(
            result.incoming_file.cache_key.as_deref(),
            Some("session-cache:merge-conflict-incoming")
        );
        assert_eq!(
            result.file_diff.cache_key.as_deref(),
            Some("session-cache:merge-conflict-diff")
        );
        assert_eq!(
            result.file_diff.deletion_lines,
            split_file_contents_owned(&result.current_file.contents)
        );
        assert_eq!(
            result.file_diff.addition_lines,
            split_file_contents_owned(&result.incoming_file.contents)
        );

        let action = result.actions[0].as_ref().unwrap();
        assert_eq!(action.conflict_index, 0);
        assert_eq!(action.conflict_data.hunk_index, 0);
        assert_eq!(action.conflict_data.start_content_index, 1);
        assert_eq!(action.conflict_data.current_content_index, Some(1));
        assert_eq!(action.conflict_data.incoming_content_index, Some(1));
        assert_eq!(action.conflict_data.end_marker_content_index, 1);
        assert_eq!(action.marker_lines.start, "<<<<<<< HEAD\n");
        assert_eq!(action.marker_lines.separator, "=======\n");
        assert_eq!(action.marker_lines.end, ">>>>>>> feature\n");
        assert_eq!(
            action.conflict,
            MergeConflictRegion {
                conflict_index: 0,
                start_line_index: 1,
                start_line_number: 2,
                separator_line_index: 3,
                separator_line_number: 4,
                end_line_index: 5,
                end_line_number: 6,
                base_marker_line_index: None,
                base_marker_line_number: None,
            }
        );
        assert_eq!(result.marker_rows.len(), 3);
    }

    #[test]
    fn parse_merge_conflict_diff_from_file_preserves_diff3_base_context() {
        let result = parse_merge_conflict_diff_from_file(
            &FileContents {
                name: "merge.ts".to_string(),
                contents: [
                    "before",
                    "<<<<<<< HEAD",
                    "ours",
                    "||||||| base",
                    "base value",
                    "=======",
                    "theirs",
                    ">>>>>>> topic",
                    "after",
                    "",
                ]
                .join("\n"),
                lang: None,
                header: None,
                cache_key: None,
            },
            6,
        )
        .unwrap();

        assert!(result.current_file.contents.contains("ours\n"));
        assert!(result.current_file.contents.contains("base value\n"));
        assert!(!result.current_file.contents.contains("theirs\n"));
        assert!(result.incoming_file.contents.contains("theirs\n"));
        assert!(result.incoming_file.contents.contains("base value\n"));
        assert!(!result.incoming_file.contents.contains("ours\n"));

        let action = result.actions[0].as_ref().unwrap();
        assert_eq!(action.conflict_data.start_content_index, 1);
        assert_eq!(action.conflict_data.current_content_index, Some(1));
        assert_eq!(action.conflict_data.base_content_index, Some(2));
        assert_eq!(action.conflict_data.incoming_content_index, Some(3));
        assert_eq!(action.conflict_data.end_marker_content_index, 3);
        assert_eq!(action.marker_lines.base.as_deref(), Some("||||||| base\n"));
        assert_eq!(
            action.conflict,
            MergeConflictRegion {
                conflict_index: 0,
                start_line_index: 1,
                start_line_number: 2,
                separator_line_index: 5,
                separator_line_number: 6,
                end_line_index: 7,
                end_line_number: 8,
                base_marker_line_index: Some(3),
                base_marker_line_number: Some(4),
            }
        );
        assert_eq!(
            result
                .marker_rows
                .iter()
                .map(|row| row.row_type)
                .collect::<Vec<_>>(),
            vec![
                MergeConflictMarkerRowType::MarkerStart,
                MergeConflictMarkerRowType::MarkerBase,
                MergeConflictMarkerRowType::MarkerSeparator,
                MergeConflictMarkerRowType::MarkerEnd,
            ]
        );
    }

    #[test]
    fn parse_merge_conflict_diff_from_file_leaves_plain_files_without_hunks() {
        let contents = "one\ntwo\nthree\n".to_string();
        let result = parse_merge_conflict_diff_from_file(
            &FileContents {
                name: "plain.ts".to_string(),
                contents: contents.clone(),
                lang: None,
                header: None,
                cache_key: Some("plain-cache".to_string()),
            },
            6,
        )
        .unwrap();

        assert_eq!(result.current_file.contents, contents);
        assert_eq!(result.incoming_file.contents, contents);
        assert!(result.file_diff.hunks.is_empty());
        assert!(result.actions.is_empty());
        assert!(result.marker_rows.is_empty());
        assert_eq!(result.file_diff.split_line_count, 0);
        assert_eq!(result.file_diff.unified_line_count, 0);
    }

    #[test]
    fn parse_merge_conflict_diff_from_file_splits_large_context_gaps() {
        let result = parse_merge_conflict_diff_from_file(
            &FileContents {
                name: "split.ts".to_string(),
                contents: [
                    "pre-0",
                    "pre-1",
                    "<<<<<<< A",
                    "ours-1",
                    "=======",
                    "theirs-1",
                    ">>>>>>> B",
                    "gap-0",
                    "gap-1",
                    "gap-2",
                    "gap-3",
                    "<<<<<<< A",
                    "ours-2",
                    "=======",
                    "theirs-2",
                    ">>>>>>> B",
                    "post-0",
                    "post-1",
                    "",
                ]
                .join("\n"),
                lang: None,
                header: None,
                cache_key: None,
            },
            1,
        )
        .unwrap();

        assert_eq!(result.file_diff.hunks.len(), 2);
        assert_eq!(result.actions.len(), 2);
        assert_eq!(
            result.actions[0].as_ref().unwrap().conflict_data.hunk_index,
            0
        );
        assert_eq!(
            result.actions[1].as_ref().unwrap().conflict_data.hunk_index,
            1
        );
        assert_eq!(result.file_diff.hunks[0].collapsed_before, 1);
        assert_eq!(result.file_diff.hunks[1].collapsed_before, 2);
        assert_eq!(result.file_diff.hunks[0].addition_start, 2);
        assert_eq!(result.file_diff.hunks[1].addition_start, 7);
        assert_eq!(result.file_diff.hunks[0].hunk_content.len(), 3);
        assert_eq!(result.file_diff.hunks[1].hunk_content.len(), 3);
        assert_eq!(result.marker_rows.len(), 6);

        let first_anchor = get_merge_conflict_action_anchor(
            result.actions[0].as_ref().unwrap(),
            &result.file_diff,
        );
        assert_eq!(
            first_anchor,
            Some(MergeConflictActionAnchor {
                hunk_index: 0,
                line_index: 2,
            })
        );
        let second_anchor = get_merge_conflict_action_anchor(
            result.actions[1].as_ref().unwrap(),
            &result.file_diff,
        );
        assert_eq!(
            second_anchor,
            Some(MergeConflictActionAnchor {
                hunk_index: 1,
                line_index: 8,
            })
        );

        let mut missing_hunk_action = result.actions[0].as_ref().unwrap().clone();
        missing_hunk_action.conflict_data.hunk_index = 99;
        assert_eq!(
            get_merge_conflict_action_anchor(&missing_hunk_action, &result.file_diff),
            None
        );
    }

    #[test]
    fn parse_merge_conflict_diff_from_file_anchors_empty_current_side() {
        let result = parse_merge_conflict_diff_from_file(
            &FileContents {
                name: "empty-current.ts".to_string(),
                contents: [
                    "before",
                    "<<<<<<< HEAD",
                    "=======",
                    "incoming only",
                    ">>>>>>> topic",
                    "after",
                    "",
                ]
                .join("\n"),
                lang: None,
                header: None,
                cache_key: None,
            },
            6,
        )
        .unwrap();

        assert_eq!(result.current_file.contents, "before\nafter\n");
        assert_eq!(
            result.incoming_file.contents,
            "before\nincoming only\nafter\n"
        );
        let action = result.actions[0].as_ref().unwrap();
        assert_eq!(action.conflict_data.start_content_index, 1);
        assert_eq!(action.conflict_data.current_content_index, Some(1));
        assert_eq!(action.conflict_data.incoming_content_index, Some(1));
        assert_eq!(action.conflict_data.end_marker_content_index, 1);

        let current = resolve_conflict(
            &result.file_diff,
            &action.conflict_data,
            MergeConflictResolution::Current,
        )
        .unwrap();
        assert_eq!(current.deletion_lines, vec!["before\n", "after\n"]);
        assert_eq!(current.addition_lines, vec!["before\n", "after\n"]);

        let incoming = resolve_conflict(
            &result.file_diff,
            &action.conflict_data,
            MergeConflictResolution::Incoming,
        )
        .unwrap();
        assert_eq!(
            incoming.deletion_lines,
            vec!["before\n", "incoming only\n", "after\n"]
        );
        assert_eq!(
            incoming.addition_lines,
            vec!["before\n", "incoming only\n", "after\n"]
        );
        assert_eq!(
            result
                .marker_rows
                .iter()
                .map(|row| row.row_type)
                .collect::<Vec<_>>(),
            vec![
                MergeConflictMarkerRowType::MarkerStart,
                MergeConflictMarkerRowType::MarkerSeparator,
                MergeConflictMarkerRowType::MarkerEnd,
            ]
        );
    }

    #[test]
    fn parse_merge_conflict_diff_from_file_anchors_empty_incoming_side() {
        let result = parse_merge_conflict_diff_from_file(
            &FileContents {
                name: "empty-incoming.ts".to_string(),
                contents: [
                    "before",
                    "<<<<<<< HEAD",
                    "current only",
                    "=======",
                    ">>>>>>> topic",
                    "after",
                    "",
                ]
                .join("\n"),
                lang: None,
                header: None,
                cache_key: None,
            },
            6,
        )
        .unwrap();

        assert_eq!(
            result.current_file.contents,
            "before\ncurrent only\nafter\n"
        );
        assert_eq!(result.incoming_file.contents, "before\nafter\n");
        let action = result.actions[0].as_ref().unwrap();
        assert_eq!(action.conflict_data.start_content_index, 1);
        assert_eq!(action.conflict_data.current_content_index, Some(1));
        assert_eq!(action.conflict_data.incoming_content_index, Some(1));
        assert_eq!(action.conflict_data.end_marker_content_index, 1);

        let current = resolve_conflict(
            &result.file_diff,
            &action.conflict_data,
            MergeConflictResolution::Current,
        )
        .unwrap();
        assert_eq!(
            current.deletion_lines,
            vec!["before\n", "current only\n", "after\n"]
        );
        assert_eq!(
            current.addition_lines,
            vec!["before\n", "current only\n", "after\n"]
        );

        let incoming = resolve_conflict(
            &result.file_diff,
            &action.conflict_data,
            MergeConflictResolution::Incoming,
        )
        .unwrap();
        assert_eq!(incoming.deletion_lines, vec!["before\n", "after\n"]);
        assert_eq!(incoming.addition_lines, vec!["before\n", "after\n"]);
        assert_eq!(
            result
                .marker_rows
                .iter()
                .map(|row| row.row_type)
                .collect::<Vec<_>>(),
            vec![
                MergeConflictMarkerRowType::MarkerStart,
                MergeConflictMarkerRowType::MarkerSeparator,
                MergeConflictMarkerRowType::MarkerEnd,
            ]
        );
    }

    #[test]
    fn get_singular_patch_requires_one_patch_with_one_file() {
        let one_file = [
            "diff --git a/a.txt b/a.txt",
            "--- a/a.txt",
            "+++ b/a.txt",
            "@@ -1 +1 @@",
            "-old",
            "+new",
        ]
        .join("\n");

        let file = get_singular_patch(&one_file).unwrap();
        assert_eq!(file.name, "a.txt");
        assert_eq!(file.hunks.len(), 1);

        let two_files = [
            one_file.as_str(),
            "diff --git a/b.txt b/b.txt",
            "--- a/b.txt",
            "+++ b/b.txt",
            "@@ -1 +1 @@",
            "-old",
            "+new",
        ]
        .join("\n");
        assert!(get_singular_patch(&two_files).is_err());

        let two_patches = [
            "From abc Mon Sep 17 00:00:00 2001",
            "diff --git a/a.txt b/a.txt",
            "--- a/a.txt",
            "+++ b/a.txt",
            "@@ -1 +1 @@",
            "-old",
            "+new",
            "From def Mon Sep 17 00:00:00 2001",
            "diff --git a/b.txt b/b.txt",
            "--- a/b.txt",
            "+++ b/b.txt",
            "@@ -1 +1 @@",
            "-old",
            "+new",
        ]
        .join("\n");
        assert!(get_singular_patch(&two_patches).is_err());
    }

    #[test]
    fn diff_accept_reject_hunk_resolves_whole_hunks_and_reindexes_later_hunks() {
        let diff = create_resolution_fixture();
        let trailing_before = hunk_lines(&diff, 3);
        let expected_accept = expected_resolved_hunk_lines(&diff, 2, DiffHunkResolution::Accept);
        let expected_reject = expected_resolved_hunk_lines(&diff, 2, DiffHunkResolution::Reject);
        let expected_both = expected_resolved_hunk_lines(&diff, 3, DiffHunkResolution::Both);

        let accepted = diff_accept_reject_hunk(&diff, 2, DiffHunkResolution::Accept).unwrap();
        assert_eq!(
            accepted.cache_key.as_deref(),
            Some("old-key:new-key:a-2:0-2")
        );
        assert_resolved_hunk(&accepted, 2, &expected_accept);
        assert_eq!(hunk_lines(&accepted, 3), trailing_before);

        let rejected = diff_accept_reject_hunk(&diff, 2, DiffHunkResolution::Reject).unwrap();
        assert_eq!(
            rejected.cache_key.as_deref(),
            Some("old-key:new-key:d-2:0-2")
        );
        assert_resolved_hunk(&rejected, 2, &expected_reject);
        assert_eq!(hunk_lines(&rejected, 3), trailing_before);

        let both = diff_accept_reject_hunk(&diff, 3, DiffHunkResolution::Both).unwrap();
        assert_eq!(both.cache_key.as_deref(), Some("old-key:new-key:b-3:0-4"));
        assert_resolved_hunk(&both, 3, &expected_both);
    }

    #[test]
    fn diff_accept_reject_content_resolves_one_change_block_and_updates_cache_key() {
        let diff = create_resolution_fixture();
        let expected = expected_resolved_hunk_lines(&diff, 2, DiffHunkResolution::Accept);
        let result = diff_accept_reject_content(&diff, 2, 1, DiffHunkResolution::Accept).unwrap();

        assert_eq!(result.cache_key.as_deref(), Some("old-key:new-key:a-2:1-1"));
        let hunk = &result.hunks[2];
        assert!(matches!(hunk.hunk_content[1], HunkContent::Context { .. }));
        assert_eq!(
            &result.addition_lines
                [hunk.addition_line_index..hunk.addition_line_index + hunk.addition_count],
            expected.as_slice()
        );
        verify_file_hunk_values(&result).unwrap();
    }

    #[test]
    fn diff_accept_reject_hunk_resolves_partial_patches_without_materializing_omitted_context() {
        let patch = "\
diff --git a/index.html b/index.html
index 36c553c..711c67c 100644
--- a/index.html
+++ b/index.html
@@ -6,8 +6,9 @@
 </head>
 <body>
 <header>
-  <h1>Welcome</h1>
-  <p>Thanks for visiting</p>
+  <h1>Welcome to Our Site</h1>
+  <p>We're glad you're here</p>
+  <a href=\"/about\" class=\"btn\">Learn More</a>
 </header>
 <footer>
   <p>&copy; Acme Inc.</p>";
        let diff = parse_patch_files(patch, None, true).unwrap()[0].files[0].clone();
        let expected = expected_resolved_hunk_lines(&diff, 0, DiffHunkResolution::Accept);

        let result = diff_accept_reject_hunk(&diff, 0, DiffHunkResolution::Accept).unwrap();
        let hunk = &result.hunks[0];

        assert!(result.is_partial);
        assert_eq!(result.deletion_lines, expected);
        assert_eq!(result.addition_lines, expected);
        assert_eq!(hunk.collapsed_before, 5);
        assert_eq!(hunk.addition_start, 6);
        assert_eq!(hunk.deletion_start, 6);
        assert_eq!(hunk.addition_line_index, 0);
        assert_eq!(hunk.deletion_line_index, 0);
        assert_eq!(result.split_line_count, 14);
        assert_eq!(result.unified_line_count, 14);
        verify_file_hunk_values(&result).unwrap();
    }

    #[test]
    fn diff_accept_reject_hunk_both_inherits_no_eof_cr_from_additions() {
        let diff = parse_diff_from_file(
            &FileContents {
                name: "example.ts".to_string(),
                contents: "start\nold\n".to_string(),
                lang: None,
                header: None,
                cache_key: None,
            },
            &FileContents {
                name: "example.ts".to_string(),
                contents: "start\nnew".to_string(),
                lang: None,
                header: None,
                cache_key: None,
            },
            ParseDiffOptions::default(),
        );
        let expected_lines = vec![
            "start\n".to_string(),
            "old\n".to_string(),
            "new".to_string(),
        ];

        let result = diff_accept_reject_hunk(&diff, 0, DiffHunkResolution::Both).unwrap();
        let hunk = &result.hunks[0];

        assert!(hunk.no_eof_cr_additions);
        assert!(hunk.no_eof_cr_deletions);
        assert_eq!(result.deletion_lines, expected_lines);
        assert_eq!(result.addition_lines, expected_lines);
    }

    #[test]
    fn render_range_and_virtual_window_helpers_match_pierre() {
        let default_range = RenderRange::default();
        let bounded_range = RenderRange {
            starting_line: 12,
            total_lines: Some(30),
            buffer_before: 4,
            buffer_after: 8,
        };
        let same_bounded_range = RenderRange {
            starting_line: 12,
            total_lines: Some(30),
            buffer_before: 4,
            buffer_after: 8,
        };

        assert!(is_default_render_range(&default_range));
        assert!(!is_default_render_range(&bounded_range));
        assert!(are_render_ranges_equal(None, None));
        assert!(!are_render_ranges_equal(Some(&default_range), None));
        assert!(are_render_ranges_equal(
            Some(&bounded_range),
            Some(&same_bounded_range)
        ));
        assert!(!are_render_ranges_equal(
            Some(&default_range),
            Some(&bounded_range)
        ));

        let window = VirtualWindowSpecs {
            top: 10.0,
            bottom: 120.0,
        };
        let same_window = VirtualWindowSpecs {
            top: 10.0,
            bottom: 120.0,
        };
        let shifted_window = VirtualWindowSpecs {
            top: 11.0,
            bottom: 120.0,
        };

        assert!(are_virtual_window_specs_equal(None, None));
        assert!(!are_virtual_window_specs_equal(Some(&window), None));
        assert!(are_virtual_window_specs_equal(
            Some(&window),
            Some(&same_window)
        ));
        assert!(!are_virtual_window_specs_equal(
            Some(&window),
            Some(&shifted_window)
        ));
    }

    #[test]
    fn create_window_from_scroll_position_matches_pierre_edge_cases() {
        assert_eq!(
            create_window_from_scroll_position(WindowFromScrollPositionOptions {
                scroll_top: 0.0,
                height: 100.0,
                scroll_height: 1000.0,
                fit_perfectly: false,
                fit_perfectly_overscroll: 0.0,
                overscroll_size: 25.0,
            }),
            VirtualWindowSpecs {
                top: 0.0,
                bottom: 125.0,
            }
        );
        assert_eq!(
            create_window_from_scroll_position(WindowFromScrollPositionOptions {
                scroll_top: 475.25,
                height: 100.0,
                scroll_height: 1000.0,
                fit_perfectly: false,
                fit_perfectly_overscroll: 0.0,
                overscroll_size: 30.0,
            }),
            VirtualWindowSpecs {
                top: 445.0,
                bottom: 606.0,
            }
        );
        assert_eq!(
            create_window_from_scroll_position(WindowFromScrollPositionOptions {
                scroll_top: 930.0,
                height: 100.0,
                scroll_height: 1000.0,
                fit_perfectly: false,
                fit_perfectly_overscroll: 0.0,
                overscroll_size: 40.0,
            }),
            VirtualWindowSpecs {
                top: 890.0,
                bottom: 1000.0,
            }
        );
        assert_eq!(
            create_window_from_scroll_position(WindowFromScrollPositionOptions {
                scroll_top: 12.5,
                height: 100.0,
                scroll_height: 90.0,
                fit_perfectly: false,
                fit_perfectly_overscroll: 0.0,
                overscroll_size: 20.0,
            }),
            VirtualWindowSpecs {
                top: 12.5,
                bottom: 140.0,
            }
        );
        assert_eq!(
            create_window_from_scroll_position(WindowFromScrollPositionOptions {
                scroll_top: 150.0,
                height: 100.0,
                scroll_height: 1000.0,
                fit_perfectly: true,
                fit_perfectly_overscroll: 15.0,
                overscroll_size: 50.0,
            }),
            VirtualWindowSpecs {
                top: 135.0,
                bottom: 280.0,
            }
        );
    }

    #[test]
    fn get_total_line_count_from_hunks_matches_pierre() {
        assert_eq!(get_total_line_count_from_hunks(&[]), 0);

        let diff = create_two_hunk_diff();
        let last_hunk = diff.hunks.last().unwrap();
        assert_eq!(
            get_total_line_count_from_hunks(&diff.hunks),
            (last_hunk.addition_start + last_hunk.addition_count)
                .max(last_hunk.deletion_start + last_hunk.deletion_count)
        );
    }

    #[test]
    fn virtual_diff_layout_helpers_match_pierre_separator_and_expansion_rules() {
        let metrics = virtual_metrics_fixture();
        assert_eq!(
            get_expanded_region_public(false, 10, None, 1, 1),
            ExpandedRegion {
                from_start: 0,
                from_end: 0,
                range_size: 10,
                collapsed_lines: 10,
                render_all: false,
            }
        );
        assert_eq!(
            get_expanded_region_public(false, 10, Some(ExpandedHunks::All), 1, 1),
            ExpandedRegion {
                from_start: 10,
                from_end: 0,
                range_size: 10,
                collapsed_lines: 0,
                render_all: true,
            }
        );
        assert_eq!(
            get_expanded_region_public(false, 1, None, 1, 1),
            ExpandedRegion {
                from_start: 1,
                from_end: 0,
                range_size: 1,
                collapsed_lines: 0,
                render_all: true,
            }
        );
        let mut expanded_hunks = HashMap::new();
        expanded_hunks.insert(
            1,
            HunkExpansionRegion {
                from_start: 3,
                from_end: 20,
            },
        );
        assert_eq!(
            get_expanded_region_public(
                false,
                10,
                Some(ExpandedHunks::Regions(&expanded_hunks)),
                1,
                1
            ),
            ExpandedRegion {
                from_start: 10,
                from_end: 0,
                range_size: 10,
                collapsed_lines: 0,
                render_all: true,
            }
        );
        assert_eq!(
            get_expanded_region_public(true, 10, Some(ExpandedHunks::All), 1, 1),
            ExpandedRegion {
                from_start: 0,
                from_end: 0,
                range_size: 10,
                collapsed_lines: 10,
                render_all: false,
            }
        );

        let leading_cases = [
            (HunkSeparatorKind::Simple, 0, Some("@@ -1 +1 @@"), None),
            (HunkSeparatorKind::Simple, 1, Some("@@ -1 +1 @@"), Some(4)),
            (HunkSeparatorKind::Metadata, 0, None, None),
            (
                HunkSeparatorKind::Metadata,
                0,
                Some("@@ -1 +1 @@"),
                Some(32),
            ),
            (
                HunkSeparatorKind::LineInfo,
                0,
                Some("@@ -1 +1 @@"),
                Some(36),
            ),
            (
                HunkSeparatorKind::LineInfo,
                1,
                Some("@@ -1 +1 @@"),
                Some(40),
            ),
            (
                HunkSeparatorKind::LineInfoBasic,
                0,
                Some("@@ -1 +1 @@"),
                Some(32),
            ),
            (HunkSeparatorKind::Custom, 0, Some("@@ -1 +1 @@"), Some(36)),
            (HunkSeparatorKind::Custom, 1, Some("@@ -1 +1 @@"), Some(40)),
        ];
        for (kind, hunk_index, hunk_specs, total_height) in leading_cases {
            assert_eq!(
                get_leading_hunk_separator_layout(kind, &metrics, hunk_index, hunk_specs)
                    .map(|layout| layout.total_height),
                total_height
            );
        }

        let trailing_cases = [
            (HunkSeparatorKind::Simple, None),
            (HunkSeparatorKind::Metadata, None),
            (HunkSeparatorKind::LineInfo, Some(36)),
            (HunkSeparatorKind::LineInfoBasic, Some(32)),
            (HunkSeparatorKind::Custom, Some(36)),
        ];
        for (kind, total_height) in trailing_cases {
            assert_eq!(
                get_trailing_hunk_separator_layout(kind, &metrics)
                    .map(|layout| layout.total_height),
                total_height
            );
        }

        let custom_metrics = VirtualFileMetrics {
            hunk_separator_height: Some(12),
            ..metrics
        };
        assert_eq!(
            get_leading_hunk_separator_layout(
                HunkSeparatorKind::LineInfo,
                &custom_metrics,
                1,
                Some("@@ -1 +1 @@")
            )
            .map(|layout| layout.total_height),
            Some(20)
        );
    }

    #[test]
    fn compute_estimated_diff_heights_matches_pierre_cases() {
        let metrics = virtual_metrics_fixture();
        let base_options = EstimatedDiffHeightOptions {
            metrics,
            disable_file_header: false,
            hunk_separator_kind: HunkSeparatorKind::LineInfo,
            expand_unchanged: false,
            expanded_hunks: None,
            collapsed_context_threshold: 1,
        };

        let same = parse_diff_from_file(
            &FileContents {
                name: "same.ts".to_string(),
                contents: "one\n".to_string(),
                lang: None,
                header: None,
                cache_key: None,
            },
            &FileContents {
                name: "same.ts".to_string(),
                contents: "one\n".to_string(),
                lang: None,
                header: None,
                cache_key: None,
            },
            ParseDiffOptions::default(),
        );
        assert_eq!(
            compute_height_for_test(
                &same,
                EstimatedDiffHeightOptions {
                    metrics: VirtualFileMetrics {
                        padding_top: Some(6),
                        padding_bottom: Some(13),
                        ..metrics
                    },
                    ..base_options
                }
            ),
            EstimatedDiffHeights {
                split_height: 36,
                unified_height: 36,
            }
        );

        let no_newline = parse_diff_from_file(
            &FileContents {
                name: "no-newline.ts".to_string(),
                contents: "one\ntwo".to_string(),
                lang: None,
                header: None,
                cache_key: None,
            },
            &FileContents {
                name: "no-newline.ts".to_string(),
                contents: "one\nTWO".to_string(),
                lang: None,
                header: None,
                cache_key: None,
            },
            ParseDiffOptions::default(),
        );
        assert_eq!(
            compute_height_for_test(&no_newline, base_options),
            EstimatedDiffHeights {
                split_height: 64,
                unified_height: 84,
            }
        );

        let two_hunk = create_two_hunk_diff();
        assert_eq!(
            compute_height_for_test(&two_hunk, base_options),
            EstimatedDiffHeights {
                split_height: 326,
                unified_height: 346,
            }
        );
        assert_eq!(
            compute_height_for_test(
                &two_hunk,
                EstimatedDiffHeightOptions {
                    hunk_separator_kind: HunkSeparatorKind::Simple,
                    ..base_options
                }
            ),
            EstimatedDiffHeights {
                split_height: 218,
                unified_height: 238,
            }
        );
        assert_eq!(
            compute_height_for_test(
                &two_hunk,
                EstimatedDiffHeightOptions {
                    expand_unchanged: true,
                    ..base_options
                }
            ),
            EstimatedDiffHeights {
                split_height: 1434,
                unified_height: 1454,
            }
        );
        let mut expanded_hunks = HashMap::new();
        expanded_hunks.insert(
            0,
            HunkExpansionRegion {
                from_start: 2,
                from_end: 3,
            },
        );
        assert_eq!(
            compute_height_for_test(
                &two_hunk,
                EstimatedDiffHeightOptions {
                    expanded_hunks: Some(ExpandedHunks::Regions(&expanded_hunks)),
                    ..base_options
                }
            ),
            EstimatedDiffHeights {
                split_height: 376,
                unified_height: 396,
            }
        );
        let partial = FileDiffMetadata {
            is_partial: true,
            ..two_hunk.clone()
        };
        assert_eq!(
            compute_height_for_test(&partial, base_options),
            EstimatedDiffHeights {
                split_height: 290,
                unified_height: 310,
            }
        );
        assert_eq!(
            compute_height_for_test(
                &two_hunk,
                EstimatedDiffHeightOptions {
                    hunk_separator_kind: HunkSeparatorKind::Metadata,
                    ..base_options
                }
            ),
            EstimatedDiffHeights {
                split_height: 278,
                unified_height: 298,
            }
        );
    }

    #[test]
    fn iterate_over_file_matches_pierre_windowing_and_last_line_behavior() {
        let lines = split_file_contents_owned("line1\nline2\nline3\n\n\n");
        let mut contents = Vec::new();
        iterate_over_file(&lines, FileIterationOptions::default(), |line| {
            contents.push((
                line.line_index,
                line.line_number,
                line.content.to_string(),
                line.is_last_line,
            ));
            false
        });

        assert_eq!(
            contents,
            vec![
                (0, 1, "line1\n".to_string(), false),
                (1, 2, "line2\n".to_string(), false),
                (2, 3, "line3\n".to_string(), false),
                (3, 4, "\n".to_string(), true),
            ]
        );

        let lines = split_file_contents_owned(
            &(0..10)
                .map(|index| format!("line{index}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let mut window = Vec::new();
        iterate_over_file(
            &lines,
            FileIterationOptions {
                starting_line: 5,
                total_lines: Some(3),
            },
            |line| {
                window.push((line.line_index, line.is_last_line));
                false
            },
        );
        assert_eq!(window, vec![(5, false), (6, false), (7, false)]);

        let mut early = Vec::new();
        iterate_over_file(&lines, FileIterationOptions::default(), |line| {
            early.push(line.line_index);
            line.line_index == 4
        });
        assert_eq!(early, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn collect_diff_lines_matches_pierre_iteration_shapes() {
        let old_file = FileContents {
            name: "sample.txt".to_string(),
            contents: (1..=20)
                .map(|index| {
                    if index == 10 || index == 18 {
                        format!("old {index}\n")
                    } else {
                        format!("line {index}\n")
                    }
                })
                .collect(),
            lang: None,
            header: None,
            cache_key: None,
        };
        let new_file = FileContents {
            name: "sample.txt".to_string(),
            contents: (1..=20)
                .map(|index| {
                    if index == 10 || index == 18 {
                        format!("new {index}\n")
                    } else {
                        format!("line {index}\n")
                    }
                })
                .collect(),
            lang: None,
            header: None,
            cache_key: None,
        };
        let diff = parse_diff_from_file(
            &old_file,
            &new_file,
            ParseDiffOptions {
                context_lines: 1,
                ..ParseDiffOptions::default()
            },
        );

        assert_eq!(diff.hunks.len(), 2);
        assert_eq!(diff.hunks[0].collapsed_before, 8);
        assert_eq!(diff.hunks[1].collapsed_before, 5);

        let unified = collect_diff_lines(
            &diff,
            DiffIterationOptions {
                diff_style: DiffStyle::Unified,
                collapsed_context_threshold: 0,
                ..DiffIterationOptions::default()
            },
        )
        .unwrap();
        assert_eq!(unified.len(), 8);
        assert_eq!(unified[0].line_type, DiffLineType::Context);
        assert_eq!(unified[0].collapsed_before, 8);
        assert_eq!(unified[0].addition_line.unwrap().line_number, 9);
        assert_eq!(unified[1].deletion_line.unwrap().line_number, 10);
        assert!(unified[1].addition_line.is_none());
        assert_eq!(unified[2].addition_line.unwrap().line_number, 10);
        assert!(unified[2].deletion_line.is_none());
        assert_eq!(unified[4].collapsed_before, 5);

        let split = collect_diff_lines(
            &diff,
            DiffIterationOptions {
                diff_style: DiffStyle::Split,
                collapsed_context_threshold: 0,
                ..DiffIterationOptions::default()
            },
        )
        .unwrap();
        assert_eq!(split.len(), 6);
        assert_eq!(split[1].deletion_line.unwrap().line_number, 10);
        assert_eq!(split[1].addition_line.unwrap().line_number, 10);
        assert_eq!(split[3].collapsed_before, 5);

        let window = collect_diff_lines(
            &diff,
            DiffIterationOptions {
                diff_style: DiffStyle::Unified,
                starting_line: 1,
                total_lines: Some(3),
                collapsed_context_threshold: 0,
                ..DiffIterationOptions::default()
            },
        )
        .unwrap();
        assert_eq!(window.len(), 3);
        assert_eq!(
            window
                .iter()
                .map(|line| line
                    .addition_line
                    .or(line.deletion_line)
                    .unwrap()
                    .unified_line_index)
                .collect::<Vec<_>>(),
            vec![9, 10, 11]
        );
    }

    #[test]
    fn collect_diff_lines_expands_full_file_context_like_pierre() {
        let old_file = FileContents {
            name: "sample.txt".to_string(),
            contents: (1..=12).map(|index| format!("line {index}\n")).collect(),
            lang: None,
            header: None,
            cache_key: None,
        };
        let mut new_contents = (1..=12)
            .map(|index| format!("line {index}\n"))
            .collect::<String>();
        new_contents = new_contents.replace("line 8\n", "changed 8\n");
        let new_file = FileContents {
            name: "sample.txt".to_string(),
            contents: new_contents,
            lang: None,
            header: None,
            cache_key: None,
        };
        let diff = parse_diff_from_file(
            &old_file,
            &new_file,
            ParseDiffOptions {
                context_lines: 1,
                ..ParseDiffOptions::default()
            },
        );
        let mut expanded_hunks = HashMap::new();
        expanded_hunks.insert(
            0,
            HunkExpansionRegion {
                from_start: 2,
                from_end: 1,
            },
        );

        let lines = collect_diff_lines(
            &diff,
            DiffIterationOptions {
                diff_style: DiffStyle::Unified,
                expanded_hunks: Some(ExpandedHunks::Regions(&expanded_hunks)),
                collapsed_context_threshold: 1,
                ..DiffIterationOptions::default()
            },
        )
        .unwrap();

        assert_eq!(lines[0].line_type, DiffLineType::ContextExpanded);
        assert_eq!(lines[1].line_type, DiffLineType::ContextExpanded);
        assert_eq!(lines[2].line_type, DiffLineType::ContextExpanded);
        assert_eq!(lines[2].collapsed_before, 3);
        assert_eq!(lines[3].line_type, DiffLineType::Context);
        assert_eq!(lines[3].addition_line.unwrap().line_number, 7);
    }

    #[test]
    fn unified_render_expands_tabs_before_rendering() {
        let diff = "@@ -1 +1 @@\n-\told\n+\tnew";
        let mut view = build_diff_view_from_diff_text(diff, Some("go"));
        let rendered = view.rendered_lines(DiffViewMode::Unified, 24).to_vec();
        let rows = render_lines_to_strings(rendered, 24);

        assert_eq!(rows[0], "   1 -     old          ");
        assert_eq!(rows[1], "   1 +     new          ");
    }

    #[test]
    fn split_render_expands_tabs_on_both_sides() {
        let diff = "@@ -1 +1 @@\n-\told\n+\tnew";
        let mut view = build_diff_view_from_diff_text(diff, Some("go"));
        let rendered = view.rendered_lines(DiffViewMode::Split, 29).to_vec();
        let rows = render_lines_to_strings(rendered, 29);

        assert_eq!(rows, vec!["   1     old      1     new  "]);
    }

    #[test]
    fn split_render_wraps_sides_and_keeps_columns_aligned() {
        let diff = "@@ -1 +1 @@\n-abcdefghijklmnop\n+xy";
        let mut view = build_diff_view_from_diff_text(diff, Some("rust"));
        let rendered = view.rendered_lines(DiffViewMode::Split, 29).to_vec();
        let rows = render_lines_to_strings(rendered, 29);

        assert_eq!(
            rows,
            vec![
                "   1 abcdefg      1 xy       ",
                "     hijklmn                 ",
                "     op                      ",
            ]
        );
    }

    #[test]
    fn split_wrapped_rows_keep_line_navigation_targets() {
        let diff = "@@ -1 +1 @@\n-abcdefghijklmnop\n+xy";
        let mut view = build_diff_view_from_diff_text(diff, Some("rust"));

        assert_eq!(view.display_line_count(DiffViewMode::Split, 29), 3);
        assert_eq!(
            view.selected_line_number(DiffViewMode::Split, 29, 0),
            Some(1)
        );
        assert_eq!(
            view.selected_line_number(DiffViewMode::Split, 29, 1),
            Some(1)
        );
        assert_eq!(
            view.selected_line_number(DiffViewMode::Split, 29, 2),
            Some(1)
        );
    }

    #[test]
    fn unified_render_wraps_long_lines_with_indented_continuations() {
        let diff = "@@ -1 +1 @@\n+abcdefghijklmnop";
        let mut view = build_diff_view_from_diff_text(diff, Some("rust"));
        let rendered = view.rendered_lines(DiffViewMode::Unified, 16).to_vec();
        let rows = render_lines_to_strings(rendered, 16);

        assert_eq!(rows, vec!["   1 + abcdefgh ", "       ijklmnop "]);
    }

    #[test]
    fn unified_wrapped_rows_keep_line_navigation_targets() {
        let diff = "@@ -1 +1 @@\n+abcdefghijklmnop";
        let mut view = build_diff_view_from_diff_text(diff, Some("rust"));

        assert_eq!(view.display_line_count(DiffViewMode::Unified, 16), 2);
        assert_eq!(
            view.selected_line_number(DiffViewMode::Unified, 16, 0),
            Some(1)
        );
        assert_eq!(
            view.selected_line_number(DiffViewMode::Unified, 16, 1),
            Some(1)
        );
    }

    #[test]
    fn compare_source_line_navigation_ignores_removed_only_rows() {
        let diff = "@@ -1,2 +1,2 @@\n-old\n+new\n context";
        let mut view = build_diff_view_from_diff_text(diff, Some("rust"));

        assert_eq!(
            view.selected_new_line_number(DiffViewMode::Unified, 80, 0),
            None
        );
        assert_eq!(
            view.selected_new_line_number(DiffViewMode::Unified, 80, 1),
            Some(1)
        );
        assert_eq!(
            view.selected_new_line_number(DiffViewMode::Split, 80, 0),
            Some(1)
        );

        let deleted_diff = "@@ -1 +0,0 @@\n-old";
        let mut deleted_view = build_diff_view_from_diff_text(deleted_diff, Some("rust"));
        assert_eq!(
            deleted_view.selected_new_line_number(DiffViewMode::Split, 80, 0),
            None
        );
    }

    #[test]
    fn selection_hit_testing_ignores_prefix_and_targets_split_panes() {
        let diff = "@@ -1 +1 @@\n-old\n+new";
        let mut view = build_diff_view_from_diff_text(diff, Some("rust"));

        assert!(
            view.selection_point_at(DiffViewMode::Unified, 24, 0, 3)
                .is_none()
        );
        assert_eq!(
            view.selection_point_at(DiffViewMode::Unified, 24, 0, 8),
            Some(DiffSelectionPoint {
                display_index: 0,
                pane: DiffSelectionPane::Unified,
                column: 1,
            })
        );
        assert_eq!(
            view.selection_point_at(DiffViewMode::Split, 29, 0, 21),
            Some(DiffSelectionPoint {
                display_index: 0,
                pane: DiffSelectionPane::Right,
                column: 1,
            })
        );
    }

    #[test]
    fn split_selection_extracts_only_selected_pane_text() {
        let diff = "\
@@ -1,2 +1,2 @@
-old_one
+new_one
-old_two
+new_two
";
        let mut view = build_diff_view_from_diff_text(diff, Some("rust"));

        let selected = view.selected_text(
            DiffViewMode::Split,
            40,
            DiffSelectionPoint {
                display_index: 0,
                pane: DiffSelectionPane::Right,
                column: 0,
            },
            DiffSelectionPoint {
                display_index: 1,
                pane: DiffSelectionPane::Right,
                column: 2,
            },
        );

        assert_eq!(selected.as_deref(), Some("new_one\nnew"));
    }

    #[test]
    fn expanded_context_lines_render_with_exact_syntax_highlighting() {
        let mut old_file_lines = (1..=40)
            .map(|index| format!("let gap_value_{index} = old_call_{index}();"))
            .collect::<Vec<_>>();
        let mut new_file_lines = (1..=40)
            .map(|index| format!("let gap_value_{index} = new_call_{index}();"))
            .collect::<Vec<_>>();
        old_file_lines[0] = "let old_start = 0;".to_string();
        new_file_lines[0] = "let new_start = 1;".to_string();
        old_file_lines[39] = "let old_end = 0;".to_string();
        new_file_lines[39] = "let new_end = 1;".to_string();

        let diff = "\
@@ -1 +1 @@
-let old_start = 0;
+let new_start = 1;
@@ -40 +40 @@
-let old_end = 0;
+let new_end = 1;
";
        let mut view = build_diff_view_from_diff_text_with_context(
            diff,
            Some("rust"),
            Some(old_file_lines),
            Some(new_file_lines.clone()),
        );
        let registry = HighlightRegistry::new_for_filetypes(["rust"])
            .expect("highlight registry should initialize");
        view.apply_exact_syntax_highlighting(Some("rust"), &registry);

        let gap_index = (0..view.display_line_count(DiffViewMode::Unified, 120))
            .find(|index| {
                matches!(
                    view.selected_gap_action(DiffViewMode::Unified, 120, *index),
                    Some((_, GapExpandDirection::Up))
                )
            })
            .expect("expected expandable gap");
        let expanded_gap_index = view.expand_selected_gap(DiffViewMode::Unified, 120, gap_index, 1);
        assert!(
            expanded_gap_index > 0,
            "expanded line should precede the gap control"
        );

        let rendered = view.rendered_lines(DiffViewMode::Unified, 120);
        let expanded_line = &rendered[expanded_gap_index - 1];
        let target_text = new_file_lines[1].as_str();

        assert!(
            expanded_line
                .spans
                .iter()
                .skip(2)
                .any(|span| span.content.as_ref() == "let"),
            "expected tokenized syntax spans for expanded context line `{target_text}`, got {expanded_line:?}"
        );
        assert!(
            expanded_line
                .spans
                .iter()
                .skip(2)
                .all(|span| span.content.as_ref() != target_text),
            "expanded context line should not render as a single fallback span: {expanded_line:?}"
        );
    }

    #[test]
    fn tab_expansion_tracks_columns_across_spans() {
        let spans = expand_tabs_in_spans(vec![Span::raw("ab"), Span::raw("\t"), Span::raw("cd")]);

        let contents = spans
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<Vec<_>>();

        assert_eq!(contents, vec!["ab", "  ", "cd"]);
    }
}
