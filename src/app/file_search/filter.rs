use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};

use super::super::App;

struct FileSearchCandidate {
    index: usize,
    haystack: String,
}

impl AsRef<str> for FileSearchCandidate {
    fn as_ref(&self) -> &str {
        &self.haystack
    }
}

impl App {
    pub fn filtered_file_search_indices(&mut self) -> Vec<usize> {
        let query = self.file_search_query.trim();
        if query.is_empty() {
            return (0..self.files.len()).collect();
        }

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let candidates = self
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| FileSearchCandidate {
                index,
                haystack: format!("{} {} {}", file.path, file.label, file.status),
            })
            .collect::<Vec<_>>();

        pattern
            .match_list(candidates, &mut self.file_search_matcher)
            .into_iter()
            .map(|(candidate, _score)| candidate.index)
            .collect()
    }
}
