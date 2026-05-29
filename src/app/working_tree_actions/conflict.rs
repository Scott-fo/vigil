use color_eyre::eyre::{WrapErr, eyre};
use tokio::fs;

use crate::git;

use super::{
    super::{ActivePane, App},
    merge_conflict_resolution_label,
};

impl App {
    pub(in crate::app) async fn resolve_selected_merge_conflict(
        &mut self,
        resolution: git::MergeConflictResolution,
    ) -> color_eyre::Result<()> {
        if !self.is_working_tree_mode() {
            self.status_message =
                Some("merge conflict resolution is unavailable in compare mode".to_string());
            return Ok(());
        }

        if self.active_pane != ActivePane::Diff {
            self.status_message = Some("focus the diff pane to resolve a conflict".to_string());
            return Ok(());
        }

        let Some(file) = self.selected_file().cloned() else {
            return Ok(());
        };
        let Some(conflict_index) = self.diff_view.selected_conflict_index(
            self.diff_view_mode,
            self.current_diff_display_width(),
            self.selected_diff_line_index,
        ) else {
            self.status_message = Some("selected row is not inside a merge conflict".to_string());
            return Ok(());
        };

        let full_path = self.repo_root.join(&file.path);
        let contents = fs::read_to_string(&full_path)
            .await
            .wrap_err_with(|| format!("failed to read {}", full_path.display()))?;
        let parsed = git::parse_merge_conflict_diff_from_file(
            &git::FileContents {
                name: file.path.clone(),
                contents: contents.clone(),
                lang: file.filetype.map(str::to_string),
                header: None,
                cache_key: Some(format!("{}:{}:merge-conflict", file.path, file.status)),
            },
            6,
        )?;
        let action = parsed
            .actions
            .iter()
            .flatten()
            .find(|action| action.conflict_index == conflict_index)
            .ok_or_else(|| {
                eyre!(
                    "failed to locate merge conflict action {} for {}",
                    conflict_index,
                    file.path
                )
            })?;
        let resolved_contents =
            git::resolve_merge_conflict_contents(&contents, &action.conflict, resolution);

        fs::write(&full_path, resolved_contents)
            .await
            .wrap_err_with(|| format!("failed to write {}", full_path.display()))?;

        self.refresh_working_tree_file(&file.path).await?;
        self.active_pane = ActivePane::Diff;
        self.status_message = Some(format!(
            "resolved conflict {} in {} using {}",
            conflict_index.saturating_add(1),
            file.path,
            merge_conflict_resolution_label(resolution)
        ));
        Ok(())
    }
}
