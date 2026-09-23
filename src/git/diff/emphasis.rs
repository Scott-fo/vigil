//! Intra-line emphasis: which parts of a modified line actually changed.
//!
//! When a change block replaces lines, the removed and added lines are paired
//! by index (the same pairing the split view uses) and diffed token by token.
//! The result is a set of byte ranges on each side that the renderer draws
//! with a stronger background, so a one-word edit reads as one word rather
//! than as two whole lines.
//!
//! Emphasis is a hint, not a guarantee. Pairs that are too long, too
//! different, or too expensive to compare get no emphasis and render as plain
//! whole-line changes.
//!
//! Cost model: nothing happens while rows are built. The first render of a
//! view allocates a small per-row table and pairs rows in one linear pass;
//! each pair is then diffed the first time either of its rows is drawn, and
//! the result is cached for the life of the view. A small edit costs one pass
//! over each line plus a table the size of the edited middle.

use std::{cell::RefCell, ops::Range, sync::OnceLock};

use super::{DiffHunkBlock, DiffLineKind, DiffRow, DiffView, rendering::RenderRow};

/// Lines longer than this (in bytes) are never compared.
const MAX_LINE_BYTES: usize = 400;
/// Minimum share of non-whitespace bytes that must be unchanged, measured
/// against the longer line. Below this the lines are treated as unrelated.
const MIN_SIMILARITY: f32 = 0.45;
/// Upper bound on LCS table cells for one pair, after trimming the shared
/// prefix and suffix.
const MAX_PAIR_CELLS: usize = 40_000;

/// Lazily built emphasis state for one view. Lives in the display cache but
/// survives its invalidation, because rows never change after a view is built.
#[derive(Debug, Clone, Default)]
pub(crate) struct EmphasisCache {
    rows: OnceLock<Box<[RowEmphasis]>>,
}

#[derive(Debug, Clone, Default)]
struct RowEmphasis {
    /// Row on the other side of the same change block, if any.
    pair: Option<u32>,
    ranges: OnceLock<Box<[Range<usize>]>>,
}

impl DiffView {
    /// A row ready to draw, with its changed byte ranges.
    pub(super) fn render_row(&self, row_index: usize) -> RenderRow<'_> {
        RenderRow {
            row: &self.rows[row_index],
            emphasis: self.row_emphasis(row_index),
        }
    }

    /// Changed byte ranges of `row_index` relative to its paired row. Empty
    /// for unpaired rows and for pairs too dissimilar to emphasize.
    pub(super) fn row_emphasis(&self, row_index: usize) -> &[Range<usize>] {
        let table = self
            .display_cache
            .emphasis
            .rows
            .get_or_init(|| pair_change_rows(&self.rows, &self.hunks));
        let Some(entry) = table.get(row_index) else {
            return &[];
        };
        let Some(pair_index) = entry.pair.map(|index| index as usize) else {
            return &[];
        };
        entry.ranges.get_or_init(|| {
            let row = &self.rows[row_index];
            let pair = &self.rows[pair_index];
            let row_is_removed = row.kind == DiffLineKind::Removed;
            let (removed, added) = if row_is_removed {
                (row, pair)
            } else {
                (pair, row)
            };
            let emphasis = line_emphasis(&removed.text, &added.text).unwrap_or_default();
            let (own, other) = if row_is_removed {
                (emphasis.removed, emphasis.added)
            } else {
                (emphasis.added, emphasis.removed)
            };
            // The pair's cell is distinct from ours, so this cannot re-enter.
            let _ = table[pair_index].ranges.set(other.into_boxed_slice());
            own.into_boxed_slice()
        })
    }
}

