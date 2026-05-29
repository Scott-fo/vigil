use std::path::PathBuf;

use color_eyre::eyre::WrapErr;
use tokio::fs;

use crate::git;

use super::{
    super::{App, ReviewMode},
    snapshot::revision_snapshot_path,
};

pub(super) struct EditorOpenTarget {
    pub(super) full_path: PathBuf,
    pub(super) display_path: String,
    pub(super) line_number: Option<usize>,
}

impl App {
    pub(super) async fn resolve_editor_open_target(
        &self,
        file_path: &str,
        line_number: Option<usize>,
    ) -> color_eyre::Result<EditorOpenTarget> {
        match &self.review_mode {
            ReviewMode::WorkingTree => Ok(self.worktree_editor_open_target(file_path, line_number)),
            ReviewMode::CommitCompare(selection) => {
                self.snapshot_revision_editor_open_target(
                    selection.commit_hash.as_str(),
                    file_path,
                    line_number,
                )
                .await
            }
            ReviewMode::BranchCompare(selection) => {
                self.branch_compare_editor_open_target(
                    selection.source_ref.as_str(),
                    file_path,
                    line_number,
                )
                .await
            }
        }
    }

    fn worktree_editor_open_target(
        &self,
        file_path: &str,
        line_number: Option<usize>,
    ) -> EditorOpenTarget {
        EditorOpenTarget {
            full_path: self.repo_root.join(file_path),
            display_path: file_path.to_string(),
            line_number,
        }
    }

    async fn branch_compare_editor_open_target(
        &self,
        source_ref: &str,
        file_path: &str,
        line_number: Option<usize>,
    ) -> color_eyre::Result<EditorOpenTarget> {
        if git::revision_matches_head(&self.repo_root, source_ref)
            .await
            .unwrap_or(false)
            && fs::metadata(self.repo_root.join(file_path)).await.is_ok()
        {
            return Ok(self.worktree_editor_open_target(file_path, line_number));
        }

        self.snapshot_revision_editor_open_target(source_ref, file_path, line_number)
            .await
    }

    async fn snapshot_revision_editor_open_target(
        &self,
        revision: &str,
        file_path: &str,
        line_number: Option<usize>,
    ) -> color_eyre::Result<EditorOpenTarget> {
        let full_path = self.materialize_revision_file(revision, file_path).await?;
        Ok(EditorOpenTarget {
            full_path,
            display_path: format!("{revision}:{file_path}"),
            line_number,
        })
    }

    async fn materialize_revision_file(
        &self,
        revision: &str,
        file_path: &str,
    ) -> color_eyre::Result<PathBuf> {
        let bytes = git::load_revision_file_bytes(&self.repo_root, revision, file_path).await?;
        let snapshot_path = revision_snapshot_path(&self.repo_root, revision, file_path)?;
        if let Some(parent) = snapshot_path.parent() {
            fs::create_dir_all(parent).await.wrap_err_with(|| {
                format!("failed to create snapshot directory {}", parent.display())
            })?;
        }
        fs::write(&snapshot_path, bytes).await.wrap_err_with(|| {
            format!(
                "failed to write revision snapshot {}",
                snapshot_path.display()
            )
        })?;
        Ok(snapshot_path)
    }
}
