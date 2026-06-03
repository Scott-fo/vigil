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
        let filtered = self.filtered_branch_compare_ref_indices();

        if filtered.is_empty() {
            match self.branch_compare_active_field {
                BranchCompareField::Source => {
                    self.branch_compare_source_ref = None;
                    self.branch_compare_selected_source_index = 0;
                }
                BranchCompareField::Destination => {
                    self.branch_compare_destination_ref = None;
                    self.branch_compare_selected_destination_index = 0;
                }
            }
            return;
        }

        let existing_ref = match self.branch_compare_active_field {
            BranchCompareField::Source => self.branch_compare_source_ref.as_deref(),
            BranchCompareField::Destination => self.branch_compare_destination_ref.as_deref(),
        };

        if let Some(existing) = existing_ref
            && let Some(index) = filtered.iter().position(|ref_index| {
                self.branch_compare_available_refs[*ref_index].as_str() == existing
            })
        {
            match self.branch_compare_active_field {
                BranchCompareField::Source => self.branch_compare_selected_source_index = index,
                BranchCompareField::Destination => {
                    self.branch_compare_selected_destination_index = index;
                }
            }
            return;
        }

        let first_ref = self.branch_compare_available_refs[filtered[0]].clone();
        match self.branch_compare_active_field {
            BranchCompareField::Source => {
                self.branch_compare_source_ref = Some(first_ref);
                self.branch_compare_selected_source_index = 0;
            }
            BranchCompareField::Destination => {
                self.branch_compare_destination_ref = Some(first_ref);
                self.branch_compare_selected_destination_index = 0;
            }
        }
    }

    pub(in crate::app) fn move_branch_compare_selection(&mut self, delta: i32) {
        let filtered = self.filtered_branch_compare_ref_indices();
        if filtered.is_empty() {
            return;
        }

        let selected_index = match self.branch_compare_active_field {
            BranchCompareField::Source => self.branch_compare_selected_source_index,
            BranchCompareField::Destination => self.branch_compare_selected_destination_index,
        };
        let next_index = move_index(selected_index, filtered.len(), delta);
        let next_ref = self.branch_compare_available_refs[filtered[next_index]].clone();

        match self.branch_compare_active_field {
            BranchCompareField::Source => {
                self.branch_compare_selected_source_index = next_index;
                self.branch_compare_source_ref = Some(next_ref);
            }
            BranchCompareField::Destination => {
                self.branch_compare_selected_destination_index = next_index;
                self.branch_compare_destination_ref = Some(next_ref);
            }
        }
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
