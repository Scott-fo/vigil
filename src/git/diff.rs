use super::highlight::SyntaxToken;
use crate::app::DiffViewMode;

mod display;
mod equality;
mod full_file;
mod highlighting;
mod icons;
mod iteration;
mod layout;
mod line;
mod merge_conflict;
mod model;
mod patch;
mod preview;
mod rendering;
mod resolution;
mod rows;
mod search;
mod snapshot;
mod view;

pub(super) use self::display::{
    DiffDisplayCache, DisplayNavTarget, DisplayRowRefs, DisplaySelectionLine,
    DisplaySelectionSegment,
};
pub use self::equality::{
    are_diff_line_annotations_equal, are_diff_render_options_equal, are_diff_targets_equal,
    are_file_render_options_equal, are_files_equal, are_hunk_data_equal,
    are_line_annotations_equal, are_merge_conflict_actions_equal, are_objects_equal,
    are_pre_properties_equal, are_selections_equal, are_themes_equal, are_worker_stats_equal,
    get_line_annotation_name,
};
pub use self::full_file::parse_diff_from_file;
pub use self::icons::{ChangeType, DiffIconType, get_icon_for_type};
pub use self::iteration::{collect_diff_lines, iterate_over_diff, iterate_over_file};
pub use self::layout::{
    DiffIterationOptions, EstimatedDiffHeightOptions, EstimatedDiffHeights, ExpandedHunks,
    ExpandedRegion, HunkExpansionRegion, HunkSeparatorLayout, RenderRange, VirtualFileMetrics,
    VirtualWindowSpecs, WindowFromScrollPositionOptions, are_render_ranges_equal,
    are_virtual_window_specs_equal, compute_estimated_diff_heights, compute_virtual_file_metrics,
    create_window_from_scroll_position, get_default_hunk_separator_height,
    get_expanded_region_public, get_hunk_separator_gap, get_hunk_separator_height,
    get_leading_hunk_separator_layout, get_total_line_count_from_hunks,
    get_trailing_hunk_separator_layout, get_virtual_file_header_region,
    get_virtual_file_padding_bottom, get_virtual_file_padding_top, has_leading_hunk_separator,
    has_trailing_hunk_separator, is_default_render_range,
};
use self::line::line_without_ending;
pub use self::line::{
    clean_last_newline, get_hunk_separator_slot_name, get_line_ending_type, parse_line_type,
};
pub use self::merge_conflict::{
    build_merge_conflict_marker_rows, get_merge_conflict_action_anchor,
    get_merge_conflict_action_line_number, get_merge_conflict_action_slot_name,
    get_merge_conflict_line_types, get_merge_conflict_parse_result,
    parse_merge_conflict_diff_from_file, resolve_merge_conflict_contents,
};
pub use self::model::{
    CodeColumnType, CodeOverflow, DiffHunkResolution, DiffIndicators, DiffLine, DiffLineAnnotation,
    DiffLineMetadata, DiffLineType, DiffStyle, FileContents, FileDiffMetadata,
    FileIterationOptions, FileLine, GapExpandDirection, Hunk, HunkContent, HunkData,
    HunkDataExpandable, HunkLineType, HunkSeparatorKind, LineAnnotation, LineAnnotationName,
    LineDiffType, LineEndingType, MergeConflictActionAnchor, MergeConflictActionSlotInput,
    MergeConflictDiffAction, MergeConflictLabels, MergeConflictLineType, MergeConflictMarkerLines,
    MergeConflictMarkerRow, MergeConflictMarkerRowType, MergeConflictParseResult,
    MergeConflictRegion, MergeConflictResolution, ParseDiffOptions,
    ParseMergeConflictDiffFromFileResult, ParsedLine, ParsedPatch, PreNodeType,
    PrePropertiesConfig, ProcessFileConflictData, RenderDiffOptions, RenderFileOptions,
    SelectedLineRange, SelectionSide, ThemeSpec, WorkerStats,
};
pub use self::patch::{
    get_singular_patch, parse_patch_files, process_file, process_patch, trim_patch_context,
};
pub use self::preview::{
    load_diff_preview_for_branch_compare, load_diff_preview_for_commit_compare,
    load_diff_preview_for_working_tree, load_diff_view, load_diff_view_for_branch_compare,
    load_diff_view_for_commit_compare, load_diff_view_for_working_tree,
};
pub use self::resolution::{diff_accept_reject_content, diff_accept_reject_hunk, resolve_conflict};
pub use self::search::{
    DiffSearchIndex, DiffSearchLineKind, DiffSearchMatcher, DiffSearchMode, DiffSearchOptions,
    DiffSearchPreviewLine, DiffSearchResult, DiffSearchResults, DiffSearchSyntaxRange,
    load_diff_search_index_for_branch_compare, load_diff_search_index_for_commit_compare,
    load_diff_search_index_for_working_tree,
};
pub use self::snapshot::{
    DiffFileMetrics, ReviewDiffSnapshot, load_review_diff_snapshot_for_branch_compare,
    load_review_diff_snapshot_for_commit_compare, load_review_diff_snapshot_for_working_tree,
};
pub use self::view::{
    DiffDisplayLineAnchor, DiffPreviewData, DiffSelectionPane, DiffSelectionPoint, DiffView,
    build_diff_view_from_diff_text, build_diff_view_from_diff_text_with_context,
    build_diff_view_from_file_metadata, build_diff_view_from_preview_data,
};
use self::view::{DiffHunkBlock, DiffHunkGap, DiffLineKind, DiffRow, DiffRowSyntax};

const DIFF_TAB_WIDTH: usize = 4;

#[cfg(test)]
mod tests;
