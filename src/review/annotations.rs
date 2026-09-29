//! Review threads shown alongside the diff.
//!
//! [`ReviewThreads`] turns a pull request's review threads into what the
//! review screen draws: threads anchored under the diff line they comment on,
//! and threads that cannot be placed inline (file-level comments, outdated
//! threads, and threads on files not in the diff), which the pull request
//! overview lists instead. Anchoring follows GitHub: a thread on the
//! [`DiffSide::Right`] side sits under that line of the new file, a
//! [`DiffSide::Left`] thread under that line of the old file, and a
//! multi-line thread under its last line.
//!
//! Every thread carries a [`ThreadSource`], so comments from other sources
//! (the reviewer's unsubmitted drafts) can join the same display model.
//!
//! [`DisplayThread::rows`] lays a thread out into terminal rows for a given
//! diff width. Rendering and scroll math both count these rows, so the diff
//! viewport and what is drawn cannot disagree.

use std::collections::HashMap;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    forge::{CommentState, DiffSide, ReviewThread, ThreadId, ThreadSubject, Timestamp},
    git::DiffDisplayLineAnchor,
};

/// Columns a thread box takes beyond its text: a five-column `  │  ` gutter
/// and a two-column right margin.
const THREAD_BOX_CHROME_WIDTH: usize = 7;
/// Narrowest text column a thread box wraps to, however narrow the pane.
const MIN_THREAD_TEXT_WIDTH: usize = 16;

/// Where a displayed thread came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadSource {
    /// A review thread on the pull request on GitHub.
    PullRequest(ThreadId),
}

/// A line in the diff, numbered in the file on `side`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LineAnchor {
    pub side: DiffSide,
    pub line: u32,
}

impl LineAnchor {
    fn matches(self, anchor: DiffDisplayLineAnchor) -> bool {
        let line = Some(self.line as usize);
        match self.side {
            DiffSide::Left => anchor.old_line == line,
            DiffSide::Right => anchor.new_line == line,
        }
    }
}

/// Where a thread is attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPlacement {
    /// Under `end`, the last commented line. `start_line` is the first line
    /// of a multi-line range.
    Line {
        end: LineAnchor,
        start_line: Option<u32>,
    },
    /// The file as a whole.
    File,
    /// The commented lines are outdated. `original_line` numbers them in the
    /// commit the thread was written on.
    Outdated { original_line: Option<u32> },
}

/// Why a thread is listed in the overview rather than drawn in the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnplacedReason {
    FileLevel,
    Outdated {
        original_line: Option<u32>,
    },
    /// The thread's file is not part of the diff (for example, it is hidden
    /// by a file filter).
    NotInDiff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadStatus {
    Unresolved,
    Resolved { by: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayComment {
    pub author: String,
    /// Markdown.
    pub body: String,
    pub created_at: Timestamp,
    pub state: CommentState,
}

/// A thread as the review screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayThread {
    pub source: ThreadSource,
    pub path: String,
    pub placement: ThreadPlacement,
    pub status: ThreadStatus,
    pub comments: Vec<DisplayComment>,
}

/// One terminal row of a thread drawn under a diff line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadRow {
    /// `✓ resolved · 3 comments`: a resolved thread collapsed to one row.
    Collapsed {
        resolved_by: Option<String>,
        comment_count: usize,
    },
    /// `╭─ lines 10–14 · 2 comments`: the top of an unresolved thread.
    Heading {
        start_line: Option<u32>,
        end_line: Option<u32>,
        comment_count: usize,
    },
    /// `│  alice · 3h ago`
    Author {
        author: String,
        created_at: Timestamp,
        pending: bool,
    },
    /// `│  ` plus one wrapped row of comment text.
    Body(String),
    /// A blank row between two comments.
    Gap,
    /// `╰─`
    End,
}

