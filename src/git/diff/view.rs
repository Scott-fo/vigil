use std::{collections::HashMap, sync::Arc};

use super::super::{
    FileEntry,
    highlight::{HighlightRegistry, SyntaxToken},
};
use super::{
    DiffDisplayCache, FileDiffMetadata, MergeConflictLabels, MergeConflictMarkerRowType,
    ParseMergeConflictDiffFromFileResult,
    rows::{append_file_diff_rows_with_conflicts, build_diff_rows, diff_gaps_from_hunks},
};

#[derive(Debug, Default, Clone)]
pub struct DiffView {
    pub(super) rows: Vec<DiffRow>,
    pub note: Option<String>,
    pub(super) hunks: Vec<DiffHunkBlock>,
    pub(super) gaps: Vec<DiffHunkGap>,
    pub(super) gap_expansions: HashMap<usize, DiffGapExpansion>,
    pub(super) old_file_source: Option<Arc<str>>,
    pub(super) old_exact_highlighted_lines: Option<Arc<[Vec<SyntaxToken>]>>,
    pub(super) new_file_lines: Option<Vec<String>>,
    pub(super) new_file_source: Option<Arc<str>>,
    pub(super) new_exact_highlighted_lines: Option<Arc<[Vec<SyntaxToken>]>>,
    pub(super) display_cache: DiffDisplayCache,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffDisplayLineAnchor {
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
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

    pub fn has_diff_rows(&self) -> bool {
        !self.rows.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffLineKind {
    Context,
    Added,
    Removed,
    ConflictAction,
    ConflictMarker(MergeConflictMarkerRowType),
}

#[derive(Debug, Clone)]
pub(super) struct DiffRow {
    pub(super) kind: DiffLineKind,
    pub(super) old_line: Option<usize>,
    pub(super) new_line: Option<usize>,
    pub(super) conflict_index: Option<usize>,
    pub(super) text: String,
    pub(super) syntax: DiffRowSyntax,
}

#[derive(Debug, Clone, Default)]
pub(super) struct DiffRowSyntax {
    pub(super) left: Option<Vec<SyntaxToken>>,
    pub(super) right: Option<Vec<SyntaxToken>>,
}

impl DiffRow {
    pub(super) fn unified_content(&self) -> Option<&[SyntaxToken]> {
        match self.kind {
            DiffLineKind::Removed => self.syntax.left.as_deref(),
            DiffLineKind::Added | DiffLineKind::Context => self.syntax.right.as_deref(),
            DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => None,
        }
    }

    pub(super) fn side_content(&self, left_side: bool) -> Option<&[SyntaxToken]> {
        if left_side {
            self.syntax.left.as_deref()
        } else {
            self.syntax.right.as_deref()
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct DiffHunkBlock {
    pub(super) new_start: usize,
    pub(super) new_count: usize,
    pub(super) row_start: usize,
    pub(super) row_end: usize,
}

#[derive(Debug, Clone)]
pub(super) struct DiffHunkGap {
    pub(super) gap_index: usize,
    pub(super) new_start: usize,
    pub(super) new_count: usize,
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct DiffGapExpansion {
    pub(super) from_previous: usize,
    pub(super) from_next: usize,
}
pub fn build_diff_view_from_preview_data(
    preview: &DiffPreviewData,
    file: &FileEntry,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    if let Some(merge_conflict) = &preview.merge_conflict {
        let mut diff_view = build_merge_conflict_diff_view(
            merge_conflict,
            file.filetype,
            preview.merge_conflict_labels.as_ref(),
            preview.note.clone(),
        );
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
    labels: Option<&MergeConflictLabels>,
    note: Option<String>,
) -> DiffView {
    let mut rows = Vec::new();
    let mut hunks = Vec::new();
    let default_labels;
    let labels = match labels {
        Some(labels) => labels,
        None => {
            default_labels = MergeConflictLabels::default();
            &default_labels
        }
    };
    append_file_diff_rows_with_conflicts(
        &merge_conflict.file_diff,
        &merge_conflict.actions,
        &merge_conflict.marker_rows,
        labels,
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

pub fn build_diff_view_from_file_metadata(file: &FileDiffMetadata) -> DiffView {
    let mut rows = Vec::new();
    let mut hunks = Vec::new();
    append_file_diff_rows_with_conflicts(
        file,
        &[],
        &[],
        &MergeConflictLabels::default(),
        &mut rows,
        &mut hunks,
    );
    let gaps = diff_gaps_from_hunks(&hunks);

    DiffView {
        rows,
        note: None,
        hunks,
        gaps,
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
    pub(super) diff: String,
    pub(super) note: Option<String>,
    pub(super) old_file_source: Option<Arc<str>>,
    pub(super) new_file_lines: Option<Vec<String>>,
    pub(super) new_file_source: Option<Arc<str>>,
    pub(super) merge_conflict: Option<ParseMergeConflictDiffFromFileResult>,
    pub(super) merge_conflict_labels: Option<MergeConflictLabels>,
}

impl DiffPreviewData {
    pub(super) fn from_sources(
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
            merge_conflict_labels: None,
        }
    }

    pub(super) fn from_merge_conflict(
        merge_conflict: ParseMergeConflictDiffFromFileResult,
        labels: MergeConflictLabels,
    ) -> Self {
        Self {
            diff: String::new(),
            note: None,
            old_file_source: None,
            new_file_lines: None,
            new_file_source: None,
            merge_conflict: Some(merge_conflict),
            merge_conflict_labels: Some(labels),
        }
    }
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
