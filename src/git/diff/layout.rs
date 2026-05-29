use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::iteration::{
    ExpandedRegionResult, get_expanded_region, get_trailing_range_size, has_final_collapsed_hunk,
    no_newline_metadata_line_counts,
};
use super::{DiffStyle, FileDiffMetadata, Hunk, HunkSeparatorKind};

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

#[inline]
fn expanded_region_from_result(region: ExpandedRegionResult) -> ExpandedRegion {
    ExpandedRegion {
        from_start: region.from_start,
        from_end: region.from_end,
        range_size: region.range_size,
        collapsed_lines: region.collapsed_lines,
        render_all: region.collapsed_lines == 0 && region.range_size > 0,
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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

#[inline]
pub fn compute_virtual_file_metrics(metrics: Option<VirtualFileMetrics>) -> VirtualFileMetrics {
    metrics.unwrap_or_default()
}

#[inline]
pub fn are_render_ranges_equal(
    render_range_a: Option<&RenderRange>,
    render_range_b: Option<&RenderRange>,
) -> bool {
    render_range_a == render_range_b
}

#[inline]
pub fn is_default_render_range(render_range: &RenderRange) -> bool {
    render_range.starting_line == 0
        && render_range.total_lines.is_none()
        && render_range.buffer_before == 0
        && render_range.buffer_after == 0
}

#[inline]
pub fn are_virtual_window_specs_equal(
    window_specs_a: Option<&VirtualWindowSpecs>,
    window_specs_b: Option<&VirtualWindowSpecs>,
) -> bool {
    window_specs_a == window_specs_b
}

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
pub fn get_virtual_file_padding_bottom(metrics: &VirtualFileMetrics) -> usize {
    metrics.padding_bottom.unwrap_or(metrics.spacing)
}

#[inline]
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

#[inline]
pub fn get_default_hunk_separator_height(kind: HunkSeparatorKind) -> usize {
    match kind {
        HunkSeparatorKind::Simple => 4,
        HunkSeparatorKind::Metadata
        | HunkSeparatorKind::LineInfo
        | HunkSeparatorKind::LineInfoBasic
        | HunkSeparatorKind::Custom => 32,
    }
}

#[inline]
pub fn get_expanded_region_public(
    is_partial: bool,
    range_size: usize,
    expanded_hunks: Option<ExpandedHunks<'_>>,
    hunk_index: usize,
    collapsed_context_threshold: usize,
) -> ExpandedRegion {
    expanded_region_from_result(get_expanded_region(
        is_partial,
        range_size,
        expanded_hunks,
        hunk_index,
        collapsed_context_threshold,
    ))
}

#[inline]
pub fn get_hunk_separator_height(kind: HunkSeparatorKind, metrics: &VirtualFileMetrics) -> usize {
    metrics
        .hunk_separator_height
        .unwrap_or_else(|| get_default_hunk_separator_height(kind))
}

#[inline]
pub fn get_hunk_separator_gap(kind: HunkSeparatorKind, metrics: &VirtualFileMetrics) -> usize {
    match kind {
        HunkSeparatorKind::Simple
        | HunkSeparatorKind::Metadata
        | HunkSeparatorKind::LineInfoBasic => 0,
        HunkSeparatorKind::LineInfo | HunkSeparatorKind::Custom => metrics.spacing,
    }
}

#[inline]
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

#[inline]
pub fn has_trailing_hunk_separator(kind: HunkSeparatorKind) -> bool {
    !matches!(
        kind,
        HunkSeparatorKind::Simple | HunkSeparatorKind::Metadata
    )
}

#[inline]
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

#[inline]
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

#[inline]
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