/// Pairs removed and added rows within each change block by index, the same
/// pairing the split view uses. Conflict rows are never paired, because
/// conflict sides are alternatives rather than edits of each other.
fn pair_change_rows(rows: &[DiffRow], hunks: &[DiffHunkBlock]) -> Box<[RowEmphasis]> {
    let mut table: Vec<RowEmphasis> = vec![RowEmphasis::default(); rows.len()];
    let is_change =
        |row: &DiffRow, kind: DiffLineKind| row.kind == kind && row.conflict_index.is_none();
    for hunk in hunks {
        let mut index = hunk.row_start;
        while index < hunk.row_end {
            if !is_change(&rows[index], DiffLineKind::Removed) {
                index += 1;
                continue;
            }
            let removed_start = index;
            while index < hunk.row_end && is_change(&rows[index], DiffLineKind::Removed) {
                index += 1;
            }
            let added_start = index;
            while index < hunk.row_end && is_change(&rows[index], DiffLineKind::Added) {
                index += 1;
            }
            let pair_count = (added_start - removed_start).min(index - added_start);
            for offset in 0..pair_count {
                let (removed, added) = (removed_start + offset, added_start + offset);
                table[removed].pair = Some(added as u32);
                table[added].pair = Some(removed as u32);
            }
        }
    }
    table.into_boxed_slice()
}

/// Byte ranges that changed within a paired removed/added line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct LineEmphasis {
    pub(super) removed: Vec<Range<usize>>,
    pub(super) added: Vec<Range<usize>>,
}

/// A non-whitespace token: its byte range and a hash of its bytes. Weight is
/// the byte length, so matching long identifiers counts for more than
/// matching punctuation.
#[derive(Debug, Clone, Copy)]
struct Token {
    start: usize,
    end: usize,
    id: u64,
}

impl Token {
    fn weight(self) -> u32 {
        (self.end - self.start) as u32
    }
}

thread_local! {
    static LCS_TABLE: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
}

/// Compares one removed line with its paired added line.
///
/// Returns `None` when the pair should render without emphasis: identical
/// lines, lines over [`MAX_LINE_BYTES`], lines below [`MIN_SIMILARITY`], or a
/// comparison larger than [`MAX_PAIR_CELLS`].
pub(super) fn line_emphasis(removed: &str, added: &str) -> Option<LineEmphasis> {
    if removed == added || removed.len() > MAX_LINE_BYTES || added.len() > MAX_LINE_BYTES {
        return None;
    }

    let old_tokens = tokenize(removed);
    let new_tokens = tokenize(added);
    let longer = total_weight(&old_tokens).max(total_weight(&new_tokens));
    if longer == 0 {
        return None;
    }
    let required = MIN_SIMILARITY * longer as f32;

    // Common prefix and suffix are unchanged by definition and are usually
    // most of the line, so only the middle goes through the LCS table.
    let prefix = old_tokens
        .iter()
        .zip(&new_tokens)
        .take_while(|(old, new)| old.id == new.id)
        .count();
    let suffix = old_tokens[prefix..]
        .iter()
        .rev()
        .zip(new_tokens[prefix..].iter().rev())
        .take_while(|(old, new)| old.id == new.id)
        .count();
    let old_middle = &old_tokens[prefix..old_tokens.len() - suffix];
    let new_middle = &new_tokens[prefix..new_tokens.len() - suffix];
    let anchored = total_weight(&old_tokens[..prefix])
        + total_weight(&old_tokens[old_tokens.len() - suffix..]);

    // Shared-token overlap bounds any alignment from above, so pairs that
    // cannot reach the threshold skip the table entirely.
    if ((anchored + overlap_weight(old_middle, new_middle)) as f32) < required {
        return None;
    }

    if (old_middle.len() + 1) * (new_middle.len() + 1) > MAX_PAIR_CELLS {
        return None;
    }

    let (matched_weight, old_matched, new_matched) = lcs_matches(old_middle, new_middle);
    if ((anchored + matched_weight) as f32) < required {
        return None;
    }

    Some(LineEmphasis {
        removed: changed_ranges(removed, old_middle, &old_matched),
        added: changed_ranges(added, new_middle, &new_matched),
    })
}

