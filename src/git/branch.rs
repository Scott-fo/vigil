//! Branches, HEAD, and remote sync.
//!
//! This module owns every git command that reads or changes which branch is
//! checked out and how it relates to its upstream. Callers get two entry
//! points:
//!
//! - [`load_branch_snapshot`] reads HEAD, local and remote branches with their
//!   upstream divergence, the previously checked-out branch, the last fetch
//!   time, and any merge or rebase in progress. It is read-only and cheap
//!   enough to reload on every review refresh. Divergence is measured against
//!   the local remote-tracking refs, so it is only as fresh as the last fetch.
//! - [`run_branch_operation`] performs one typed [`BranchOperation`] (fetch,
//!   pull, push, switch, create, rename, delete) and reports a typed outcome or
//!   a [`BranchOperationError`] callers can react to, such as a delete refused
//!   because the branch is not fully merged.
//!
//! Git never prompts: commands run without a terminal, so a remote that needs
//! interactive credentials fails with git's message instead of hanging.

mod load;
mod operation;
mod types;

pub use self::load::load_branch_snapshot;
pub use self::operation::{
    BranchOperation, BranchOperationError, BranchOperationOutcome, run_branch_operation,
};
pub use self::types::{
    BranchEntry, BranchLocation, BranchSnapshot, BranchTip, Divergence, HeadState, RepoOperation,
    Upstream,
};
