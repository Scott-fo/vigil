use std::{collections::HashMap, path::Path, sync::Arc};

use color_eyre::eyre::WrapErr;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
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
    new_file_lines: Option<Vec<String>>,
    new_file_source: Option<Arc<str>>,
    display_cache: DiffDisplayCache,
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
            new_file_lines: None,
            new_file_source: None,
            display_cache: DiffDisplayCache::default(),
        }
    }

    pub fn rendered_lines(&mut self, mode: DiffViewMode, width: usize) -> &[Line<'static>] {
        self.ensure_display_cache(mode, width);
        &self.display_cache.entry(mode).lines
    }

    pub fn first_selectable_index(&mut self, mode: DiffViewMode) -> usize {
        self.nav_targets(mode)
            .iter()
            .position(|target| target.is_some())
            .unwrap_or(0)
    }

    pub fn last_selectable_index(&mut self, mode: DiffViewMode) -> usize {
        self.nav_targets(mode)
            .iter()
            .rposition(|target| target.is_some())
            .unwrap_or(0)
    }

    pub fn move_selection(&mut self, mode: DiffViewMode, current: usize, delta: i32) -> usize {
        let nav = self.nav_targets(mode);
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

    pub fn selected_line_number(&mut self, mode: DiffViewMode, index: usize) -> Option<usize> {
        match self.nav_targets(mode).get(index).copied().flatten() {
            Some(DisplayNavTarget::Line(line_number)) => Some(line_number),
            _ => None,
        }
    }

    pub fn selected_gap_index(&mut self, mode: DiffViewMode, index: usize) -> Option<usize> {
        self.selected_gap_action(mode, index)
            .map(|(gap_index, _)| gap_index)
    }

    pub fn selected_gap_action(
        &mut self,
        mode: DiffViewMode,
        index: usize,
    ) -> Option<(usize, GapExpandDirection)> {
        match self.nav_targets(mode).get(index).copied().flatten() {
            Some(DisplayNavTarget::Gap(gap_index, direction)) => Some((gap_index, direction)),
            _ => None,
        }
    }

    pub fn display_line_count(&mut self, mode: DiffViewMode) -> usize {
        self.nav_targets(mode).len()
    }

    pub fn expand_selected_gap(
        &mut self,
        mode: DiffViewMode,
        index: usize,
        amount: usize,
    ) -> usize {
        let Some((gap_index, direction)) = self.selected_gap_action(mode, index) else {
            return index;
        };
        let _ = self.expand_gap(gap_index, direction, amount);
        self.nav_targets(mode)
            .iter()
            .position(|target| {
                matches!(
                    target,
                    Some(DisplayNavTarget::Gap(candidate, candidate_direction))
                        if *candidate == gap_index && *candidate_direction == direction
                )
            })
            .unwrap_or(index.min(self.nav_targets(mode).len().saturating_sub(1)))
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

        let (lines, nav, row_refs) = if self.rows.is_empty() {
            (
                vec![Line::from(Span::styled(
                    self.note
                        .clone()
                        .unwrap_or_else(|| "No textual diff available.".to_string()),
                    ui::diff_meta_style(),
                ))],
                vec![None],
                vec![DisplayRowRefs::default()],
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
        cache.valid = true;
    }

    fn nav_targets(&mut self, mode: DiffViewMode) -> &[Option<DisplayNavTarget>] {
        self.ensure_display_cache(mode, 0);
        &self.display_cache.entry(mode).nav
    }

    fn build_unified_display(
        &self,
        width: usize,
    ) -> (
        Vec<Line<'static>>,
        Vec<Option<DisplayNavTarget>>,
        Vec<DisplayRowRefs>,
    ) {
        let mut lines = Vec::new();
        let mut nav = Vec::new();
        let mut row_refs = Vec::new();

        for (hunk_offset, hunk) in self.hunks.iter().enumerate() {
            for row_index in hunk.row_start..hunk.row_end {
                let row = &self.rows[row_index];
                let line_number = match row.kind {
                    DiffLineKind::Added | DiffLineKind::Context => row.new_line,
                    DiffLineKind::Removed => row.old_line,
                };
                lines.push(render_unified_code_line(row, width));
                nav.push(line_number.map(DisplayNavTarget::Line));
                row_refs.push(match row.kind {
                    DiffLineKind::Removed => DisplayRowRefs {
                        left: Some(row_index),
                        right: None,
                    },
                    DiffLineKind::Added | DiffLineKind::Context => DisplayRowRefs {
                        left: None,
                        right: Some(row_index),
                    },
                });
            }

            if let Some(gap) = self.gaps.get(hunk_offset) {
                self.push_gap_display_rows(&mut lines, &mut nav, &mut row_refs, gap, width, false);
            }
        }

        (lines, nav, row_refs)
    }

    fn build_split_display(
        &self,
        width: usize,
    ) -> (
        Vec<Line<'static>>,
        Vec<Option<DisplayNavTarget>>,
        Vec<DisplayRowRefs>,
    ) {
        let total_width = width.saturating_sub(1);
        let gutter_width = 3;
        let side_width = total_width.saturating_sub(gutter_width) / 2;
        let mut lines = Vec::new();
        let mut nav = Vec::new();
        let mut row_refs = Vec::new();

        for (hunk_offset, hunk) in self.hunks.iter().enumerate() {
            for (line, target_line, refs) in render_split_hunk_rows(
                &self.rows[hunk.row_start..hunk.row_end],
                hunk.row_start,
                side_width,
            ) {
                lines.push(line);
                nav.push(target_line.map(DisplayNavTarget::Line));
                row_refs.push(refs);
            }

            if let Some(gap) = self.gaps.get(hunk_offset) {
                self.push_gap_display_rows(
                    &mut lines,
                    &mut nav,
                    &mut row_refs,
                    gap,
                    total_width,
                    true,
                );
            }
        }

        (lines, nav, row_refs)
    }

    fn push_gap_display_rows(
        &self,
        lines: &mut Vec<Line<'static>>,
        nav: &mut Vec<Option<DisplayNavTarget>>,
        row_refs: &mut Vec<DisplayRowRefs>,
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
                lines.push(render_expanded_context_line(
                    line_number,
                    &text,
                    None,
                    width,
                    split,
                ));
                nav.push(Some(DisplayNavTarget::Line(line_number)));
                row_refs.push(DisplayRowRefs::default());
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
        }

        if let Some(file_lines) = self.new_file_lines.as_ref() {
            let start = gap
                .new_start
                .saturating_sub(1)
                .saturating_add(gap.new_count.saturating_sub(context_before_count));
            for offset in 0..context_before_count {
                let line_number = gap.new_start + gap.new_count - context_before_count + offset;
                let text = file_lines.get(start + offset).cloned().unwrap_or_default();
                lines.push(render_expanded_context_line(
                    line_number,
                    &text,
                    None,
                    width,
                    split,
                ));
                nav.push(Some(DisplayNavTarget::Line(line_number)));
                row_refs.push(DisplayRowRefs::default());
            }
        }
    }

    fn invalidate_display_cache(&mut self) {
        self.display_cache = DiffDisplayCache::default();
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
        let (left, right) = run_optional_pair(
            left_source.is_some(),
            right_source.is_some(),
            || {
                left_source.and_then(|source| {
                    prepare_exact_side_highlighting(
                        rows,
                        HighlightSide::Left,
                        &source,
                        filetype,
                        registry,
                    )
                })
            },
            || {
                right_source.and_then(|source| {
                    prepare_exact_side_highlighting(
                        rows,
                        HighlightSide::Right,
                        &source,
                        filetype,
                        registry,
                    )
                })
            },
        );

        if left.is_none() && right.is_none() {
            self.apply_syntax_highlighting(Some(filetype), registry);
            return;
        }

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
    source: &Arc<str>,
    filetype: &'static str,
    registry: &HighlightRegistry,
) -> Option<CompletedHighlightSide> {
    if source.is_empty() {
        return None;
    }

    let highlighted_lines = highlight_source_lines_cached_exact(registry, filetype, source)?;
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
    valid: bool,
}

#[derive(Debug, Clone, Copy)]
enum DisplayNavTarget {
    Line(usize),
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
                .map(|row_index| rows[row_index].syntax.left.is_some())
                .or_else(|| {
                    self.right
                        .map(|row_index| rows[row_index].syntax.right.is_some())
                })
                .unwrap_or(true),
            DiffViewMode::Split => {
                let left_ready = self
                    .left
                    .map(|row_index| rows[row_index].syntax.left.is_some())
                    .unwrap_or(true);
                let right_ready = self
                    .right
                    .map(|row_index| rows[row_index].syntax.right.is_some())
                    .unwrap_or(true);
                left_ready && right_ready
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffLineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone)]
struct DiffRow {
    kind: DiffLineKind,
    old_line: Option<usize>,
    new_line: Option<usize>,
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
        new_file_lines: preview.new_file_lines.clone(),
        new_file_source: preview.new_file_source.clone(),
        display_cache: DiffDisplayCache::default(),
    };

    if let Some(registry) = highlight_registry {
        diff_view.apply_syntax_highlighting(file.filetype, registry);
    }

    Ok(diff_view)
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
        new_file_lines,
        new_file_source,
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
    if file.status == "??" {
        load_untracked_preview(repo_root, &file.path, include_exact_context).await
    } else {
        load_tracked_preview(repo_root, &file.path, include_exact_context).await
    }
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
        Some(PreviewTarget::Revision(selection.source_ref.as_str())),
        Some(PreviewTarget::Revision(selection.destination_ref.as_str())),
        file.path.as_str(),
        include_exact_context,
    )
    .await
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
        }
    } else {
        DiffPreviewData {
            diff,
            note: None,
            old_file_source: None,
            new_file_lines,
            new_file_source,
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
    let mut body = lines
        .into_iter()
        .map(|line| format!("+{}", line))
        .collect::<Vec<_>>()
        .join("\n");

    if line_count > 0 && has_trailing_newline {
        body.push('\n');
    }

    [
        format!("diff --git a/{input_path} b/{input_path}"),
        "new file mode 100644".to_string(),
        "index 0000000..1111111".to_string(),
        "--- /dev/null".to_string(),
        format!("+++ b/{input_path}"),
        hunk_header,
        body,
        String::new(),
    ]
    .join("\n")
}

#[derive(Debug, Clone)]
struct ParsedHunkHeader {
    old_start: usize,
    new_start: usize,
    new_count: usize,
}

#[inline]
fn build_diff_rows(
    diff: &str,
    _filetype: Option<&'static str>,
) -> (Vec<DiffRow>, Vec<DiffHunkBlock>, Vec<DiffHunkGap>) {
    let normalized = diff.replace("\r\n", "\n");
    let mut rows = Vec::new();
    let mut hunks = Vec::new();
    let mut old_line = 0usize;
    let mut new_line = 0usize;
    let mut in_hunk = false;
    let mut current_hunk_header: Option<ParsedHunkHeader> = None;
    let mut current_row_start = 0usize;

    for raw_line in normalized.split('\n') {
        if raw_line.is_empty() && rows.is_empty() {
            continue;
        }

        if let Some(header) = parse_hunk_header(raw_line) {
            if let Some(previous_header) = current_hunk_header.take() {
                hunks.push(DiffHunkBlock {
                    new_start: previous_header.new_start,
                    new_count: previous_header.new_count,
                    row_start: current_row_start,
                    row_end: rows.len(),
                });
            }
            current_row_start = rows.len();
            old_line = header.old_start;
            new_line = header.new_start;
            in_hunk = true;
            current_hunk_header = Some(header);
            continue;
        }

        if !in_hunk {
            continue;
        }

        if raw_line.starts_with("\\ ") {
            continue;
        }

        let marker = raw_line.chars().next().unwrap_or(' ');
        let content = raw_line.get(1..).unwrap_or("");

        match marker {
            '+' => {
                rows.push(render_diff_row(
                    None,
                    Some(new_line),
                    content,
                    DiffLineKind::Added,
                ));
                new_line += 1;
            }
            '-' => {
                rows.push(render_diff_row(
                    Some(old_line),
                    None,
                    content,
                    DiffLineKind::Removed,
                ));
                old_line += 1;
            }
            ' ' => {
                rows.push(render_diff_row(
                    Some(old_line),
                    Some(new_line),
                    content,
                    DiffLineKind::Context,
                ));
                old_line += 1;
                new_line += 1;
            }
            _ => {}
        }
    }

    if let Some(previous_header) = current_hunk_header.take() {
        hunks.push(DiffHunkBlock {
            new_start: previous_header.new_start,
            new_count: previous_header.new_count,
            row_start: current_row_start,
            row_end: rows.len(),
        });
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

#[inline]
fn parse_hunk_header(raw_line: &str) -> Option<ParsedHunkHeader> {
    if !raw_line.starts_with("@@ -") {
        return None;
    }

    let remainder = raw_line.strip_prefix("@@ -")?;
    let (old_part, rest) = remainder.split_once(" +")?;
    let (new_part, _) = rest.split_once(" @@")?;

    let old_start = old_part.split(',').next()?.parse::<usize>().ok()?;
    let new_start = new_part.split(',').next()?.parse::<usize>().ok()?;
    let new_count = new_part
        .split_once(',')
        .and_then(|(_, count)| count.parse::<usize>().ok())
        .unwrap_or(1);

    Some(ParsedHunkHeader {
        old_start,
        new_start,
        new_count,
    })
}

#[inline]
fn render_diff_row(
    old_line: Option<usize>,
    new_line: Option<usize>,
    content: &str,
    kind: DiffLineKind,
) -> DiffRow {
    DiffRow {
        kind,
        old_line,
        new_line,
        text: content.to_string(),
        syntax: DiffRowSyntax::default(),
    }
}

fn resolve_split_target_line(left: Option<&DiffRow>, right: Option<&DiffRow>) -> Option<usize> {
    right
        .and_then(|row| row.new_line)
        .or_else(|| left.and_then(|row| row.old_line))
}

fn render_unified_code_line(row: &DiffRow, width: usize) -> Line<'static> {
    let base_style = base_style(row.kind);
    let sign_style = match row.kind {
        DiffLineKind::Context => ui::context_sign_style(),
        DiffLineKind::Added => ui::added_sign_style(),
        DiffLineKind::Removed => ui::removed_sign_style(),
    };
    let marker = match row.kind {
        DiffLineKind::Context => ' ',
        DiffLineKind::Added => '+',
        DiffLineKind::Removed => '-',
    };
    let unified_line_number = match row.kind {
        DiffLineKind::Added | DiffLineKind::Context => row.new_line,
        DiffLineKind::Removed => row.old_line,
    };

    let mut spans = vec![
        Span::styled(
            format_line_number(unified_line_number),
            base_style.patch(ui::line_number_style()),
        ),
        Span::styled(format!("{marker} "), sign_style),
    ];
    spans.extend(render_row_content(
        row.unified_content(),
        &row.text,
        base_style,
    ));
    let padded = fit_spans_to_width(spans, width.saturating_sub(1), base_style);
    Line::from(padded).style(base_style)
}

fn render_split_pair_line(
    left: Option<&DiffRow>,
    right: Option<&DiffRow>,
    side_width: usize,
) -> Line<'static> {
    let gap = Span::styled("   ", ui::diff_context_style());
    let mut spans = Vec::new();
    spans.extend(render_split_side(left, true, side_width));
    spans.push(gap);
    spans.extend(render_split_side(right, false, side_width));
    Line::from(spans)
}

fn render_split_hunk_rows(
    rows: &[DiffRow],
    row_index_offset: usize,
    side_width: usize,
) -> Vec<(Line<'static>, Option<usize>, DisplayRowRefs)> {
    let mut rendered = Vec::with_capacity(rows.len());
    let mut pending_removed: Vec<(usize, &DiffRow)> = Vec::new();
    let mut pending_added: Vec<(usize, &DiffRow)> = Vec::new();

    let flush_pending = |rendered: &mut Vec<(Line<'static>, Option<usize>, DisplayRowRefs)>,
                         removed: &mut Vec<(usize, &DiffRow)>,
                         added: &mut Vec<(usize, &DiffRow)>| {
        let row_count = removed.len().max(added.len());
        for index in 0..row_count {
            let left = removed.get(index).copied();
            let right = added.get(index).copied();
            rendered.push((
                render_split_pair_line(
                    left.map(|(_, row)| row),
                    right.map(|(_, row)| row),
                    side_width,
                ),
                resolve_split_target_line(left.map(|(_, row)| row), right.map(|(_, row)| row)),
                DisplayRowRefs {
                    left: left.map(|(row_index, _)| row_index),
                    right: right.map(|(row_index, _)| row_index),
                },
            ));
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
                rendered.push((
                    render_split_pair_line(Some(row), Some(row), side_width),
                    resolve_split_target_line(Some(row), Some(row)),
                    DisplayRowRefs {
                        left: Some(row_index),
                        right: Some(row_index),
                    },
                ));
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

fn render_expanded_context_line(
    line_number: usize,
    text: &str,
    highlighted_content: Option<Vec<SyntaxToken>>,
    width: usize,
    split: bool,
) -> Line<'static> {
    let row = DiffRow {
        kind: DiffLineKind::Context,
        old_line: Some(line_number),
        new_line: Some(line_number),
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
        render_split_pair_line(Some(&row), Some(&row), side_width)
    } else {
        render_unified_code_line(&row, width)
    }
}

fn render_split_side(row: Option<&DiffRow>, left_side: bool, width: usize) -> Vec<Span<'static>> {
    let Some(row) = row else {
        return vec![Span::styled(" ".repeat(width), ui::diff_context_style())];
    };

    let line_number = if left_side {
        row.old_line
    } else {
        row.new_line
    };
    let base_style = base_style(row.kind);
    let mut spans = vec![Span::styled(
        format_line_number(line_number),
        base_style.patch(ui::line_number_style()),
    )];
    spans.extend(render_row_content(
        row.side_content(left_side),
        &row.text,
        base_style,
    ));
    fit_spans_to_width(spans, width, base_style)
}

fn render_row_content(
    syntax_tokens: Option<&[SyntaxToken]>,
    text: &str,
    fallback: Style,
) -> Vec<Span<'static>> {
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

    expand_tabs_in_spans(raw_spans)
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
    fn tab_expansion_tracks_columns_across_spans() {
        let spans = expand_tabs_in_spans(vec![
            Span::raw("ab"),
            Span::raw("\t"),
            Span::raw("cd"),
        ]);

        let contents = spans
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<Vec<_>>();

        assert_eq!(contents, vec!["ab", "  ", "cd"]);
    }
}
