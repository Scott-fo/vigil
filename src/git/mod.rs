use std::sync::Arc;

mod changed_files;
mod command;
mod commit;
mod diff;
mod highlight;
mod merge;
mod parse;
mod refs;
mod repo;
mod status;
mod types;
mod worktree;

pub use command::git_output;
pub use commit::{
    commit_staged_changes, list_searchable_commits, load_blame_commit_details,
    load_files_with_commit_diff, resolve_commit_base_ref,
};
pub use diff::{
    ChangeType, CodeColumnType, CodeOverflow, DiffDisplayLineAnchor, DiffFileMetrics,
    DiffHunkResolution, DiffIconType, DiffIndicators, DiffIterationOptions, DiffLine,
    DiffLineAnnotation, DiffLineMetadata, DiffLineType, DiffPreviewData, DiffSearchIndex,
    DiffSearchLineKind, DiffSearchMatcher, DiffSearchMode, DiffSearchOptions,
    DiffSearchPreviewLine, DiffSearchResult, DiffSearchResults, DiffSearchSyntaxRange,
    DiffSelectionPane, DiffSelectionPoint, DiffStyle, DiffView, EstimatedDiffHeightOptions,
    EstimatedDiffHeights, ExpandedHunks, ExpandedRegion, FileContents, FileDiffMetadata,
    FileIterationOptions, FileLine, GapExpandDirection, Hunk, HunkContent, HunkData,
    HunkDataExpandable, HunkExpansionRegion, HunkLineType, HunkSeparatorKind, HunkSeparatorLayout,
    LineAnnotation, LineAnnotationName, LineDiffType, LineEndingType, MergeConflictActionAnchor,
    MergeConflictActionSlotInput, MergeConflictDiffAction, MergeConflictLineType,
    MergeConflictMarkerLines, MergeConflictMarkerRow, MergeConflictMarkerRowType,
    MergeConflictParseResult, MergeConflictRegion, MergeConflictResolution, ParseDiffOptions,
    ParseMergeConflictDiffFromFileResult, ParsedLine, ParsedPatch, PreNodeType,
    PrePropertiesConfig, ProcessFileConflictData, RenderDiffOptions, RenderFileOptions,
    RenderRange, ReviewDiffSnapshot, ReviewDiffStats, ReviewDiffTextIndex, SelectedLineRange,
    SelectionSide, ThemeSpec, VirtualFileMetrics, VirtualWindowSpecs,
    WindowFromScrollPositionOptions, WorkerStats, are_diff_line_annotations_equal,
    are_diff_render_options_equal, are_diff_targets_equal, are_file_render_options_equal,
    are_files_equal, are_hunk_data_equal, are_line_annotations_equal,
    are_merge_conflict_actions_equal, are_objects_equal, are_pre_properties_equal,
    are_render_ranges_equal, are_selections_equal, are_themes_equal,
    are_virtual_window_specs_equal, are_worker_stats_equal, build_diff_view_from_diff_text,
    build_diff_view_from_diff_text_with_context, build_diff_view_from_file_metadata,
    build_diff_view_from_preview_data, build_merge_conflict_marker_rows, clean_last_newline,
    collect_diff_lines, compute_estimated_diff_heights, compute_virtual_file_metrics,
    create_window_from_scroll_position, diff_accept_reject_content, diff_accept_reject_hunk,
    get_default_hunk_separator_height, get_expanded_region_public, get_hunk_separator_gap,
    get_hunk_separator_height, get_hunk_separator_slot_name, get_icon_for_type,
    get_leading_hunk_separator_layout, get_line_annotation_name, get_line_ending_type,
    get_merge_conflict_action_anchor, get_merge_conflict_action_line_number,
    get_merge_conflict_action_slot_name, get_merge_conflict_line_types,
    get_merge_conflict_parse_result, get_singular_patch, get_total_line_count_from_hunks,
    get_trailing_hunk_separator_layout, get_virtual_file_header_region,
    get_virtual_file_padding_bottom, get_virtual_file_padding_top, has_leading_hunk_separator,
    has_trailing_hunk_separator, is_default_render_range, iterate_over_diff, iterate_over_file,
    load_diff_preview_for_branch_compare, load_diff_preview_for_commit_compare,
    load_diff_preview_for_working_tree, load_diff_search_index_for_branch_compare,
    load_diff_search_index_for_commit_compare, load_diff_search_index_for_working_tree,
    load_diff_view, load_diff_view_for_branch_compare, load_diff_view_for_commit_compare,
    load_diff_view_for_working_tree, load_review_diff_snapshot_for_branch_compare,
    load_review_diff_snapshot_for_commit_compare, load_review_diff_snapshot_for_working_tree,
    load_review_diff_stats_for_branch_compare, load_review_diff_stats_for_commit_compare,
    load_review_diff_stats_for_working_tree, load_review_diff_text_index_for_branch_compare,
    load_review_diff_text_index_for_commit_compare, load_review_diff_text_index_for_working_tree,
    parse_diff_from_file, parse_line_type, parse_merge_conflict_diff_from_file, parse_patch_files,
    process_file, process_patch, resolve_conflict, resolve_merge_conflict_contents,
    trim_patch_context,
};
pub use highlight::{HighlightRegistry, clear_exact_highlight_cache, prewarm_highlight_registry};
pub use merge::prepare_branch_merge;
pub use refs::{list_comparable_refs, load_branch_compare_refs, load_files_with_branch_diff};
pub use repo::{
    init_repo, load_revision_file_bytes, pull_from_remote, push_to_remote, resolve_repo_root,
    resolve_repo_root_from, revision_matches_head,
};
pub use status::{
    WorkingTreeStatus, discard_file_changes, is_file_fully_staged, is_file_staged,
    load_files_with_status, load_status_for_path, load_working_tree_status,
    should_refresh_for_paths, stage_all_changes, status_color, toggle_file_stage,
    unstage_all_changes,
};
pub use types::{
    BlameCommitDetails, BlameTarget, BranchCompareRefs, BranchCompareSelection, BranchMergeOutcome,
    BranchMergeRequest, CommitCompareSelection, CommitSearchEntry, FileEntry, WorktreeEntry,
};
pub use worktree::list_worktrees;

pub type SharedHighlightRegistry = Arc<HighlightRegistry>;
pub const EMPTY_TREE_HASH: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
pub(crate) const LOG_FIELD_SEPARATOR: char = '\u{001f}';
pub(crate) const LOG_RECORD_SEPARATOR: char = '\u{001e}';
