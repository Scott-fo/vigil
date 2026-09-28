use crossterm::event::{KeyCode, KeyEvent};

use crate::git::{BranchOperation, BranchSnapshot};

use super::super::{App, input::is_plain_text_key};
use super::BranchPanelMode;

impl App {
    pub(in crate::app) fn handle_branch_panel_key(&mut self, key_event: KeyEvent) -> bool {
        let Some(panel) = self.branch_panel.as_ref() else {
            return false;
        };

        match panel.mode().clone() {
            BranchPanelMode::Browse => self.handle_branch_browse_key(key_event),
            BranchPanelMode::Filter => self.handle_branch_filter_key(key_event),
            BranchPanelMode::Create { start_point, name } => {
                self.handle_branch_name_key(
                    key_event,
                    |name_value| BranchOperation::Create {
                        name: name_value,
                        start_point: start_point.clone(),
                    },
                    &name,
                );
            }
            BranchPanelMode::Rename { branch, name } => {
                self.handle_branch_name_key(
                    key_event,
                    |name_value| BranchOperation::Rename {
                        from: branch.clone(),
                        to: name_value,
                    },
                    &name,
                );
            }
            BranchPanelMode::Delete { branch, force } => match key_event.code {
                KeyCode::Esc => self.set_branch_panel_mode(BranchPanelMode::Browse),
                KeyCode::Enter => {
                    self.run_panel_operation(BranchOperation::Delete { branch, force });
                }
                _ => {}
            },
        }
        true
    }