/// Splits a line into identifier/word runs and single punctuation characters.
/// Whitespace separates tokens but is never a token itself, so indentation
/// and spacing changes do not pull unrelated lines into alignment.
fn tokenize(line: &str) -> Vec<Token> {
    let mut tokens = Vec::with_capacity(line.len() / 3);
    let bytes = line.as_bytes();
    let mut word_start: Option<usize> = None;
    for (index, ch) in line.char_indices() {
        let is_word = ch.is_alphanumeric() || ch == '_';
        if is_word {
            word_start.get_or_insert(index);
            continue;
        }
        if let Some(start) = word_start.take() {
            tokens.push(token(bytes, start, index));
        }
        if !ch.is_whitespace() {
            tokens.push(token(bytes, index, index + ch.len_utf8()));
        }
    }
    if let Some(start) = word_start {
        tokens.push(token(bytes, start, line.len()));
    }
    tokens
}

fn token(bytes: &[u8], start: usize, end: usize) -> Token {
    // FNV-1a: tiny, fast on short tokens, and collisions only cost a
    // cosmetic mis-highlight.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in &bytes[start..end] {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    Token {
        start,
        end,
        id: hash,
    }
}

fn total_weight(tokens: &[Token]) -> u32 {
    tokens.iter().map(|token| token.weight()).sum()
}

/// Weight of the multiset intersection of two token lists: an upper bound on
/// any order-preserving alignment between them.
fn overlap_weight(old: &[Token], new: &[Token]) -> u32 {
    let mut old_ids: Vec<(u64, u32)> = old.iter().map(|t| (t.id, t.weight())).collect();
    let mut new_ids: Vec<(u64, u32)> = new.iter().map(|t| (t.id, t.weight())).collect();
    old_ids.sort_unstable();
    new_ids.sort_unstable();

    let (mut i, mut j, mut weight) = (0, 0, 0);
    while i < old_ids.len() && j < new_ids.len() {
        match old_ids[i].0.cmp(&new_ids[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                weight += old_ids[i].1;
                i += 1;
                j += 1;
            }
        }
    }
    weight
}

