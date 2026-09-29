//! What a reviewer sees and remembers about a review.
//!
//! This module owns review state that is independent of any one screen. A
//! review target is named by a [`ReviewScope`] (the working tree, a commit, a
//! branch comparison, or a pull request).
//!
//! - Per-file "viewed" marks ([`ViewedFiles`]) are persisted in a local SQLite
//!   database through [`ReviewStore`]. They are tied to each file's diff
//!   fingerprint and clear on their own when that diff changes. Start with
//!   [`ViewedScope::new`] to key marks for a target, then load and save them
//!   with [`ReviewStore::load_viewed_files`] and
//!   [`ReviewStore::set_file_viewed`].
//! - Draft review comments ([`DraftComment`]) are the reviewer's pending
//!   inline comments on a pull request, persisted per [`DraftScope`] with the
//!   head commit they were written against. [`DraftAnchor::from_patch_lines`]
//!   decides which lines GitHub will accept a comment on, and
//!   [`DraftAnchor::reanchor`] follows a draft to a newer head.
//! - Review threads ([`ReviewThreads`]) are the comments drawn under diff
//!   lines or, when they cannot be placed, listed in the pull request
//!   overview: GitHub's threads from a point-in-time load, plus drafts.
//!
//! Every [`ReviewStore`] call is blocking SQLite I/O; interactive callers
//! should run them off the UI thread. Callers should not rely on the database
//! layout or the encoding of [`ViewedScope`] keys.

mod annotations;
mod drafts;
mod scope;
mod store;
mod viewed;

pub use self::annotations::{
    DisplayComment, DisplayThread, LineAnchor, ReviewThreads, ThreadPlacement, ThreadRow,
    ThreadSource, ThreadStatus, UnplacedReason, wrap_comment_text,
};
pub use self::drafts::{
    AnchorRefusal, DraftAnchor, DraftComment, DraftId, DraftLine, DraftScope, GITHUB_DIFF_CONTEXT,
    is_commentable, suggestion_block, suggestion_source,
};
pub use self::scope::ReviewScope;
pub use self::store::ReviewStore;
pub use self::viewed::{ViewedFiles, ViewedScope};
