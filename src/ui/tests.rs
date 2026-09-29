use std::path::PathBuf;

use super::*;
use crate::git::FileEntry;

fn build_test_app() -> App {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-ui-tests"));
    app.files.push(FileEntry {
        status: "M ".to_string(),
        path: "src/main.rs".to_string(),
        label: "main.rs".to_string(),
        filetype: Some("rust"),
    });
    app
}

#[test]
fn hovered_pane_uses_full_width_diff_when_sidebar_is_hidden() {
    let mut app = build_test_app();
    app.sidebar_hidden = true;

    assert_eq!(hovered_pane_at(&app, 2, 2, 120, 40), Some(ActivePane::Diff));
}

#[test]
fn sidebar_hit_testing_is_disabled_when_sidebar_is_hidden() {
    let mut app = build_test_app();
    app.sidebar_hidden = true;

    assert_eq!(sidebar_file_at(&app, 2, 2, 120, 40), None);
}

#[test]
fn sidebar_click_targets_the_row_that_rendered_the_file() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut app = build_test_app();
    app.sidebar_items =
        crate::sidebar::build_sidebar_items(&app.files, &std::collections::HashSet::new());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| render(frame, &mut app)).unwrap();

    let buffer = terminal.backend().buffer();
    let (column, row) = (0..40u16)
        .find_map(|row| {
            let text: String = (0..40u16)
                .map(|column| buffer[(column, row)].symbol())
                .collect();
            text.find("main.rs")
                .map(|byte_index| (text[..byte_index].chars().count() as u16, row))
        })
        .expect("sidebar should render the changed file");

    assert_eq!(
        sidebar_file_at(&app, column, row, 120, 40),
        Some("src/main.rs".to_string())
    );
    assert_eq!(
        hovered_pane_at(&app, column, row, 120, 40),
        Some(ActivePane::Sidebar)
    );
}

