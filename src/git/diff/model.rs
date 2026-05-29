use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::ChangeType;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeConflictLabels {
    pub current: String,
    pub incoming: String,
}

impl Default for MergeConflictLabels {
    fn default() -> Self {
        Self {
            current: "current".to_string(),
            incoming: "incoming".to_string(),
        }
    }
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
