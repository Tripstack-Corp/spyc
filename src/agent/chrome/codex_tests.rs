use crate::agent::detect;

fn lines(text: &str) -> Vec<String> {
    text.lines().map(String::from).collect()
}

fn boundary(text: &str) -> usize {
    detect("codex").output_len(&lines(text))
}

#[test]
fn current_composers_start_after_the_printed_reply() {
    for (fixture, expected) in [
        (
            include_str!("../../../tests/fixtures/codex-output/draft.txt"),
            2,
        ),
        (
            include_str!("../../../tests/fixtures/codex-output/vim.txt"),
            2,
        ),
        (
            include_str!("../../../tests/fixtures/codex-output/statusline.txt"),
            3,
        ),
    ] {
        let screen = format!("• Printed printed.rs\n{fixture}");
        assert_eq!(boundary(&screen), expected, "{screen}");
    }
}

#[test]
fn multiline_and_ultra_drafts_do_not_become_output() {
    for glyph in ["›", "»", "!"] {
        let screen = format!(
            "• Printed printed.rs\n\n{glyph} inspect draft-only.rs\n  and wrapped.rs\n\n  100% context left | Vim: Insert\n"
        );
        assert_eq!(boundary(&screen), 2, "{glyph}");
    }
}

#[test]
fn trailing_blank_rows_do_not_move_the_live_boundary() {
    let screen = "• Printed printed.rs\n\n› draft-only.rs\n\n  97% context left\n";
    assert_eq!(boundary(&(screen.to_owned() + &"\n".repeat(80))), 2);
}

#[test]
fn prompt_glyphs_without_a_complete_footer_are_output() {
    for screen in [
        "• Here is an example:\n\n› output.rs\n  continued output.rs",
        "• Here is an example:\n\n» output.rs\n\n  custom status without verified markers",
        "› output.rs\n\n  101% context left",
        "› output.rs\n\n  context left",
        "› output.rs\n\n  10% context",
        "  › indented-output.rs\n\n  100% context left",
    ] {
        assert_eq!(boundary(screen), lines(screen).len(), "{screen}");
    }
}

#[test]
fn an_assistant_reply_after_a_prompt_is_retained() {
    let screen = "› submitted.rs\n\n• Printed reply.rs\n\n  100% context left";
    assert_eq!(boundary(screen), lines(screen).len());
}

#[test]
fn complete_current_approval_forms_are_excluded() {
    for fixture in [
        include_str!("../../../tests/fixtures/codex-output/approval-exec.txt"),
        include_str!("../../../tests/fixtures/codex-output/approval-edit.txt"),
        include_str!("../../../tests/fixtures/codex-output/approval-mcp.txt"),
        include_str!("../../../tests/fixtures/codex-approval-exec-narrow.txt"),
        include_str!("../../../tests/fixtures/codex-approval-network.txt"),
    ] {
        let screen = format!("• Printed printed.rs\n{fixture}");
        let cut = boundary(&screen);
        assert!(cut > 0 && cut <= 4, "cut={cut}: {screen}");
        assert_eq!(lines(&screen)[0], "• Printed printed.rs");
        assert!(
            !lines(&screen)[..cut]
                .iter()
                .any(|row| row.contains("Destination:") || row.contains("$ echo"))
        );
    }
}

#[test]
fn incomplete_approvals_and_quoted_modals_remain_output() {
    let approval = include_str!("../../../tests/fixtures/codex-output/approval-exec.txt");
    let clipped = approval.replace("Press enter to confirm or esc to cancel", "Press enter");
    assert_eq!(boundary(&clipped), lines(&clipped).len());
    let quoted = format!("{approval}\n› draft-only.rs\n\n  100% context left");
    assert_eq!(
        boundary(&quoted),
        lines(&(approval.to_owned() + "\n")).len()
    );
}

#[test]
fn shortcut_overlay_is_not_output() {
    let fixture = include_str!("../../../tests/fixtures/codex-output/shortcuts.txt");
    let screen = format!("• Printed printed.rs\n\n{fixture}");
    assert_eq!(boundary(&screen), 2);
}

#[test]
fn native_transcript_views_and_other_agents_retain_their_text() {
    let screen = "T R A N S C R I P T  T R A N S C R I P T\n\n› history.rs\n\n  100% context left";
    assert_eq!(boundary(screen), lines(screen).len());
    let screen = lines("\n› output.rs\n\n  100% context left");
    assert_eq!(detect("sh").output_len(&screen), screen.len());
    assert_eq!(detect("codex").output_len(&[]), 0);
}
