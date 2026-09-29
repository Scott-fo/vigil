//! Draft review comments: what a reviewer writes before submitting a review.
//!
//! A [`DraftComment`] is local until it is sent with a review. It names the
//! pull request it belongs to ([`DraftScope`]: repository root and number),
//! the head commit it was written against, the file, and a [`DraftAnchor`]:
//! one line or a range, each end numbered on a [`DiffSide`] the way GitHub's
//! review API expects.
//!
//! # Where GitHub accepts comments
//!
//! GitHub rejects inline comments on lines outside the hunks of the pull
//! request's diff, which uses three lines of context. vigil can show more
//! (expanded gaps, a larger `diff.context`), so anchors are built from the
//! patch's own lines ([`PatchLine`]) and refused unless every line lies
//! within [`GITHUB_DIFF_CONTEXT`] lines of a change in one hunk; see
//! [`DraftAnchor::from_patch_lines`]. Sides follow GitHub: removed lines are
//! on the left (old line numbers), added and unchanged lines on the right.
//!
//! # When the head moves
//!
//! Drafts are pinned to the head they were written against, and a review is
//! submitted against that head. When the review moves to a newer head,
//! [`DraftAnchor::reanchor`] looks for each line's recorded text on a
//! commentable line of the new patch, nearest the old line number first. A
//! draft whose lines are found moves there and is re-pinned; one whose lines
//! are not stays on its old head and needs the reviewer's attention.
//!
//! # Storage
//!
//! Drafts persist in the review database through [`ReviewStore`], so they
//! survive a crash or restart. Every call is blocking SQLite I/O.

use std::{
    fmt,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use rusqlite::params;

use super::store::{ReviewStore, now_ms_i64};
use crate::{
    forge::{DiffPosition, DiffSide, DraftReviewComment},
    git::{PatchLine, PatchLineKind},
};

/// Context lines GitHub keeps around each change in a pull request diff.
pub const GITHUB_DIFF_CONTEXT: usize = 3;

/// Identity of a draft, unique across sessions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DraftId(String);

impl DraftId {
    /// A fresh id. Made locally, so a draft has its identity before it is
    /// saved.
    pub fn generate() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(format!(
            "{:x}-{:x}-{count:x}",
            now_ms_i64(),
            std::process::id()
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The pull request a set of drafts belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DraftScope {
    repo_root: String,
    number: u64,
}

impl DraftScope {
    pub fn new(repo_root: &Path, number: u64) -> Self {
        Self {
            repo_root: repo_root.display().to_string(),
            number,
        }
    }

    pub fn number(&self) -> u64 {
        self.number
    }
}

/// One end of a draft's range: where it is and the text it had there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftLine {
    pub position: DiffPosition,
    /// The line's text when the draft was written, used to find it again
    /// after new commits.
    pub text: String,
}

/// The lines a draft comments on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftAnchor {
    /// The first line of a multi-line range.
    pub start: Option<DraftLine>,
    /// The commented line, or the last line of a range.
    pub end: DraftLine,
}

/// Why a selection cannot carry a review comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorRefusal {
    /// Nothing selected.
    Empty,
    /// The selection includes a row the patch does not contain: expanded
    /// context, a gap band, or a header.
    NotInPatch,
    /// A line lies further from a change than GitHub's diff shows.
    OutsideGitHubHunks,
    /// A range crosses from one hunk into another.
    SpansHunks,
}

impl fmt::Display for AnchorRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "select a diff line to comment on",
            Self::NotInPatch => {
                "GitHub only takes comments on lines in the diff's hunks, not expanded context"
            }
            Self::OutsideGitHubHunks => {
                "GitHub only takes comments within 3 lines of a change; this line is outside its hunks"
            }
            Self::SpansHunks => "a multi-line comment must stay within one hunk",
        })
    }
}

