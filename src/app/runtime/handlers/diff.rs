use crate::{event::DiffPrefetchedEvent, git};

use super::super::super::App;

impl App {
    pub(in crate::app) fn handle_highlight_registry_ready(
        &mut self,
        result: Result<git::SharedHighlightRegistry, String>,
    ) {
        self.highlight_registry_loading = false;
        match result {
            Ok(registry) => {
                self.highlight_registry = Some(registry);
                self.diff_highlight_complete = false;
                self.status_message = Some(self.current_status_message());
            }
            Err(error) => {
                self.status_message = Some(format!("highlight registry init failed: {error}"));
            }
        }
    }

    pub(in crate::app) fn handle_diff_loaded(
        &mut self,
        request_id: u64,
        result: Result<git::DiffView, String>,
    ) -> bool {
        if request_id != self.diff_request_id {
            return false;
        }

        self.diff_load_task = None;
        match result {
            Ok(mut diff_view) => {
                if let Some(cache_key) = self.pending_diff_cache_key.clone() {
                    self.diff_view_cache
                        .insert_plain(cache_key, diff_view.clone());
                }
                let max_index = diff_view
                    .last_selectable_index(self.diff_view_mode, self.current_diff_display_width());
                self.selected_diff_line_index = self.selected_diff_line_index.min(max_index);
                self.diff_view = diff_view;
                self.diff_highlight_complete = self.highlight_registry.is_none();
                self.status_message = Some(self.current_status_message());
            }
            Err(error) => {
                self.diff_view = git::DiffView::empty(error);
                self.diff_highlight_complete = true;
            }
        }
        true
    }

    pub(in crate::app) fn handle_diff_highlight_updated(
        &mut self,
        request_id: u64,
        complete: bool,
        result: Result<git::DiffView, String>,
    ) -> bool {
        if request_id != self.diff_request_id {
            return false;
        }

        self.diff_highlight_task = None;
        self.diff_highlight_job = None;

        match result {
            Ok(diff_view) => {
                if let Some(cache_key) = self.pending_diff_cache_key.clone() {
                    self.diff_view_cache
                        .insert_highlighted(cache_key, diff_view.clone(), complete);
                }
                if complete {
                    self.diff_view = diff_view;
                    self.diff_highlight_complete = true;
                    self.spawn_diff_prefetch();
                } else {
                    self.diff_view.merge_highlighting_from(&diff_view);
                }
                self.status_message = Some(self.current_status_message());
            }
            Err(error) => {
                self.status_message = Some(format!("syntax highlight failed: {error}"));
            }
        }
        true
    }

    pub(in crate::app) fn handle_diff_prefetched(&mut self, prefetched: DiffPrefetchedEvent) {
        let DiffPrefetchedEvent {
            generation,
            key,
            plain,
            highlighted,
            highlight_complete,
        } = prefetched;
        if generation != self.diff_cache_generation {
            return;
        }

        self.diff_view_cache.insert_plain(key.clone(), plain);
        if let Some(highlighted_view) = highlighted {
            self.diff_view_cache
                .insert_highlighted(key, highlighted_view, highlight_complete);
        }
    }
}