fn render_to_buffer(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    use ratatui::{Terminal, backend::TestBackend};

    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

/// Column where `needle` starts on `row`, read from the rendered cells.
fn column_of(buffer: &ratatui::buffer::Buffer, row: u16, needle: &str) -> Option<u16> {
    let width = buffer.area.width;
    let cells: Vec<&str> = (0..width)
        .map(|column| buffer[(column, row)].symbol())
        .collect();
    (0..width as usize).find_map(|start| {
        let text: String = cells[start..].concat();
        text.starts_with(needle).then_some(start as u16)
    })
}

/// The view-mode chip shows the current mode (padded, as rendered); the `v`
/// hint names the mode it switches to.
fn mode_labels(app: &App) -> (&'static str, &'static str) {
    match app.diff_view_mode {
        crate::app::DiffViewMode::Unified => (" unified ", "v split"),
        crate::app::DiffViewMode::Split => (" split ", "v unified"),
    }
}

#[test]
fn footer_clicks_land_on_the_rendered_chips_and_hints() {
    let mut app = build_test_app();
    let (width, height) = (160, 20);
    let buffer = render_to_buffer(&mut app, width, height);
    let footer_row = height - 1;

    let (mode_chip, mode_hint) = mode_labels(&app);
    let expectations = [
        (mode_chip, Some(FooterAction::ToggleDiffViewMode)),
        (mode_hint, Some(FooterAction::ToggleDiffViewMode)),
        (" wrap ", Some(FooterAction::ToggleLineWrap)),
        ("? help", Some(FooterAction::OpenHelp)),
        ("tab diff", Some(FooterAction::SwitchPane)),
        ("j/k move", None),
    ];
    for (needle, expected) in expectations {
        let start = column_of(&buffer, footer_row, needle)
            .unwrap_or_else(|| panic!("footer should render {needle:?}"));
        for column in start..start + needle.chars().count() as u16 {
            assert_eq!(
                footer_action_at(&app, column, footer_row, width, height),
                expected,
                "{needle:?} at column {column}"
            );
        }
    }

    // The summary on the left and rows above the footer are not controls.
    assert_eq!(footer_action_at(&app, 1, footer_row, width, height), None);
    let chip = column_of(&buffer, footer_row, mode_chip).unwrap();
    assert_eq!(
        footer_action_at(&app, chip, footer_row - 1, width, height),
        None
    );
}

#[test]
fn footer_hit_testing_follows_hints_that_drop_on_narrow_terminals() {
    let mut app = build_test_app();
    let (width, height) = (70, 20);
    let buffer = render_to_buffer(&mut app, width, height);
    let footer_row = height - 1;

    let footer_text: String = (0..width)
        .map(|column| buffer[(column, footer_row)].symbol())
        .collect();
    let chip = column_of(&buffer, footer_row, mode_labels(&app).0)
        .unwrap_or_else(|| panic!("chips always render: {footer_text:?}"));
    assert_eq!(
        footer_action_at(&app, chip, footer_row, width, height),
        Some(FooterAction::ToggleDiffViewMode)
    );
    assert_eq!(column_of(&buffer, footer_row, "ff find file"), None);
    for column in 0..width {
        assert_ne!(
            footer_action_at(&app, column, footer_row, width, height),
            Some(FooterAction::FindFile),
            "a dropped hint must not stay clickable"
        );
    }
}

#[test]
fn hovered_sidebar_row_is_tinted_and_selection_still_wins() {
    let mut app = build_test_app();
    app.files.push(FileEntry {
        status: "M ".to_string(),
        path: "src/lib.rs".to_string(),
        label: "lib.rs".to_string(),
        filetype: Some("rust"),
    });
    app.sidebar_items =
        crate::sidebar::build_sidebar_items(&app.files, &std::collections::HashSet::new());
    let row_of = |buffer: &ratatui::buffer::Buffer, name: &str| {
        (0..20u16)
            .find(|row| column_of(buffer, *row, name).is_some_and(|column| column < 32))
            .expect("file should render in the sidebar")
    };
    app.selected_sidebar_row = app
        .sidebar_items
        .iter()
        .position(|item| item.path() == "src/lib.rs")
        .unwrap();
    let buffer = render_to_buffer(&mut app, 120, 20);
    let cursor_row = row_of(&buffer, "lib.rs");
    let hover_row = row_of(&buffer, "main.rs");

    app.mouse_position = Some(ratatui::layout::Position::new(10, hover_row));
    let buffer = render_to_buffer(&mut app, 120, 20);
    assert_eq!(buffer[(10, hover_row)].bg, hover_color());

    app.mouse_position = Some(ratatui::layout::Position::new(10, cursor_row));
    let buffer = render_to_buffer(&mut app, 120, 20);
    assert_eq!(
        buffer[(10, cursor_row)].bg,
        selection_color(),
        "the cursor row keeps its selection tint under the mouse"
    );
    assert_ne!(buffer[(10, hover_row)].bg, hover_color());
}

fn branch_snapshot() -> crate::git::BranchSnapshot {
    use crate::git::{
        BranchEntry, BranchLocation, BranchSnapshot, BranchTip, Divergence, HeadState, Upstream,
    };

    let branch = |name: &str, location: BranchLocation, upstream: Option<Upstream>| BranchEntry {
        name: name.to_string(),
        location,
        is_head: name == "main",
        upstream,
        tip: BranchTip {
            short_hash: "abc1234".to_string(),
            subject: format!("Latest work on {name}"),
            committed_at: 1_700_000_000,
        },
    };
    BranchSnapshot {
        head: HeadState::Branch("main".to_string()),
        branches: vec![
            branch(
                "main",
                BranchLocation::Local,
                Some(Upstream {
                    name: "origin/main".to_string(),
                    divergence: Divergence::Tracking {
                        ahead: 2,
                        behind: 1,
                    },
                }),
            ),
            branch("feature/login", BranchLocation::Local, None),
            branch(
                "origin/review",
                BranchLocation::Remote {
                    remote: "origin".to_string(),
                },
                None,
            ),
        ],
        previous_branch: Some("feature/login".to_string()),
        remotes: vec!["origin".to_string()],
        last_fetch: None,
        operation: None,
    }
}

fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn footer_branch_readout_opens_the_branch_panel() {
    let mut app = build_test_app();
    app.set_branch_snapshot(branch_snapshot());
    let (width, height) = (160, 20);
    let buffer = render_to_buffer(&mut app, width, height);
    let footer_row = height - 1;

    let start = column_of(&buffer, footer_row, "main ↑2 ↓1")
        .expect("footer should show the branch and its divergence");
    assert_eq!(
        footer_action_at(&app, start, footer_row, width, height),
        Some(FooterAction::OpenBranches)
    );
    assert_eq!(footer_action_at(&app, 1, footer_row, width, height), None);
}

#[tokio::test]
async fn branch_panel_shows_sync_state_sections_and_selection() {
    let mut app = build_test_app();
    app.set_branch_snapshot(branch_snapshot());
    // Opening queues a reload that never lands in tests.
    app.open_branch_panel();

    let text = buffer_text(&render_to_buffer(&mut app, 140, 36));
    if std::env::var_os("VIGIL_PRINT_UI").is_some() {
        println!("{text}");
    }

    assert!(text.contains("Branches"));
    assert!(text.contains("main  →  origin/main ↑2 ↓1"));
    assert!(text.contains("Diverged from origin/main: 2 commits to push, 1 to pull."));
    assert!(text.contains("LOCAL"));
    assert!(text.contains("REMOTE"));
    assert!(text.contains("never fetched"));
    let selected_line = text
        .lines()
        .find(|line| line.contains("▎"))
        .expect("a branch is selected");
    assert!(
        selected_line.contains("feature/login"),
        "previous branch starts selected: {selected_line}"
    );
}

mod pull_requests {
    use super::*;
    use crate::{
        app::{PullRequestPage, pull_request_fixtures as fixtures},
        forge::{DiffSide, ThreadSubject},
    };

    fn pull_request_app(threads: Vec<crate::forge::ReviewThread>) -> App {
        let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-ui-tests"));
        app.open_pull_request_for_test(
            fixtures::pull_request(17, threads),
            vec![fixtures::file("src/app.rs")],
        );
        app
    }

    fn show_diff(app: &mut App) {
        app.select_pull_request_page_for_test(PullRequestPage::Files);
        app.diff_view = crate::git::build_diff_view_from_diff_text(
            "diff --git a/src/app.rs b/src/app.rs\n\
             --- a/src/app.rs\n\
             +++ b/src/app.rs\n\
             @@ -1,0 +1,3 @@\n\
             +fn one() {}\n\
             +fn two() {}\n\
             +fn three() {}\n",
            Some("rust"),
        );
    }

    fn print(text: &str) {
        if std::env::var_os("VIGIL_PRINT_UI").is_some() {
            println!("{text}");
        }
    }

    #[test]
    fn overview_shows_reviews_checks_merge_state_description_and_stray_threads() {
        let mut file_level = fixtures::thread("T2", "src/app.rs", DiffSide::Right, None);
        file_level.subject = ThreadSubject::File;
        file_level.comments[0].body = "The whole file needs a doc comment.".to_string();
        let mut app = pull_request_app(vec![file_level]);

        let text = buffer_text(&render_to_buffer(&mut app, 140, 60));
        print(&text);

        assert!(text.contains("Overview"), "sidebar pins the overview row");
        assert!(text.contains("Remove Codex review integration"));
        assert!(text.contains("#17 · open · Scott-fo · master ← review/remove-codex"));
        assert!(text.contains("REVIEWS  approved"));
        assert!(text.contains("✓ reviewer  approved"));
        let lint = text.find("✗ CI / lint").expect("failed check listed");
        let test = text.find("✓ CI / test").expect("passing check listed");
        assert!(lint < test, "failures come first");
        assert!(text.contains("mergeability unknown · GitHub is computing it"));
        assert!(text.contains("## Summary"));
        assert!(text.contains("Ready for review."));
        assert!(text.contains("THREADS OUTSIDE THE DIFF"));
        assert!(text.contains("src/app.rs · file comment · ● unresolved"));
        assert!(text.contains("The whole file needs a doc comment."));
    }

    #[test]
    fn unresolved_threads_render_under_their_line_with_every_comment() {
        let mut app = pull_request_app(vec![fixtures::thread(
            "T1",
            "src/app.rs",
            DiffSide::Right,
            Some(2),
        )]);
        show_diff(&mut app);

        let text = buffer_text(&render_to_buffer(&mut app, 120, 30));
        print(&text);
        let lines = text.lines().collect::<Vec<_>>();
        let two = lines
            .iter()
            .position(|line| line.contains("fn two()"))
            .expect("line two renders");
        assert!(
            lines[two + 1].contains("╭─ ● 1 comment"),
            "{}",
            lines[two + 1]
        );
        assert!(lines[two + 2].contains("│  reviewer · "));
        assert!(lines[two + 3].contains("Should this handle the empty case?"));
        assert!(lines[two + 4].contains("╰─"));
        assert!(lines[two + 5].contains("fn three()"));
        assert!(text.contains("●1"), "the sidebar marks the file");
    }

    #[test]
    fn resolved_threads_collapse_to_one_row() {
        let mut resolved = fixtures::thread("T1", "src/app.rs", DiffSide::Right, Some(2));
        resolved.is_resolved = true;
        resolved.resolved_by = Some("Scott-fo".to_string());
        let mut app = pull_request_app(vec![resolved]);
        show_diff(&mut app);

        let text = buffer_text(&render_to_buffer(&mut app, 120, 30));
        print(&text);
        let lines = text.lines().collect::<Vec<_>>();
        let two = lines
            .iter()
            .position(|line| line.contains("fn two()"))
            .expect("line two renders");
        assert!(
            lines[two + 1].contains("── ✓ resolved · 1 comment · by Scott-fo"),
            "{}",
            lines[two + 1]
        );
        assert!(lines[two + 2].contains("fn three()"));
        assert!(!text.contains("Should this handle the empty case?"));
        assert!(!text.contains("●1"), "resolved threads are not counted");
    }

    #[test]
    fn footer_chip_shows_the_current_branch_pull_request_and_opens_it() {
        let mut app = build_test_app();
        let mut summary = fixtures::summary(17);
        summary.is_draft = true;
        app.set_current_branch_pull_request_for_test("review/remove-codex", summary);
        let (width, height) = (160, 20);
        let buffer = render_to_buffer(&mut app, width, height);
        let footer_row = height - 1;

        let chip = "#17 draft ✓ approved";
        let start = column_of(&buffer, footer_row, chip).unwrap_or_else(|| {
            panic!(
                "footer should show the chip: {:?}",
                buffer_text(&buffer).lines().last()
            )
        });
        for column in start..start + chip.chars().count() as u16 {
            assert_eq!(
                footer_action_at(&app, column, footer_row, width, height),
                Some(FooterAction::OpenPullRequest)
            );
        }
    }

    #[test]
    fn pull_request_list_renders_tabs_rows_and_truncation_where_clicks_land() {
        use crate::{forge::PullRequestListFilter, ui::PullRequestListTarget};

        let mut app = build_test_app();
        app.show_pull_request_list_for_test(fixtures::pull_request_list(&[5, 17], 120));
        let (width, height) = (140, 20);
        let buffer = render_to_buffer(&mut app, width, height);
        let text = buffer_text(&buffer);
        print(&text);

        assert!(text.contains("PULL REQUESTS  Scott-fo/vigil"));
        assert!(text.contains("Needs my review 120"));
        assert!(text.contains("Showing the 2 most recently updated of 120"));
        assert!(!text.contains("CHANGES"), "the review screen is not drawn");

        let tab = column_of(&buffer, 1, "Mine").expect("tabs render on row 1");
        assert_eq!(
            pull_request_list_target_at(&app, tab, 1, width, height),
            Some(PullRequestListTarget::Tab(PullRequestListFilter::Mine))
        );
        let row = (0..height)
            .find(|row| column_of(&buffer, *row, "Pull request number 17").is_some())
            .expect("row renders");
        assert!(text.lines().nth(row as usize).unwrap().contains("#17"));
        assert_eq!(
            pull_request_list_target_at(&app, 10, row, width, height),
            Some(PullRequestListTarget::Row(1))
        );
        assert_eq!(
            pull_request_list_target_at(&app, 10, row + 1, width, height),
            None,
            "below the last row"
        );

        // Review-screen hit testing is off while the list is up.
        assert_eq!(footer_action_at(&app, 1, height - 1, width, height), None);
        assert_eq!(hovered_pane_at(&app, 50, 5, width, height), None);
        assert_eq!(sidebar_item_index_at(&app, 5, 5, width, height), None);
    }

    fn draft_on_two(body: &str) -> crate::review::DraftComment {
        use crate::{
            forge::DiffPosition,
            review::{DraftAnchor, DraftComment, DraftLine},
        };

        DraftComment::new(
            "src/app.rs".to_string(),
            DraftAnchor {
                start: None,
                end: DraftLine {
                    position: DiffPosition {
                        side: DiffSide::Right,
                        line: 2,
                    },
                    text: "fn two() {}".to_string(),
                },
            },
            body.to_string(),
            fixtures::summary(17).head_oid,
        )
    }

    const THREE_ADDED: &str = "diff --git a/src/app.rs b/src/app.rs\n\
                               --- a/src/app.rs\n\
                               +++ b/src/app.rs\n\
                               @@ -1,0 +1,3 @@\n\
                               +fn one() {}\n\
                               +fn two() {}\n\
                               +fn three() {}\n";

    #[test]
    fn drafts_render_under_their_line_as_pending_and_mark_the_file() {
        let mut app = pull_request_app(Vec::new());
        app.show_pull_request_diff_for_test(THREE_ADDED, 0);
        app.save_draft_for_test(draft_on_two("Guard the empty case."));

        let text = buffer_text(&render_to_buffer(&mut app, 120, 30));
        print(&text);
        let lines = text.lines().collect::<Vec<_>>();
        let two = lines
            .iter()
            .position(|line| line.contains("fn two()"))
            .expect("line two renders");
        assert!(
            lines[two + 1].contains("╭─ ◌ pending · line 2 · your draft"),
            "{}",
            lines[two + 1]
        );
        assert!(lines[two + 2].contains("│  Guard the empty case."));
        assert!(lines[two + 3].contains("╰─"));
        assert!(lines[two + 4].contains("fn three()"));
        assert!(
            text.contains("●1"),
            "drafts count toward the sidebar marker"
        );
        assert!(
            text.contains("1 pending · S submit"),
            "the footer counts drafts"
        );
    }

    #[test]
    fn the_overview_lists_drafts_and_the_review_keys() {
        let mut app = pull_request_app(Vec::new());
        app.save_draft_for_test(draft_on_two("Guard the empty case."));

        let text = buffer_text(&render_to_buffer(&mut app, 140, 50));
        print(&text);

        assert!(text.contains("c comment · S submit review · D drafts"));
        assert!(text.contains("YOUR DRAFTS  1 pending · S submits · D edits"));
        assert!(text.contains("src/app.rs · line 2 · ◌ pending"));
        assert!(text.contains("Guard the empty case."));
    }

    #[test]
    fn the_composer_shows_where_the_comment_goes_and_its_keys() {
        let mut app = pull_request_app(Vec::new());
        app.show_pull_request_diff_for_test(THREE_ADDED, 1);
        app.start_inline_comment_for_test();
        app.type_in_pull_request_modal_for_test("Could this be\nsimpler?");

        let text = buffer_text(&render_to_buffer(&mut app, 120, 30));
        print(&text);

        assert!(text.contains("New comment"));
        assert!(text.contains("src/app.rs · line 2"));
        assert!(text.contains("draft, sent with your review"));
        assert!(text.contains("Could this be"));
        assert!(text.contains("simpler?▏"), "the caret follows the text");
        assert!(text.contains("ctrl-s save draft"));
        assert!(text.contains("ctrl-e $EDITOR"));
        assert!(text.contains("ctrl-g suggestion"));
    }

    #[test]
    fn the_submit_form_counts_drafts_and_limits_authors() {
        let mut detail = fixtures::pull_request(17, Vec::new());
        detail.viewer.is_author = true;
        let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-ui-tests"));
        app.open_pull_request_for_test(detail, vec![fixtures::file("src/app.rs")]);
        app.save_draft_for_test(draft_on_two("Nit."));
        app.open_submit_review_for_test();

        let text = buffer_text(&render_to_buffer(&mut app, 120, 30));
        print(&text);

        assert!(text.contains("Submit review · #17"));
        assert!(text.contains("● Comment"));
        assert!(text.contains("your pull request: comment only"));
        assert!(text.contains("Sends 1 draft comment."));
        assert!(text.contains("tab verdict"));
        assert!(text.contains("ctrl-s submit"));
    }

    #[test]
    fn pull_request_footer_names_the_branches_under_review() {
        let mut app = pull_request_app(Vec::new());
        let text = buffer_text(&render_to_buffer(&mut app, 160, 20));

        assert!(
            text.lines()
                .last()
                .is_some_and(|footer| footer.contains("#17  master ← review/remove-codex")),
            "{text}"
        );
    }
}
