use std::sync::Arc;

use super::{DiffLineKind, DiffLineWrapMode, DiffRow, DiffView, DiffViewMode, DisplayRowRefs};
use crate::git::highlight::{
    HighlightRegistry, SyntaxToken, highlight_source_lines, highlight_source_lines_cached_exact,
};

const VIEWPORT_HIGHLIGHT_PADDING_ROWS: usize = 64;

impl DiffView {
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
        line_wrap: DiffLineWrapMode,
        start: usize,
        end: usize,
        filetype: Option<&'static str>,
        registry: &HighlightRegistry,
    ) {
        let Some(filetype) = filetype else {
            return;
        };

        self.ensure_display_cache(mode, width, line_wrap);
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
        let left_source = self.old_file_source.clone();
        let right_source = self.new_file_source.clone();
        let rows = &self.rows;
        let (left_result, right_result) = run_optional_pair(
            left_window.is_some(),
            right_window.is_some(),
            || {
                prepare_display_range_highlighting(
                    rows,
                    HighlightSide::Left,
                    left_window,
                    left_source.as_ref(),
                    filetype,
                    registry,
                )
            },
            || {
                prepare_display_range_highlighting(
                    rows,
                    HighlightSide::Right,
                    right_window,
                    right_source.as_ref(),
                    filetype,
                    registry,
                )
            },
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
        line_wrap: DiffLineWrapMode,
        start: usize,
        end: usize,
    ) -> bool {
        self.ensure_display_cache(mode, width, line_wrap);
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
    prepare_exact_side_highlighting_in_row_window(
        rows,
        side,
        highlighted_lines,
        0,
        rows.len().saturating_sub(1),
    )
}

fn prepare_exact_side_highlighting_in_row_window(
    rows: &[DiffRow],
    side: HighlightSide,
    highlighted_lines: &[Vec<SyntaxToken>],
    start: usize,
    end: usize,
) -> Option<CompletedHighlightSide> {
    if rows.is_empty() || start > end {
        return None;
    }
    if highlighted_lines.is_empty() {
        return None;
    }
    let start = start.min(rows.len().saturating_sub(1));
    let end = end.min(rows.len().saturating_sub(1));
    let mut row_indices = Vec::new();
    let mut exact_row_lines = Vec::new();

    for (row_offset, row) in rows[start..=end].iter().enumerate() {
        if !side.includes(row.kind) {
            continue;
        }
        let row_index = start + row_offset;

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

fn prepare_display_range_highlighting(
    rows: &[DiffRow],
    side: HighlightSide,
    window: Option<(usize, usize)>,
    exact_source: Option<&Arc<str>>,
    filetype: &'static str,
    registry: &HighlightRegistry,
) -> Option<CompletedHighlightSide> {
    let (window_start, window_end) = window?;

    if let Some(exact) = exact_source
        .and_then(|source| highlight_source_lines_cached_exact(registry, filetype, source))
        .and_then(|highlighted_lines| {
            prepare_exact_side_highlighting_in_row_window(
                rows,
                side,
                highlighted_lines.as_ref(),
                window_start,
                window_end,
            )
        })
    {
        return Some(exact);
    }

    prepare_side_highlighting_in_row_window(rows, side, window_start, window_end)
        .and_then(|request| request.highlight(filetype, registry))
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
