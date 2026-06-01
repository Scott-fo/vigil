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
            Ok(diff_view) => {
                if let Some(cache_key) = self.pending_diff_cache_key.clone() {
                    self.diff_view_cache
                        .insert_plain(cache_key, diff_view.clone());
                }
                self.diff_view = diff_view;
                self.apply_pending_diff_search_target();
                self.diff_highlight_complete = self.highlight_registry.is_none();
                self.status_message = Some(self.current_status_message());
                self.spawn_diff_prefetch();
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

    pub(in crate::app) fn handle_diff_prefetched(
        &mut self,
        prefetched: DiffPrefetchedEvent,
    ) -> bool {
        let DiffPrefetchedEvent {
            generation,
            key,
            plain,
            highlighted,
            highlight_complete,
        } = prefetched;
        if generation != self.diff_cache_generation {
            return false;
        }

        self.diff_view_cache.insert_plain(key.clone(), plain);
        if let Some(highlighted_view) = highlighted {
            self.diff_view_cache.insert_highlighted(
                key.clone(),
                highlighted_view,
                highlight_complete,
            );
        }

        if self.pending_diff_cache_key.as_ref() != Some(&key) {
            return false;
        }

        if let Some(task) = self.diff_load_task.take() {
            task.abort();
        }

        let loaded = if let Some((diff_view, complete)) = self.diff_view_cache.get_highlighted(&key)
        {
            Some((diff_view, complete))
        } else {
            self.diff_view_cache
                .get_plain(&key)
                .map(|diff_view| (diff_view, self.highlight_registry.is_none()))
        };

        let Some((diff_view, highlight_complete)) = loaded else {
            return false;
        };

        self.diff_view = diff_view;
        self.apply_pending_diff_search_target();
        self.diff_highlight_complete = highlight_complete;
        self.status_message = Some(self.current_status_message());
        true
    }
}
