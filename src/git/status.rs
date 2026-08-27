//! Working-tree status and staging operations.
//!
//! This module owns Vigil's view of `git status` output: how paths become
//! `FileEntry` values, how staged state is interpreted, and which filesystem
//! changes should refresh the review. It keeps porcelain parsing and git
//! commands behind typed functions so app and UI code do not need to know raw
//! status formats.

mod display;
mod load;
mod refresh;
mod stage;

pub use self::{
    display::status_color,
    load::{
        WorkingTreeStatus, load_files_with_status, load_status_for_path, load_working_tree_status,
    },
    refresh::should_refresh_for_paths,
    stage::{
        discard_file_changes, is_file_fully_staged, is_file_staged, is_untracked_status,
        stage_all_changes, toggle_file_stage, unstage_all_changes,
    },
};
