use std::{
    cell::RefCell,
    cmp::Reverse,
    collections::BinaryHeap,
    collections::HashMap,
    collections::HashSet,
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
};

use color_eyre::eyre::{WrapErr, eyre};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use streaming_iterator::StreamingIterator;
use tokio::{fs, process::Command};
use tree_sitter::{Parser, Query, QueryCursor};
use unicode_width::UnicodeWidthStr;

use crate::{app::DiffViewMode, ui};

pub type SharedHighlightRegistry = Arc<HighlightRegistry>;
const LOG_FIELD_SEPARATOR: char = '\u{001f}';
const LOG_RECORD_SEPARATOR: char = '\u{001e}';
pub const EMPTY_TREE_HASH: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const VIEWPORT_HIGHLIGHT_PADDING_ROWS: usize = 64;
const EXACT_HIGHLIGHT_CACHE_CAPACITY: usize = 8;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub status: String,
    pub path: String,
    pub label: String,
    pub filetype: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct CommitSearchEntry {
    pub hash: String,
    pub short_hash: String,
    pub parent_hashes: Vec<String>,
    pub author: String,
    pub date: String,
    pub subject: String,
}

#[derive(Debug, Clone)]
pub struct CommitCompareSelection {
    pub base_ref: String,
    pub commit_hash: String,
    pub short_hash: String,
    pub subject: String,
}

#[derive(Debug, Clone)]
pub struct BlameTarget {
    pub file_path: String,
    pub line_number: usize,
}

#[derive(Debug, Clone)]
pub struct BlameCommitDetails {
    pub target: BlameTarget,
    pub commit_hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
    pub description: String,
    pub is_uncommitted: bool,
    pub compare_selection: Option<CommitCompareSelection>,
}

#[derive(Debug, Clone)]
pub struct BranchCompareSelection {
    pub source_ref: String,
    pub destination_ref: String,
}

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
        let should_parallelize = left.is_some()
            && right.is_some()
            && std::thread::available_parallelism()
                .map(|parallelism| parallelism.get() > 1)
                .unwrap_or(false);

        let (left_result, right_result) = if should_parallelize {
            std::thread::scope(|scope| {
                let right_task = scope
                    .spawn(move || right.and_then(|request| request.highlight(filetype, registry)));
                let left_result = left.and_then(|request| request.highlight(filetype, registry));
                let right_result = right_task.join().ok().flatten();
                (left_result, right_result)
            })
        } else {
            (
                left.and_then(|request| request.highlight(filetype, registry)),
                right.and_then(|request| request.highlight(filetype, registry)),
            )
        };

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
        let should_parallelize = left_source.is_some()
            && right_source.is_some()
            && std::thread::available_parallelism()
                .map(|parallelism| parallelism.get() > 1)
                .unwrap_or(false);

        let (left, right) = if should_parallelize {
            std::thread::scope(|scope| {
                let right_task = scope.spawn(move || {
                    right_source.and_then(|source| {
                        prepare_exact_side_highlighting(
                            rows,
                            HighlightSide::Right,
                            &source,
                            filetype,
                            registry,
                        )
                    })
                });
                let left = left_source.and_then(|source| {
                    prepare_exact_side_highlighting(
                        rows,
                        HighlightSide::Left,
                        &source,
                        filetype,
                        registry,
                    )
                });
                let right = right_task.join().ok().flatten();
                (left, right)
            })
        } else {
            (
                left_source.and_then(|source| {
                    prepare_exact_side_highlighting(
                        rows,
                        HighlightSide::Left,
                        &source,
                        filetype,
                        registry,
                    )
                }),
                right_source.and_then(|source| {
                    prepare_exact_side_highlighting(
                        rows,
                        HighlightSide::Right,
                        &source,
                        filetype,
                        registry,
                    )
                }),
            )
        };

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
        let should_parallelize = left.is_some()
            && right.is_some()
            && std::thread::available_parallelism()
                .map(|parallelism| parallelism.get() > 1)
                .unwrap_or(false);

        let (left_result, right_result) = if should_parallelize {
            std::thread::scope(|scope| {
                let right_task = scope
                    .spawn(move || right.and_then(|request| request.highlight(filetype, registry)));
                let left_result = left.and_then(|request| request.highlight(filetype, registry));
                let right_result = right_task.join().ok().flatten();
                (left_result, right_result)
            })
        } else {
            (
                left.and_then(|request| request.highlight(filetype, registry)),
                right.and_then(|request| request.highlight(filetype, registry)),
            )
        };

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

#[derive(Debug, Clone)]
struct StatusEntry {
    status: String,
    path: String,
    original_path: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyntaxToken {
    start: usize,
    end: usize,
    highlight_name: Option<&'static str>,
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

static HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "attribute.builtin",
    "boolean",
    "character",
    "character.special",
    "comment",
    "comment.documentation",
    "conditional",
    "constant",
    "constant.builtin",
    "constructor",
    "constructor.builtin",
    "delimiter",
    "embedded",
    "exception",
    "field",
    "function",
    "function.builtin",
    "function.call",
    "function.method",
    "function.method.call",
    "function.method.builtin",
    "function.macro",
    "function.special",
    "keyword",
    "keyword.conditional",
    "keyword.conditional.ternary",
    "keyword.coroutine",
    "keyword.debug",
    "keyword.directive",
    "keyword.exception",
    "keyword.function",
    "keyword.import",
    "keyword.modifier",
    "keyword.operator",
    "keyword.repeat",
    "keyword.return",
    "keyword.type",
    "label",
    "method",
    "method.call",
    "markup.heading",
    "markup.heading.1",
    "markup.heading.2",
    "markup.heading.3",
    "markup.heading.4",
    "markup.heading.5",
    "markup.heading.6",
    "markup.link",
    "markup.link.label",
    "markup.link.url",
    "markup.list",
    "markup.list.checked",
    "markup.list.unchecked",
    "markup.quote",
    "markup.raw",
    "markup.raw.block",
    "module",
    "module.builtin",
    "namespace",
    "number",
    "number.float",
    "operator",
    "parameter",
    "property",
    "property.definition",
    "property.builtin",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "repeat",
    "string",
    "string.escape",
    "string.regexp",
    "string.special",
    "string.special.url",
    "string.special.key",
    "string.special.path",
    "string.special.regex",
    "string.special.symbol",
    "string.special.uri",
    "tag",
    "tag.attribute",
    "tag.builtin",
    "tag.delimiter",
    "tag.error",
    "type",
    "type.builtin",
    "type.definition",
    "type.qualifier",
    "variable",
    "variable.builtin",
    "variable.member",
    "variable.parameter",
];

pub fn is_file_staged(status: &str) -> bool {
    if status == "??" {
        return false;
    }

    let index_status = status.chars().next().unwrap_or(' ');
    index_status != ' ' && index_status != '?'
}

pub fn status_color(status: &str) -> ratatui::style::Color {
    let palette = crate::theme::active_palette();

    if status == "??" || status.contains('A') {
        return palette.diff_highlight_added;
    }
    if status.contains('U') || status.contains('D') {
        return palette.diff_highlight_removed;
    }
    if status.contains('R') || status.contains('C') {
        return palette.secondary;
    }
    if status.contains('M') {
        return palette.warning;
    }
    palette.text_muted
}

pub async fn toggle_file_stage(repo_root: &Path, file: &FileEntry) -> color_eyre::Result<()> {
    let args: Vec<&str> = if is_file_staged(&file.status) {
        vec!["restore", "--staged", "--", file.path.as_str()]
    } else {
        vec!["add", "--", file.path.as_str()]
    };

    let _ = git_output(repo_root, &args).await?;
    Ok(())
}

pub async fn discard_file_changes(repo_root: &Path, file: &FileEntry) -> color_eyre::Result<()> {
    let args: Vec<&str> = if file.status == "??" {
        vec!["clean", "-f", "--", file.path.as_str()]
    } else {
        vec![
            "restore",
            "--source=HEAD",
            "--staged",
            "--worktree",
            "--",
            file.path.as_str(),
        ]
    };

    let _ = git_output(repo_root, &args).await?;
    Ok(())
}

pub async fn commit_staged_changes(repo_root: &Path, message: &str) -> color_eyre::Result<()> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(eyre!("Commit message is required."));
    }

    let _ = git_output(repo_root, &["commit", "-m", trimmed]).await?;
    Ok(())
}

