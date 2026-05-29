//! Commit search, commit comparison, and blame metadata.
//!
//! This module owns commit-shaped repository facts. It converts git log, show,
//! blame, and commit-diff output into typed values used by the app, keeping
//! porcelain formatting details behind the public `git` facade.

mod blame;
mod diff;
mod search;
mod stage;

pub use self::{
    blame::load_blame_commit_details,
    diff::load_files_with_commit_diff,
    search::{list_searchable_commits, resolve_commit_base_ref},
    stage::commit_staged_changes,
};
