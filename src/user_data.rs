//! Where vigil keeps its per-user files.
//!
//! Every database vigil writes lives in one directory:
//! `$XDG_DATA_HOME/vigil`, or `~/.local/share/vigil` when `XDG_DATA_HOME` is
//! unset or empty. Each file in it has one owner (the review database, the
//! forge cache), and owners create the directory on first write. Resolving
//! the path reads the environment only; it touches no files.

use std::{
    env,
    path::{Path, PathBuf},
};

/// The directory vigil's per-user files live in.
pub(crate) fn data_dir() -> PathBuf {
    if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
        let trimmed = xdg_data_home.trim();
        if !trimmed.is_empty() {
            return data_dir_from_data_home(Path::new(trimmed));
        }
    }

    data_dir_from_data_home(
        &env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".local")
            .join("share"),
    )
}

fn data_dir_from_data_home(data_home: &Path) -> PathBuf {
    data_home.join("vigil")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_directory_uses_xdg_data_home_style_location() {
        let path = data_dir_from_data_home(Path::new("/tmp/vigil-xdg-data"));

        assert_eq!(path, PathBuf::from("/tmp/vigil-xdg-data").join("vigil"));
    }
}