pub async fn push_to_remote(repo_root: &Path) -> color_eyre::Result<()> {
    let _ = git_output(repo_root, &["push"]).await?;
    Ok(())
}

pub async fn pull_from_remote(repo_root: &Path) -> color_eyre::Result<()> {
    let _ = git_output(repo_root, &["pull"]).await?;
    Ok(())
}

pub async fn init_repo(repo_root: &Path) -> color_eyre::Result<()> {
    let _ = git_output(repo_root, &["init"]).await?;
    Ok(())
}

pub async fn list_searchable_commits(
    repo_root: &Path,
    limit: usize,
) -> color_eyre::Result<Vec<CommitSearchEntry>> {
    let output = git_output(
        repo_root,
        &[
            "log",
            &format!("--max-count={}", limit.max(1)),
            "--date=short",
            "--pretty=format:%H%x1f%P%x1f%h%x1f%ad%x1f%an%x1f%s%x1e",
        ],
    )
    .await?;

    Ok(parse_commit_log_entries(&output))
}

pub fn resolve_commit_base_ref(commit: &CommitSearchEntry) -> String {
    commit
        .parent_hashes
        .first()
        .cloned()
        .unwrap_or_else(|| EMPTY_TREE_HASH.to_string())
}

pub async fn load_blame_commit_details(
    repo_root: &Path,
    target: &BlameTarget,
) -> color_eyre::Result<BlameCommitDetails> {
    let blame_output = git_output(
        repo_root,
        &[
            "blame",
            "--porcelain",
            "-L",
            &format!("{0},{0}", target.line_number),
            "--",
            target.file_path.as_str(),
        ],
    )
    .await?;

    let header = parse_blame_porcelain_header(&blame_output).ok_or_else(|| {
        eyre!(
            "unable to parse blame output for {}:{}",
            target.file_path,
            target.line_number
        )
    })?;

    if is_uncommitted_blame_hash(&header.commit_hash) {
        return Ok(BlameCommitDetails {
            target: target.clone(),
            commit_hash: header.commit_hash,
            short_hash: "working-tree".to_string(),
            author: if header.author.is_empty() {
                "Uncommitted".to_string()
            } else {
                header.author
            },
            date: header.date,
            subject: if header.summary.is_empty() {
                "Uncommitted line changes".to_string()
            } else {
                header.summary
            },
            description: "This line has uncommitted changes. Commit comparison is unavailable."
                .to_string(),
            is_uncommitted: true,
            compare_selection: None,
        });
    }

    let show_output = git_output(
        repo_root,
        &[
            "show",
            "-s",
            "--date=short",
            "--format=%H%x1f%h%x1f%P%x1f%ad%x1f%an%x1f%s%x1f%b",
            header.commit_hash.as_str(),
        ],
    )
    .await?;

    let commit = parse_commit_show_output(&show_output)
        .ok_or_else(|| eyre!("unable to parse commit metadata for {}", header.commit_hash))?;
    let subject = if commit.subject.is_empty() {
        header.summary
    } else {
        commit.subject
    };
    let description = if commit.description.trim().is_empty() {
        "No commit description.".to_string()
    } else {
        commit.description
    };
    let compare_base = commit
        .parent_hashes
        .first()
        .cloned()
        .unwrap_or_else(|| EMPTY_TREE_HASH.to_string());
    let commit_hash = commit.commit_hash;
    let short_hash = commit.short_hash;

    Ok(BlameCommitDetails {
        target: target.clone(),
        commit_hash: commit_hash.clone(),
        short_hash: short_hash.clone(),
        author: if commit.author.is_empty() {
            header.author
        } else {
            commit.author
        },
        date: if commit.date.is_empty() {
            header.date
        } else {
            commit.date
        },
        description,
        is_uncommitted: false,
        compare_selection: Some(CommitCompareSelection {
            base_ref: compare_base,
            commit_hash: commit_hash.clone(),
            short_hash: short_hash.clone(),
            subject: subject.clone(),
        }),
        subject,
    })
}

pub async fn load_files_with_commit_diff(
    repo_root: &Path,
    selection: &CommitCompareSelection,
) -> color_eyre::Result<Vec<FileEntry>> {
    let output = git_output(
        repo_root,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            selection.base_ref.as_str(),
            selection.commit_hash.as_str(),
        ],
    )
    .await?;

    Ok(parse_diff_name_status_entries(&output)
        .into_iter()
        .map(to_file_entry)
        .collect())
}

pub async fn list_comparable_refs(repo_root: &Path) -> color_eyre::Result<Vec<String>> {
    let output = git_output(
        repo_root,
        &[
            "for-each-ref",
            "--format=%(refname)\t%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ],
    )
    .await?;

    let mut refs = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let (full_ref, short_ref) = line.split_once('\t')?;
            let short_ref = short_ref.trim();
            if short_ref.is_empty() || short_ref == "HEAD" {
                return None;
            }
            if full_ref.starts_with("refs/remotes/")
                && (!short_ref.contains('/') || short_ref.ends_with("/HEAD"))
            {
                return None;
            }
            Some(short_ref.to_string())
        })
        .collect::<Vec<_>>();

    refs.sort();
    refs.dedup();
    Ok(refs)
}

pub async fn load_files_with_branch_diff(
    repo_root: &Path,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<Vec<FileEntry>> {
    let output = git_output(
        repo_root,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            build_branch_diff_range(selection).as_str(),
        ],
    )
    .await?;

    Ok(parse_diff_name_status_entries(&output)
        .into_iter()
        .map(to_file_entry)
        .collect())
}

