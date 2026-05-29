use ratatui::style::{Color, Modifier, Style};

use crate::theme;

mod syntax;

pub use self::syntax::syntax_style;

#[inline]
fn palette() -> theme::ThemePalette {
    theme::active_palette()
}

#[inline]
pub(super) fn background_color() -> Color {
    palette().background
}

#[inline]
pub(super) fn panel_color() -> Color {
    palette().background_panel
}

#[inline]
pub(super) fn element_color() -> Color {
    palette().background_element
}

#[inline]
pub(super) fn border_color() -> Color {
    palette().border
}

#[inline]
pub(super) fn border_active_color() -> Color {
    palette().border_active
}

#[inline]
pub(super) fn text_color() -> Color {
    palette().text
}

#[inline]
pub(super) fn text_muted_color() -> Color {
    palette().text_muted
}

#[inline]
pub(super) fn primary_color() -> Color {
    palette().primary
}

#[inline]
pub(super) fn error_color() -> Color {
    palette().error
}

#[inline]
pub(super) fn warning_color() -> Color {
    palette().warning
}

#[inline]
pub(super) fn success_color() -> Color {
    palette().success
}

#[inline]
pub(super) fn diff_context_color() -> Color {
    palette().diff_context
}

#[inline]
pub(super) fn add_bg_color() -> Color {
    palette().diff_added_bg
}

#[inline]
pub(super) fn remove_bg_color() -> Color {
    palette().diff_removed_bg
}

#[inline]
pub(super) fn selected_list_item_text_color() -> Color {
    palette().selected_list_item_text
}

#[inline]
pub fn diff_meta_style() -> Style {
    Style::new().fg(diff_context_color())
}

#[inline]
pub fn diff_hunk_style() -> Style {
    Style::new()
        .fg(palette().diff_hunk_header)
        .add_modifier(Modifier::BOLD)
}

#[inline]
pub fn diff_context_style() -> Style {
    Style::new().fg(text_color())
}

#[inline]
pub fn diff_added_style() -> Style {
    Style::new().fg(text_color()).bg(add_bg_color())
}

#[inline]
pub fn diff_removed_style() -> Style {
    Style::new().fg(text_color()).bg(remove_bg_color())
}

#[inline]
pub fn line_number_style() -> Style {
    Style::new().fg(palette().diff_line_number)
}

#[inline]
pub fn added_sign_style() -> Style {
    Style::new()
        .fg(palette().diff_highlight_added)
        .bg(add_bg_color())
        .add_modifier(Modifier::BOLD)
}

#[inline]
pub fn removed_sign_style() -> Style {
    Style::new()
        .fg(palette().diff_highlight_removed)
        .bg(remove_bg_color())
        .add_modifier(Modifier::BOLD)
}

#[inline]
pub fn context_sign_style() -> Style {
    Style::new().fg(diff_context_color())
}
