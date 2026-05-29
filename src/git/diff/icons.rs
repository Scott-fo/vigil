//! Diff change kinds and icon names.
//!
//! This module keeps file-level change classification and its UI icon mapping
//! together. Parsing and rendering code consume the typed change kind rather
//! than repeating string/icon conventions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeType {
    Change,
    RenamePure,
    RenameChanged,
    New,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiffIconType {
    File,
    Change,
    RenamePure,
    RenameChanged,
    New,
    Deleted,
}

impl From<ChangeType> for DiffIconType {
    fn from(change_type: ChangeType) -> Self {
        match change_type {
            ChangeType::Change => Self::Change,
            ChangeType::RenamePure => Self::RenamePure,
            ChangeType::RenameChanged => Self::RenameChanged,
            ChangeType::New => Self::New,
            ChangeType::Deleted => Self::Deleted,
        }
    }
}

pub fn get_icon_for_type(icon_type: DiffIconType) -> &'static str {
    match icon_type {
        DiffIconType::File => "diffs-icon-file-code",
        DiffIconType::Change => "diffs-icon-symbol-modified",
        DiffIconType::New => "diffs-icon-symbol-added",
        DiffIconType::Deleted => "diffs-icon-symbol-deleted",
        DiffIconType::RenamePure | DiffIconType::RenameChanged => "diffs-icon-symbol-moved",
    }
}
