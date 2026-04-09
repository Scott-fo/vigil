use super::{ThemeMode, TuiPreference};

use std::{env, fs, io, path::Path, path::PathBuf};

pub fn read_tui_preference() -> TuiPreference {
    let file_preference = read_tui_preference_from_config();

    let env_theme = env::var("VIGIL_THEME")
        .ok()
        .filter(|value| !value.trim().is_empty());

    let env_mode = env::var("VIGIL_THEME_MODE")
        .ok()
        .and_then(|value| value.trim().parse().ok());

    TuiPreference {
        theme: env_theme.or(file_preference.theme),
        mode: env_mode.or(file_preference.mode),
        diff_view_mode: file_preference.diff_view_mode,
    }
}

pub fn persist_theme_preference(theme: &str, mode: ThemeMode) -> io::Result<()> {
    let preference = apply_theme_preference(read_tui_preference_from_config(), theme, mode);
    write_tui_preference_to_config(&preference)
}

pub fn persist_diff_view_mode(diff_view_mode: &str) -> io::Result<()> {
    let preference = apply_diff_view_mode(read_tui_preference_from_config(), diff_view_mode);
    write_tui_preference_to_config(&preference)
}

fn read_tui_preference_from_config() -> TuiPreference {
    let path = resolve_tui_config_path();
    read_tui_preference_from_path(&path)
}

fn read_tui_preference_from_path(path: &Path) -> TuiPreference {
    let contents = fs::read_to_string(path);

    let Ok(contents) = contents else {
        return TuiPreference {
            theme: None,
            mode: None,
            diff_view_mode: None,
        };
    };

    serde_json::from_str(&contents).unwrap_or(TuiPreference {
        theme: None,
        mode: None,
        diff_view_mode: None,
    })
}

fn write_tui_preference_to_config(preference: &TuiPreference) -> io::Result<()> {
    let path = resolve_tui_config_path();
    write_tui_preference_to_path(&path, preference)
}

fn write_tui_preference_to_path(path: &Path, preference: &TuiPreference) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents = serde_json::to_string_pretty(preference).map_err(io::Error::other)?;
    fs::write(path, format!("{contents}\n"))
}

fn apply_theme_preference(
    mut preference: TuiPreference,
    theme: &str,
    mode: ThemeMode,
) -> TuiPreference {
    preference.theme = Some(theme.to_owned());
    preference.mode = Some(mode);
    preference
}

fn apply_diff_view_mode(mut preference: TuiPreference, diff_view_mode: &str) -> TuiPreference {
    preference.diff_view_mode = Some(diff_view_mode.to_owned());
    preference
}

fn resolve_tui_config_path() -> PathBuf {
    if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
        let trimmed = xdg_data_home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("vigil").join("tui.json");
        }
    }

    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("share")
        .join("vigil")
        .join("tui.json")
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config_path(name: &str) -> PathBuf {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();

        std::env::temp_dir()
            .join("vigil-tests")
            .join(format!("{name}-{unique_suffix}.json"))
    }

    #[test]
    fn theme_write_preserves_diff_view_mode() {
        let path = temp_config_path("theme-write-preserves-diff-view-mode");
        let initial = TuiPreference {
            theme: None,
            mode: None,
            diff_view_mode: Some("unified".to_string()),
        };
        write_tui_preference_to_path(&path, &initial).expect("should write initial preference");

        let updated = apply_theme_preference(
            read_tui_preference_from_path(&path),
            "tokyo-night",
            ThemeMode::Light,
        );
        write_tui_preference_to_path(&path, &updated).expect("should write updated preference");

        let persisted = read_tui_preference_from_path(&path);
        assert_eq!(persisted.theme.as_deref(), Some("tokyo-night"));
        assert_eq!(persisted.mode, Some(ThemeMode::Light));
        assert_eq!(persisted.diff_view_mode.as_deref(), Some("unified"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn diff_view_write_preserves_theme_settings() {
        let path = temp_config_path("diff-view-write-preserves-theme-settings");
        let initial = TuiPreference {
            theme: Some("catppuccin-macchiato".to_string()),
            mode: Some(ThemeMode::Dark),
            diff_view_mode: None,
        };
        write_tui_preference_to_path(&path, &initial).expect("should write initial preference");

        let updated = apply_diff_view_mode(read_tui_preference_from_path(&path), "split");
        write_tui_preference_to_path(&path, &updated).expect("should write updated preference");

        let persisted = read_tui_preference_from_path(&path);
        assert_eq!(persisted.theme.as_deref(), Some("catppuccin-macchiato"));
        assert_eq!(persisted.mode, Some(ThemeMode::Dark));
        assert_eq!(persisted.diff_view_mode.as_deref(), Some("split"));

        let _ = fs::remove_file(path);
    }
}
