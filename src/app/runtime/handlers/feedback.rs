use super::super::super::App;

impl App {
    pub(in crate::app) fn handle_clear_snackbar(&mut self, generation: u64) -> bool {
        if self.snackbar_generation != generation {
            return false;
        }

        self.snackbar_notice = None;
        true
    }
}
