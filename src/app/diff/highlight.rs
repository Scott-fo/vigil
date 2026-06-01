use tokio::task;

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiffHighlightJobKind {
    Viewport(DiffViewport),
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) struct DiffHighlightJob {
    request_id: u64,
    key: DiffCacheKey,
    kind: DiffHighlightJobKind,
}

impl App {
    pub(super) fn cancel_inflight_diff_highlight(&mut self) {
        if let Some(task) = self.diff_highlight_task.take() {
            task.abort();
        }
        self.diff_highlight_job = None;
    }

    pub(in crate::app) fn maybe_queue_diff_highlight(&mut self) {
        let Some(highlight_registry) = self.highlight_registry.clone() else {
            self.diff_highlight_complete = true;
            return;
        };
        if self.diff_load_task.is_some() {
            return;
        }
        let Some(cache_key) = self.pending_diff_cache_key.clone() else {
            return;
        };
        let Some(file) = self.selected_file().cloned() else {
            self.diff_highlight_complete = true;
            return;
        };
        let Some(_filetype) = file.filetype else {
            self.diff_highlight_complete = true;
            return;
        };
        let Some(viewport) = self.diff_viewport else {
            return;
        };

        let viewport_ready = self.diff_view.is_display_range_fully_highlighted(
            viewport.mode,
            viewport.width,
            viewport.line_wrap,
            viewport.start,
            viewport.end,
        );

        if !viewport_ready {
            let needs_new_viewport_job = !matches!(
                self.diff_highlight_job.as_ref(),
                Some(DiffHighlightJob {
                    request_id,
                    key,
                    kind: DiffHighlightJobKind::Viewport(existing_viewport),
                }) if *request_id == self.diff_request_id
                    && *key == cache_key
                    && *existing_viewport == viewport
            );
            if needs_new_viewport_job {
                self.cancel_inflight_diff_highlight();
                self.spawn_selected_diff_highlight(
                    cache_key,
                    file.clone(),
                    highlight_registry,
                    DiffHighlightJobKind::Viewport(viewport),
                );
            }
            return;
        }

        if self.diff_highlight_complete {
            return;
        }

        if self.active_pane == ActivePane::Sidebar {
            return;
        }

        let full_inflight = matches!(
            self.diff_highlight_job.as_ref(),
            Some(DiffHighlightJob {
                request_id,
                key,
                kind: DiffHighlightJobKind::Full,
            }) if *request_id == self.diff_request_id && *key == cache_key
        );
        if full_inflight {
            return;
        }

        self.cancel_inflight_diff_highlight();
        self.spawn_selected_diff_highlight(
            cache_key,
            file,
            highlight_registry,
            DiffHighlightJobKind::Full,
        );
    }

    fn spawn_selected_diff_highlight(
        &mut self,
        cache_key: DiffCacheKey,
        file: FileEntry,
        highlight_registry: SharedHighlightRegistry,
        kind: DiffHighlightJobKind,
    ) {
        let request_id = self.diff_request_id;
        let sender = self.events.sender();
        let mut diff_view = self.diff_view.clone();
        let review_mode = self.review_mode.clone();
        let repo_root = self.repo_root.clone();
        self.diff_highlight_job = Some(DiffHighlightJob {
            request_id,
            key: cache_key,
            kind: kind.clone(),
        });
        self.diff_highlight_task = Some(task::spawn(async move {
            let (complete, result) = match kind {
                DiffHighlightJobKind::Viewport(viewport) => {
                    if diff_view.has_exact_syntax_context() {
                        let result = task::spawn_blocking(move || {
                            diff_view.apply_exact_syntax_highlighting(
                                file.filetype,
                                highlight_registry.as_ref(),
                            );
                            Ok::<_, String>(diff_view)
                        })
                        .await
                        .unwrap_or_else(|error| Err(error.to_string()));
                        (true, result)
                    } else {
                        match load_exact_highlighted_diff_view(
                            repo_root.clone(),
                            file.clone(),
                            review_mode.clone(),
                            highlight_registry.clone(),
                        )
                        .await
                        {
                            Ok(Some(diff_view)) => (true, Ok(diff_view)),
                            Ok(None) | Err(_) => {
                                let result = task::spawn_blocking(move || {
                                    diff_view.apply_syntax_highlighting_for_display_range(
                                        viewport.mode,
                                        viewport.width,
                                        viewport.line_wrap,
                                        viewport.start,
                                        viewport.end,
                                        file.filetype,
                                        highlight_registry.as_ref(),
                                    );
                                    Ok::<_, String>(diff_view)
                                })
                                .await
                                .unwrap_or_else(|error| Err(error.to_string()));
                                (false, result)
                            }
                        }
                    }
                }
                DiffHighlightJobKind::Full => {
                    let preview_result = match &review_mode {
                        ReviewMode::WorkingTree => {
                            git::load_diff_preview_for_working_tree(&repo_root, &file, true).await
                        }
                        ReviewMode::CommitCompare(selection) => {
                            git::load_diff_preview_for_commit_compare(
                                &repo_root, &file, selection, true,
                            )
                            .await
                        }
                        ReviewMode::BranchCompare(selection) => {
                            git::load_diff_preview_for_branch_compare(
                                &repo_root, &file, selection, true,
                            )
                            .await
                        }
                    };

                    let preview = match preview_result {
                        Ok(preview) => preview,
                        Err(error) => {
                            let _ = sender.send(Event::DiffHighlightUpdated {
                                request_id,
                                complete: true,
                                result: Err(error.to_string()),
                            });
                            return;
                        }
                    };

                    let result = task::spawn_blocking(move || {
                        let mut diff_view =
                            git::build_diff_view_from_preview_data(&preview, &file, None)
                                .map_err(|error| error.to_string())?;
                        diff_view.apply_exact_syntax_highlighting(
                            file.filetype,
                            highlight_registry.as_ref(),
                        );
                        Ok::<_, String>(diff_view)
                    })
                    .await
                    .unwrap_or_else(|error| Err(error.to_string()));
                    (true, result)
                }
            };
            let _ = sender.send(Event::DiffHighlightUpdated {
                request_id,
                complete,
                result,
            });
        }));
    }
}

async fn load_exact_highlighted_diff_view(
    repo_root: std::path::PathBuf,
    file: FileEntry,
    review_mode: ReviewMode,
    highlight_registry: SharedHighlightRegistry,
) -> Result<Option<git::DiffView>, String> {
    let preview_result = match &review_mode {
        ReviewMode::WorkingTree => {
            git::load_diff_preview_for_working_tree(&repo_root, &file, true).await
        }
        ReviewMode::CommitCompare(selection) => {
            git::load_diff_preview_for_commit_compare(&repo_root, &file, selection, true).await
        }
        ReviewMode::BranchCompare(selection) => {
            git::load_diff_preview_for_branch_compare(&repo_root, &file, selection, true).await
        }
    };
    let preview = preview_result.map_err(|error| error.to_string())?;

    task::spawn_blocking(move || {
        let mut diff_view = git::build_diff_view_from_preview_data(&preview, &file, None)
            .map_err(|error| error.to_string())?;
        if !diff_view.has_exact_syntax_context() {
            return Ok(None);
        }
        diff_view.apply_exact_syntax_highlighting(file.filetype, highlight_registry.as_ref());
        Ok(Some(diff_view))
    })
    .await
    .unwrap_or_else(|error| Err(error.to_string()))
}
