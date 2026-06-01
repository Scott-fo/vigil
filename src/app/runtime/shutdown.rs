use super::super::App;

impl App {
    pub(in crate::app) fn quit(&mut self) {
        self.cancel_inflight_diff_load();
        self.cancel_inflight_blame_load();
        self.cancel_inflight_diff_prefetch();
        self.cancel_diff_search_tasks();
        self.cancel_inflight_review();
        self.abort_background_tasks();
        self.repo_watcher = None;
        self.repo_watcher_loading = false;
        self.remote_sync = None;
        self.events.suspend();
        self.running = false;
    }
}
