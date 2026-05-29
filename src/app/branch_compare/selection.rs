use super::super::{App, BranchCompareField, ReviewMode, navigation::move_index};

impl App {
    pub(in crate::app) fn toggle_branch_compare_field(&mut self) {
        self.branch_compare_active_field = match self.branch_compare_active_field {
            BranchCompareField::Source => BranchCompareField::Destination,
            BranchCompareField::Destination => BranchCompareField::Source,
        };
        self.sync_branch_compare_selection_after_query_change();
    }

    pub(in crate::app) fn seed_branch_compare_selection(
        &mut self,
        preferred_source_ref: Option<&str>,
    ) {
        if self.branch_compare_available_refs.is_empty() {
            self.branch_compare_source_ref = None;
            self.branch_compare_destination_ref = None;
            self.branch_compare_selected_source_index = 0;
            self.branch_compare_selected_destination_index = 0;
            return;
        }

        match &self.review_mode {
            ReviewMode::BranchCompare(selection) => {
                self.branch_compare_source_ref = Some(selection.source_ref.clone());
                self.branch_compare_destination_ref = Some(selection.destination_ref.clone());
            }
            _ => {
                self.branch_compare_source_ref = preferred_source_ref
                    .and_then(|current_ref| {
                        self.branch_compare_available_refs
                            .iter()
                            .find(|ref_name| ref_name.as_str() == current_ref)
                            .cloned()
                    })
                    .or_else(|| self.branch_compare_available_refs.first().cloned());
                self.branch_compare_destination_ref = resolve_default_destination_ref(
                    &self.branch_compare_available_refs,
                    self.branch_compare_source_ref.as_deref(),
                );
            }
        }

        self.sync_branch_compare_selection_after_query_change();
    }

    pub(in crate::app) fn active_branch_compare_query_mut(&mut self) -> &mut String {
        match self.branch_compare_active_field {
            BranchCompareField::Source => &mut self.branch_compare_source_query,
            BranchCompareField::Destination => &mut self.branch_compare_destination_query,
        }
    }

    pub(in crate::app) fn sync_branch_compare_selection_after_query_change(&mut self) {
        let filtered = self.filtered_branch_compare_refs();
        let current_ref = match self.branch_compare_active_field {
            BranchCompareField::Source => &mut self.branch_compare_source_ref,
            BranchCompareField::Destination => &mut self.branch_compare_destination_ref,
        };
        let current_index = match self.branch_compare_active_field {
            BranchCompareField::Source => &mut self.branch_compare_selected_source_index,
            BranchCompareField::Destination => &mut self.branch_compare_selected_destination_index,
        };

        if filtered.is_empty() {
            *current_ref = None;
            *current_index = 0;
            return;
        }

        if let Some(existing) = current_ref.as_ref()
            && let Some(index) = filtered.iter().position(|ref_name| ref_name == existing)
        {
            *current_index = index;
            return;
        }

        *current_ref = filtered.first().cloned();
        *current_index = 0;
    }

    pub(in crate::app) fn move_branch_compare_selection(&mut self, delta: i32) {
        let filtered = self.filtered_branch_compare_refs();
        if filtered.is_empty() {
            return;
        }

        let current_index = match self.branch_compare_active_field {
            BranchCompareField::Source => &mut self.branch_compare_selected_source_index,
            BranchCompareField::Destination => &mut self.branch_compare_selected_destination_index,
        };
        let current_ref = match self.branch_compare_active_field {
            BranchCompareField::Source => &mut self.branch_compare_source_ref,
            BranchCompareField::Destination => &mut self.branch_compare_destination_ref,
        };

        let next_index = move_index(*current_index, filtered.len(), delta);

        *current_index = next_index;
        *current_ref = filtered.get(next_index).cloned();
    }
}

fn resolve_default_destination_ref(refs: &[String], source_ref: Option<&str>) -> Option<String> {
    let preferred = refs.iter().find(|ref_name| {
        (**ref_name == "main" || **ref_name == "master")
            && source_ref.is_none_or(|source| source != ref_name.as_str())
    });
    if let Some(preferred) = preferred {
        return Some(preferred.clone());
    }

    refs.iter()
        .find(|ref_name| source_ref.is_none_or(|source| source != ref_name.as_str()))
        .cloned()
        .or_else(|| refs.first().cloned())
}
