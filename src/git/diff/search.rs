//! Searchable indexes for diff hunks.
//!
//! This module turns parsed diff data into a compact, reusable search surface.
//! Callers build an index for a point-in-time review scope, then reuse a
//! `DiffSearchMatcher` while the user edits the query. Search scans the
//! prepared line candidates, keeps only the requested top results, and computes
//! match ranges after ranking so large diffs do not allocate one result per
//! match.

use std::{
    cmp::{Ordering, Reverse},
    collections::BinaryHeap,
    ops::Range,
    path::Path,
};

use color_eyre::eyre::WrapErr;
use tokio::task;
use unicode_segmentation::UnicodeSegmentation;

use super::super::{
    BranchCompareSelection, CommitCompareSelection, FileEntry,
    command::git_output,
    highlight::{HighlightRegistry, highlight_source_lines},
    parse::{build_branch_diff_range, resolve_diff_filetype},
};
use super::{
    DiffPreviewData, FileDiffMetadata, Hunk, HunkContent, ParsedPatch, line_without_ending,
    preview::load_diff_preview_for_working_tree,
};

mod case_insensitive_memmem;

const DIFF_SEARCH_PREVIEW_CONTEXT_LINES: usize = 4;

#[derive(Debug, Default, Clone)]
pub struct DiffSearchIndex {
    files: Vec<IndexedDiffFile>,
    lines: Vec<IndexedDiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedDiffFile {
    path: Box<str>,
    filetype: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedDiffLine {
    file_index: usize,
    hunk_index: usize,
    hunk_old_start: usize,
    hunk_new_start: usize,
    kind: DiffSearchLineKind,
    old_line: Option<usize>,
    new_line: Option<usize>,
    text: Box<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffSearchOptions {
    pub limit: usize,
    pub include_context: bool,
    pub mode: DiffSearchMode,
}

impl Default for DiffSearchOptions {
    fn default() -> Self {
        Self {
            limit: 80,
            include_context: true,
            mode: DiffSearchMode::Literal,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DiffSearchMode {
    /// Treat the query as literal text. Whitespace-separated terms may appear
    /// with separators between them, so `render status` matches
    /// `render_status`.
    #[default]
    Literal,
    /// Score candidate lines with neo_frizbee's fuzzy matcher.
    Fuzzy,
}

impl DiffSearchMode {
    pub fn toggle(self) -> Self {
        match self {
            Self::Literal => Self::Fuzzy,
            Self::Fuzzy => Self::Literal,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Fuzzy => "fuzzy",
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct DiffSearchResults {
    pub total_matched: usize,
    pub items: Vec<DiffSearchResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSearchResult {
    pub file_path: String,
    pub filetype: Option<&'static str>,
    pub hunk_index: usize,
    pub hunk_old_start: usize,
    pub hunk_new_start: usize,
    pub kind: DiffSearchLineKind,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub line: String,
    pub match_ranges: Vec<Range<usize>>,
    pub syntax_ranges: Vec<DiffSearchSyntaxRange>,
    pub preview_lines: Vec<DiffSearchPreviewLine>,
    pub score: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSearchPreviewLine {
    pub kind: DiffSearchLineKind,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub line: String,
    pub is_match: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSearchSyntaxRange {
    pub start: usize,
    pub end: usize,
    pub highlight_name: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSearchLineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Default, Clone)]
pub struct DiffSearchMatcher;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RankedLine {
    score: u32,
    line_index: usize,
}

impl Ord for RankedLine {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .cmp(&other.score)
            .then_with(|| other.line_index.cmp(&self.line_index))
    }
}

impl PartialOrd for RankedLine {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl DiffSearchIndex {
    pub fn from_diff_text(diff: &str) -> color_eyre::Result<Self> {
        let mut index = Self::default();
        index.append_diff_text(diff)?;
        Ok(index)
    }

    pub fn from_patches(patches: &[ParsedPatch]) -> Self {
        let mut index = Self::default();
        for patch in patches {
            for file in &patch.files {
                index.push_file(file);
            }
        }
        index
    }

    pub(super) fn from_file_metadata<'a>(
        files: impl IntoIterator<Item = &'a FileDiffMetadata>,
    ) -> Self {
        let mut index = Self::default();
        for file in files {
            index.push_file(file);
        }
        index
    }

    pub fn append_preview_data(&mut self, preview: &DiffPreviewData) -> color_eyre::Result<()> {
        if let Some(merge_conflict) = &preview.merge_conflict {
            self.push_file(&merge_conflict.file_diff);
            return Ok(());
        }

        self.append_diff_text(&preview.diff)
    }

    pub fn append_diff_text(&mut self, diff: &str) -> color_eyre::Result<()> {
        let patches = super::parse_patch_files(diff, None, false)?;
        for patch in patches {
            for file in patch.files {
                self.push_file(&file);
            }
        }

        Ok(())
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn search(
        &self,
        query: &str,
        options: DiffSearchOptions,
        _matcher: &mut DiffSearchMatcher,
    ) -> DiffSearchResults {
        let (mode, query) = match normalize_search_query(query, options.mode) {
            Some(query) => query,
            None => return DiffSearchResults::default(),
        };

        if query.is_empty() || options.limit == 0 || self.lines.is_empty() {
            return DiffSearchResults::default();
        }

        match mode {
            DiffSearchMode::Literal => self.search_literal(query, options),
            DiffSearchMode::Fuzzy => self.search_fuzzy(query, options),
        }
    }

    fn search_literal(&self, query: &str, options: DiffSearchOptions) -> DiffSearchResults {
        let Some(query) = LiteralDiffSearchQuery::new(query) else {
            return DiffSearchResults::default();
        };

        let mut total_matched = 0usize;
        let mut items = Vec::with_capacity(options.limit.min(self.lines.len()));
        for (line_index, line) in self.lines.iter().enumerate() {
            if !options.include_context && line.kind == DiffSearchLineKind::Context {
                continue;
            }

            if items.len() < options.limit {
                if let Some(match_ranges) = query.match_ranges(&line.text) {
                    total_matched = total_matched.saturating_add(1);
                    let score = literal_match_score(&line.text, &match_ranges);
                    items.push(self.search_result_from_ranges(line_index, match_ranges, score));
                }
            } else if query.is_match(&line.text) {
                total_matched = total_matched.saturating_add(1);
            }
        }

        DiffSearchResults {
            total_matched,
            items,
        }
    }

    fn search_fuzzy(&self, query: &str, options: DiffSearchOptions) -> DiffSearchResults {
        let config = fuzzy_search_config(query);
        let mut searchable_lines = Vec::with_capacity(self.lines.len());
        let mut line_indices = Vec::with_capacity(self.lines.len());
        for (line_index, line) in self.lines.iter().enumerate() {
            if !options.include_context && line.kind == DiffSearchLineKind::Context {
                continue;
            }
            searchable_lines.push(line.text.as_ref());
            line_indices.push(line_index);
        }

        if searchable_lines.is_empty() {
            return DiffSearchResults::default();
        }

        let min_score = fuzzy_min_score(query);
        let mut total_matched = 0usize;
        let mut top = BinaryHeap::with_capacity(options.limit.saturating_add(1));
        for matched in neo_frizbee::match_list(query, &searchable_lines, &config) {
            if matched.score < min_score {
                continue;
            }

            let Some(line_index) = line_indices.get(matched.index as usize).copied() else {
                continue;
            };

            total_matched = total_matched.saturating_add(1);
            let ranked = RankedLine {
                score: matched.score.into(),
                line_index,
            };
            if top.len() < options.limit {
                top.push(Reverse(ranked));
            } else if let Some(worst) = top.peek()
                && ranked > worst.0
            {
                let _ = top.pop();
                top.push(Reverse(ranked));
            }
        }

        let mut ranked = top
            .into_iter()
            .map(|Reverse(ranked)| ranked)
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.line_index.cmp(&right.line_index))
        });

        let items = ranked
            .into_iter()
            .filter_map(|ranked| {
                let line = &self.lines[ranked.line_index];
                let match_ranges = fuzzy_match_ranges(query, &line.text, &config)?;
                Some(self.search_result_from_ranges(ranked.line_index, match_ranges, ranked.score))
            })
            .collect();

        DiffSearchResults {
            total_matched,
            items,
        }
    }

    fn push_file(&mut self, file: &FileDiffMetadata) {
        let file_index = self.files.len();
        self.files.push(IndexedDiffFile {
            path: file.name.clone().into_boxed_str(),
            filetype: resolve_diff_filetype(&file.name),
        });

        for (hunk_index, hunk) in file.hunks.iter().enumerate() {
            self.push_hunk(file_index, hunk_index, hunk, file);
        }
    }

    fn append_index(&mut self, mut other: DiffSearchIndex) {
        let file_index_offset = self.files.len();
        for line in &mut other.lines {
            line.file_index += file_index_offset;
        }
        self.files.extend(other.files);
        self.lines.extend(other.lines);
    }

    fn push_hunk(
        &mut self,
        file_index: usize,
        hunk_index: usize,
        hunk: &Hunk,
        file: &FileDiffMetadata,
    ) {
        let mut old_line = hunk.deletion_start;
        let mut new_line = hunk.addition_start;

        for content in &hunk.hunk_content {
            match content {
                HunkContent::Context {
                    lines,
                    addition_line_index,
                    ..
                } => {
                    for offset in 0..*lines {
                        let text = file
                            .addition_lines
                            .get(addition_line_index + offset)
                            .map(|line| line_without_ending(line))
                            .unwrap_or("");
                        self.push_line(
                            file_index,
                            hunk_index,
                            hunk,
                            DiffSearchLineKind::Context,
                            Some(old_line),
                            Some(new_line),
                            text,
                        );
                        old_line += 1;
                        new_line += 1;
                    }
                }
                HunkContent::Change {
                    deletions,
                    deletion_line_index,
                    additions,
                    addition_line_index,
                } => {
                    for offset in 0..*deletions {
                        let text = file
                            .deletion_lines
                            .get(deletion_line_index + offset)
                            .map(|line| line_without_ending(line))
                            .unwrap_or("");
                        self.push_line(
                            file_index,
                            hunk_index,
                            hunk,
                            DiffSearchLineKind::Deletion,
                            Some(old_line),
                            None,
                            text,
                        );
                        old_line += 1;
                    }
                    for offset in 0..*additions {
                        let text = file
                            .addition_lines
                            .get(addition_line_index + offset)
                            .map(|line| line_without_ending(line))
                            .unwrap_or("");
                        self.push_line(
                            file_index,
                            hunk_index,
                            hunk,
                            DiffSearchLineKind::Addition,
                            None,
                            Some(new_line),
                            text,
                        );
                        new_line += 1;
                    }
                }
            }
        }
    }

    fn push_line(
        &mut self,
        file_index: usize,
        hunk_index: usize,
        hunk: &Hunk,
        kind: DiffSearchLineKind,
        old_line: Option<usize>,
        new_line: Option<usize>,
        text: &str,
    ) {
        self.lines.push(IndexedDiffLine {
            file_index,
            hunk_index,
            hunk_old_start: hunk.deletion_start,
            hunk_new_start: hunk.addition_start,
            kind,
            old_line,
            new_line,
            text: text.into(),
        });
    }

    fn search_result_from_ranges(
        &self,
        line_index: usize,
        match_ranges: Vec<Range<usize>>,
        score: u32,
    ) -> DiffSearchResult {
        let line = &self.lines[line_index];

        DiffSearchResult {
            file_path: self.files[line.file_index].path.to_string(),
            filetype: self.files[line.file_index].filetype,
            hunk_index: line.hunk_index,
            hunk_old_start: line.hunk_old_start,
            hunk_new_start: line.hunk_new_start,
            kind: line.kind,
            old_line: line.old_line,
            new_line: line.new_line,
            line: line.text.to_string(),
            match_ranges,
            syntax_ranges: Vec::new(),
            preview_lines: self.preview_lines_for(line_index),
            score,
        }
    }

    fn preview_lines_for(&self, line_index: usize) -> Vec<DiffSearchPreviewLine> {
        let Some(target) = self.lines.get(line_index) else {
            return Vec::new();
        };

        let mut start = line_index;
        let mut before = 0usize;
        while start > 0 && before < DIFF_SEARCH_PREVIEW_CONTEXT_LINES {
            let previous_index = start - 1;
            if !self.same_hunk(previous_index, target) {
                break;
            }
            start = previous_index;
            before += 1;
        }

        let mut end = line_index + 1;
        let mut after = 0usize;
        while end < self.lines.len() && after < DIFF_SEARCH_PREVIEW_CONTEXT_LINES {
            if !self.same_hunk(end, target) {
                break;
            }
            end += 1;
            after += 1;
        }

        self.lines[start..end]
            .iter()
            .enumerate()
            .map(|(offset, line)| DiffSearchPreviewLine {
                kind: line.kind,
                old_line: line.old_line,
                new_line: line.new_line,
                line: line.text.to_string(),
                is_match: start + offset == line_index,
            })
            .collect()
    }

    fn same_hunk(&self, line_index: usize, target: &IndexedDiffLine) -> bool {
        self.lines.get(line_index).is_some_and(|line| {
            line.file_index == target.file_index && line.hunk_index == target.hunk_index
        })
    }
}

impl DiffSearchResults {
    pub fn group_items_by_file(&mut self) {
        let mut groups = Vec::<(String, Vec<DiffSearchResult>)>::new();
        for item in std::mem::take(&mut self.items) {
            if let Some((_, items)) = groups
                .iter_mut()
                .find(|(file_path, _)| file_path == &item.file_path)
            {
                items.push(item);
            } else {
                groups.push((item.file_path.clone(), vec![item]));
            }
        }

        self.items = groups.into_iter().flat_map(|(_, items)| items).collect();
    }

    pub fn apply_syntax_highlighting(&mut self, registry: &HighlightRegistry) {
        for item in &mut self.items {
            item.apply_syntax_highlighting(registry);
        }
    }
}

impl DiffSearchResult {
    fn apply_syntax_highlighting(&mut self, registry: &HighlightRegistry) {
        let Some(filetype) = self.filetype else {
            return;
        };

        let Some(highlighted_lines) = highlight_source_lines(registry, filetype, &self.line) else {
            return;
        };
        let Some(tokens) = highlighted_lines.into_iter().next() else {
            return;
        };

        self.syntax_ranges = tokens
            .into_iter()
            .map(|token| DiffSearchSyntaxRange {
                start: token.start,
                end: token.end,
                highlight_name: token.highlight_name,
            })
            .collect();
    }
}

pub async fn load_diff_search_index_for_working_tree(
    repo_root: &Path,
    files: &[FileEntry],
) -> color_eyre::Result<DiffSearchIndex> {
    if files.iter().any(|file| is_unmerged_status(&file.status)) {
        return load_working_tree_index_file_by_file(repo_root, files).await;
    }

    let mut index = DiffSearchIndex::default();
    if files.iter().any(|file| file.status != "??") {
        let diff = git_output(
            repo_root,
            &["diff", "--no-color", "--find-renames", "HEAD", "--"],
        )
        .await?;
        index.append_index(index_from_diff_text(diff).await?);
    }

    for file in files.iter().filter(|file| file.status == "??") {
        let preview = load_diff_preview_for_working_tree(repo_root, file, false).await?;
        index.append_preview_data(&preview)?;
    }

    Ok(index)
}

pub async fn load_diff_search_index_for_commit_compare(
    repo_root: &Path,
    selection: &CommitCompareSelection,
) -> color_eyre::Result<DiffSearchIndex> {
    let diff = git_output(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            selection.base_ref.as_str(),
            selection.commit_hash.as_str(),
        ],
    )
    .await?;
    index_from_diff_text(diff).await
}

pub async fn load_diff_search_index_for_branch_compare(
    repo_root: &Path,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<DiffSearchIndex> {
    let diff_range = build_branch_diff_range(selection);
    let diff = git_output(
        repo_root,
        &["diff", "--no-color", "--find-renames", diff_range.as_str()],
    )
    .await?;
    index_from_diff_text(diff).await
}

async fn load_working_tree_index_file_by_file(
    repo_root: &Path,
    files: &[FileEntry],
) -> color_eyre::Result<DiffSearchIndex> {
    let mut index = DiffSearchIndex::default();
    for file in files {
        let preview = load_diff_preview_for_working_tree(repo_root, file, false).await?;
        index.append_preview_data(&preview)?;
    }
    Ok(index)
}

fn is_unmerged_status(status: &str) -> bool {
    status.contains('U')
}

async fn index_from_diff_text(diff: String) -> color_eyre::Result<DiffSearchIndex> {
    task::spawn_blocking(move || DiffSearchIndex::from_diff_text(&diff))
        .await
        .wrap_err("diff search index parse task failed")?
}

fn normalize_search_query(query: &str, mode: DiffSearchMode) -> Option<(DiffSearchMode, &str)> {
    let mut query = query.trim();
    if query.is_empty() {
        return None;
    }

    if let Some(stripped) = query.strip_prefix('\'') {
        query = stripped.trim_start();
        return (!query.is_empty()).then_some((DiffSearchMode::Literal, query));
    }

    Some((mode, query))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiteralDiffSearchQuery {
    tokens: Vec<LiteralDiffSearchToken>,
    case_insensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiteralDiffSearchToken {
    bytes: Box<[u8]>,
    use_packed_pair: bool,
}

impl LiteralDiffSearchQuery {
    fn new(query: &str) -> Option<Self> {
        let case_insensitive = uses_ascii_case_insensitive_literal_search(query);
        let tokens = query
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|token| {
                let bytes = if case_insensitive {
                    token.as_bytes().to_ascii_lowercase().into_boxed_slice()
                } else {
                    token.as_bytes().to_vec().into_boxed_slice()
                };
                let use_packed_pair = case_insensitive
                    && case_insensitive_memmem::should_use_packed_pair_for_needle(&bytes);
                LiteralDiffSearchToken {
                    bytes,
                    use_packed_pair,
                }
            })
            .collect::<Vec<_>>();

        (!tokens.is_empty()).then_some(Self {
            tokens,
            case_insensitive,
        })
    }

    fn is_match(&self, text: &str) -> bool {
        self.tokens
            .iter()
            .all(|token| literal_token_matches(text.as_bytes(), token, self.case_insensitive))
    }

    fn match_ranges(&self, text: &str) -> Option<Vec<Range<usize>>> {
        let haystack = text.as_bytes();
        let mut ranges = Vec::new();
        for token in &self.tokens {
            if !literal_token_matches(haystack, token, self.case_insensitive) {
                return None;
            }

            let mut start = 0usize;
            let mut found = false;
            while start < haystack.len() {
                let Some(match_start) =
                    find_literal_token_at(haystack, &token.bytes, self.case_insensitive, start)
                else {
                    break;
                };

                found = true;
                ranges.push(match_start..match_start + token.bytes.len());
                start = match_start.saturating_add(1);
            }

            if !found {
                return None;
            }
        }

        Some(normalize_literal_match_ranges(text, ranges))
    }
}

fn literal_token_matches(
    haystack: &[u8],
    token: &LiteralDiffSearchToken,
    case_insensitive: bool,
) -> bool {
    if case_insensitive {
        if token.use_packed_pair && haystack.len() >= token.bytes.len().saturating_add(32) {
            case_insensitive_memmem::search_packed_pair(haystack, &token.bytes)
        } else {
            ascii_case_insensitive_find(haystack, &token.bytes).is_some()
        }
    } else {
        memchr::memmem::find(haystack, &token.bytes).is_some()
    }
}

fn uses_ascii_case_insensitive_literal_search(query: &str) -> bool {
    query.is_ascii() && !query.bytes().any(|byte| byte.is_ascii_uppercase())
}

fn find_literal_token_at(
    haystack: &[u8],
    needle: &[u8],
    case_insensitive: bool,
    start: usize,
) -> Option<usize> {
    if needle.is_empty() || start > haystack.len() {
        return None;
    }

    let haystack = &haystack[start..];
    let found = if case_insensitive {
        ascii_case_insensitive_find(haystack, needle)
    } else {
        memchr::memmem::find(haystack, needle)
    };
    found.map(|position| start + position)
}

fn ascii_case_insensitive_find(haystack: &[u8], needle_lower: &[u8]) -> Option<usize> {
    let needle_len = needle_lower.len();
    if needle_len == 0 {
        return Some(0);
    }

    if haystack.len() < needle_len {
        return None;
    }

    let first_lower = needle_lower[0];
    let first_upper = first_lower.to_ascii_uppercase();
    let search_end = haystack.len() - needle_len;

    if first_lower == first_upper {
        for position in memchr::memchr_iter(first_lower, &haystack[..=search_end]) {
            if ascii_case_eq(&haystack[position..position + needle_len], needle_lower) {
                return Some(position);
            }
        }
    } else {
        for position in memchr::memchr2_iter(first_lower, first_upper, &haystack[..=search_end]) {
            if ascii_case_eq(&haystack[position..position + needle_len], needle_lower) {
                return Some(position);
            }
        }
    }

    None
}

fn ascii_case_eq(left: &[u8], right_lower: &[u8]) -> bool {
    left.len() == right_lower.len()
        && left
            .iter()
            .zip(right_lower)
            .all(|(&left, &right)| left == right || left.eq_ignore_ascii_case(&right))
}

fn normalize_literal_match_ranges(text: &str, ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let ranges = if text.is_ascii() {
        ranges
    } else {
        expand_byte_ranges_to_grapheme_boundaries(text, ranges)
    };
    merge_byte_ranges(ranges)
}

fn expand_byte_ranges_to_grapheme_boundaries(
    text: &str,
    ranges: Vec<Range<usize>>,
) -> Vec<Range<usize>> {
    let clusters = text
        .grapheme_indices(true)
        .map(|(start, cluster)| start..start + cluster.len())
        .collect::<Vec<_>>();
    let mut expanded = Vec::with_capacity(ranges.len());

    for range in ranges {
        let start_index = clusters.partition_point(|cluster| cluster.end <= range.start);
        let end_index = clusters.partition_point(|cluster| cluster.start < range.end);
        if start_index >= end_index {
            continue;
        }
        expanded.push(clusters[start_index].start..clusters[end_index - 1].end);
    }

    expanded
}

fn merge_byte_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.retain(|range| range.start < range.end);
    ranges.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then_with(|| left.end.cmp(&right.end))
    });

    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
            continue;
        }
        merged.push(range);
    }
    merged
}

fn literal_match_score(text: &str, ranges: &[Range<usize>]) -> u32 {
    let matched_bytes = ranges
        .iter()
        .map(|range| range.end.saturating_sub(range.start))
        .sum::<usize>();
    let first_match = ranges.first().map_or(text.len(), |range| range.start);
    matched_bytes
        .saturating_mul(1_000)
        .saturating_sub(first_match)
        .saturating_sub(text.len() / 8)
        .min(u32::MAX as usize) as u32
}

fn fuzzy_search_config(query: &str) -> neo_frizbee::Config {
    let has_uppercase = query.bytes().any(|byte| byte.is_ascii_uppercase());
    neo_frizbee::Config {
        max_typos: Some((query.len() / 3).min(2) as u16),
        sort: false,
        scoring: neo_frizbee::Scoring {
            exact_match_bonus: 100,
            prefix_bonus: 0,
            capitalization_bonus: if has_uppercase { 4 } else { 0 },
            ..neo_frizbee::Scoring::default()
        },
        ..neo_frizbee::Config::default()
    }
}

fn fuzzy_min_score(query: &str) -> u16 {
    let perfect_score = query.len().saturating_mul(16).min(u16::MAX as usize) as u16;
    perfect_score / 2
}

fn fuzzy_match_ranges(
    query: &str,
    text: &str,
    config: &neo_frizbee::Config,
) -> Option<Vec<Range<usize>>> {
    let mut matched = neo_frizbee::match_list_indices(query, &[text], config)
        .into_iter()
        .next()?;
    matched.indices.sort_unstable();
    matched.indices.dedup();
    let indices = matched
        .indices
        .into_iter()
        .map(|index| index as u32)
        .collect::<Vec<_>>();
    Some(char_indices_to_byte_ranges(text, &indices))
}

fn char_indices_to_byte_ranges(text: &str, indices: &[u32]) -> Vec<Range<usize>> {
    if indices.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    if text.is_ascii() {
        push_ascii_ranges(indices, &mut ranges);
        return ranges;
    }

    let mut byte_offsets = text
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    byte_offsets.push(text.len());

    let mut start = indices[0] as usize;
    let mut end = start + 1;
    for index in indices.iter().copied().skip(1).map(|index| index as usize) {
        if index == end {
            end += 1;
            continue;
        }
        push_utf8_range(start, end, &byte_offsets, &mut ranges);
        start = index;
        end = index + 1;
    }
    push_utf8_range(start, end, &byte_offsets, &mut ranges);

    ranges
}

fn push_ascii_ranges(indices: &[u32], ranges: &mut Vec<Range<usize>>) {
    let mut start = indices[0] as usize;
    let mut end = start + 1;
    for index in indices.iter().copied().skip(1).map(|index| index as usize) {
        if index == end {
            end += 1;
            continue;
        }
        ranges.push(start..end);
        start = index;
        end = index + 1;
    }
    ranges.push(start..end);
}

fn push_utf8_range(
    start: usize,
    end: usize,
    byte_offsets: &[usize],
    ranges: &mut Vec<Range<usize>>,
) {
    let Some(byte_start) = byte_offsets.get(start).copied() else {
        return;
    };
    let Some(byte_end) = byte_offsets.get(end).copied() else {
        return;
    };
    ranges.push(byte_start..byte_end);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(diff: &str, query: &str) -> DiffSearchResults {
        let index = DiffSearchIndex::from_diff_text(diff).expect("diff should parse");
        let mut matcher = DiffSearchMatcher::default();
        index.search(query, DiffSearchOptions::default(), &mut matcher)
    }

    #[test]
    fn index_searches_added_lines_with_navigation_targets() {
        let results = search(
            concat!(
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -1,2 +1,2 @@\n",
                " pub fn stable() {}\n",
                "-fn legacy_parser() {}\n",
                "+fn dashboard_parser() {}\n",
            ),
            "'dashboard",
        );

        assert_eq!(results.total_matched, 1);
        assert_eq!(results.items.len(), 1);
        assert_eq!(results.items[0].file_path, "src/lib.rs");
        assert_eq!(results.items[0].kind, DiffSearchLineKind::Addition);
        assert_eq!(results.items[0].new_line, Some(2));
        assert_eq!(results.items[0].hunk_index, 0);
    }

    #[test]
    fn search_can_exclude_context_lines() {
        let diff = concat!(
            "diff --git a/src/lib.rs b/src/lib.rs\n",
            "--- a/src/lib.rs\n",
            "+++ b/src/lib.rs\n",
            "@@ -10,3 +10,3 @@\n",
            " shared_context_token\n",
            "-old_value\n",
            "+new_value\n",
        );
        let index = DiffSearchIndex::from_diff_text(diff).expect("diff should parse");
        let mut matcher = DiffSearchMatcher::default();

        let with_context = index.search(
            "'shared_context_token",
            DiffSearchOptions {
                include_context: true,
                ..DiffSearchOptions::default()
            },
            &mut matcher,
        );
        let without_context = index.search(
            "'shared_context_token",
            DiffSearchOptions {
                include_context: false,
                ..DiffSearchOptions::default()
            },
            &mut matcher,
        );

        assert_eq!(with_context.total_matched, 1);
        assert_eq!(without_context.total_matched, 0);
    }

    #[test]
    fn search_keeps_only_the_requested_top_results() {
        let mut diff = String::from(concat!(
            "diff --git a/src/lib.rs b/src/lib.rs\n",
            "--- a/src/lib.rs\n",
            "+++ b/src/lib.rs\n",
            "@@ -0,0 +1,4 @@\n",
        ));
        for index in 0..4 {
            diff.push_str(&format!("+fn target_result_{index}() {{}}\n"));
        }
        let index = DiffSearchIndex::from_diff_text(&diff).expect("diff should parse");
        let mut matcher = DiffSearchMatcher::default();
        let results = index.search(
            "target",
            DiffSearchOptions {
                limit: 2,
                ..DiffSearchOptions::default()
            },
            &mut matcher,
        );

        assert_eq!(results.total_matched, 4);
        assert_eq!(results.items.len(), 2);
    }

    #[test]
    fn literal_search_matches_space_separated_terms_across_separators() {
        let results = search(
            concat!(
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -0,0 +1,1 @@\n",
                "+let render_status_line = true;\n",
            ),
            "render status line",
        );

        assert_eq!(results.total_matched, 1);
        let line = &results.items[0].line;
        let ranges = results.items[0]
            .match_ranges
            .iter()
            .map(|range| &line[range.clone()])
            .collect::<Vec<_>>();
        assert_eq!(ranges, vec!["render", "status", "line"]);
    }

    #[test]
    fn fuzzy_search_can_be_selected_with_options() {
        let diff = concat!(
            "diff --git a/src/lib.rs b/src/lib.rs\n",
            "--- a/src/lib.rs\n",
            "+++ b/src/lib.rs\n",
            "@@ -0,0 +1,1 @@\n",
            "+fn dashboard_parser() {}\n",
        );
        let index = DiffSearchIndex::from_diff_text(diff).expect("diff should parse");
        let mut matcher = DiffSearchMatcher;
        let results = index.search(
            "dashbord parser",
            DiffSearchOptions {
                mode: DiffSearchMode::Fuzzy,
                ..DiffSearchOptions::default()
            },
            &mut matcher,
        );

        assert_eq!(results.total_matched, 1);
        assert_eq!(results.items[0].new_line, Some(1));
        assert!(!results.items[0].match_ranges.is_empty());
    }

    #[test]
    fn literal_search_does_not_fall_back_to_fuzzy() {
        let results = search(
            concat!(
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -0,0 +1,1 @@\n",
                "+fn dashboard_parser() {}\n",
            ),
            "dashbord parser",
        );

        assert_eq!(results.total_matched, 0);
        assert!(results.items.is_empty());
    }

    #[test]
    fn search_result_ranges_are_byte_ranges() {
        let results = search(
            concat!(
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -0,0 +1,1 @@\n",
                "+let café_token = true;\n",
            ),
            "café",
        );

        let result = &results.items[0];
        assert_eq!(
            &result.line[result.match_ranges[0].clone()],
            "café",
            "ranges should index the original UTF-8 line"
        );
    }

    #[test]
    fn search_result_ranges_preserve_grapheme_clusters() {
        let results = search(
            concat!(
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -0,0 +1,1 @@\n",
                "+cafe\u{301}_token = true;\n",
            ),
            "'e",
        );

        let result = &results.items[0];
        assert_eq!(
            &result.line[result.match_ranges[0].clone()],
            "e\u{301}",
            "ranges should not split a matched grapheme cluster"
        );
    }

    #[test]
    fn search_results_can_attach_syntax_highlighting() {
        let mut results = search(
            concat!(
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -0,0 +1,1 @@\n",
                "+fn highlighted_target() {}\n",
            ),
            "'highlighted",
        );
        let registry =
            crate::git::HighlightRegistry::new_for_filetypes(["rust"]).expect("registry");

        results.apply_syntax_highlighting(&registry);

        assert!(
            results.items[0]
                .syntax_ranges
                .iter()
                .any(|range| range.highlight_name.is_some()),
            "expected syntax ranges for rust result: {:?}",
            results.items[0].syntax_ranges
        );
    }

    #[test]
    fn search_results_include_bounded_hunk_preview() {
        let results = search(
            concat!(
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -1,8 +1,8 @@\n",
                " line one\n",
                " line two\n",
                " line three\n",
                "-legacy_target()\n",
                "+modern_target()\n",
                " line six\n",
                " line seven\n",
                " line eight\n",
            ),
            "'modern",
        );

        let result = &results.items[0];
        assert_eq!(result.preview_lines.len(), 8);
        assert!(
            result
                .preview_lines
                .iter()
                .any(|line| line.is_match && line.line == "modern_target()")
        );
        assert_eq!(result.preview_lines[0].line, "line one");
        assert_eq!(result.preview_lines.last().unwrap().line, "line eight");
    }

    #[test]
    fn search_results_group_by_file_preserving_file_discovery_order() {
        let mut results = search(
            concat!(
                "diff --git a/src/a.rs b/src/a.rs\n",
                "--- a/src/a.rs\n",
                "+++ b/src/a.rs\n",
                "@@ -0,0 +1,2 @@\n",
                "+target a one\n",
                "+target a two\n",
                "diff --git a/src/b.rs b/src/b.rs\n",
                "--- a/src/b.rs\n",
                "+++ b/src/b.rs\n",
                "@@ -0,0 +1,1 @@\n",
                "+target b one\n",
            ),
            "target",
        );
        results.items.swap(1, 2);

        results.group_items_by_file();

        assert_eq!(
            results
                .items
                .iter()
                .map(|item| item.file_path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/a.rs", "src/a.rs", "src/b.rs"]
        );
    }
}