pub async fn should_refresh_for_paths(
    repo_root: &Path,
    changed_paths: &[PathBuf],
) -> color_eyre::Result<bool> {
    if changed_paths.is_empty() {
        return Ok(true);
    }

    let mut candidate_paths = Vec::new();
    let mut seen_paths = HashSet::new();

    for path in changed_paths {
        let Ok(relative_path) = path.strip_prefix(repo_root) else {
            return Ok(true);
        };

        if relative_path.as_os_str().is_empty() {
            return Ok(true);
        }

        if relative_path
            .file_name()
            .is_some_and(|file_name| file_name == ".gitignore")
        {
            return Ok(true);
        }

        let relative = relative_path.to_string_lossy().replace('\\', "/");
        if seen_paths.insert(relative.clone()) {
            candidate_paths.push(relative);
        }
    }

    if candidate_paths.is_empty() {
        return Ok(false);
    }

    let ignored_paths = git_check_ignored(repo_root, &candidate_paths).await?;
    Ok(candidate_paths
        .iter()
        .any(|path| !ignored_paths.contains(path)))
}

pub async fn resolve_repo_root() -> color_eyre::Result<PathBuf> {
    resolve_repo_root_from(Path::new(".")).await
}

pub async fn resolve_repo_root_from(probe_path: &Path) -> color_eyre::Result<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(probe_path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .await
        .wrap_err("failed to resolve git repository root")?;

    if !output.status.success() {
        return Err(eyre!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        ));
    }

    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

pub async fn load_files_with_status(repo_root: &Path) -> color_eyre::Result<Vec<FileEntry>> {
    let output = git_output(
        repo_root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await?;
    let mut files = Vec::new();

    for entry in parse_status_entries(&output) {
        if entry.status == "!!" || is_directory_status_entry(repo_root, &entry.path).await {
            continue;
        }
        files.push(to_file_entry(entry));
    }

    Ok(files)
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

pub fn build_diff_view_from_diff_text(diff: &str, filetype: Option<&'static str>) -> DiffView {
    build_diff_view_from_diff_text_with_context(diff, filetype, None, None)
}

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
    let output = git_output(
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
    )
    .await?;
    let old_file_lines = if include_exact_context {
        load_revision_file_lines(repo_root, selection.base_ref.as_str(), file.path.as_str()).await?
    } else {
        None
    };
    let new_file_lines = if include_exact_context || diff_needs_context_lines(&output) {
        load_revision_file_lines(
            repo_root,
            selection.commit_hash.as_str(),
            file.path.as_str(),
        )
        .await?
    } else {
        None
    };
    let old_file_source = old_file_lines.as_deref().map(source_from_lines);
    let new_file_source = new_file_lines.as_deref().map(source_from_lines);

    Ok(DiffPreviewData {
        diff: output,
        note: None,
        old_file_source,
        new_file_lines,
        new_file_source,
    })
}

async fn load_branch_preview(
    repo_root: &Path,
    file: &FileEntry,
    selection: &BranchCompareSelection,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    let output = git_output(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            build_branch_diff_range(selection).as_str(),
            "--",
            file.path.as_str(),
        ],
    )
    .await?;
    let old_file_lines = if include_exact_context {
        load_revision_file_lines(repo_root, selection.source_ref.as_str(), file.path.as_str())
            .await?
    } else {
        None
    };
    let new_file_lines = if include_exact_context || diff_needs_context_lines(&output) {
        load_revision_file_lines(
            repo_root,
            selection.destination_ref.as_str(),
            file.path.as_str(),
        )
        .await?
    } else {
        None
    };
    let old_file_source = old_file_lines.as_deref().map(source_from_lines);
    let new_file_source = new_file_lines.as_deref().map(source_from_lines);

    Ok(DiffPreviewData {
        diff: output,
        note: None,
        old_file_source,
        new_file_lines,
        new_file_source,
    })
}

async fn load_tracked_preview(
    repo_root: &Path,
    file_path: &str,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    let output = git_output(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            "HEAD",
            "--",
            file_path,
        ],
    )
    .await?;
    let old_file_lines = if include_exact_context {
        load_revision_file_lines(repo_root, "HEAD", file_path).await?
    } else {
        None
    };
    let new_file_lines = if include_exact_context || diff_needs_context_lines(&output) {
        load_working_tree_file_lines(repo_root, file_path).await?
    } else {
        None
    };
    let old_file_source = old_file_lines.as_deref().map(source_from_lines);
    let new_file_source = new_file_lines.as_deref().map(source_from_lines);
    Ok(DiffPreviewData {
        diff: output,
        note: None,
        old_file_source,
        new_file_lines,
        new_file_source,
    })
}

