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

// Layout surfaces and text tiers.
//
// Every region sits on the theme's main `background`, which is also the
// terminal background in matching terminal themes (catppuccin's "base"), so
// vigil reads as one continuous surface. Regions are separated by thin rules
// rather than background shifts. Text tiers and tints are blended from the
// theme's own colors so they read the same in every palette, dark or light.

/// Background of every region: diff, sidebar, header, and footer.
#[inline]
pub(super) fn surface_color() -> Color {
    palette().background
}

/// Thin rules between regions, such as the sidebar divider.
#[inline]
pub(super) fn rule_color() -> Color {
    let palette = palette();
    mix(palette.background, palette.text, 0.16)
}

/// Background of the selected sidebar row and the diff cursor line.
#[inline]
pub(super) fn selection_color() -> Color {
    let palette = palette();
    mix(palette.background, palette.primary, 0.24)
}

/// Background for inline keycaps and chips.
#[inline]
pub(super) fn chip_color() -> Color {
    let palette = palette();
    mix(palette.background, palette.text, 0.11)
}

/// Secondary text: directory names, labels, hints.
#[inline]
pub(super) fn text_subtle_color() -> Color {
    let palette = palette();
    mix(palette.background, palette.text, 0.62)
}

/// Tertiary text: tree guides, separators, disabled hints.
#[inline]
pub(super) fn text_faint_color() -> Color {
    let palette = palette();
    mix(palette.background, palette.text, 0.36)
}

fn mix(base: Color, over: Color, amount: f32) -> Color {
    let (Color::Rgb(br, bg, bb), Color::Rgb(or, og, ob)) = (base, over) else {
        return base;
    };
    let blend = |from: u8, to: u8| -> u8 {
        (f32::from(from) + (f32::from(to) - f32::from(from)) * amount).round() as u8
    };
    Color::Rgb(blend(br, or), blend(bg, og), blend(bb, ob))
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

/// Background for the changed words inside an added line. Blends the line
/// background toward the theme's addition accent so it reads as "more added".
#[inline]
pub fn diff_added_emphasis_style() -> Style {
    let palette = palette();
    Style::new().bg(mix(
        palette.diff_added_bg,
        palette.diff_highlight_added,
        0.28,
    ))
}

/// Background for the changed words inside a removed line.
#[inline]
pub fn diff_removed_emphasis_style() -> Style {
    let palette = palette();
    Style::new().bg(mix(
        palette.diff_removed_bg,
        palette.diff_highlight_removed,
        0.28,
    ))
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

/// Collapsed-context band between hunks.
#[inline]
pub fn diff_gap_style() -> Style {
    Style::new().fg(text_subtle_color())
}

/// Dotted rule filling the rest of a collapsed-context row.
#[inline]
pub fn diff_gap_rule_style() -> Style {
    Style::new().fg(rule_color())
}

/// Expand arrow inside the collapsed-context band.
#[inline]
pub fn diff_gap_action_style() -> Style {
    diff_gap_style()
        .fg(palette().diff_hunk_header)
        .add_modifier(Modifier::BOLD)
}

#[inline]
pub fn context_sign_style() -> Style {
    Style::new().fg(diff_context_color())
}
