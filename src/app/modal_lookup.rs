//! Prepared exact and fuzzy lookup for modal lists.
//!
//! Modal searches receive small, typed item lists from the app and turn them
//! into stable haystacks once per load. Callers keep owning selection and
//! display state; this module owns query parsing, matcher reuse, and the
//! index-to-haystack contract. Queries use `nucleo_matcher` pattern syntax, so
//! fuzzy matching is the default while `'term`, `^term`, `term$`, and `^term$`
//! request substring, prefix, postfix, and exact matches.

use nucleo_matcher::{
    Matcher,
    pattern::{CaseMatching, Normalization, Pattern},
};

#[derive(Debug, Default, Clone)]
pub(in crate::app) struct ModalLookupIndex {
    haystacks: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModalLookupCandidate<'a> {
    index: usize,
    haystack: &'a str,
}

impl AsRef<str> for ModalLookupCandidate<'_> {
    fn as_ref(&self) -> &str {
        self.haystack
    }
}

impl ModalLookupIndex {
    pub(in crate::app) fn replace(&mut self, haystacks: Vec<String>) {
        self.haystacks = haystacks;
    }

    pub(in crate::app) fn clear(&mut self) {
        self.haystacks.clear();
    }

    pub(in crate::app) fn is_aligned_with(&self, item_count: usize) -> bool {
        self.haystacks.len() == item_count
    }

    pub(in crate::app) fn matching_indices(
        &self,
        query: &str,
        matcher: &mut Matcher,
    ) -> Vec<usize> {
        let query = query.trim();
        if query.is_empty() {
            return (0..self.haystacks.len()).collect();
        }

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let candidates =
            self.haystacks
                .iter()
                .enumerate()
                .map(|(index, haystack)| ModalLookupCandidate {
                    index,
                    haystack: haystack.as_str(),
                });

        pattern
            .match_list(candidates, matcher)
            .into_iter()
            .map(|(candidate, _score)| candidate.index)
            .collect()
    }
}
