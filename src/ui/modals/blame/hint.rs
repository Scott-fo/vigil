use crate::app::App;

use super::super::hints::KeyHint;

/// Footer hints for the blame modal, plus a note when the blamed line has no
/// commit to compare against.
pub(super) fn blame_hints(app: &App) -> (&'static [KeyHint], Option<&'static str>) {
    if app.blame_loading {
        (&[("esc", "close")], None)
    } else if app
        .blame_details
        .as_ref()
        .and_then(|details| details.compare_selection.as_ref())
        .is_some()
    {
        (
            &[("⏎", "compare commit"), ("j/k", "scroll"), ("esc", "close")],
            None,
        )
    } else {
        (
            &[("j/k", "scroll"), ("esc", "close")],
            Some("no commit compare for this line"),
        )
    }
}
