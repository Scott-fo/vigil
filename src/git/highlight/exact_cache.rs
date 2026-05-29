use std::{
    cell::RefCell,
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::Arc,
};

use super::SyntaxToken;

const EXACT_HIGHLIGHT_CACHE_CAPACITY: usize = 8;

struct ExactHighlightCacheEntry {
    filetype: &'static str,
    source_hash: u64,
    source_len: usize,
    source: Arc<str>,
    highlighted_lines: Arc<[Vec<SyntaxToken>]>,
}

thread_local! {
    static EXACT_HIGHLIGHT_CACHE: RefCell<Vec<ExactHighlightCacheEntry>> =
        const { RefCell::new(Vec::new()) };
}

#[inline]
pub(super) fn exact_cache_get(
    filetype: &'static str,
    source: &Arc<str>,
) -> Option<Arc<[Vec<SyntaxToken>]>> {
    let source_hash = hash_source(source.as_ref());
    let source_len = source.len();

    EXACT_HIGHLIGHT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let position = cache.iter().position(|entry| {
            entry.filetype == filetype
                && entry.source_hash == source_hash
                && entry.source_len == source_len
                && entry.source.as_ref() == source.as_ref()
        })?;
        let entry = cache.remove(position);
        let highlighted_lines = entry.highlighted_lines.clone();
        cache.push(entry);
        Some(highlighted_lines)
    })
}

#[inline]
pub(super) fn exact_cache_insert(
    filetype: &'static str,
    source: &Arc<str>,
    highlighted_lines: &Arc<[Vec<SyntaxToken>]>,
) {
    let source_hash = hash_source(source.as_ref());
    let source_len = source.len();

    EXACT_HIGHLIGHT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.push(ExactHighlightCacheEntry {
            filetype,
            source_hash,
            source_len,
            source: source.clone(),
            highlighted_lines: highlighted_lines.clone(),
        });
        if cache.len() > EXACT_HIGHLIGHT_CACHE_CAPACITY {
            let overflow = cache.len() - EXACT_HIGHLIGHT_CACHE_CAPACITY;
            cache.drain(..overflow);
        }
    });
}

pub(super) fn clear_exact_cache() {
    EXACT_HIGHLIGHT_CACHE.with(|cache| cache.borrow_mut().clear());
}

#[inline]
fn hash_source(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(label: &'static str) -> Arc<[Vec<SyntaxToken>]> {
        Arc::from([vec![SyntaxToken {
            start: 0,
            end: label.len(),
            highlight_name: Some(label),
        }]])
    }

    #[test]
    fn exact_cache_returns_inserted_source_and_distinguishes_content() {
        clear_exact_cache();
        let source: Arc<str> = Arc::from("fn main() {}");
        let other_source: Arc<str> = Arc::from("fn main() { println!(); }");
        let highlighted = lines("function");

        exact_cache_insert("rust", &source, &highlighted);

        assert_eq!(exact_cache_get("rust", &source), Some(highlighted));
        assert!(exact_cache_get("rust", &other_source).is_none());
    }

    #[test]
    fn exact_cache_evicts_oldest_entry_after_capacity() {
        clear_exact_cache();
        let first: Arc<str> = Arc::from("source-0");
        exact_cache_insert("rust", &first, &lines("first"));
        for index in 1..=EXACT_HIGHLIGHT_CACHE_CAPACITY {
            let source: Arc<str> = Arc::from(format!("source-{index}"));
            exact_cache_insert("rust", &source, &lines("next"));
        }

        assert!(exact_cache_get("rust", &first).is_none());
    }
}
