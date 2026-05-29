use super::super::types::BranchCompareSelection;

pub(crate) fn build_branch_diff_range(selection: &BranchCompareSelection) -> String {
    format!(
        "{}...{}",
        selection.destination_ref.trim(),
        selection.source_ref.trim()
    )
}
