use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSnapshot {
    pub id: String,
    pub repo_root: PathBuf,
    pub worktree_root: PathBuf,
    pub head_sha: Option<String>,
    pub branch: Option<String>,
    pub scope: ReviewScope,
    pub files: Vec<String>,
    #[serde(default)]
    pub extra_context: String,
    pub patch: String,
    pub created_at_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ReviewScope {
    WorkingTree,
    CommitCompare {
        base_ref: String,
        base_sha: Option<String>,
        commit_hash: String,
        short_hash: String,
        subject: String,
    },
    BranchCompare {
        source_ref: String,
        source_sha: Option<String>,
        destination_ref: String,
        destination_sha: Option<String>,
        merge_base: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewTarget {
    pub snapshot: ReviewSnapshot,
    pub instructions: String,
}

impl ReviewScope {
    pub fn label(&self) -> String {
        match self {
            Self::WorkingTree => "working tree".to_string(),
            Self::CommitCompare {
                short_hash,
                subject,
                ..
            } => {
                format!("commit {short_hash}: {subject}")
            }
            Self::BranchCompare {
                source_ref,
                destination_ref,
                ..
            } => {
                format!("{source_ref} -> {destination_ref}")
            }
        }
    }
}