    fn handle_branch_browse_key(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                let panel = self.branch_panel.as_mut().expect("panel is open");
                if panel.query().is_empty() {
                    self.close_branch_panel();
                } else {
                    panel.query_mut().clear();
                }
            }
            KeyCode::Char('B') => self.close_branch_panel(),
            KeyCode::Down | KeyCode::Char('j') => self.move_branch_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_branch_selection(-1),
            KeyCode::PageDown => self.move_branch_selection(10),
            KeyCode::PageUp => self.move_branch_selection(-10),
            KeyCode::Home | KeyCode::Char('g') => self.move_branch_selection(i32::MIN),
            KeyCode::End | KeyCode::Char('G') => self.move_branch_selection(i32::MAX),
            KeyCode::Char('/') => self.set_branch_panel_mode(BranchPanelMode::Filter),
            KeyCode::Enter | KeyCode::Char(' ') => self.switch_to_selected_branch(),
            KeyCode::Char('n') => self.begin_branch_create(),
            KeyCode::Char('R') => self.begin_branch_rename(),
            KeyCode::Char('d') => self.begin_branch_delete(),
            KeyCode::Char('f') => self.run_panel_operation(BranchOperation::Fetch),
            KeyCode::Char('p') => self.run_panel_operation(BranchOperation::Pull),
            KeyCode::Char('P') => self.run_panel_operation(BranchOperation::Push),
            KeyCode::Char('r') => self.queue_branch_status_load(),
            _ => {}
        }
    }

    fn handle_branch_filter_key(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc => self.set_branch_panel_mode(BranchPanelMode::Browse),
            KeyCode::Enter => self.switch_to_selected_branch(),
            KeyCode::Down => self.move_branch_selection(1),
            KeyCode::Up => self.move_branch_selection(-1),
            KeyCode::Backspace => {
                self.branch_panel_mut().query_mut().pop();
            }
            KeyCode::Char(ch) if is_plain_text_key(key_event) => {
                self.branch_panel_mut().query_mut().push(ch);
            }
            _ => {}
        }
    }

    fn handle_branch_name_key(
        &mut self,
        key_event: KeyEvent,
        operation: impl FnOnce(String) -> BranchOperation,
        name: &str,
    ) {
        match key_event.code {
            KeyCode::Esc => self.set_branch_panel_mode(BranchPanelMode::Browse),
            KeyCode::Enter => self.run_panel_operation(operation(name.to_string())),
            KeyCode::Backspace => {
                let panel = self.branch_panel_mut();
                panel.clear_error();
                if let Some(name) = panel.name_mut() {
                    name.pop();
                }
            }
            // Spaces are never valid in branch names; typing one reads as a
            // word break, so it becomes a dash.
            KeyCode::Char(ch) if is_plain_text_key(key_event) => {
                let panel = self.branch_panel_mut();
                panel.clear_error();
                if let Some(name) = panel.name_mut() {
                    name.push(if ch == ' ' { '-' } else { ch });
                }
            }
            _ => {}
        }
    }

    fn switch_to_selected_branch(&mut self) {
        let Some((snapshot, panel)) = self.branch_panel_parts() else {
            return;
        };
        let Some(branch) = panel.selected_branch(snapshot) else {
            return;
        };

        let operation = if branch.is_head {
            panel.set_error(format!("already on {}", branch.name));
            return;
        } else if !branch.is_remote() {
            BranchOperation::Switch {
                branch: branch.name.clone(),
            }
        } else if snapshot.local_branch(branch.local_name()).is_some() {
            BranchOperation::Switch {
                branch: branch.local_name().to_string(),
            }
        } else {
            BranchOperation::Track {
                remote_branch: branch.name.clone(),
                local_name: branch.local_name().to_string(),
            }
        };
        self.run_panel_operation(operation);
    }

    fn begin_branch_create(&mut self) {
        let Some((snapshot, panel)) = self.branch_panel_parts() else {
            return;
        };
        let start_point = panel
            .selected_branch(snapshot)
            .map(|branch| branch.name.clone())
            .unwrap_or_else(|| "HEAD".to_string());
        panel.set_mode(BranchPanelMode::Create {
            start_point,
            name: String::new(),
        });
    }

    fn begin_branch_rename(&mut self) {
        let Some((snapshot, panel)) = self.branch_panel_parts() else {
            return;
        };
        match panel.selected_branch(snapshot) {
            Some(branch) if branch.is_remote() => {
                panel.set_error("remote branches can't be renamed here");
            }
            Some(branch) => panel.set_mode(BranchPanelMode::Rename {
                branch: branch.name.clone(),
                name: branch.name.clone(),
            }),
            None => {}
        }
    }

    fn begin_branch_delete(&mut self) {
        let Some((snapshot, panel)) = self.branch_panel_parts() else {
            return;
        };
        match panel.selected_branch(snapshot) {
            Some(branch) if branch.is_remote() => {
                panel.set_error("remote branches can't be deleted here");
            }
            Some(branch) if branch.is_head => {
                panel.set_error("can't delete the checked-out branch");
            }
            Some(branch) => panel.set_mode(BranchPanelMode::Delete {
                branch: branch.name.clone(),
                force: false,
            }),
            None => {}
        }
    }

    fn move_branch_selection(&mut self, delta: i32) {
        if let Some((snapshot, panel)) = self.branch_panel_parts() {
            panel.move_selection(snapshot, delta);
        }
    }

    /// Starts `operation` from the panel. The panel returns to browsing; a
    /// refusal because another operation is running is shown in the panel.
    fn run_panel_operation(&mut self, operation: BranchOperation) {
        if self.branch_operation.is_some() {
            self.branch_panel_mut()
                .set_error("wait for the current git operation to finish");
            return;
        }
        self.set_branch_panel_mode(BranchPanelMode::Browse);
        self.start_branch_operation(operation);
    }

    fn set_branch_panel_mode(&mut self, mode: BranchPanelMode) {
        self.branch_panel_mut().set_mode(mode);
    }

    fn branch_panel_mut(&mut self) -> &mut super::BranchPanel {
        self.branch_panel
            .as_mut()
            .expect("branch panel keys are only handled while it is open")
    }

    /// The loaded snapshot and the open panel, borrowed together.
    fn branch_panel_parts(&mut self) -> Option<(&BranchSnapshot, &mut super::BranchPanel)> {
        Some((self.branch_status.snapshot()?, self.branch_panel.as_mut()?))
    }
}
