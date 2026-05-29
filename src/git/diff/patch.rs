use color_eyre::eyre::eyre;

use super::{ChangeType, FileDiffMetadata, Hunk, HunkContent, ParsedPatch};

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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
pub fn trim_patch_context(patch: &str, context_size: usize) -> String {
    let context_window = context_size.saturating_mul(2);
    let estimated_lines = patch.len().saturating_div(40).saturating_add(1);
    let mut lines = Vec::with_capacity(estimated_lines);
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
                hunk_lines: Vec::with_capacity(context_window.saturating_add(8)),
                context_lines: Vec::with_capacity(context_window.saturating_add(1)),
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
            if !hunk.hunk_lines.is_empty() && hunk.context_lines.len() > context_window {
                let omitted_context_line_count = hunk.context_lines.len() - context_window;
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
                    hunk_lines: Vec::with_capacity(context_window.saturating_add(8)),
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

    let mut result = lines.join("\n");
    if patch.ends_with('\n') {
        result.push('\n');
    }
    result
}

#[inline]
fn has_commit_metadata_boundary(data: &str) -> bool {
    data.starts_with("From ") || data.contains("\nFrom ")
}

#[inline]
fn is_git_diff_patch(data: &str) -> bool {
    data.starts_with("diff --git") || data.contains("\ndiff --git")
}

#[inline]
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

#[inline]
fn find_line_prefix_index(contents: &str, prefix: &str, from_index: usize) -> Option<usize> {
    if from_index == 0 && contents.starts_with(prefix) {
        return Some(0);
    }

    let newline_prefix = format!("\n{prefix}");
    contents[from_index..]
        .find(&newline_prefix)
        .map(|index| from_index + index + 1)
}

#[inline]
fn split_at_unified_file_break(contents: &str) -> Vec<&str> {
    split_at_line_boundaries(contents, is_unified_file_break)
}

#[inline]
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

#[inline]
fn is_unified_file_break(line: &str) -> bool {
    let trimmed_newline = line.trim_end_matches(['\r', '\n']);
    let Some(rest) = trimmed_newline.strip_prefix("---") else {
        return false;
    };
    let mut chars = rest.chars();
    chars.next().is_some_and(char::is_whitespace)
        && chars.next().is_some_and(|ch| !ch.is_whitespace())
}

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
fn flush_trim_hunk(hunk: &TrimmedPatchHunk, lines: &mut Vec<String>) {
    lines.push(format!(
        "@@ -{} +{} @@",
        format_trim_hunk_range(hunk.deletion_start, hunk.deletion_count),
        format_trim_hunk_range(hunk.addition_start, hunk.addition_count)
    ));
    lines.extend(hunk.hunk_lines.iter().cloned());
}

#[inline]
fn format_trim_hunk_range(start: usize, count: usize) -> String {
    if count == 1 {
        start.to_string()
    } else {
        format!("{start},{count}")
    }
}

#[inline]
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

#[inline]
fn parse_raw_line_type(first_char: char) -> Option<ParsedRawLineType> {
    match first_char {
        ' ' => Some(ParsedRawLineType::Context),
        '+' => Some(ParsedRawLineType::Addition),
        '-' => Some(ParsedRawLineType::Deletion),
        '\\' => Some(ParsedRawLineType::Metadata),
        _ => None,
    }
}

#[inline]
fn get_parsed_line_content(raw_line: &str) -> String {
    let processed = raw_line.get(1..).unwrap_or("");
    if processed.is_empty() {
        "\n".to_string()
    } else {
        processed.to_string()
    }
}

#[inline]
fn clean_last_line(lines: &mut [String]) {
    if let Some(line) = lines.last_mut() {
        if line.ends_with("\r\n") {
            line.truncate(line.len() - 2);
        } else if line.ends_with('\n') {
            line.truncate(line.len() - 1);
        }
    }
}

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
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

#[inline]
fn parse_git_diff_names(line: &str) -> Option<(String, String)> {
    let mut rest = line.strip_prefix("diff --git ")?;
    let prev = parse_git_header_path(&mut rest)?;
    rest = rest.trim_start();
    let next = parse_git_header_path(&mut rest)?;
    Some((prev, next))
}

#[inline]
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

#[inline]
fn strip_git_side_prefix(path: &str) -> Option<&str> {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .or_else(|| (path == "/dev/null").then_some(path))
}

#[inline]
fn trim_line_end(value: &str) -> &str {
    value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(value)
}
