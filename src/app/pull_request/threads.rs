//! Replying to and resolving GitHub review threads from the diff.
//!
//! Both act on the threads drawn under the cursor's line (a thread is drawn
//! under the last line it comments on). When a line has several, the rule
//! is: reply goes to the first unresolved thread, or the first thread if all
//! are resolved; resolve toggles the first thread the viewer may change,
//! resolving unresolved threads before reopening resolved ones. Threads are
//! in GitHub's order, oldest first. Drafts on the line are skipped; edit
//! them from the drafts list (`D`).

use crate::forge::{ReviewThread, ThreadId};

use super::{
    super::{App, SnackbarVariant},
    composer::{Composer, ComposerTarget},
    gateway::{ForgeMutation, MutationOutcome},
};

/// What to do with the thread under the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) enum ThreadAction {
    Reply(ThreadId),
    SetResolved { thread: ThreadId, resolved: bool },
}

/// Why the threads under the cursor allow no action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum ThreadActionRefusal {
    NoThread,
    NotPermitted,
}

/// Picks the thread a reply goes to among `threads` on one line.
pub(in crate::app) fn reply_target(
    threads: &[&ReviewThread],
) -> Result<ThreadAction, ThreadActionRefusal> {
    let chosen = threads
        .iter()
        .find(|thread| !thread.is_resolved)
        .or_else(|| threads.first())
        .ok_or(ThreadActionRefusal::NoThread)?;
    if !chosen.viewer_can_reply {
        return Err(ThreadActionRefusal::NotPermitted);
    }
    Ok(ThreadAction::Reply(chosen.id.clone()))
}

/// Picks the thread to resolve or reopen among `threads` on one line.
pub(in crate::app) fn resolve_target(
    threads: &[&ReviewThread],
) -> Result<ThreadAction, ThreadActionRefusal> {
    if threads.is_empty() {
        return Err(ThreadActionRefusal::NoThread);
    }
    let resolve = threads
        .iter()
        .find(|thread| !thread.is_resolved && thread.viewer_can_resolve)
        .map(|thread| (thread, true));
    let reopen = || {
        threads
            .iter()
            .find(|thread| thread.is_resolved && thread.viewer_can_unresolve)
            .map(|thread| (thread, false))
    };
    let (thread, resolved) = resolve
        .or_else(reopen)
        .ok_or(ThreadActionRefusal::NotPermitted)?;
    Ok(ThreadAction::SetResolved {
        thread: thread.id.clone(),
        resolved,
    })
}

impl App {
    /// GitHub's threads drawn under the cursor's line, oldest first.
    fn review_threads_at_cursor(&mut self) -> Vec<ReviewThread> {
        if self.pull_request_overview_visible() {
            return Vec::new();
        }
        let Some(path) = self.selected_file().map(|file| file.path.clone()) else {
            return Vec::new();
        };
        let Some(anchor) = self.diff_view.line_anchor_at(
            self.diff_view_mode,
            self.current_diff_display_width(),
            self.diff_line_wrap_mode,
            self.selected_diff_line_index,
        ) else {
            return Vec::new();
        };
        let Some(open) = self.pull_requests.open() else {
            return Vec::new();
        };
        let Some(detail) = open.detail() else {
            return Vec::new();
        };
        open.threads()
            .threads_at(&path, anchor)
            .into_iter()
            .filter_map(|thread| thread.thread_id())
            .filter_map(|id| detail.review_threads.iter().find(|thread| &thread.id == id))
            .cloned()
            .collect()
    }

    /// `R`: reply to the thread under the cursor.
    pub(in crate::app) fn start_thread_reply(&mut self) {
        let threads = self.review_threads_at_cursor();
        let refs = threads.iter().collect::<Vec<_>>();
        match reply_target(&refs) {
            Ok(ThreadAction::Reply(id)) => {
                let thread = threads
                    .iter()
                    .find(|thread| thread.id == id)
                    .expect("target comes from threads");
                self.open_composer(Composer::new(
                    ComposerTarget::Reply {
                        thread: id,
                        path: thread.path.clone(),
                        line: thread.line,
                    },
                    "",
                    None,
                ));
            }
            Ok(ThreadAction::SetResolved { .. }) => {}
            Err(refusal) => self.report_thread_refusal(refusal, "reply to"),
        }
    }