impl DisplayThread {
    /// The display form of a GitHub review thread. Threads GitHub marks
    /// outdated, or whose lines no longer map onto the current diff, are
    /// placed as [`ThreadPlacement::Outdated`].
    pub fn from_review_thread(thread: &ReviewThread) -> Self {
        let placement = match (thread.subject, thread.line) {
            (ThreadSubject::File, _) => ThreadPlacement::File,
            (ThreadSubject::Line, Some(line)) if !thread.is_outdated => ThreadPlacement::Line {
                end: LineAnchor {
                    side: thread.side,
                    line,
                },
                start_line: thread
                    .start
                    .map(|start| start.line)
                    .filter(|start| *start != line),
            },
            (ThreadSubject::Line, _) => ThreadPlacement::Outdated {
                original_line: thread.original_line,
            },
        };
        Self {
            source: ThreadSource::PullRequest(thread.id.clone()),
            path: thread.path.clone(),
            placement,
            status: if thread.is_resolved {
                ThreadStatus::Resolved {
                    by: thread.resolved_by.clone(),
                }
            } else {
                ThreadStatus::Unresolved
            },
            comments: thread
                .comments
                .iter()
                .map(|comment| DisplayComment {
                    author: comment.author.clone(),
                    body: comment.body.clone(),
                    created_at: comment.created_at.clone(),
                    state: comment.state,
                })
                .collect(),
        }
    }

    pub fn is_resolved(&self) -> bool {
        matches!(self.status, ThreadStatus::Resolved { .. })
    }

    /// The rows this thread takes under a diff line in a pane `width`
    /// columns wide. Resolved threads collapse to a single row; unresolved
    /// threads show every comment.
    pub fn rows(&self, width: usize) -> Vec<ThreadRow> {
        if let ThreadStatus::Resolved { by } = &self.status {
            return vec![ThreadRow::Collapsed {
                resolved_by: by.clone(),
                comment_count: self.comments.len(),
            }];
        }

        let (start_line, end_line) = match self.placement {
            ThreadPlacement::Line { end, start_line } => (start_line, Some(end.line)),
            ThreadPlacement::File | ThreadPlacement::Outdated { .. } => (None, None),
        };
        let text_width = thread_text_width(width);
        let mut rows = vec![ThreadRow::Heading {
            start_line,
            end_line,
            comment_count: self.comments.len(),
        }];
        for (index, comment) in self.comments.iter().enumerate() {
            if index > 0 {
                rows.push(ThreadRow::Gap);
            }
            rows.push(ThreadRow::Author {
                author: comment.author.clone(),
                created_at: comment.created_at.clone(),
                pending: comment.state == CommentState::Pending,
            });
            rows.extend(
                wrap_comment_text(&comment.body, text_width)
                    .into_iter()
                    .map(ThreadRow::Body),
            );
        }
        rows.push(ThreadRow::End);
        rows
    }
}

/// Review threads for one review target, indexed for drawing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewThreads {
    threads: Vec<DisplayThread>,
    /// Indices into `threads` of line-anchored threads, by path.
    inline: HashMap<String, Vec<usize>>,
}

