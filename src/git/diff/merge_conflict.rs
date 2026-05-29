use color_eyre::eyre::eyre;

use super::{
    ChangeType, FileContents, FileDiffMetadata, Hunk, HunkContent, MergeConflictActionAnchor,
    MergeConflictActionSlotInput, MergeConflictDiffAction, MergeConflictLineType,
    MergeConflictMarkerLines, MergeConflictMarkerRow, MergeConflictMarkerRowType,
    MergeConflictParseResult, MergeConflictRegion, MergeConflictResolution,
    ParseMergeConflictDiffFromFileResult, ProcessFileConflictData,
};

#[inline]
pub fn get_merge_conflict_line_types(lines: &[String]) -> Vec<MergeConflictLineType> {
    get_merge_conflict_parse_result(lines).line_types
}

#[inline]
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

#[inline]
pub fn get_merge_conflict_action_line_number(conflict: &MergeConflictRegion) -> usize {
    conflict.start_line_number.saturating_sub(1).max(1)
}

#[inline]
pub fn get_merge_conflict_action_slot_name(input: MergeConflictActionSlotInput) -> String {
    format!(
        "merge-conflict-action-{}-{}-{}",
        input.hunk_index, input.line_index, input.conflict_index
    )
}

#[inline]
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

#[inline]
fn is_merge_conflict_start_marker(line: &str) -> bool {
    is_repeated_marker_with_optional_label(line, b'<')
}

#[inline]
fn is_merge_conflict_base_marker(line: &str) -> bool {
    is_repeated_marker_with_optional_label(line, b'|')
}

#[inline]
fn is_merge_conflict_end_marker(line: &str) -> bool {
    is_repeated_marker_with_optional_label(line, b'>')
}

#[inline]
fn is_merge_conflict_separator_marker(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.len() >= 7 && bytes.iter().all(|byte| *byte == b'=')
}

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
fn ensure_synthetic_conflict_hunk(state: &mut SyntheticConflictParseState) {
    if state.active_hunk.is_none() {
        state.active_hunk = Some(create_synthetic_conflict_hunk_builder(
            state.addition_lines.len() + 1,
            state.deletion_lines.len() + 1,
        ));
    }
}

#[inline]
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

#[inline]
fn format_hunk_range_for_header(start: usize, count: usize) -> String {
    if count == 1 {
        start.to_string()
    } else {
        format!("{start},{count}")
    }
}

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
fn unified_line_start_from_cache(starts: &[usize], content_index: usize) -> usize {
    starts
        .get(content_index)
        .copied()
        .or_else(|| starts.last().copied())
        .unwrap_or(0)
}

#[inline]
fn unified_line_end_from_cache(starts: &[usize], content_index: usize) -> usize {
    let start = unified_line_start_from_cache(starts, content_index);
    let end_exclusive = unified_line_start_from_cache(starts, content_index.saturating_add(1));
    start.max(end_exclusive.saturating_sub(1))
}

#[inline]
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

#[inline]
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

#[inline]
fn is_merge_conflict_marker_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b' ')
}

#[inline]
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

#[inline]
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

#[inline]
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
