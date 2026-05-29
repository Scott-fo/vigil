use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};

use super::super::{App, BranchCompareField};

impl App {
    pub fn filtered_branch_compare_refs(&mut self) -> Vec<String> {
        let query = match self.branch_compare_active_field {
            BranchCompareField::Source => self.branch_compare_source_query.trim(),
            BranchCompareField::Destination => self.branch_compare_destination_query.trim(),
        };

        if query.is_empty() {
            return self.branch_compare_available_refs.clone();
        }

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let candidates = self.branch_compare_available_refs.to_vec();
        pattern
            .match_list(candidates, &mut self.branch_compare_matcher)
            .into_iter()
            .map(|(candidate, _score)| candidate)
            .collect()
    }
}
