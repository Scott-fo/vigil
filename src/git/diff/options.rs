//! Options shared by every `git diff` vigil runs.
//!
//! Every loader that shells out to `git diff` takes a [`DiffOptions`] so that
//! the per-file preview, review snapshot, line stats, and search indexes all
//! describe the same diff. Options change diff content, so callers that cache
//! diff-derived data must make the options part of the cache identity.

use strum_macros::{EnumString, IntoStaticStr};

/// How whitespace-only edits appear in diffs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum WhitespaceMode {
    /// Every change, including re-indentation and trailing whitespace.
    #[default]
    Show,
    /// Changes that only add, remove, or alter whitespace are hidden, as with
    /// `git diff --ignore-all-space`.
    Ignore,
}

impl WhitespaceMode {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Show => Self::Ignore,
            Self::Ignore => Self::Show,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct DiffOptions {
    pub whitespace: WhitespaceMode,
}

impl DiffOptions {
    /// `args` with option flags inserted right after the leading `diff`
    /// subcommand. Non-diff commands are returned unchanged.
    pub(crate) fn diff_args<'a>(self, args: &[&'a str]) -> Vec<&'a str> {
        let mut resolved = args.to_vec();
        if resolved.first() == Some(&"diff") && self.whitespace == WhitespaceMode::Ignore {
            resolved.insert(1, "--ignore-all-space");
        }
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_whitespace_adds_flag_after_diff_subcommand() {
        let options = DiffOptions {
            whitespace: WhitespaceMode::Ignore,
        };

        assert_eq!(
            options.diff_args(&["diff", "--no-color", "HEAD", "--"]),
            vec!["diff", "--ignore-all-space", "--no-color", "HEAD", "--"]
        );
    }

    #[test]
    fn default_options_leave_args_untouched() {
        let args = ["diff", "--numstat", "-z", "HEAD"];

        assert_eq!(DiffOptions::default().diff_args(&args), args.to_vec());
    }

    #[test]
    fn non_diff_commands_are_never_modified() {
        let options = DiffOptions {
            whitespace: WhitespaceMode::Ignore,
        };

        assert_eq!(
            options.diff_args(&["show", "HEAD:a.rs"]),
            vec!["show", "HEAD:a.rs"]
        );
    }

    #[test]
    fn whitespace_mode_round_trips_through_its_preference_name() {
        for mode in [WhitespaceMode::Show, WhitespaceMode::Ignore] {
            assert_eq!(mode.as_str().parse::<WhitespaceMode>(), Ok(mode));
        }
        assert_eq!(WhitespaceMode::Show.toggle(), WhitespaceMode::Ignore);
    }
}
