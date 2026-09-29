//! Review progress that outlives a session.
//!
//! This module owns what Vigil remembers about a review between runs. A review
//! target is named by a [`ReviewScope`] (the working tree, a commit, or a
//! branch comparison) and persisted in a local SQLite database through
//! [`ReviewStore`].
//!
//! Today that state is per-file "viewed" marks ([`ViewedFiles`]), which are
//! tied to each file's diff fingerprint and clear on their own when that diff
//! changes. Start with [`ViewedScope::new`] to key marks for a target, then
//! load and save them with [`ReviewStore::load_viewed_files`] and
//! [`ReviewStore::set_file_viewed`].
//!
//! Every [`ReviewStore`] call is blocking SQLite I/O; interactive callers
//! should run them off the UI thread. Callers should not rely on the database
//! layout or the encoding of [`ViewedScope`] keys.

mod scope;
mod store;
mod viewed;

pub use self::scope::ReviewScope;
pub use self::store::ReviewStore;
pub use self::viewed::{ViewedFiles, ViewedScope};
