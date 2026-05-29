use tokio::task;

use super::*;

impl App {
    pub(super) fn build_diff_cache_key(review_mode: &ReviewMode, file: &FileEntry) -> DiffCacheKey {
        let review_scope = match review_mode {
            ReviewMode::WorkingTree => "working-tree".to_string(),
            ReviewMode::CommitCompare(selection) => {
                format!("commit:{}:{}", selection.base_ref, selection.commit_hash)
            }
            ReviewMode::BranchCompare(selection) => format!(
                "branch:{}:{}",
                selection.source_ref, selection.destination_ref
            ),
        };

        DiffCacheKey {
            review_scope,
            file_path: file.path.clone(),
            file_status: file.status.clone(),
        }
    }

    fn diff_cache_key(&self, file: &FileEntry) -> DiffCacheKey {
        Self::build_diff_cache_key(&self.review_mode, file)
    }

    pub(in crate::app) fn spawn_diff_prefetch(&mut self) {
        let Some(selected_visible_index) = self.selected_visible_file_index() else {
            return;
        };

        let visible_paths = self.visible_file_paths();
        if visible_paths.is_empty() {
            return;
        }

        let mut prefetch_files = Vec::new();
        for distance in 1..=DIFF_PREFETCH_DISTANCE {
            for candidate_index in [
                selected_visible_index.checked_sub(distance),
                selected_visible_index.checked_add(distance),
            ] {
                let Some(candidate_index) = candidate_index else {
                    continue;
                };
                let Some(path) = visible_paths.get(candidate_index) else {
                    continue;
                };
                let Some(file_index) = self.file_index_by_path(path) else {
                    continue;
                };
                let file = self.files[file_index].clone();
                let cache_key = self.diff_cache_key(&file);
                let should_prefetch_highlight = self.highlight_registry.is_some()
                    && file.filetype.is_some()
                    && !self.diff_view_cache.has_complete_highlight(&cache_key);
                if !should_prefetch_highlight && self.diff_view_cache.has_plain(&cache_key) {
                    continue;
                }
                prefetch_files.push((cache_key, file, should_prefetch_highlight));
            }
        }

        if prefetch_files.is_empty() {
            return;
        }

        let generation = self.diff_cache_generation;
        let review_mode = self.review_mode.clone();
        let repo_root = self.repo_root.clone();
        let highlight_registry = self.highlight_registry.clone();
        let sender = self.events.sender();

        self.track_background_task(task::spawn(async move {
            for (cache_key, file, should_prefetch_highlight) in prefetch_files {
                let include_exact_context = should_prefetch_highlight;
                let preview_result = match &review_mode {
                    ReviewMode::WorkingTree => {
                        git::load_diff_preview_for_working_tree(
                            &repo_root,
                            &file,
                            include_exact_context,
                        )
                        .await
                    }
                    ReviewMode::CommitCompare(selection) => {
                        git::load_diff_preview_for_commit_compare(
                            &repo_root,
                            &file,
                            selection,
                            include_exact_context,
                        )
                        .await
                    }
                    ReviewMode::BranchCompare(selection) => {
                        git::load_diff_preview_for_branch_compare(
                            &repo_root,
                            &file,
                            selection,
                            include_exact_context,
                        )
                        .await
                    }
                };

                let Ok(preview) = preview_result else {
                    continue;
                };

                let registry = highlight_registry.clone();
                let build_result = task::spawn_blocking(move || {
                    let plain = git::build_diff_view_from_preview_data(&preview, &file, None)?;
                    let highlighted = if should_prefetch_highlight {
                        registry.map(|registry| {
                            let mut highlighted = plain.clone();
                            highlighted
                                .apply_exact_syntax_highlighting(file.filetype, registry.as_ref());
                            highlighted
                        })
                    } else {
                        None
                    };
                    Ok::<_, color_eyre::Report>((plain, highlighted))
                })
                .await;

                let Ok(Ok((plain_view, highlighted_view))) = build_result else {
                    continue;
                };
                let highlight_complete = highlighted_view.is_some();

                let _ = sender.send(Event::DiffPrefetched(Box::new(DiffPrefetchedEvent {
                    generation,
                    key: cache_key,
                    plain: plain_view,
                    highlighted: highlighted_view,
                    highlight_complete,
                })));
            }
        }));
    }

