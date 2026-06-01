use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};

use crate::{
    app::{ActivePane, App, PreparedDiffViewport},
    git::{DiffSelectionPane, DiffSelectionPoint},
};

use super::super::{
    layout::main_layout,
    panel::{bordered_panel, diff_pane_label},
};

pub fn diff_gap_click_at(
    app: &mut App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<usize> {
    let (body_area, display_index) = diff_body_hit(
        app,
        mouse_column,
        mouse_row,
        terminal_width,
        terminal_height,
    )?;

    app.diff_view.selected_gap_action(
        app.diff_view_mode,
        body_area.width as usize,
        app.diff_line_wrap_mode,
        display_index,
    )?;

    Some(display_index)
}

pub fn diff_selection_point_at(
    app: &mut App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<DiffSelectionPoint> {
    let (body_area, display_index) = diff_body_hit(
        app,
        mouse_column,
        mouse_row,
        terminal_width,
        terminal_height,
    )?;
    let relative_column = mouse_column.saturating_sub(body_area.x) as usize;
    app.diff_view.selection_point_at(
        app.diff_view_mode,
        body_area.width as usize,
        app.diff_line_wrap_mode,
        display_index,
        relative_column,
    )
}

pub fn diff_selection_drag_point_at(
    app: &mut App,
    anchor_pane: DiffSelectionPane,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<DiffSelectionPoint> {
    let (body_area, display_index) = diff_body_clamped_hit(
        app,
        mouse_column,
        mouse_row,
        terminal_width,
        terminal_height,
    )?;
    let relative_column = mouse_column.saturating_sub(body_area.x) as usize;
    app.diff_view.selection_point_for_pane(
        app.diff_view_mode,
        body_area.width as usize,
        app.diff_line_wrap_mode,
        display_index,
        anchor_pane,
        relative_column,
    )
}

pub fn prepare_diff_viewport_for_terminal(
    app: &mut App,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<PreparedDiffViewport> {
    let (body_area, _) = diff_body_state(app, terminal_width, terminal_height)?;
    app.prepare_diff_viewport(
        app.diff_view_mode,
        body_area.width as usize,
        body_area.height as usize,
    )
}

fn diff_body_hit(
    app: &mut App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<(Rect, usize)> {
    let (body_area, viewport) = diff_body_state(app, terminal_width, terminal_height)?;
    let point = Position::new(mouse_column, mouse_row);
    if !body_area.contains(point) {
        return None;
    }

    let relative_row = mouse_row.saturating_sub(body_area.y) as usize;
    let display_index = viewport
        .visible_display_indices
        .get(relative_row)
        .copied()
        .flatten()?;
    Some((body_area, display_index))
}

fn diff_body_clamped_hit(
    app: &mut App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<(Rect, usize)> {
    let (body_area, viewport) = diff_body_state(app, terminal_width, terminal_height)?;
    if viewport.start >= viewport.end {
        return None;
    }

    let clamped_row = mouse_row.clamp(
        body_area.y,
        body_area
            .y
            .saturating_add(body_area.height.saturating_sub(1)),
    );
    let relative_row = clamped_row.saturating_sub(body_area.y) as usize;
    let display_index = nearest_display_index(&viewport, relative_row)?;
    let _ = mouse_column;
    Some((body_area, display_index))
}

fn nearest_display_index(viewport: &PreparedDiffViewport, row: usize) -> Option<usize> {
    let len = viewport.visible_display_indices.len();
    if len == 0 {
        return None;
    }

    let row = row.min(len.saturating_sub(1));
    if let Some(display_index) = viewport.visible_display_indices[row] {
        return Some(display_index);
    }

    for offset in 1..len {
        if let Some(previous) = row
            .checked_sub(offset)
            .and_then(|index| viewport.visible_display_indices[index])
        {
            return Some(previous);
        }
        let next_index = row.saturating_add(offset);
        if next_index < len
            && let Some(next) = viewport.visible_display_indices[next_index]
        {
            return Some(next);
        }
    }

    None
}

fn diff_body_state(
    app: &mut App,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<(Rect, PreparedDiffViewport)> {
    if app.show_splash() {
        return None;
    }

    let [_, diff_area] = main_layout(
        Rect::new(0, 0, terminal_width, terminal_height),
        app.sidebar_hidden,
    );
    let body_area = diff_body_area(app, diff_area);
    let viewport = app.prepare_diff_viewport(
        app.diff_view_mode,
        body_area.width as usize,
        body_area.height as usize,
    )?;
    Some((body_area, viewport))
}

fn diff_body_area(app: &App, diff_area: Rect) -> Rect {
    let title = app
        .files
        .get(app.selected_file_index)
        .map(|file| file.label.clone())
        .unwrap_or_else(|| "No file selected".to_string());
    let mode_label = app.review_mode_label();
    let block = bordered_panel(
        &title,
        app.active_pane == ActivePane::Diff,
        Some(if mode_label.is_empty() {
            diff_pane_label(app)
        } else {
            format!("{}  {mode_label}", diff_pane_label(app))
        }),
    );
    let inner = block.inner(diff_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    chunks[0]
}
