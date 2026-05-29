use super::WorktreeEntry;

mod filter;
mod modal;
mod selection;
mod switch;

pub(crate) fn worktree_display_name(entry: &WorktreeEntry) -> String {
    entry.branch.clone().unwrap_or_else(|| {
        entry
            .head
            .as_deref()
            .map(|head| head.chars().take(7).collect())
            .unwrap_or_else(|| "unknown".to_string())
    })
}

#[cfg(test)]
mod tests;
