use crate::app::App;

pub(super) fn blame_hint(app: &App) -> &'static str {
    if app.blame_loading {
        "Esc closes."
    } else if app
        .blame_details
        .as_ref()
        .and_then(|details| details.compare_selection.as_ref())
        .is_some()
    {
        "Enter or o opens commit compare. j/k scroll. Esc closes."
    } else {
        "No commit compare available for this line. j/k scroll. Esc closes."
    }
}