/// Finds a common subsequence of maximum byte weight. Weighting by length
/// keeps shared identifiers ahead of shared punctuation, so `a, b` →
/// `x, y, a, b` matches `a, b` rather than two commas. Returns the matched
/// weight and which tokens on each side belong to the alignment.
fn lcs_matches(old: &[Token], new: &[Token]) -> (u32, Vec<bool>, Vec<bool>) {
    let width = new.len() + 1;
    LCS_TABLE.with_borrow_mut(|table| {
        table.clear();
        table.resize((old.len() + 1) * width, 0);
        for i in (0..old.len()).rev() {
            let (row, next_row) = table[i * width..(i + 2) * width].split_at_mut(width);
            let old_token = old[i];
            for j in (0..new.len()).rev() {
                let skip = next_row[j].max(row[j + 1]);
                row[j] = if old_token.id == new[j].id {
                    (next_row[j + 1] + old_token.weight()).max(skip)
                } else {
                    skip
                };
            }
        }

        // Equal tokens always belong to some optimal alignment of the
        // remaining suffixes, since their weight is the same on both sides.
        let mut old_matched = vec![false; old.len()];
        let mut new_matched = vec![false; new.len()];
        let (mut i, mut j) = (0, 0);
        while i < old.len() && j < new.len() {
            if old[i].id == new[j].id {
                old_matched[i] = true;
                new_matched[j] = true;
                i += 1;
                j += 1;
            } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
        (table[0], old_matched, new_matched)
    })
}

/// Merges unmatched tokens into byte ranges. Ranges separated only by
/// whitespace are joined so `foo bar` → `baz qux` emphasizes one span.
fn changed_ranges(line: &str, tokens: &[Token], matched: &[bool]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (token, _) in tokens
        .iter()
        .zip(matched)
        .filter(|(_, is_matched)| !**is_matched)
    {
        match ranges.last_mut() {
            Some(last)
                if line.as_bytes()[last.end..token.start]
                    .trim_ascii()
                    .is_empty() =>
            {
                last.end = token.end;
            }
            _ => ranges.push(token.start..token.end),
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emphasis(removed: &str, added: &str) -> Option<LineEmphasis> {
        line_emphasis(removed, added)
    }

    fn texts<'a>(line: &'a str, ranges: &[Range<usize>]) -> Vec<&'a str> {
        ranges.iter().map(|range| &line[range.clone()]).collect()
    }

    #[test]
    fn tokenizer_splits_words_and_punctuation_and_skips_whitespace() {
        let line = "let foo_bar = baz(1);";
        let tokens: Vec<&str> = tokenize(line)
            .into_iter()
            .map(|token| &line[token.start..token.end])
            .collect();

        assert_eq!(tokens, ["let", "foo_bar", "=", "baz", "(", "1", ")", ";"]);
    }

    #[test]
    fn identical_lines_have_no_emphasis() {
        assert_eq!(emphasis("let x = 1;", "let x = 1;"), None);
    }

    #[test]
    fn single_word_change_emphasizes_only_that_word() {
        let removed = "    view.display_line_count(DiffViewMode::Split, WIDTH)";
        let added = "    view.display_line_count(DiffViewMode::Unified, WIDTH)";
        let result = emphasis(removed, added).expect("similar lines should be emphasized");

        assert_eq!(texts(removed, &result.removed), ["Split"]);
        assert_eq!(texts(added, &result.added), ["Unified"]);
    }

    #[test]
    fn insertion_emphasizes_only_the_added_side() {
        let removed = "use std::path::PathBuf;";
        let added = "use std::{path::PathBuf, sync::Arc};";
        let result = emphasis(removed, added).expect("similar lines should be emphasized");

        assert!(result.removed.is_empty());
        assert_eq!(texts(added, &result.added), ["{", ", sync::Arc}"]);
    }

    #[test]
    fn reordered_arguments_emphasize_the_moved_token() {
        let removed = "call(alpha, beta, gamma)";
        let added = "call(beta, alpha, gamma)";
        let result = emphasis(removed, added).expect("similar lines should be emphasized");

        assert!(!result.removed.is_empty());
        assert!(!result.added.is_empty());
        assert!(!texts(added, &result.added).concat().contains("gamma"));
    }

    #[test]
    fn unicode_ranges_fall_on_char_boundaries() {
        let removed = "let greeting = \"héllo wörld\";";
        let added = "let greeting = \"héllo wærld\";";
        let result = emphasis(removed, added).expect("similar lines should be emphasized");

        assert_eq!(texts(removed, &result.removed), ["wörld"]);
        assert_eq!(texts(added, &result.added), ["wærld"]);
    }

    #[test]
    fn shared_identifiers_outweigh_shared_punctuation() {
        let removed = "        SharedHighlightRegistry, WorkingTreeStatus, WorktreeEntry,";
        let added = "        ReviewDiffStreamedFile, ReviewDiffTextIndex, SharedHighlightRegistry, WorkingTreeStatus,";
        let result = emphasis(removed, added).expect("similar lines should be emphasized");

        assert_eq!(texts(removed, &result.removed), [", WorktreeEntry"]);
        assert_eq!(
            texts(added, &result.added),
            ["ReviewDiffStreamedFile, ReviewDiffTextIndex,"]
        );
    }

    #[test]
    fn dissimilar_lines_are_not_emphasized() {
        assert_eq!(
            emphasis("let total = items.len();", "return Err(error.into());"),
            None
        );
    }

    #[test]
    fn overlong_lines_and_oversized_tables_skip_emphasis() {
        let long = "x".repeat(MAX_LINE_BYTES + 1);
        assert_eq!(emphasis(&long, "x"), None);

        // Similar enough to pass the overlap bound, but with no shared prefix
        // or suffix the middle table would be 201 × 201 cells.
        let removed = format!("a{} b", " x".repeat(198));
        let added = format!("c{} d", " x".repeat(198));
        assert!(removed.len() <= MAX_LINE_BYTES);
        assert_eq!(emphasis(&removed, &added), None);
    }

    #[test]
    fn changes_separated_by_whitespace_merge_into_one_range() {
        let removed = "pub fn render(frame: &mut Frame, old name: Area) {}";
        let added = "pub fn render(frame: &mut Frame, new title: Area) {}";
        let result = emphasis(removed, added).expect("similar lines should be emphasized");

        assert_eq!(texts(added, &result.added), ["new title"]);
    }
}
