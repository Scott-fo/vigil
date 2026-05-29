mod blame;
mod branch_compare;
mod branch_merge;
mod commit;
mod commit_search;
mod discard;
mod file_search;
mod frame;
mod help;
mod list;
mod theme;
mod worktree;

use ratatui::Frame;

use crate::app::App;

use self::{
    blame::render_blame_modal, branch_compare::render_branch_compare_modal,
    branch_merge::render_branch_merge_modal, commit::render_commit_modal,
    commit_search::render_commit_search_modal, discard::render_discard_modal,
    file_search::render_file_search_modal, help::render_help_modal, theme::render_theme_modal,
    worktree::render_worktree_modal,
};

pub(super) fn render_modals(frame: &mut Frame, app: &mut App) {
    if app.commit_modal_open {
        render_commit_modal(frame, app);
    }

    if app.discard_target.is_some() {
        render_discard_modal(frame, app);
    }

    if app.theme_modal_open {
        render_theme_modal(frame, app);
    }

    if app.file_search_modal_open {
        render_file_search_modal(frame, app);
    }

    if app.commit_search_modal_open {
        render_commit_search_modal(frame, app);
    }

    if app.branch_compare_modal_open {
        render_branch_compare_modal(frame, app);
    }

    if app.branch_merge_target.is_some() {
        render_branch_merge_modal(frame, app);
    }

    if app.worktree_modal_open {
        render_worktree_modal(frame, app);
    }

    if app.blame_modal_open {
        render_blame_modal(frame, app);
    }

    if app.help_modal_open {
        render_help_modal(frame, app);
    }
}
