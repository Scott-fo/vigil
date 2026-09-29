use super::super::App;

impl App {
    pub(in crate::app) fn quit(&mut self) {
        self.cancel_inflight_diff_load();
        self.cancel_inflight_blame_load();
        self.cancel_inflight_diff_prefetch();
        self.cancel_inflight_review_diff_snapshot();
        self.cancel_inflight_review_diff_stats();
        self.cancel_diff_search_tasks();
        self.abort_background_tasks();
        self.repo_watcher = None;
        self.repo_watcher_loading = false;
        self.branch_operation = None;
        self.events.suspend();
        self.running = false;
    }
}