    pub(in crate::app) fn queue_selected_diff_load(
        &mut self,
        show_loading: bool,
        reset_viewport: bool,
    ) {
        self.cancel_inflight_diff_load();
        self.clear_diff_text_selection();
        self.diff_request_id = self.diff_request_id.saturating_add(1);
        let request_id = self.diff_request_id;
        self.diff_highlight_complete = false;
        self.pending_diff_cache_key = None;

        if reset_viewport {
            self.diff_scroll = 0;
            self.selected_diff_line_index = 0;
        }

        let Some(file) = self.selected_file().cloned() else {
            self.diff_view = DiffView::empty("No changed files found.");
            return;
        };

        self.spawn_diff_prefetch();

        let cache_key = self.diff_cache_key(&file);
        self.pending_diff_cache_key = Some(cache_key.clone());
        if let Some((mut diff_view, highlight_complete)) =
            self.diff_view_cache.get_highlighted(&cache_key)
        {
            let max_index = diff_view
                .last_selectable_index(self.diff_view_mode, self.current_diff_display_width());
            self.selected_diff_line_index = self.selected_diff_line_index.min(max_index);
            self.diff_view = diff_view;
            self.diff_highlight_complete = highlight_complete;
            self.status_message = Some(self.current_status_message());
            return;
        }

        if let Some(mut plain_diff_view) = self.diff_view_cache.get_plain(&cache_key) {
            let max_index = plain_diff_view
                .last_selectable_index(self.diff_view_mode, self.current_diff_display_width());
            self.selected_diff_line_index = self.selected_diff_line_index.min(max_index);
            self.diff_view = plain_diff_view;
            self.status_message = Some(self.current_status_message());
            return;
        }

        if show_loading {
            self.diff_view = DiffView::empty("Loading diff...");
        }

        let review_mode = self.review_mode.clone();
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        let plain_file = file.clone();

        self.diff_load_task = Some(task::spawn(async move {
            let preview_result = match &review_mode {
                ReviewMode::WorkingTree => {
                    git::load_diff_preview_for_working_tree(&repo_root, &file, false).await
                }
                ReviewMode::CommitCompare(selection) => {
                    git::load_diff_preview_for_commit_compare(&repo_root, &file, selection, false)
                        .await
                }
                ReviewMode::BranchCompare(selection) => {
                    git::load_diff_preview_for_branch_compare(&repo_root, &file, selection, false)
                        .await
                }
            };

            let preview = match preview_result {
                Ok(preview) => preview,
                Err(error) => {
                    let _ = sender.send(Event::DiffLoaded {
                        request_id,
                        result: Err(error.to_string()),
                    });
                    return;
                }
            };

            let plain_result = task::spawn_blocking(move || {
                git::build_diff_view_from_preview_data(&preview, &plain_file, None)
                    .map_err(|error| error.to_string())
            })
            .await
            .unwrap_or_else(|error| Err(error.to_string()));

            let plain_diff_view = match plain_result {
                Ok(diff_view) => {
                    let _ = sender.send(Event::DiffLoaded {
                        request_id,
                        result: Ok(diff_view.clone()),
                    });
                    diff_view
                }
                Err(error) => {
                    let _ = sender.send(Event::DiffLoaded {
                        request_id,
                        result: Err(error),
                    });
                    return;
                }
            };
            let _ = plain_diff_view;
        }));
    }

    pub(in crate::app) fn cancel_inflight_diff_load(&mut self) {
        if let Some(task) = self.diff_load_task.take() {
            task.abort();
        }
        self.cancel_inflight_diff_highlight();
        self.pending_diff_cache_key = None;
        self.diff_highlight_complete = false;
    }
}
