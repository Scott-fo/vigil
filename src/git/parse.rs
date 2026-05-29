//! Parsers for git porcelain output.
//!
//! This module is a private facade over product-specific parsers. Repository
//! modules ask for typed parser functions from here instead of knowing which
//! parser file handles commits, statuses, refs, worktrees, or filetype
//! inference.

mod commit;
mod filetype;
mod refs;
mod status;
mod worktree;

pub(crate) use self::commit::{
    is_uncommitted_blame_hash, parse_blame_porcelain_header, parse_commit_log_entries,
    parse_commit_show_output,
};
pub(crate) use self::refs::build_branch_diff_range;
pub(crate) use self::status::{
    parse_diff_name_status_entries, parse_status_entries, to_file_entry,
};
pub(crate) use self::worktree::parse_worktree_entries;