    /// `T`: resolve or reopen the thread under the cursor.
    pub(in crate::app) fn toggle_thread_resolved(&mut self) {
        let threads = self.review_threads_at_cursor();
        let refs = threads.iter().collect::<Vec<_>>();
        match resolve_target(&refs) {
            Ok(ThreadAction::SetResolved { thread, resolved }) => {
                self.start_forge_mutation(ForgeMutation::SetThreadResolved { thread, resolved });
            }
            Ok(ThreadAction::Reply(_)) => {}
            Err(refusal) => self.report_thread_refusal(refusal, "resolve"),
        }
    }

    fn report_thread_refusal(&mut self, refusal: ThreadActionRefusal, verb: &str) {
        let message = match refusal {
            ThreadActionRefusal::NoThread => {
                format!(
                    "no review thread on this line; move to the line a thread is drawn under to {verb} it"
                )
            }
            ThreadActionRefusal::NotPermitted => {
                format!("GitHub does not let you {verb} this thread")
            }
        };
        self.show_snackbar(message, SnackbarVariant::Info);
    }

    pub(super) fn finish_thread_resolve(
        &mut self,
        result: Result<MutationOutcome, crate::forge::ForgeError>,
    ) {
        match result {
            Ok(MutationOutcome::ThreadResolved { resolved }) => {
                let message = if resolved {
                    "thread resolved"
                } else {
                    "thread reopened"
                };
                self.show_snackbar(message.to_string(), SnackbarVariant::Info);
                self.reload_after_mutation(false);
            }
            Ok(_) => {}
            Err(error) => self.show_snackbar(
                format!("could not change the thread: {error}"),
                SnackbarVariant::Error,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::pull_request_fixtures as fixtures, forge::DiffSide};

    fn thread(id: &str, resolved: bool) -> ReviewThread {
        let mut thread = fixtures::thread(id, "src/app.rs", DiffSide::Right, Some(2));
        thread.is_resolved = resolved;
        thread.viewer_can_resolve = !resolved;
        thread.viewer_can_unresolve = resolved;
        thread
    }

    #[test]
    fn replies_go_to_the_first_unresolved_thread() {
        let (resolved, open, later) =
            (thread("T1", true), thread("T2", false), thread("T3", false));

        assert_eq!(
            reply_target(&[&resolved, &open, &later]),
            Ok(ThreadAction::Reply(ThreadId::new("T2")))
        );
        assert_eq!(
            reply_target(&[&resolved]),
            Ok(ThreadAction::Reply(ThreadId::new("T1"))),
            "a resolved thread still takes replies"
        );
        assert_eq!(reply_target(&[]), Err(ThreadActionRefusal::NoThread));
        let mut locked = thread("T4", false);
        locked.viewer_can_reply = false;
        assert_eq!(
            reply_target(&[&locked]),
            Err(ThreadActionRefusal::NotPermitted)
        );
    }

    #[test]
    fn resolving_prefers_unresolved_threads_and_respects_permissions() {
        let (resolved, open) = (thread("T1", true), thread("T2", false));

        assert_eq!(
            resolve_target(&[&resolved, &open]),
            Ok(ThreadAction::SetResolved {
                thread: ThreadId::new("T2"),
                resolved: true
            })
        );
        assert_eq!(
            resolve_target(&[&resolved]),
            Ok(ThreadAction::SetResolved {
                thread: ThreadId::new("T1"),
                resolved: false
            })
        );
        let mut not_mine = thread("T3", false);
        not_mine.viewer_can_resolve = false;
        assert_eq!(
            resolve_target(&[&not_mine]),
            Err(ThreadActionRefusal::NotPermitted)
        );
    }
}
