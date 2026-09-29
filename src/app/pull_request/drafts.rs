//! The reviewer's draft comments on the pull request under review.
//!
//! Drafts load from the review database when a pull request opens and are
//! written back in the background, one write at a time and in order, as
//! they change. A draft is *attached* while its head is the head the review
//! shows; attached drafts draw under their lines and go out with the next
//! review. When the review moves to a newer head, drafts from the older
//! head are re-anchored against the new patch once its snapshot loads (see
//! [`crate::review::DraftAnchor::reanchor`]): found ones move and are
//! re-pinned, the rest stay behind and need attention in the overview.
//! Re-anchoring waits while whitespace changes are hidden, since that patch
//! is not GitHub's.

use tokio::{sync::mpsc, task};

use crate::{
    event::Event,
    git::{self, PatchLine},
    review::{DraftAnchor, DraftComment, DraftId, DraftScope, ReviewStore},
};

use super::{
    super::{App, SnackbarVariant},
    PullRequestEvent,
    state::DraftLoad,
};

/// Whether drafts are read from and written to the review database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum DraftPersistence {
    /// The per-user review database.
    Database,
    /// Keep drafts in memory only. Used where touching the user's database
    /// is unwanted (benchmarks, tests).
    Off,
}

/// Drafts for one pull request, in creation order.
#[derive(Debug, Default)]
pub(in crate::app) struct DraftBook {
    drafts: Vec<DraftComment>,
    load: DraftLoad,
}

impl DraftBook {
    pub(in crate::app) fn all(&self) -> &[DraftComment] {
        &self.drafts
    }

    pub(in crate::app) fn get(&self, id: &DraftId) -> Option<&DraftComment> {
        self.drafts.iter().find(|draft| &draft.id == id)
    }

    pub(in crate::app) fn load(&self) -> &DraftLoad {
        &self.load
    }

    pub(in crate::app) fn set_load(&mut self, load: DraftLoad) {
        self.load = load;
    }

    /// Drafts pinned to `head`, which a review of `head` sends.
    pub(in crate::app) fn attached<'a>(
        &'a self,
        head: &'a str,
    ) -> impl Iterator<Item = &'a DraftComment> + 'a {
        self.drafts
            .iter()
            .filter(move |draft| draft.head_oid == head)
    }

    /// Drafts from another head whose lines were not found at `head`.
    pub(in crate::app) fn needing_attention<'a>(
        &'a self,
        head: &'a str,
    ) -> impl Iterator<Item = &'a DraftComment> + 'a {
        self.drafts
            .iter()
            .filter(move |draft| draft.head_oid != head)
    }

    /// Adds `draft`, or replaces the draft with its id.
    pub(in crate::app) fn upsert(&mut self, draft: DraftComment) {
        match self
            .drafts
            .iter_mut()
            .find(|existing| existing.id == draft.id)
        {
            Some(existing) => *existing = draft,
            None => self.drafts.push(draft),
        }
    }

    pub(in crate::app) fn remove(&mut self, ids: &[DraftId]) {
        self.drafts.retain(|draft| !ids.contains(&draft.id));
    }

    /// Adds drafts loaded from the database. Drafts written before the load
    /// finished are kept; the database copy wins for ids both have, unless
    /// the in-memory copy is newer.
    pub(in crate::app) fn merge_loaded(&mut self, loaded: Vec<DraftComment>) {
        for draft in loaded {
            match self
                .drafts
                .iter_mut()
                .find(|existing| existing.id == draft.id)
            {
                Some(existing) if existing.updated_at_ms >= draft.updated_at_ms => {}
                Some(existing) => *existing = draft,
                None => self.drafts.push(draft),
            }
        }
        self.drafts
            .sort_by(|a, b| (a.created_at_ms, &a.id).cmp(&(b.created_at_ms, &b.id)));
    }

    /// Moves drafts from other heads onto `head` where `patch_lines` finds
    /// their lines. Returns the drafts that moved, to persist.
    pub(in crate::app) fn reanchor(
        &mut self,
        head: &str,
        patch_lines: impl Fn(&str) -> Option<Vec<PatchLine>>,
    ) -> Vec<DraftComment> {
        let mut moved = Vec::new();
        for draft in self
            .drafts
            .iter_mut()
            .filter(|draft| draft.head_oid != head)
        {
            let Some(lines) = patch_lines(&draft.path) else {
                continue;
            };
            if let Some(anchor) = draft.anchor.reanchor(&lines) {
                draft.anchor = anchor;
                draft.head_oid = head.to_string();
                draft.updated_at_ms = draft.updated_at_ms.saturating_add(1);
                moved.push(draft.clone());
            }
        }
        moved
    }
}

/// A queued database write.
#[derive(Debug)]
enum DraftWrite {
    Save(DraftScope, Box<DraftComment>),
    Delete(Vec<DraftId>),
}

/// Sends draft writes to one background task, which applies them in order.
#[derive(Debug, Clone)]
pub(in crate::app) struct DraftWriter(mpsc::UnboundedSender<DraftWrite>);