impl ReviewThreads {
    pub fn new(threads: Vec<DisplayThread>) -> Self {
        let mut inline: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, thread) in threads.iter().enumerate() {
            if matches!(thread.placement, ThreadPlacement::Line { .. }) {
                inline.entry(thread.path.clone()).or_default().push(index);
            }
        }
        Self { threads, inline }
    }

    pub fn from_pull_request(threads: &[ReviewThread]) -> Self {
        Self::new(
            threads
                .iter()
                .map(DisplayThread::from_review_thread)
                .collect(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }

    pub fn threads(&self) -> &[DisplayThread] {
        &self.threads
    }

    /// Whether any thread is drawn inside `path`'s diff.
    pub fn has_inline_threads(&self, path: &str) -> bool {
        self.inline.contains_key(path)
    }

    /// Threads drawn under the diff row with `anchor`, oldest first.
    pub fn threads_at(&self, path: &str, anchor: DiffDisplayLineAnchor) -> Vec<&DisplayThread> {
        let Some(indices) = self.inline.get(path) else {
            return Vec::new();
        };
        indices
            .iter()
            .map(|index| &self.threads[*index])
            .filter(|thread| match thread.placement {
                ThreadPlacement::Line { end, .. } => end.matches(anchor),
                ThreadPlacement::File | ThreadPlacement::Outdated { .. } => false,
            })
            .collect()
    }

    /// Unresolved threads on `path`, wherever they are placed.
    pub fn unresolved_count(&self, path: &str) -> usize {
        self.threads
            .iter()
            .filter(|thread| thread.path == path && !thread.is_resolved())
            .count()
    }

    /// Threads the diff cannot show, given which paths are in it, in the
    /// order GitHub reported them.
    pub fn unplaced(
        &self,
        in_diff: impl Fn(&str) -> bool,
    ) -> Vec<(UnplacedReason, &DisplayThread)> {
        self.threads
            .iter()
            .filter_map(|thread| {
                let reason = match thread.placement {
                    ThreadPlacement::File => UnplacedReason::FileLevel,
                    ThreadPlacement::Outdated { original_line } => {
                        UnplacedReason::Outdated { original_line }
                    }
                    ThreadPlacement::Line { .. } if in_diff(&thread.path) => return None,
                    ThreadPlacement::Line { .. } => UnplacedReason::NotInDiff,
                };
                Some((reason, thread))
            })
            .collect()
    }
}

fn thread_text_width(width: usize) -> usize {
    width
        .saturating_sub(THREAD_BOX_CHROME_WIDTH)
        .max(MIN_THREAD_TEXT_WIDTH)
}

/// Word-wraps comment text to `width` columns, keeping the author's line
/// breaks and blank lines. Words wider than a row are split by character.
pub fn wrap_comment_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for source_line in text.trim_end().lines() {
        let source_line = source_line.trim_end_matches('\r');
        if source_line.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        wrap_line(source_line, width, &mut lines);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn wrap_line(text: &str, width: usize, lines: &mut Vec<String>) {
    let mut current = String::new();
    let mut current_width = 0;
    for word in text.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if word_width > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            push_wrapped_word(word, width, lines);
            continue;
        }

        if current.is_empty() {
            current.push_str(word);
            current_width = word_width;
        } else if current_width + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
            current_width = word_width;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
}

fn push_wrapped_word(word: &str, width: usize, lines: &mut Vec<String>) {
    let mut current = String::new();
    let mut current_width = 0;
    for ch in word.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width > 0 && current_width + ch_width > width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }
    if !current.is_empty() {
        lines.push(current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::{DiffPosition, ThreadComment};

    fn thread(side: DiffSide, line: Option<u32>) -> ReviewThread {
        ReviewThread {
            id: ThreadId::new("T1"),
            path: "src/lib.rs".to_string(),
            subject: ThreadSubject::Line,
            side,
            line,
            start: None,
            original_line: line,
            original_start_line: None,
            is_resolved: false,
            resolved_by: None,
            is_outdated: false,
            viewer_can_reply: true,
            viewer_can_resolve: true,
            viewer_can_unresolve: false,
            diff_hunk: String::new(),
            comments: vec![ThreadComment {
                id: "C1".to_string(),
                author: "alice".to_string(),
                body: "This line matters.".to_string(),
                created_at: Timestamp::new("2026-09-01T10:00:00Z"),
                url: String::new(),
                state: CommentState::Submitted,
            }],
        }
    }

    fn anchor(old_line: Option<usize>, new_line: Option<usize>) -> DiffDisplayLineAnchor {
        DiffDisplayLineAnchor { old_line, new_line }
    }

    #[test]
    fn right_side_threads_anchor_to_new_lines() {
        let threads = ReviewThreads::from_pull_request(&[thread(DiffSide::Right, Some(9))]);

        assert_eq!(
            threads
                .threads_at("src/lib.rs", anchor(None, Some(9)))
                .len(),
            1
        );
        assert_eq!(
            threads
                .threads_at("src/lib.rs", anchor(Some(9), Some(12)))
                .len(),
            0
        );
        assert_eq!(
            threads
                .threads_at("src/main.rs", anchor(None, Some(9)))
                .len(),
            0
        );
    }

    #[test]
    fn left_side_threads_anchor_to_old_lines() {
        let threads = ReviewThreads::from_pull_request(&[thread(DiffSide::Left, Some(4))]);

        assert_eq!(
            threads
                .threads_at("src/lib.rs", anchor(Some(4), None))
                .len(),
            1
        );
        assert_eq!(
            threads
                .threads_at("src/lib.rs", anchor(Some(4), Some(5)))
                .len(),
            1
        );
        assert_eq!(
            threads
                .threads_at("src/lib.rs", anchor(None, Some(4)))
                .len(),
            0
        );
    }

    #[test]
    fn multi_line_threads_anchor_at_their_end_line() {
        let mut range = thread(DiffSide::Right, Some(14));
        range.start = Some(DiffPosition {
            side: DiffSide::Right,
            line: 10,
        });
        let threads = ReviewThreads::from_pull_request(&[range]);

        assert!(
            threads
                .threads_at("src/lib.rs", anchor(None, Some(10)))
                .is_empty()
        );
        let at_end = threads.threads_at("src/lib.rs", anchor(None, Some(14)));
        assert_eq!(at_end.len(), 1);
        assert_eq!(
            at_end[0].rows(80)[0],
            ThreadRow::Heading {
                start_line: Some(10),
                end_line: Some(14),
                comment_count: 1,
            }
        );
    }

    #[test]
    fn file_level_outdated_and_missing_file_threads_are_unplaced() {
        let mut file_level = thread(DiffSide::Right, None);
        file_level.subject = ThreadSubject::File;
        let mut outdated = thread(DiffSide::Right, None);
        outdated.original_line = Some(3);
        let mut stale_but_mapped = thread(DiffSide::Right, Some(8));
        stale_but_mapped.is_outdated = true;
        let mut elsewhere = thread(DiffSide::Right, Some(2));
        elsewhere.path = "docs/README.md".to_string();
        let inline = thread(DiffSide::Right, Some(1));
        let threads = ReviewThreads::from_pull_request(&[
            file_level,
            outdated,
            stale_but_mapped,
            elsewhere,
            inline,
        ]);

        let reasons = threads
            .unplaced(|path| path == "src/lib.rs")
            .into_iter()
            .map(|(reason, _)| reason)
            .collect::<Vec<_>>();
        assert_eq!(
            reasons,
            vec![
                UnplacedReason::FileLevel,
                UnplacedReason::Outdated {
                    original_line: Some(3)
                },
                UnplacedReason::Outdated {
                    original_line: Some(8)
                },
                UnplacedReason::NotInDiff,
            ]
        );
        assert!(threads.has_inline_threads("src/lib.rs"));
    }

    #[test]
    fn resolved_threads_collapse_to_one_row() {
        let mut resolved = thread(DiffSide::Right, Some(3));
        resolved.is_resolved = true;
        resolved.resolved_by = Some("bob".to_string());
        resolved.comments.push(resolved.comments[0].clone());
        let display = DisplayThread::from_review_thread(&resolved);

        assert_eq!(
            display.rows(20),
            vec![ThreadRow::Collapsed {
                resolved_by: Some("bob".to_string()),
                comment_count: 2,
            }]
        );
    }

    #[test]
    fn unresolved_threads_show_every_comment_wrapped_to_the_pane() {
        let mut open = thread(DiffSide::Right, Some(3));
        let mut reply = open.comments[0].clone();
        reply.author = "bob".to_string();
        reply.body = "Agreed, and this reply is long enough to wrap.".to_string();
        open.comments.push(reply);
        let rows = DisplayThread::from_review_thread(&open).rows(30);

        let bodies = rows
            .iter()
            .filter_map(|row| match row {
                ThreadRow::Body(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            bodies,
            vec![
                "This line matters.",
                "Agreed, and this reply",
                "is long enough to wrap."
            ]
        );
        assert!(matches!(rows.first(), Some(ThreadRow::Heading { .. })));
        assert!(rows.contains(&ThreadRow::Gap));
        assert_eq!(rows.last(), Some(&ThreadRow::End));
    }

    #[test]
    fn unresolved_counts_include_every_placement() {
        let mut resolved = thread(DiffSide::Right, Some(3));
        resolved.is_resolved = true;
        let mut file_level = thread(DiffSide::Right, None);
        file_level.subject = ThreadSubject::File;
        let threads = ReviewThreads::from_pull_request(&[
            thread(DiffSide::Right, Some(1)),
            resolved,
            file_level,
        ]);

        assert_eq!(threads.unresolved_count("src/lib.rs"), 2);
        assert_eq!(threads.unresolved_count("src/main.rs"), 0);
    }

    #[test]
    fn comment_text_keeps_paragraph_breaks_and_splits_long_words() {
        assert_eq!(
            wrap_comment_text("first line\n\nsecond", 20),
            vec!["first line", "", "second"]
        );
        assert_eq!(wrap_comment_text("abcdefgh", 3), vec!["abc", "def", "gh"]);
        assert_eq!(wrap_comment_text("", 10), vec![""]);
    }
}
