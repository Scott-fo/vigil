use ratatui::style::Color;
use unicode_width::UnicodeWidthChar;

use super::super::text_muted_color;

pub(super) fn devicon_for_path(path: &str) -> Option<(char, Color)> {
    let icon = devicons::icon_for_file(path, &Some(devicons::Theme::Dark));
    if icon.icon == '*' {
        return None;
    }

    Some((
        icon.icon,
        hex_color(icon.color).unwrap_or_else(text_muted_color),
    ))
}

fn hex_color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }

    let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(red, green, blue))
}

pub(super) fn truncate_middle(value: &str, max_width: usize) -> String {
    if display_width(value) <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let left_width = max_width.saturating_sub(1) / 2;
    let right_width = max_width.saturating_sub(1).saturating_sub(left_width);
    format!(
        "{}…{}",
        take_width_prefix(value, left_width),
        take_width_suffix(value, right_width)
    )
}

fn take_width_prefix(value: &str, max_width: usize) -> String {
    let mut width = 0usize;
    let mut result = String::new();
    for char in value.chars() {
        let char_width = char.width().unwrap_or(0);
        if width.saturating_add(char_width) > max_width {
            break;
        }
        width = width.saturating_add(char_width);
        result.push(char);
    }
    result
}

fn take_width_suffix(value: &str, max_width: usize) -> String {
    let mut width = 0usize;
    let mut chars = Vec::new();
    for char in value.chars().rev() {
        let char_width = char.width().unwrap_or(0);
        if width.saturating_add(char_width) > max_width {
            break;
        }
        width = width.saturating_add(char_width);
        chars.push(char);
    }
    chars.into_iter().rev().collect()
}

pub(super) fn display_width(value: &str) -> usize {
    value
        .chars()
        .map(|char| char.width().unwrap_or(0))
        .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devicon_resolves_known_types_and_skips_default_icon() {
        let tsx = devicon_for_path("src/components/Button.tsx").expect("tsx should have a devicon");
        assert_ne!(tsx.0, '*');

        assert!(devicon_for_path("src/components/Button.unknown-vigil-type").is_none());
    }

    #[test]
    fn parses_devicon_hex_colors() {
        assert_eq!(hex_color("#1354BF"), Some(Color::Rgb(19, 84, 191)));
        assert_eq!(hex_color("1354BF"), None);
        assert_eq!(hex_color("#1354"), None);
    }

    #[test]
    fn truncate_middle_keeps_extension_visible() {
        assert_eq!(
            truncate_middle("JavaScriptSyntaxHighlighter.tsx", 18),
            "JavaScri…ghter.tsx"
        );
        assert_eq!(truncate_middle("short.ts", 18), "short.ts");
        assert_eq!(truncate_middle("short.ts", 1), "…");
    }
}
