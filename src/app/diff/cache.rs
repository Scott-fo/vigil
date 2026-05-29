use std::collections::VecDeque;

use crate::git::DiffView;

pub(crate) const DIFF_CACHE_CAPACITY: usize = 32;
pub(crate) const DIFF_PREFETCH_DISTANCE: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffCacheKey {
    pub(super) review_scope: String,
    pub(super) file_path: String,
    pub(super) file_status: String,
}

#[derive(Debug, Clone)]
struct DiffCacheEntry {
    key: DiffCacheKey,
    plain: DiffView,
    highlighted: Option<DiffView>,
    highlight_complete: bool,
}

#[derive(Debug, Default)]
pub(crate) struct DiffViewCache {
    entries: VecDeque<DiffCacheEntry>,
}

impl DiffViewCache {
    pub(crate) fn contains(&self, key: &DiffCacheKey) -> bool {
        self.entries.iter().any(|entry| &entry.key == key)
    }

    pub(crate) fn get_plain(&mut self, key: &DiffCacheKey) -> Option<DiffView> {
        self.touch_entry(key).map(|entry| entry.plain.clone())
    }

    pub(crate) fn get_highlighted(&mut self, key: &DiffCacheKey) -> Option<(DiffView, bool)> {
        self.touch_entry(key).and_then(|entry| {
            entry
                .highlighted
                .clone()
                .map(|view| (view, entry.highlight_complete))
        })
    }

    pub(crate) fn insert_plain(&mut self, key: DiffCacheKey, plain: DiffView) {
        let (highlighted, highlight_complete) = self
            .remove_entry(&key)
            .map(|entry| (entry.highlighted, entry.highlight_complete))
            .unwrap_or((None, false));
        self.entries.push_back(DiffCacheEntry {
            key,
            plain,
            highlighted,
            highlight_complete,
        });
        self.trim();
    }

    pub(crate) fn insert_highlighted(
        &mut self,
        key: DiffCacheKey,
        highlighted: DiffView,
        complete: bool,
    ) {
        if let Some(mut entry) = self.remove_entry(&key) {
            match entry.highlighted.as_mut() {
                Some(existing) if !complete => existing.merge_highlighting_from(&highlighted),
                Some(existing) if complete => *existing = highlighted,
                Some(_) => {}
                None => entry.highlighted = Some(highlighted),
            }
            entry.highlight_complete |= complete;
            self.entries.push_back(entry);
        } else {
            self.entries.push_back(DiffCacheEntry {
                key,
                plain: highlighted.clone(),
                highlighted: Some(highlighted),
                highlight_complete: complete,
            });
        }
        self.trim();
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    fn touch_entry(&mut self, key: &DiffCacheKey) -> Option<&DiffCacheEntry> {
        let entry = self.remove_entry(key)?;
        self.entries.push_back(entry);
        self.entries.back()
    }

    fn remove_entry(&mut self, key: &DiffCacheKey) -> Option<DiffCacheEntry> {
        let index = self.entries.iter().position(|entry| &entry.key == key)?;
        self.entries.remove(index)
    }

    fn trim(&mut self) {
        while self.entries.len() > DIFF_CACHE_CAPACITY {
            let _ = self.entries.pop_front();
        }
    }
}