impl DraftAnchor {
    /// The anchor for a selection, from the patch line each selected row
    /// shows in display order (`None` for rows outside the patch). Rows of
    /// one soft-wrapped line may repeat.
    ///
    /// Removed lines take the left side and added lines the right. Unchanged
    /// lines take the right side, except at one end of a range whose other
    /// end is a removed line, where they take the left so both ends number
    /// the same file.
    pub fn from_patch_lines(rows: &[Option<PatchLine>]) -> Result<Self, AnchorRefusal> {
        let mut lines: Vec<&PatchLine> = Vec::new();
        for row in rows {
            let line = row.as_ref().ok_or(AnchorRefusal::NotInPatch)?;
            if lines.last() != Some(&line) {
                lines.push(line);
            }
        }
        let (first, last) = match (lines.first(), lines.last()) {
            (Some(first), Some(last)) => (*first, *last),
            _ => return Err(AnchorRefusal::Empty),
        };
        if !is_commentable(first) || !is_commentable(last) {
            return Err(AnchorRefusal::OutsideGitHubHunks);
        }
        if lines
            .iter()
            .any(|line| line.hunk != first.hunk || !is_commentable(line))
        {
            return Err(AnchorRefusal::SpansHunks);
        }
        if lines.len() == 1 {
            return Ok(Self {
                start: None,
                end: draft_line(first, preferred_side(first, None))?,
            });
        }
        let start_side = preferred_side(first, Some(last));
        let end_side = preferred_side(last, Some(first));
        Ok(Self {
            start: Some(draft_line(first, start_side)?),
            end: draft_line(last, end_side)?,
        })
    }

    /// The first line number of a range, for "lines 10–14" labels.
    pub fn start_line(&self) -> Option<u32> {
        self.start.as_ref().map(|start| start.position.line)
    }

    /// Finds this anchor in the patch of a newer head: each end's text on a
    /// commentable line of the same side, nearest the old position first,
    /// with a range keeping its length and staying in one hunk. `None` when
    /// the lines are gone or changed.
    pub fn reanchor(&self, lines: &[PatchLine]) -> Option<Self> {
        let mut candidates = lines
            .iter()
            .filter(|line| is_commentable(line) && line_matches(line, &self.end))
            .collect::<Vec<_>>();
        let old_end = i64::from(self.end.position.line);
        candidates.sort_by_key(|line| (position_on(line, self.end.position.side) - old_end).abs());
        candidates.into_iter().find_map(|end| {
            let new_end = position_on(end, self.end.position.side);
            let Some(start) = &self.start else {
                return Some(Self {
                    start: None,
                    end: moved(&self.end, new_end),
                });
            };
            let new_start = i64::from(start.position.line) + (new_end - old_end);
            lines
                .iter()
                .find(|line| {
                    line.hunk == end.hunk
                        && is_commentable(line)
                        && line_matches(line, start)
                        && position_on(line, start.position.side) == new_start
                })
                .map(|_| Self {
                    start: Some(moved(start, new_start)),
                    end: moved(&self.end, new_end),
                })
        })
    }
}

/// Whether GitHub's diff includes `line`: every change, and unchanged lines
/// within [`GITHUB_DIFF_CONTEXT`] of one.
pub fn is_commentable(line: &PatchLine) -> bool {
    line.kind != PatchLineKind::Context || line.distance_to_change <= GITHUB_DIFF_CONTEXT
}

/// The new-side text of the selected rows, which a GitHub suggestion
/// replaces. `None` when the selection has no new-side lines.
pub fn suggestion_source(rows: &[Option<PatchLine>]) -> Option<Vec<String>> {
    let mut lines: Vec<&PatchLine> = Vec::new();
    for line in rows.iter().flatten() {
        if line.kind != PatchLineKind::Removed && lines.last() != Some(&line) {
            lines.push(line);
        }
    }
    (!lines.is_empty()).then(|| lines.iter().map(|line| line.text.clone()).collect())
}

/// A GitHub suggestion block proposing `lines` as the replacement text.
pub fn suggestion_block(lines: &[String]) -> String {
    let mut block = String::from("```suggestion\n");
    for line in lines {
        block.push_str(line);
        block.push('\n');
    }
    block.push_str("```\n");
    block
}

