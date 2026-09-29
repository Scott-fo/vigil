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
//! - Review threads ([`ReviewThreads`]) are the comments drawn under diff
//!   lines or, when they cannot be placed, listed in the pull request
//!   overview. They are built from a point-in-time pull request load and are
//!   not persisted.
//!
//! Every [`ReviewStore`] call is blocking SQLite I/O; interactive callers
//! should run them off the UI thread. Callers should not rely on the database
//! layout or the encoding of [`ViewedScope`] keys.

mod annotations;
mod scope;
mod store;
mod viewed;

pub use self::annotations::{
    DisplayComment, DisplayThread, LineAnchor, ReviewThreads, ThreadPlacement, ThreadRow,
    ThreadSource, ThreadStatus, UnplacedReason, wrap_comment_text,
};
pub use self::scope::ReviewScope;
pub use self::store::ReviewStore;
pub use self::viewed::{ViewedFiles, ViewedScope};
