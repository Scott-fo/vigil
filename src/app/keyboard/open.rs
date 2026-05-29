use crossterm::event::KeyCode;

use super::super::{ActivePane, App, editor::AppCommand};

impl App {
    pub(super) fn handle_open_key(&mut self, key_code: KeyCode) -> Option<AppCommand> {
        if self.active_pane == ActivePane::Diff
            && matches!(key_code, KeyCode::Enter)
            && self
                .diff_view
                .selected_gap_action(
                    self.diff_view_mode,
                    self.current_diff_display_width(),
                    self.selected_diff_line_index,
                )
                .is_some()
        {
            self.selected_diff_line_index = self.diff_view.expand_selected_gap(
                self.diff_view_mode,
                self.current_diff_display_width(),
                self.selected_diff_line_index,
                20,
            );
            return None;
        }

        let Some(file_path) = self.selected_file().map(|file| file.path.clone()) else {
            if self.active_pane == ActivePane::Sidebar {
                let _ = self.toggle_focused_sidebar_directory();
            }
            return None;
        };

        match self.active_pane {
            ActivePane::Diff => Some(self.diff_pane_open_command(file_path)),
            ActivePane::Sidebar => {
                if self.toggle_focused_sidebar_directory() {
                    None
                } else {
                    Some(AppCommand::OpenFileInEditor(file_path))
                }
            }
        }
    }

    fn diff_pane_open_command(&mut self, file_path: String) -> AppCommand {
        let line_number = if self.is_working_tree_mode() {
            self.diff_view.selected_line_number(
                self.diff_view_mode,
                self.current_diff_display_width(),
                self.selected_diff_line_index,
            )
        } else {
            self.diff_view.selected_new_line_number(
                self.diff_view_mode,
                self.current_diff_display_width(),
                self.selected_diff_line_index,
            )
        };

        match line_number {
            Some(line_number) => AppCommand::OpenFileInEditorAtLine(file_path, line_number),
            None => AppCommand::OpenFileInEditor(file_path),
        }
    }
}