async fn load_untracked_preview(
    repo_root: &Path,
    file_path: &str,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    let full_path = repo_root.join(file_path);
    match fs::metadata(&full_path).await {
        Ok(metadata) if metadata.is_dir() => {
            return Ok(DiffPreviewData {
                diff: String::new(),
                note: Some("Directory or symlinked directory; no preview available.".to_string()),
                old_file_source: None,
                new_file_lines: None,
                new_file_source: None,
            });
        }
        Ok(_) => {}
        Err(_) => {
            return Ok(DiffPreviewData {
                diff: String::new(),
                note: Some("Unable to read untracked file content.".to_string()),
                old_file_source: None,
                new_file_lines: None,
                new_file_source: None,
            });
        }
    }

    let bytes = match fs::read(&full_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Ok(DiffPreviewData {
                diff: String::new(),
                note: Some("Unable to read untracked file content.".to_string()),
                old_file_source: None,
                new_file_lines: None,
                new_file_source: None,
            });
        }
    };

    if bytes.contains(&0) {
        return Ok(DiffPreviewData {
            diff: String::new(),
            note: Some("Binary or non-text file; no preview available.".to_string()),
            old_file_source: None,
            new_file_lines: None,
            new_file_source: None,
        });
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

async fn is_directory_status_entry(repo_root: &Path, path: &str) -> bool {
    match fs::metadata(repo_root.join(path)).await {
        Ok(metadata) => metadata.is_dir(),
        Err(_) => false,
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

pub async fn git_output(repo_root: &Path, args: &[&str]) -> color_eyre::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .await
        .wrap_err_with(|| format!("failed to run git {:?}", args))?;

    if !output.status.success() {
        return Err(eyre!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn git_check_ignored(
    repo_root: &Path,
    paths: &[String],
) -> color_eyre::Result<HashSet<String>> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_root);
    command.args(["check-ignore", "-z", "--stdin"]);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .wrap_err("failed to spawn git check-ignore")?;

    if let Some(mut stdin) = child.stdin.take() {
        let input = format!("{}\0", paths.join("\0"));
        tokio::io::AsyncWriteExt::write_all(&mut stdin, input.as_bytes())
            .await
            .wrap_err("failed to write git check-ignore stdin")?;
    }

    let output = child
        .wait_with_output()
        .await
        .wrap_err("failed to wait for git check-ignore")?;

    match output.status.code() {
        Some(0) | Some(1) => {}
        _ => {
            return Err(eyre!(
                "{}",
                String::from_utf8_lossy(&output.stderr).trim().to_string()
            ));
        }
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
        .collect())
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

#[derive(Debug)]
struct ParsedBlameHeader {
    commit_hash: String,
    author: String,
    date: String,
    summary: String,
}

#[derive(Debug)]
struct ParsedCommitShow {
    commit_hash: String,
    short_hash: String,
    parent_hashes: Vec<String>,
    date: String,
    author: String,
    subject: String,
    description: String,
}

fn parse_blame_porcelain_header(raw: &str) -> Option<ParsedBlameHeader> {
    let mut lines = raw.lines();
    let first_line = lines.next()?.trim();
    let commit_hash = first_line.split_whitespace().next()?.trim();
    if commit_hash.len() != 40 {
        return None;
    }

    let mut author = String::new();
    let mut date = String::new();
    let mut summary = String::new();

    for line in lines {
        if line.starts_with('\t') {
            break;
        }
        if let Some(value) = line.strip_prefix("author ") {
            author = value.trim().to_string();
            continue;
        }
        if let Some(value) = line.strip_prefix("author-time ") {
            date = format_unix_date(value.trim());
            continue;
        }
        if let Some(value) = line.strip_prefix("summary ") {
            summary = value.trim().to_string();
        }
    }

    Some(ParsedBlameHeader {
        commit_hash: commit_hash.to_string(),
        author,
        date,
        summary,
    })
}

fn parse_commit_show_output(raw: &str) -> Option<ParsedCommitShow> {
    let mut fields = raw.split(LOG_FIELD_SEPARATOR);
    let commit_hash = fields.next()?.trim();
    let short_hash = fields.next()?.trim();
    let parents_raw = fields.next().unwrap_or("").trim();
    let date = fields.next().unwrap_or("").trim();
    let author = fields.next().unwrap_or("").trim();
    let subject = fields.next().unwrap_or("").trim();
    let description = fields.next().unwrap_or("").trim_end();

    if commit_hash.is_empty() || short_hash.is_empty() {
        return None;
    }

    Some(ParsedCommitShow {
        commit_hash: commit_hash.to_string(),
        short_hash: short_hash.to_string(),
        parent_hashes: parents_raw
            .split_whitespace()
            .map(str::trim)
            .filter(|parent| !parent.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        date: date.to_string(),
        author: author.to_string(),
        subject: subject.to_string(),
        description: description.to_string(),
    })
}

fn is_uncommitted_blame_hash(hash: &str) -> bool {
    hash.trim() == "0000000000000000000000000000000000000000"
}

fn format_unix_date(raw_seconds: &str) -> String {
    match raw_seconds.parse::<u64>() {
        Ok(seconds) if seconds > 0 => String::new(),
        _ => String::new(),
    }
}

fn parse_commit_log_entries(raw: &str) -> Vec<CommitSearchEntry> {
    raw.split(LOG_RECORD_SEPARATOR)
        .map(str::trim)
        .filter(|record| !record.is_empty())
        .filter_map(|record| {
            let mut fields = record.split(LOG_FIELD_SEPARATOR);
            let hash = fields.next()?.trim();
            let parents_raw = fields.next().unwrap_or("").trim();
            let short_hash = fields.next().unwrap_or("").trim();
            let date = fields.next().unwrap_or("").trim();
            let author = fields.next().unwrap_or("").trim();
            let subject = fields.next().unwrap_or("").trim();

            if hash.is_empty() || short_hash.is_empty() {
                return None;
            }

            Some(CommitSearchEntry {
                hash: hash.to_string(),
                short_hash: short_hash.to_string(),
                parent_hashes: parents_raw
                    .split_whitespace()
                    .map(str::trim)
                    .filter(|parent| !parent.is_empty())
                    .map(ToOwned::to_owned)
                    .collect(),
                author: author.to_string(),
                date: date.to_string(),
                subject: subject.to_string(),
            })
        })
        .collect()
}

fn parse_status_entries(raw: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let fields: Vec<&str> = raw.split('\0').collect();
    let mut index = 0;

    while index < fields.len() {
        let field = fields[index];
        index += 1;

        if field.len() < 4 {
            continue;
        }

        let x = field.chars().next().unwrap_or(' ');
        let y = field.chars().nth(1).unwrap_or(' ');
        let status = to_status_pair(x, y);
        let first_path = field[3..].to_string();

        if first_path.is_empty() {
            continue;
        }

        if matches!(x, 'R' | 'C') {
            let original_path = fields.get(index).copied().unwrap_or_default().to_string();
            index += 1;
            entries.push(StatusEntry {
                status,
                path: first_path.clone(),
                original_path: (!original_path.is_empty()).then_some(original_path),
            });
            continue;
        }

        entries.push(StatusEntry {
            status,
            path: first_path,
            original_path: None,
        });
    }

    entries
}

fn build_branch_diff_range(selection: &BranchCompareSelection) -> String {
    format!(
        "{}...{}",
        selection.destination_ref.trim(),
        selection.source_ref.trim()
    )
}

fn parse_diff_name_status_entries(raw: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let fields: Vec<&str> = raw.split('\0').collect();
    let mut index = 0;

    while index < fields.len() {
        let status_field = fields[index].trim();
        index += 1;

        if status_field.is_empty() {
            continue;
        }

        let status_code = status_field.chars().next().unwrap_or(' ');
        match status_code {
            'R' | 'C' => {
                let original_path = fields.get(index).copied().unwrap_or_default().to_string();
                let path = fields
                    .get(index + 1)
                    .copied()
                    .unwrap_or_default()
                    .to_string();
                index += 2;

                if path.is_empty() {
                    continue;
                }

                entries.push(StatusEntry {
                    status: status_code.to_string(),
                    path,
                    original_path: (!original_path.is_empty()).then_some(original_path),
                });
            }
            _ => {
                let path = fields.get(index).copied().unwrap_or_default().to_string();
                index += 1;

                if path.is_empty() {
                    continue;
                }

                entries.push(StatusEntry {
                    status: status_code.to_string(),
                    path,
                    original_path: None,
                });
            }
        }
    }

    entries
}

fn to_status_pair(index_code: char, worktree_code: char) -> String {
    if index_code == '?' && worktree_code == '?' {
        return "??".to_string();
    }
    if index_code == '!' && worktree_code == '!' {
        return "!!".to_string();
    }
    format!("{index_code}{worktree_code}")
}

fn to_file_entry(entry: StatusEntry) -> FileEntry {
    let label = entry
        .original_path
        .as_ref()
        .map(|from| format!("{from} -> {}", entry.path))
        .unwrap_or_else(|| entry.path.clone());

    FileEntry {
        status: entry.status,
        filetype: resolve_diff_filetype(&entry.path),
        path: entry.path,
        label,
    }
}

fn resolve_diff_filetype(path: &str) -> Option<&'static str> {
    let file_name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    let extension = file_name.rsplit('.').next().unwrap_or("");

    match file_name.as_str() {
        "dockerfile" => None,
        "justfile" => Some("bash"),
        "cargo.toml" => Some("toml"),
        _ => match extension {
            "rs" => Some("rust"),
            "js" | "mjs" | "cjs" => Some("javascript"),
            "jsx" => Some("jsx"),
            "ts" | "mts" | "cts" => Some("typescript"),
            "tsx" => Some("tsx"),
            "py" => Some("python"),
            "go" => Some("go"),
            "c" | "h" => Some("c"),
            "cc" | "cp" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Some("cpp"),
            "cs" => Some("csharp"),
            "sh" | "bash" | "zsh" | "ksh" => Some("bash"),
            "java" => Some("java"),
            "rb" => Some("ruby"),
            "php" | "php3" | "php4" | "php5" | "phtml" => Some("php"),
            "scala" | "sc" => Some("scala"),
            "html" | "htm" => Some("html"),
            "json" => Some("json"),
            "yaml" | "yml" => Some("yaml"),
            "hs" => Some("haskell"),
            "css" => Some("css"),
            "nix" => Some("nix"),
            "md" | "mdx" | "markdown" => Some("markdown"),
            _ => None,
        },
    }
}

#[derive(Debug, Clone)]
struct ParsedHunkHeader {
    old_start: usize,
    new_start: usize,
    new_count: usize,
}

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
    let mut hunk_index = 0usize;
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
            hunk_index = hunk_index.saturating_add(1);
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
    let Some(tokens) = syntax_tokens else {
        return vec![Span::styled(text.to_string(), fallback)];
    };

    if tokens.is_empty() {
        return vec![Span::styled(text.to_string(), fallback)];
    }

    tokens
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
        .collect()
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

pub struct HighlightRegistry {
    configs: Mutex<HashMap<&'static str, Arc<QueryHighlightConfig>>>,
}

struct QueryHighlightConfig {
    language: tree_sitter::Language,
    query: Query,
    capture_highlight_names: Box<[Option<&'static str>]>,
}

struct CachedSyntaxRunner {
    parser: Parser,
    query_cursor: QueryCursor,
}

struct ExactHighlightCacheEntry {
    filetype: &'static str,
    source_hash: u64,
    source_len: usize,
    source: Arc<str>,
    highlighted_lines: Arc<[Vec<SyntaxToken>]>,
}

impl std::fmt::Debug for HighlightRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let config_count = self
            .configs
            .lock()
            .expect("highlight registry mutex poisoned")
            .len();
        f.debug_struct("HighlightRegistry")
            .field("config_count", &config_count)
            .finish()
    }
}

impl HighlightRegistry {
    pub fn new() -> color_eyre::Result<Self> {
        Self::new_for_filetypes(Self::all_filetypes().iter().copied())
    }

    pub fn new_for_filetypes<I>(filetypes: I) -> color_eyre::Result<Self>
    where
        I: IntoIterator<Item = &'static str>,
    {
        let registry = Self {
            configs: Mutex::new(HashMap::new()),
        };
        registry.ensure_filetypes(filetypes)?;
        Ok(registry)
    }

    pub fn all_filetypes() -> &'static [&'static str] {
        &[
            "rust",
            "javascript",
            "jsx",
            "typescript",
            "tsx",
            "python",
            "go",
            "c",
            "cpp",
            "csharp",
            "bash",
            "java",
            "ruby",
            "php",
            "scala",
            "html",
            "json",
            "yaml",
            "haskell",
            "css",
            "nix",
        ]
    }

    pub fn ensure_filetypes<I>(&self, filetypes: I) -> color_eyre::Result<()>
    where
        I: IntoIterator<Item = &'static str>,
    {
        for filetype in filetypes {
            let _ = self.ensure_filetype(filetype)?;
        }
        Ok(())
    }

    pub fn ensure_filetype(&self, filetype: &'static str) -> color_eyre::Result<bool> {
        if filetype == "markdown" {
            return Ok(false);
        }

        {
            let configs = self
                .configs
                .lock()
                .expect("highlight registry mutex poisoned");
            if configs.contains_key(filetype) {
                return Ok(false);
            }
        }

        let Some(config) = build_highlight_config(filetype)? else {
            return Ok(false);
        };
        let mut configs = self
            .configs
            .lock()
            .expect("highlight registry mutex poisoned");
        if configs.contains_key(filetype) {
            return Ok(false);
        }
        configs.insert(filetype, Arc::new(config));
        Ok(true)
    }

    fn config(&self, filetype: &'static str) -> Option<Arc<QueryHighlightConfig>> {
        let _ = self.ensure_filetype(filetype);
        let configs = self
            .configs
            .lock()
            .expect("highlight registry mutex poisoned");
        configs.get(filetype).cloned()
    }
}

fn build_highlight_config(
    filetype: &'static str,
) -> color_eyre::Result<Option<QueryHighlightConfig>> {
    let mut configs = HashMap::new();
    let ecma_highlights = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/ecma/highlights.scm"
    ));
    let ecma_locals = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/ecma/locals.scm"
    ));
    let ecma_injections = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/ecma/injections.scm"
    ));
    let jsx_nvim_highlights = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/jsx/highlights.scm"
    ));
    let jsx_nvim_injections = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/jsx/injections.scm"
    ));
    let typescript_highlights_query = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/typescript/highlights.scm"
    ));
    let typescript_locals_query = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/typescript/locals.scm"
    ));
    let typescript_injections_query = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/typescript/injections.scm"
    ));

    match filetype {
        "rust" => register_highlight_config(
            &mut configs,
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            "rust",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/vendor/nvim-treesitter/rust/highlights.scm"
            )),
            "",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/vendor/nvim-treesitter/rust/locals.scm"
            )),
        )?,
        "javascript" => register_highlight_config(
            &mut configs,
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            "javascript",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        )?,
        "jsx" => {
            let jsx_highlights = format!(
                "{}\n{}",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
            );
            register_highlight_config(
                &mut configs,
                "jsx",
                tree_sitter_javascript::LANGUAGE.into(),
                "javascript",
                &jsx_highlights,
                tree_sitter_javascript::INJECTIONS_QUERY,
                tree_sitter_javascript::LOCALS_QUERY,
            )?;
        }
        "typescript" => {
            let typescript_highlights = format!("{ecma_highlights}\n{typescript_highlights_query}");
            let typescript_locals = format!("{ecma_locals}\n{typescript_locals_query}");
            let typescript_injections = format!("{ecma_injections}\n{typescript_injections_query}");
            register_highlight_config(
                &mut configs,
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                "typescript",
                &typescript_highlights,
                &typescript_injections,
                &typescript_locals,
            )?;
        }
        "tsx" => {
            let typescript_locals = format!("{ecma_locals}\n{typescript_locals_query}");
            let tsx_highlights =
                format!("{ecma_highlights}\n{typescript_highlights_query}\n{jsx_nvim_highlights}");
            let tsx_injections =
                format!("{ecma_injections}\n{typescript_injections_query}\n{jsx_nvim_injections}");
            register_highlight_config(
                &mut configs,
                "tsx",
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                "tsx",
                &tsx_highlights,
                &tsx_injections,
                &typescript_locals,
            )?;
        }
        "python" => register_highlight_config(
            &mut configs,
            "python",
            tree_sitter_python::LANGUAGE.into(),
            "python",
            tree_sitter_python::HIGHLIGHTS_QUERY,
            "",
            "",
        )?,
        "go" => register_highlight_config(
            &mut configs,
            "go",
            tree_sitter_go::LANGUAGE.into(),
            "go",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/vendor/nvim-treesitter/go/highlights.scm"
            )),
            "",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/vendor/nvim-treesitter/go/locals.scm"
            )),
        )?,
        "c" => register_highlight_config(
            &mut configs,
            "c",
            tree_sitter_c::LANGUAGE.into(),
            "c",
            tree_sitter_c::HIGHLIGHT_QUERY,
            "",
            "",
        )?,
        "cpp" => register_highlight_config(
            &mut configs,
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            "cpp",
            tree_sitter_cpp::HIGHLIGHT_QUERY,
            "",
            "",
        )?,
        "csharp" => register_highlight_config(
            &mut configs,
            "csharp",
            tree_sitter_c_sharp::LANGUAGE.into(),
            "c_sharp",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/vendor/tree-sitter-c-sharp/highlights.scm"
            )),
            "",
            "",
        )?,
        "bash" => register_highlight_config(
            &mut configs,
            "bash",
            tree_sitter_bash::LANGUAGE.into(),
            "bash",
            tree_sitter_bash::HIGHLIGHT_QUERY,
            "",
            "",
        )?,
        "java" => register_highlight_config(
            &mut configs,
            "java",
            tree_sitter_java::LANGUAGE.into(),
            "java",
            tree_sitter_java::HIGHLIGHTS_QUERY,
            "",
            "",
        )?,
        "ruby" => register_highlight_config(
            &mut configs,
            "ruby",
            tree_sitter_ruby::LANGUAGE.into(),
            "ruby",
            tree_sitter_ruby::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_ruby::LOCALS_QUERY,
        )?,
        "php" => register_highlight_config(
            &mut configs,
            "php",
            tree_sitter_php::LANGUAGE_PHP.into(),
            "php",
            tree_sitter_php::HIGHLIGHTS_QUERY,
            tree_sitter_php::INJECTIONS_QUERY,
            "",
        )?,
        "scala" => register_highlight_config(
            &mut configs,
            "scala",
            tree_sitter_scala::LANGUAGE.into(),
            "scala",
            tree_sitter_scala::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_scala::LOCALS_QUERY,
        )?,
        "html" => register_highlight_config(
            &mut configs,
            "html",
            tree_sitter_html::LANGUAGE.into(),
            "html",
            tree_sitter_html::HIGHLIGHTS_QUERY,
            tree_sitter_html::INJECTIONS_QUERY,
            "",
        )?,
        "json" => register_highlight_config(
            &mut configs,
            "json",
            tree_sitter_json::LANGUAGE.into(),
            "json",
            tree_sitter_json::HIGHLIGHTS_QUERY,
            "",
            "",
        )?,
        "yaml" => register_highlight_config(
            &mut configs,
            "yaml",
            tree_sitter_yaml::LANGUAGE.into(),
            "yaml",
            tree_sitter_yaml::HIGHLIGHTS_QUERY,
            "",
            "",
        )?,
        "haskell" => register_highlight_config(
            &mut configs,
            "haskell",
            tree_sitter_haskell::LANGUAGE.into(),
            "haskell",
            tree_sitter_haskell::HIGHLIGHTS_QUERY,
            tree_sitter_haskell::INJECTIONS_QUERY,
            tree_sitter_haskell::LOCALS_QUERY,
        )?,
        "css" => register_highlight_config(
            &mut configs,
            "css",
            tree_sitter_css::LANGUAGE.into(),
            "css",
            tree_sitter_css::HIGHLIGHTS_QUERY,
            "",
            "",
        )?,
        "nix" => register_highlight_config(
            &mut configs,
            "nix",
            tree_sitter_nix::LANGUAGE.into(),
            "nix",
            tree_sitter_nix::HIGHLIGHTS_QUERY,
            tree_sitter_nix::INJECTIONS_QUERY,
            "",
        )?,
        _ => return Ok(None),
    }

    Ok(configs.remove(filetype))
}