impl DraftWriter {
    fn spawn(events: mpsc::UnboundedSender<Event>) -> Self {
        let (sender, mut receiver) = mpsc::unbounded_channel::<DraftWrite>();
        // Not tracked with the app's background tasks: quitting must not
        // abort a write that is queued.
        task::spawn(async move {
            while let Some(write) = receiver.recv().await {
                let result = task::spawn_blocking(move || {
                    let store = ReviewStore::open_default()?;
                    match write {
                        DraftWrite::Save(scope, draft) => store.save_draft(&scope, &draft),
                        DraftWrite::Delete(ids) => store.delete_drafts(&ids),
                    }
                })
                .await
                .map_err(color_eyre::Report::from)
                .and_then(|result| result);
                if let Err(error) = result {
                    let _ = events.send(Event::PullRequest(PullRequestEvent::DraftWriteFailed(
                        error.to_string(),
                    )));
                }
            }
        });
        Self(sender)
    }
}

/// Tests must never touch the user's review database.
#[cfg(test)]
fn assert_not_in_tests() {
    panic!("tests must keep drafts in memory (PullRequests::disabled)");
}

impl App {
    fn draft_scope(&self) -> Option<DraftScope> {
        let number = self.pull_requests.open_number()?;
        Some(DraftScope::new(&self.repo_root, number))
    }

    /// Loads the open pull request's drafts in the background.
    pub(in crate::app) fn load_pull_request_drafts(&mut self) {
        let Some(scope) = self.draft_scope() else {
            return;
        };
        if self.pull_requests.draft_persistence() == DraftPersistence::Off {
            if let Some(open) = self.pull_requests.open_mut() {
                open.drafts_mut().set_load(DraftLoad::Loaded);
            }
            return;
        }
        #[cfg(test)]
        assert_not_in_tests();
        let Some(request_id) = self.pull_requests.begin_drafts_load() else {
            return;
        };
        let sender = self.events.sender();
        let handle = task::spawn(async move {
            let result =
                task::spawn_blocking(move || ReviewStore::open_default()?.load_drafts(&scope))
                    .await
                    .map_err(color_eyre::Report::from)
                    .and_then(|result| result)
                    .map_err(|error| error.to_string());
            let _ = sender.send(Event::PullRequest(PullRequestEvent::DraftsLoaded {
                request_id,
                result,
            }));
        });
        self.pull_requests.attach_drafts_load(request_id, handle);
    }

    pub(super) fn handle_drafts_loaded(
        &mut self,
        request_id: u64,
        result: Result<Vec<DraftComment>, String>,
    ) -> bool {
        let Some(open) = self.pull_requests.open_mut() else {
            return false;
        };
        if !open.finish_drafts_load(request_id) {
            return false;
        }
        match result {
            Ok(drafts) => {
                open.drafts_mut().merge_loaded(drafts);
                open.drafts_mut().set_load(DraftLoad::Loaded);
                open.rebuild_threads();
                self.reanchor_drafts();
            }
            Err(error) => {
                open.drafts_mut().set_load(DraftLoad::Failed(error.clone()));
                self.show_snackbar(
                    format!("could not load draft comments: {error}"),
                    SnackbarVariant::Error,
                );
            }
        }
        true
    }

    pub(super) fn handle_draft_write_failed(&mut self, error: String) -> bool {
        self.show_snackbar(
            format!("could not save draft comments: {error}"),
            SnackbarVariant::Error,
        );
        true
    }

    /// Stores `draft` in the open pull request and the database.
    pub(in crate::app) fn save_draft(&mut self, draft: DraftComment) {
        let Some(open) = self.pull_requests.open_mut() else {
            return;
        };
        open.drafts_mut().upsert(draft.clone());
        open.rebuild_threads();
        self.write_drafts(DraftWriteRequest::Save(vec![draft]));
    }

    /// Deletes drafts from the open pull request and the database.
    pub(in crate::app) fn delete_drafts(&mut self, ids: Vec<DraftId>) {
        if let Some(open) = self.pull_requests.open_mut() {
            open.drafts_mut().remove(&ids);
            open.rebuild_threads();
        }
        self.write_drafts(DraftWriteRequest::Delete(ids));
    }

    /// Moves drafts written against an older head onto the head the review
    /// shows, now that its diff snapshot is loaded.
    pub(in crate::app) fn reanchor_drafts(&mut self) {
        if self.diff_whitespace_mode == git::WhitespaceMode::Ignore {
            return;
        }
        let Some(selection_head) = self.pull_request_selection().map(|pr| pr.head_oid.clone())
        else {
            return;
        };
        let Some(snapshot) = self.review_diff_snapshot.clone() else {
            return;
        };
        let Some(open) = self.pull_requests.open_mut() else {
            return;
        };
        if open.reviewed_head() != selection_head || open.drafts().load() != &DraftLoad::Loaded {
            return;
        }
        let moved = open
            .drafts_mut()
            .reanchor(&selection_head, |path| snapshot.patch_lines(path));
        if moved.is_empty() {
            return;
        }
        open.rebuild_threads();
        let count = moved.len();
        self.write_drafts(DraftWriteRequest::Save(moved));
        self.status_message = Some(format!(
            "{count} draft comment{} moved to the new commits",
            if count == 1 { "" } else { "s" }
        ));
    }