fn preferred_side(line: &PatchLine, other_end: Option<&PatchLine>) -> DiffSide {
    match line.kind {
        PatchLineKind::Removed => DiffSide::Left,
        PatchLineKind::Added => DiffSide::Right,
        PatchLineKind::Context
            if other_end.is_some_and(|other| other.kind == PatchLineKind::Removed) =>
        {
            DiffSide::Left
        }
        PatchLineKind::Context => DiffSide::Right,
    }
}

fn draft_line(line: &PatchLine, side: DiffSide) -> Result<DraftLine, AnchorRefusal> {
    let number = match side {
        DiffSide::Left => line.old_line,
        DiffSide::Right => line.new_line,
    }
    .and_then(|number| u32::try_from(number).ok())
    .ok_or(AnchorRefusal::NotInPatch)?;
    Ok(DraftLine {
        position: DiffPosition { side, line: number },
        text: line.text.clone(),
    })
}

/// `line`'s number on `side`, or -1 when it has none there.
fn position_on(line: &PatchLine, side: DiffSide) -> i64 {
    match side {
        DiffSide::Left => line.old_line,
        DiffSide::Right => line.new_line,
    }
    .map_or(-1, |number| number as i64)
}

fn line_matches(line: &PatchLine, target: &DraftLine) -> bool {
    position_on(line, target.position.side) >= 0 && line.text == target.text
}

fn moved(line: &DraftLine, number: i64) -> DraftLine {
    DraftLine {
        position: DiffPosition {
            side: line.position.side,
            line: number as u32,
        },
        text: line.text.clone(),
    }
}