thread_local! {
    static SYNTAX_RUNNERS: RefCell<HashMap<&'static str, CachedSyntaxRunner>> =
        RefCell::new(HashMap::new());
    static EXACT_HIGHLIGHT_CACHE: RefCell<Vec<ExactHighlightCacheEntry>> =
        const { RefCell::new(Vec::new()) };
}

fn highlight_source_lines(
    registry: &HighlightRegistry,
    filetype: &'static str,
    source: &str,
) -> Option<Vec<Vec<SyntaxToken>>> {
    if source.is_empty() {
        return Some(vec![Vec::new()]);
    }

    if filetype == "markdown" {
        return Some(
            source
                .split('\n')
                .map(highlight_markdown_line_tokens)
                .collect(),
        );
    }

    let config = registry.config(filetype)?;
    SYNTAX_RUNNERS.with(|runners| {
        let mut runners = runners.borrow_mut();
        let runner = match runners.entry(filetype) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let mut parser = Parser::new();
                parser.set_language(&config.language).ok()?;
                entry.insert(CachedSyntaxRunner {
                    parser,
                    query_cursor: QueryCursor::new(),
                })
            }
        };
        let tree = runner.parser.parse(source, None)?;
        query_captures_to_lines(
            &mut runner.query_cursor,
            &config.query,
            &config.capture_highlight_names,
            tree.root_node(),
            source,
        )
    })
}