    fn write_drafts(&mut self, request: DraftWriteRequest) {
        if self.pull_requests.draft_persistence() == DraftPersistence::Off {
            return;
        }
        #[cfg(test)]
        assert_not_in_tests();
        let writes = match request {
            DraftWriteRequest::Save(drafts) => {
                let Some(scope) = self.draft_scope() else {
                    return;
                };
                drafts
                    .into_iter()
                    .map(|draft| DraftWrite::Save(scope.clone(), Box::new(draft)))
                    .collect()
            }
            DraftWriteRequest::Delete(ids) => vec![DraftWrite::Delete(ids)],
        };
        let events = self.events.sender();
        let writer = self
            .pull_requests
            .draft_writer(|| DraftWriter::spawn(events));
        for write in writes {
            let _ = writer.0.send(write);
        }
    }

    /// Deletes drafts a submitted review sent, from the open pull request if
    /// it is still the one reviewed, and from the database either way.
    pub(in crate::app) fn delete_submitted_drafts(&mut self, number: u64, ids: Vec<DraftId>) {
        if self.pull_requests.open_number() == Some(number) {
            self.delete_drafts(ids);
        } else {
            self.write_drafts(DraftWriteRequest::Delete(ids));
        }
    }

    /// Builds a new draft on the reviewed head.
    pub(in crate::app) fn new_draft(
        &self,
        path: String,
        anchor: DraftAnchor,
        body: String,
    ) -> Option<DraftComment> {
        let head = self.pull_requests.open()?.reviewed_head().to_string();
        Some(DraftComment::new(path, anchor, body, head))
    }
}

enum DraftWriteRequest {
    Save(Vec<DraftComment>),
    Delete(Vec<DraftId>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        forge::{DiffPosition, DiffSide},
        git::PatchLineKind,
        review::DraftLine,
    };

    fn draft(line: u32, text: &str, head: &str) -> DraftComment {
        DraftComment::new(
            "src/lib.rs".to_string(),
            DraftAnchor {
                start: None,
                end: DraftLine {
                    position: DiffPosition {
                        side: DiffSide::Right,
                        line,
                    },
                    text: text.to_string(),
                },
            },
            "body".to_string(),
            head.to_string(),
        )
    }

    fn added(line: usize, text: &str) -> PatchLine {
        PatchLine {
            kind: PatchLineKind::Added,
            old_line: None,
            new_line: Some(line),
            text: text.to_string(),
            hunk: 0,
            distance_to_change: 0,
        }
    }

    #[test]
    fn drafts_follow_their_lines_to_a_new_head_or_need_attention() {
        let mut book = DraftBook::default();
        let kept = draft(4, "let kept = 1;", "old");
        let lost = draft(9, "let lost = 1;", "old");
        let current = draft(2, "fn current() {}", "new");
        for draft in [kept.clone(), lost.clone(), current.clone()] {
            book.upsert(draft);
        }

        let moved = book.reanchor("new", |_| {
            Some(vec![added(2, "fn current() {}"), added(6, "let kept = 1;")])
        });

        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].id, kept.id);
        assert_eq!(moved[0].anchor.end.position.line, 6);
        let attached = book
            .attached("new")
            .map(|d| d.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(attached, vec![kept.id, current.id]);
        let stale = book
            .needing_attention("new")
            .map(|d| d.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(stale, vec![lost.id]);
    }

    #[test]
    fn loaded_drafts_merge_with_drafts_written_while_loading() {
        let mut book = DraftBook::default();
        let written = draft(1, "a", "head");
        book.upsert(written.clone());
        let mut stored = draft(2, "b", "head");
        stored.created_at_ms = written.created_at_ms - 10;
        let mut stale_copy = written.clone();
        stale_copy.body = "older body".to_string();
        stale_copy.updated_at_ms -= 5;

        book.merge_loaded(vec![stored.clone(), stale_copy]);

        let bodies = book
            .all()
            .iter()
            .map(|d| d.body.as_str())
            .collect::<Vec<_>>();
        assert_eq!(bodies, vec!["body", "body"]);
        assert_eq!(book.all()[0].id, stored.id, "oldest first");
        assert_eq!(book.all()[1].id, written.id);
    }

    #[test]
    fn deleting_drafts_removes_only_those_ids() {
        let mut book = DraftBook::default();
        let a = draft(1, "a", "head");
        let b = draft(2, "b", "head");
        book.upsert(a.clone());
        book.upsert(b.clone());

        book.remove(std::slice::from_ref(&a.id));

        assert!(book.get(&a.id).is_none());
        assert!(book.get(&b.id).is_some());
    }
}
