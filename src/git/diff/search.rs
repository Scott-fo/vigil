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
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
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
}

impl Default for DiffSearchOptions {
    fn default() -> Self {
        Self {
            limit: 80,
            include_context: true,
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

#[derive(Debug, Clone)]
pub struct DiffSearchMatcher {
    matcher: Matcher,
    char_buf: Vec<char>,
    indices: Vec<u32>,
}

impl Default for DiffSearchMatcher {
    fn default() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            char_buf: Vec::new(),
            indices: Vec::new(),
        }
    }
}

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
        matcher: &mut DiffSearchMatcher,
    ) -> DiffSearchResults {
        let query = query.trim();
        if query.is_empty() || options.limit == 0 || self.lines.is_empty() {
            return DiffSearchResults::default();
        }

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        if pattern.atoms.is_empty() {
            return DiffSearchResults::default();
        }

        let mut total_matched = 0usize;
        let mut top = BinaryHeap::with_capacity(options.limit.saturating_add(1));
        for (line_index, line) in self.lines.iter().enumerate() {
            if !options.include_context && line.kind == DiffSearchLineKind::Context {
                continue;
            }

            let haystack = Utf32Str::new(&line.text, &mut matcher.char_buf);
            let Some(score) = pattern.score(haystack, &mut matcher.matcher) else {
                continue;
            };

            total_matched = total_matched.saturating_add(1);
            let ranked = RankedLine { score, line_index };
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
            .map(|ranked| self.search_result(ranked, &pattern, matcher))
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

    fn search_result(
        &self,
        ranked: RankedLine,
        pattern: &Pattern,
        matcher: &mut DiffSearchMatcher,
    ) -> DiffSearchResult {
        let line = &self.lines[ranked.line_index];
        matcher.indices.clear();
        let haystack = Utf32Str::new(&line.text, &mut matcher.char_buf);
        let _ = pattern.indices(haystack, &mut matcher.matcher, &mut matcher.indices);
        matcher.indices.sort_unstable();
        matcher.indices.dedup();

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
            match_ranges: char_indices_to_byte_ranges(&line.text, &matcher.indices),
            syntax_ranges: Vec::new(),
            preview_lines: self.preview_lines_for(ranked.line_index),
            score: ranked.score,
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