fn highlight_source_lines_cached_exact(
    registry: &HighlightRegistry,
    filetype: &'static str,
    source: &Arc<str>,
) -> Option<Arc<[Vec<SyntaxToken>]>> {
    if source.is_empty() {
        return Some(Arc::from([Vec::new()]));
    }

    let source_hash = hash_source(source.as_ref());
    let source_len = source.len();

    if let Some(hit) = EXACT_HIGHLIGHT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let position = cache.iter().position(|entry| {
            entry.filetype == filetype
                && entry.source_hash == source_hash
                && entry.source_len == source_len
                && entry.source.as_ref() == source.as_ref()
        })?;
        let entry = cache.remove(position);
        let highlighted_lines = entry.highlighted_lines.clone();
        cache.push(entry);
        Some(highlighted_lines)
    }) {
        return Some(hit);
    }

    let highlighted_lines = Arc::<[Vec<SyntaxToken>]>::from(
        highlight_source_lines(registry, filetype, source.as_ref())?.into_boxed_slice(),
    );
    EXACT_HIGHLIGHT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.push(ExactHighlightCacheEntry {
            filetype,
            source_hash,
            source_len,
            source: source.clone(),
            highlighted_lines: highlighted_lines.clone(),
        });
        if cache.len() > EXACT_HIGHLIGHT_CACHE_CAPACITY {
            let overflow = cache.len() - EXACT_HIGHLIGHT_CACHE_CAPACITY;
            cache.drain(..overflow);
        }
    });
    Some(highlighted_lines)
}

