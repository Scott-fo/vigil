use super::super::{App, BranchCompareField};

impl App {
    pub fn filtered_branch_compare_ref_indices(&mut self) -> Vec<usize> {
        self.sync_branch_compare_ref_index();

        let query = match self.branch_compare_active_field {
            BranchCompareField::Source => &self.branch_compare_source_query,
            BranchCompareField::Destination => &self.branch_compare_destination_query,
        };

        self.branch_compare_ref_index
            .matching_indices(query, &mut self.branch_compare_matcher)
    }

    pub fn filtered_branch_compare_refs(&mut self) -> Vec<String> {
        self.filtered_branch_compare_ref_indices()
            .into_iter()
            .filter_map(|index| self.branch_compare_available_refs.get(index))
            .cloned()
            .collect()
    }

    pub(in crate::app) fn rebuild_branch_compare_ref_index(&mut self) {
        self.branch_compare_ref_index
            .replace(self.branch_compare_available_refs.clone());
    }

    pub(in crate::app) fn sync_branch_compare_ref_index(&mut self) {
        if !self
            .branch_compare_ref_index
            .is_aligned_with(self.branch_compare_available_refs.len())
        {
            self.rebuild_branch_compare_ref_index();
        }
    }
}