/// A pending inline comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftComment {
    pub id: DraftId,
    pub path: String,
    pub anchor: DraftAnchor,
    /// Markdown.
    pub body: String,
    /// The head commit the anchor's line numbers refer to.
    pub head_oid: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl DraftComment {
    pub fn new(path: String, anchor: DraftAnchor, body: String, head_oid: String) -> Self {
        let now = now_ms_i64();
        Self {
            id: DraftId::generate(),
            path,
            anchor,
            body,
            head_oid,
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    /// Replaces the body, marking the draft as updated now.
    pub fn set_body(&mut self, body: String) {
        self.body = body;
        self.updated_at_ms = now_ms_i64().max(self.updated_at_ms.saturating_add(1));
    }

    /// The comment as GitHub's review API takes it.
    pub fn to_review_comment(&self) -> DraftReviewComment {
        DraftReviewComment {
            path: self.path.clone(),
            end: self.anchor.end.position,
            start: self.anchor.start.as_ref().map(|start| start.position),
            body: self.body.clone(),
        }
    }
}

impl ReviewStore {
    /// Every draft for `scope`, whatever head it was written against, oldest
    /// first.
    pub fn load_drafts(&self, scope: &DraftScope) -> color_eyre::Result<Vec<DraftComment>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "select id, head_oid, path, start_side, start_line, start_text,
                    end_side, end_line, end_text, body, created_at_ms, updated_at_ms
             from review_drafts
             where repo_root = ?1 and pull_request = ?2
             order by created_at_ms, id",
        )?;
        let rows = statement.query_map(params![scope.repo_root, scope.number as i64], |row| {
            Ok(StoredDraft {
                id: row.get(0)?,
                head_oid: row.get(1)?,
                path: row.get(2)?,
                start_side: row.get(3)?,
                start_line: row.get(4)?,
                start_text: row.get(5)?,
                end_side: row.get(6)?,
                end_line: row.get(7)?,
                end_text: row.get(8)?,
                body: row.get(9)?,
                created_at_ms: row.get(10)?,
                updated_at_ms: row.get(11)?,
            })
        })?;
        let mut drafts = Vec::new();
        for row in rows {
            // Rows with an unreadable anchor are skipped rather than failing
            // every other draft.
            if let Some(draft) = row?.into_draft() {
                drafts.push(draft);
            }
        }
        Ok(drafts)
    }

    /// Inserts `draft`, or replaces the stored draft with its id.
    pub fn save_draft(&self, scope: &DraftScope, draft: &DraftComment) -> color_eyre::Result<()> {
        let connection = self.connection()?;
        let start = draft.anchor.start.as_ref();
        connection.execute(
            "insert or replace into review_drafts (
                id, repo_root, pull_request, head_oid, path,
                start_side, start_line, start_text, end_side, end_line, end_text,
                body, created_at_ms, updated_at_ms
             ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                draft.id.as_str(),
                scope.repo_root,
                scope.number as i64,
                draft.head_oid,
                draft.path,
                start.map(|start| side_key(start.position.side)),
                start.map(|start| start.position.line),
                start.map(|start| start.text.as_str()),
                side_key(draft.anchor.end.position.side),
                draft.anchor.end.position.line,
                draft.anchor.end.text,
                draft.body,
                draft.created_at_ms,
                draft.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    /// Deletes the drafts with `ids`; missing ids are ignored.
    pub fn delete_drafts(&self, ids: &[DraftId]) -> color_eyre::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        for id in ids {
            transaction.execute(
                "delete from review_drafts where id = ?1",
                params![id.as_str()],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// The draft with `id`, if stored.
    #[cfg(test)]
    fn draft_head(&self, id: &DraftId) -> color_eyre::Result<Option<String>> {
        use rusqlite::OptionalExtension;

        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "select head_oid from review_drafts where id = ?1",
                params![id.as_str()],
                |row| row.get(0),
            )
            .optional()?)
    }
}

/// A row of `review_drafts` before its sides are validated.
struct StoredDraft {
    id: String,
    head_oid: String,
    path: String,
    start_side: Option<String>,
    start_line: Option<u32>,
    start_text: Option<String>,
    end_side: String,
    end_line: u32,
    end_text: String,
    body: String,
    created_at_ms: i64,
    updated_at_ms: i64,
}

impl StoredDraft {
    fn into_draft(self) -> Option<DraftComment> {
        let start = match (self.start_side, self.start_line) {
            (Some(side), Some(line)) => Some(DraftLine {
                position: DiffPosition {
                    side: side_from_key(&side)?,
                    line,
                },
                text: self.start_text.unwrap_or_default(),
            }),
            _ => None,
        };
        Some(DraftComment {
            id: DraftId(self.id),
            path: self.path,
            anchor: DraftAnchor {
                start,
                end: DraftLine {
                    position: DiffPosition {
                        side: side_from_key(&self.end_side)?,
                        line: self.end_line,
                    },
                    text: self.end_text,
                },
            },
            body: self.body,
            head_oid: self.head_oid,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        })
    }
}

/// Persisted side names. Changing them orphans stored drafts.
fn side_key(side: DiffSide) -> &'static str {
    match side {
        DiffSide::Left => "left",
        DiffSide::Right => "right",
    }
}

fn side_from_key(key: &str) -> Option<DiffSide> {
    match key {
        "left" => Some(DiffSide::Left),
        "right" => Some(DiffSide::Right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    fn line(kind: PatchLineKind, old: Option<usize>, new: Option<usize>, text: &str) -> PatchLine {
        PatchLine {
            kind,
            old_line: old,
            new_line: new,
            text: text.to_string(),
            hunk: 0,
            distance_to_change: match kind {
                PatchLineKind::Context => 1,
                _ => 0,
            },
        }
    }

    fn context(old: usize, new: usize, text: &str, distance: usize) -> PatchLine {
        PatchLine {
            distance_to_change: distance,
            ..line(PatchLineKind::Context, Some(old), Some(new), text)
        }
    }

    fn position(side: DiffSide, line: u32) -> DiffPosition {
        DiffPosition { side, line }
    }

    #[test]
    fn single_lines_take_the_side_their_change_lives_on() {
        let added = line(PatchLineKind::Added, None, Some(8), "new");
        let removed = line(PatchLineKind::Removed, Some(7), None, "old");
        let unchanged = context(3, 4, "same", 2);

        let anchor = |line: &PatchLine| {
            DraftAnchor::from_patch_lines(&[Some(line.clone())])
                .unwrap()
                .end
                .position
        };
        assert_eq!(anchor(&added), position(DiffSide::Right, 8));
        assert_eq!(anchor(&removed), position(DiffSide::Left, 7));
        assert_eq!(anchor(&unchanged), position(DiffSide::Right, 4));
    }

    #[test]
    fn ranges_keep_their_ends_and_ignore_repeated_wrapped_rows() {
        let removed = line(PatchLineKind::Removed, Some(7), None, "old");
        let added = line(PatchLineKind::Added, None, Some(7), "new");
        let anchor = DraftAnchor::from_patch_lines(&[
            Some(removed.clone()),
            Some(removed),
            Some(added.clone()),
            Some(added),
        ])
        .unwrap();

        assert_eq!(
            anchor.start.map(|start| start.position),
            Some(position(DiffSide::Left, 7))
        );
        assert_eq!(anchor.end.position, position(DiffSide::Right, 7));
        assert_eq!(anchor.end.text, "new");
    }

    #[test]
    fn unchanged_lines_join_a_removed_line_on_the_left() {
        let before = context(5, 5, "before", 1);
        let removed = line(PatchLineKind::Removed, Some(6), None, "gone");
        let anchor = DraftAnchor::from_patch_lines(&[Some(before), Some(removed)]).unwrap();

        assert_eq!(
            anchor.start.map(|start| start.position),
            Some(position(DiffSide::Left, 5))
        );
        assert_eq!(anchor.end.position, position(DiffSide::Left, 6));
    }

    #[test]
    fn anchoring_follows_githubs_hunks_not_vigils_display() {
        let near = context(4, 4, "near", 3);
        let far = context(1, 1, "far", 4);
        let other_hunk = PatchLine {
            hunk: 1,
            ..line(PatchLineKind::Added, None, Some(40), "later")
        };

        assert!(DraftAnchor::from_patch_lines(&[Some(near.clone())]).is_ok());
        assert_eq!(
            DraftAnchor::from_patch_lines(&[Some(far.clone())]),
            Err(AnchorRefusal::OutsideGitHubHunks)
        );
        assert_eq!(
            DraftAnchor::from_patch_lines(&[None]),
            Err(AnchorRefusal::NotInPatch),
            "expanded context and gap rows"
        );
        assert_eq!(
            DraftAnchor::from_patch_lines(&[Some(near.clone()), None, Some(near.clone())]),
            Err(AnchorRefusal::NotInPatch)
        );
        assert_eq!(
            DraftAnchor::from_patch_lines(&[Some(near.clone()), Some(other_hunk)]),
            Err(AnchorRefusal::SpansHunks)
        );
        let added = line(PatchLineKind::Added, None, Some(9), "x");
        assert_eq!(
            DraftAnchor::from_patch_lines(&[Some(near), Some(far), Some(added)]),
            Err(AnchorRefusal::SpansHunks),
            "a range through lines GitHub splits into two hunks"
        );
        assert_eq!(
            DraftAnchor::from_patch_lines(&[]),
            Err(AnchorRefusal::Empty)
        );
    }

    #[test]
    fn suggestions_take_only_new_side_lines() {
        let rows = [
            Some(line(PatchLineKind::Removed, Some(3), None, "let a = 1;")),
            Some(line(PatchLineKind::Added, None, Some(3), "let a = 2;")),
            Some(line(PatchLineKind::Added, None, Some(3), "let a = 2;")),
            Some(context(4, 4, "done();", 1)),
        ];

        let source = suggestion_source(&rows).unwrap();
        assert_eq!(source, vec!["let a = 2;", "done();"]);
        assert_eq!(
            suggestion_block(&source),
            "```suggestion\nlet a = 2;\ndone();\n```\n"
        );
        assert_eq!(suggestion_source(&rows[..1]), None);
    }

    #[test]
    fn reanchoring_follows_the_text_to_its_new_line() {
        let anchor = DraftAnchor::from_patch_lines(&[
            Some(line(PatchLineKind::Added, None, Some(10), "a")),
            Some(line(PatchLineKind::Added, None, Some(11), "b")),
        ])
        .unwrap();
        let shifted = [
            line(PatchLineKind::Added, None, Some(1), "inserted above"),
            line(PatchLineKind::Added, None, Some(12), "a"),
            line(PatchLineKind::Added, None, Some(13), "b"),
        ];

        let moved = anchor.reanchor(&shifted).expect("lines still exist");
        assert_eq!(moved.start_line(), Some(12));
        assert_eq!(moved.end.position, position(DiffSide::Right, 13));

        let rewritten = [
            line(PatchLineKind::Added, None, Some(10), "a"),
            line(PatchLineKind::Added, None, Some(11), "b changed"),
        ];
        assert_eq!(anchor.reanchor(&rewritten), None);
        assert_eq!(anchor.reanchor(&[]), None);
    }

    #[test]
    fn reanchoring_prefers_the_nearest_copy_of_the_line() {
        let anchor =
            DraftAnchor::from_patch_lines(&[Some(line(PatchLineKind::Added, None, Some(20), "}"))])
                .unwrap();
        let lines = [
            line(PatchLineKind::Added, None, Some(3), "}"),
            line(PatchLineKind::Added, None, Some(22), "}"),
        ];

        assert_eq!(
            anchor.reanchor(&lines).unwrap().end.position,
            position(DiffSide::Right, 22)
        );
    }

    fn temp_store(name: &str) -> (ReviewStore, PathBuf) {
        let path = std::env::temp_dir().join("vigil-draft-tests").join(format!(
            "{name}-{}-{}.sqlite3",
            now_ms_i64(),
            std::process::id()
        ));
        (ReviewStore::open(path.clone()).expect("store opens"), path)
    }

    fn draft(path: &str, line: u32, body: &str) -> DraftComment {
        DraftComment::new(
            path.to_string(),
            DraftAnchor {
                start: Some(DraftLine {
                    position: position(DiffSide::Left, line - 1),
                    text: "before".to_string(),
                }),
                end: DraftLine {
                    position: position(DiffSide::Right, line),
                    text: "after".to_string(),
                },
            },
            body.to_string(),
            "a".repeat(40),
        )
    }

    #[test]
    fn drafts_round_trip_per_pull_request() {
        let (store, path) = temp_store("round-trip");
        let scope = DraftScope::new(Path::new("/repo"), 18);
        let other_pull_request = DraftScope::new(Path::new("/repo"), 19);
        let other_checkout = DraftScope::new(Path::new("/elsewhere"), 18);
        let first = draft("src/lib.rs", 4, "first");
        let mut second = draft("src/main.rs", 9, "second");
        second.anchor.start = None;
        second.created_at_ms = first.created_at_ms + 1;

        store.save_draft(&scope, &first).unwrap();
        store.save_draft(&scope, &second).unwrap();
        assert_eq!(
            store.load_drafts(&scope).unwrap(),
            vec![first.clone(), second.clone()]
        );
        assert!(store.load_drafts(&other_pull_request).unwrap().is_empty());
        assert!(store.load_drafts(&other_checkout).unwrap().is_empty());

        let mut edited = first.clone();
        edited.body = "edited".to_string();
        edited.head_oid = "b".repeat(40);
        store.save_draft(&scope, &edited).unwrap();
        assert_eq!(store.draft_head(&first.id).unwrap(), Some("b".repeat(40)));
        assert_eq!(store.load_drafts(&scope).unwrap()[0].body, "edited");

        store.delete_drafts(&[edited.id.clone()]).unwrap();
        assert_eq!(store.load_drafts(&scope).unwrap(), vec![second]);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn drafts_become_review_comments_with_their_range() {
        let comment = draft("src/lib.rs", 4, "Consider a guard.").to_review_comment();

        assert_eq!(comment.path, "src/lib.rs");
        assert_eq!(comment.start, Some(position(DiffSide::Left, 3)));
        assert_eq!(comment.end, position(DiffSide::Right, 4));
        assert_eq!(comment.body, "Consider a guard.");
    }

    #[test]
    fn generated_ids_are_unique() {
        let ids = (0..100)
            .map(|_| DraftId::generate())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 100);
    }
}