fn hash_source(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

pub fn clear_exact_highlight_cache() {
    EXACT_HIGHLIGHT_CACHE.with(|cache| cache.borrow_mut().clear());
}

pub fn prewarm_highlight_registry<I>(
    registry: &HighlightRegistry,
    filetypes: I,
) -> color_eyre::Result<()>
where
    I: IntoIterator<Item = &'static str>,
{
    for filetype in filetypes {
        let _ = registry.ensure_filetype(filetype)?;
        if let Some(sample) = sample_source_for_filetype(filetype) {
            let _ = highlight_source_lines(registry, filetype, sample);
        }
    }
    Ok(())
}

fn sample_source_for_filetype(filetype: &'static str) -> Option<&'static str> {
    match filetype {
        "rust" => Some("fn build_user(id: usize) -> User { User::new(id) }"),
        "go" => Some("func BuildUser(id int) User { return NewUser(id) }"),
        "typescript" => Some("const user: User = await loadUser(id);"),
        "tsx" => Some("<Card title=\"demo\">{value}</Card>"),
        "javascript" => Some("const user = await loadUser(id);"),
        "jsx" => Some("<Card>{value}</Card>"),
        "python" => Some("def build_user(id: int) -> User:\n    return User(id)"),
        "bash" => Some("build_user() { echo \"$1\"; }"),
        "java" => Some("class User { String name() { return value; } }"),
        "ruby" => Some("def build_user(id) = User.new(id)"),
        "php" => Some("<?php function buildUser($id) { return new User($id); }"),
        "scala" => Some("def buildUser(id: Int): User = User(id)"),
        "html" => Some("<div class=\"card\">demo</div>"),
        "json" => Some("{\"user\": {\"id\": 1}}"),
        "yaml" => Some("user:\n  id: 1"),
        "css" => Some(".card { color: red; }"),
        "c" => Some("int build_user(int id) { return id; }"),
        "cpp" => Some("int build_user(int id) { return id; }"),
        "csharp" => Some("class User { string Name() => value; }"),
        "haskell" => Some("buildUser id = User id"),
        "nix" => Some("{ user = { id = 1; }; }"),
        "markdown" => Some("# Prefetch"),
        _ => None,
    }
}

fn push_syntax_token(
    tokens: &mut Vec<SyntaxToken>,
    start: usize,
    end: usize,
    highlight_name: Option<&'static str>,
) {
    if start >= end {
        return;
    }

    if let Some(last) = tokens.last_mut()
        && last.highlight_name == highlight_name
        && last.end == start
    {
        last.end = end;
        return;
    }

    tokens.push(SyntaxToken {
        start,
        end,
        highlight_name,
    });
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

#[derive(Clone, Copy)]
struct QueryHighlightRange {
    start: usize,
    end: usize,
    highlight_name: Option<&'static str>,
    specificity: u8,
}

fn query_captures_to_lines(
    query_cursor: &mut QueryCursor,
    query: &Query,
    capture_highlight_names: &[Option<&'static str>],
    root_node: tree_sitter::Node<'_>,
    source: &str,
) -> Option<Vec<Vec<SyntaxToken>>> {
    let mut ranges = Vec::new();
    let mut captures = query_cursor.captures(query, root_node, source.as_bytes());
    while {
        captures.advance();
        captures.get().is_some()
    } {
        let Some((query_match, capture_index)) = captures.get() else {
            continue;
        };
        let Some(query_capture) = query_match.captures.get(*capture_index) else {
            continue;
        };
        let start = query_capture.node.start_byte();
        let end = query_capture.node.end_byte();
        if start >= end || end > source.len() {
            continue;
        }
        let highlight_name = capture_highlight_names
            .get(query_capture.index as usize)
            .copied()
            .flatten();
        let specificity = highlight_name
            .map(|name| name.split('.').count() as u8)
            .unwrap_or(0);
        ranges.push(QueryHighlightRange {
            start,
            end,
            highlight_name,
            specificity,
        });
    }

    if ranges.is_empty() {
        return Some(
            source
                .split('\n')
                .map(|line| {
                    vec![SyntaxToken {
                        start: 0,
                        end: line.len(),
                        highlight_name: None,
                    }]
                })
                .collect(),
        );
    }

    let mut lines = Vec::new();
    let mut current_line = Vec::new();
    let mut active_ranges = Vec::new();
    let mut active_endings = BinaryHeap::new();
    let mut current_offset = 0usize;
    let mut current_line_start = 0usize;
    let mut next_range_index = 0usize;

    while next_range_index < ranges.len() || !active_endings.is_empty() {
        let next_start = ranges
            .get(next_range_index)
            .map(|range| range.start)
            .unwrap_or(usize::MAX);
        let next_end = active_endings
            .peek()
            .map(|ending: &Reverse<(usize, usize)>| ending.0.0)
            .unwrap_or(usize::MAX);
        let next_offset = next_start.min(next_end);

        if current_offset < next_offset {
            let highlight_name = select_active_highlight_name(&active_ranges, &ranges);
            push_highlighted_source_segment(
                &mut lines,
                &mut current_line,
                source,
                current_offset,
                next_offset,
                &mut current_line_start,
                highlight_name,
            );
        }

        if next_end <= next_start {
            while let Some(Reverse((end, range_index))) = active_endings.peek().copied() {
                if end != next_end {
                    break;
                }
                let _ = active_endings.pop();
                if let Some(position) = active_ranges
                    .iter()
                    .position(|active_range_index| *active_range_index == range_index)
                {
                    active_ranges.swap_remove(position);
                }
            }
            current_offset = next_end;
            continue;
        }

        while let Some(range) = ranges.get(next_range_index) {
            if range.start != next_start {
                break;
            }
            active_ranges.push(next_range_index);
            active_endings.push(Reverse((range.end, next_range_index)));
            next_range_index += 1;
        }
        current_offset = next_start;
    }

    if current_offset < source.len() {
        let highlight_name = select_active_highlight_name(&active_ranges, &ranges);
        push_highlighted_source_segment(
            &mut lines,
            &mut current_line,
            source,
            current_offset,
            source.len(),
            &mut current_line_start,
            highlight_name,
        );
    }

    lines.push(current_line);
    Some(lines)
}

fn select_active_highlight_name(
    active_ranges: &[usize],
    ranges: &[QueryHighlightRange],
) -> Option<&'static str> {
    active_ranges
        .iter()
        .copied()
        .max_by_key(|range_index| {
            let range = ranges[*range_index];
            (range.specificity, *range_index)
        })
        .and_then(|range_index| ranges[range_index].highlight_name)
}

fn push_highlighted_source_segment(
    lines: &mut Vec<Vec<SyntaxToken>>,
    current_line: &mut Vec<SyntaxToken>,
    source: &str,
    mut start: usize,
    end: usize,
    current_line_start: &mut usize,
    highlight_name: Option<&'static str>,
) {
    while start < end {
        let segment = &source[start..end];
        if let Some(newline_offset) = segment.find('\n') {
            let line_end = start + newline_offset;
            push_syntax_token(
                current_line,
                start.saturating_sub(*current_line_start),
                line_end.saturating_sub(*current_line_start),
                highlight_name,
            );
            lines.push(std::mem::take(current_line));
            start = line_end + 1;
            *current_line_start = start;
        } else {
            push_syntax_token(
                current_line,
                start.saturating_sub(*current_line_start),
                end.saturating_sub(*current_line_start),
                highlight_name,
            );
            break;
        }
    }
}

fn highlight_markdown_inline_tokens(text: &str) -> Vec<SyntaxToken> {
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < text.len() {
        let remainder = &text[index..];

        if let Some(rest) = remainder.strip_prefix('`')
            && let Some(end) = rest.find('`')
        {
            let code_end = index + 1 + end + 1;
            push_syntax_token(&mut tokens, index, code_end, Some("markup.raw"));
            index = code_end;
            continue;
        }

        if let Some(label_end) = remainder.find("](")
            && remainder.starts_with('[')
            && let Some(url_end) = remainder[label_end + 2..].find(')')
        {
            let label_text_end = index + label_end + 1;
            let url_start = index + label_end + 2;
            let url_end = url_start + url_end;
            push_syntax_token(&mut tokens, index, index + 1, None);
            push_syntax_token(
                &mut tokens,
                index + 1,
                label_text_end,
                Some("markup.link.label"),
            );
            push_syntax_token(&mut tokens, label_text_end, label_text_end + 2, None);
            push_syntax_token(&mut tokens, url_start, url_end, Some("markup.link.url"));
            push_syntax_token(&mut tokens, url_end, url_end + 1, None);
            index = url_end + 1;
            continue;
        }

        let mut next_break = remainder.len();
        for needle in ["`", "["] {
            if let Some(found) = remainder.find(needle) {
                next_break = next_break.min(found);
            }
        }
        if next_break == 0 {
            next_break = remainder.chars().next().map(char::len_utf8).unwrap_or(1);
        }
        push_syntax_token(&mut tokens, index, index + next_break, None);
        index += next_break;
    }

    tokens
}

fn markdown_list_prefix_len(text: &str) -> Option<usize> {
    for marker in ["- ", "* ", "+ "] {
        if text.starts_with(marker) {
            return Some(marker.len());
        }
    }

    let digit_count = text.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count > 0 {
        let remainder = &text[digit_count..];
        if remainder.starts_with(". ") || remainder.starts_with(") ") {
            return Some(digit_count + 2);
        }
    }

    None
}

fn highlight_markdown_line_tokens(line: &str) -> Vec<SyntaxToken> {
    let mut tokens = Vec::new();
    let indent_len = line.len() - line.trim_start().len();
    let (_, trimmed) = line.split_at(indent_len);
    push_syntax_token(&mut tokens, 0, indent_len, None);

    if trimmed.is_empty() {
        return tokens;
    }

    let bare = trimmed.trim();
    if bare.len() >= 3 && bare.chars().all(|ch| matches!(ch, '-' | '*' | '_')) {
        push_syntax_token(&mut tokens, indent_len, line.len(), Some("operator"));
        return tokens;
    }

    for fence in ["```", "~~~"] {
        if let Some(rest) = trimmed.strip_prefix(fence) {
            push_syntax_token(
                &mut tokens,
                indent_len,
                indent_len + fence.len(),
                Some("markup.raw"),
            );
            let ws_len = rest.len() - rest.trim_start().len();
            let info_start = indent_len + fence.len() + ws_len;
            push_syntax_token(&mut tokens, indent_len + fence.len(), info_start, None);
            push_syntax_token(&mut tokens, info_start, line.len(), Some("label"));
            return tokens;
        }
    }

    if let Some(rest) = trimmed.strip_prefix("> ") {
        push_syntax_token(
            &mut tokens,
            indent_len,
            indent_len + 2,
            Some("markup.quote"),
        );
        tokens.extend(
            highlight_markdown_inline_tokens(rest)
                .into_iter()
                .map(|token| SyntaxToken {
                    start: token.start + indent_len + 2,
                    end: token.end + indent_len + 2,
                    highlight_name: token.highlight_name,
                }),
        );
        return tokens;
    }

    let heading_marker_len = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&heading_marker_len) && trimmed[heading_marker_len..].starts_with(' ') {
        push_syntax_token(
            &mut tokens,
            indent_len,
            indent_len + heading_marker_len,
            Some("markup.heading"),
        );
        push_syntax_token(
            &mut tokens,
            indent_len + heading_marker_len,
            indent_len + heading_marker_len + 1,
            None,
        );
        push_syntax_token(
            &mut tokens,
            indent_len + heading_marker_len + 1,
            line.len(),
            Some("markup.heading"),
        );
        return tokens;
    }

    if let Some(prefix_len) = markdown_list_prefix_len(trimmed) {
        push_syntax_token(
            &mut tokens,
            indent_len,
            indent_len + prefix_len,
            Some("markup.list"),
        );
        let rest = &trimmed[prefix_len..];
        let rest_start = indent_len + prefix_len;
        if let Some(task_rest) = rest.strip_prefix("[ ] ") {
            push_syntax_token(
                &mut tokens,
                rest_start,
                rest_start + 4,
                Some("markup.list.unchecked"),
            );
            tokens.extend(
                highlight_markdown_inline_tokens(task_rest)
                    .into_iter()
                    .map(|token| SyntaxToken {
                        start: token.start + rest_start + 4,
                        end: token.end + rest_start + 4,
                        highlight_name: token.highlight_name,
                    }),
            );
            return tokens;
        }
        if let Some(task_rest) = rest
            .strip_prefix("[x] ")
            .or_else(|| rest.strip_prefix("[X] "))
        {
            push_syntax_token(
                &mut tokens,
                rest_start,
                rest_start + 4,
                Some("markup.list.checked"),
            );
            tokens.extend(
                highlight_markdown_inline_tokens(task_rest)
                    .into_iter()
                    .map(|token| SyntaxToken {
                        start: token.start + rest_start + 4,
                        end: token.end + rest_start + 4,
                        highlight_name: token.highlight_name,
                    }),
            );
            return tokens;
        }
        tokens.extend(
            highlight_markdown_inline_tokens(rest)
                .into_iter()
                .map(|token| SyntaxToken {
                    start: token.start + rest_start,
                    end: token.end + rest_start,
                    highlight_name: token.highlight_name,
                }),
        );
        return tokens;
    }

    tokens.extend(
        highlight_markdown_inline_tokens(trimmed)
            .into_iter()
            .map(|token| SyntaxToken {
                start: token.start + indent_len,
                end: token.end + indent_len,
                highlight_name: token.highlight_name,
            }),
    );
    tokens
}

fn register_highlight_config(
    configs: &mut HashMap<&'static str, QueryHighlightConfig>,
    key: &'static str,
    language: tree_sitter::Language,
    _language_name: &'static str,
    highlights: &str,
    _injections: &str,
    _locals: &str,
) -> color_eyre::Result<()> {
    let query = Query::new(&language, highlights)
        .wrap_err_with(|| format!("failed to build {key} query config"))?;
    let capture_highlight_names = query
        .capture_names()
        .iter()
        .map(|name| resolve_highlight_name(name))
        .collect();
    configs.insert(
        key,
        QueryHighlightConfig {
            language,
            query,
            capture_highlight_names,
        },
    );
    Ok(())
}

fn resolve_highlight_name(name: &str) -> Option<&'static str> {
    HIGHLIGHT_NAMES
        .iter()
        .copied()
        .find(|candidate| *candidate == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_highlight_registry() {
        HighlightRegistry::new().expect("highlight registry should initialize");
    }

    #[test]
    fn highlights_rust_go_typescript_and_markdown_without_falling_back() {
        let registry = HighlightRegistry::new().expect("highlight registry should initialize");

        for (filetype, line) in [
            ("rust", "let value = Foo::new(bar);"),
            ("go", "func buildUser(id int) Foo { return NewUser(id) }"),
            ("typescript", "const value: Foo = await loadUser(id);"),
            ("tsx", "<Card title=\"demo\">{value}</Card>"),
            ("markdown", "# Heading"),
        ] {
            let spans = highlight_source_lines(&registry, filetype, line)
                .expect("highlighting should succeed")
                .pop()
                .unwrap_or_default();
            assert!(
                spans.len() > 1,
                "expected syntax highlighting for {filetype}, got fallback spans: {spans:?}"
            );
        }
    }
}
