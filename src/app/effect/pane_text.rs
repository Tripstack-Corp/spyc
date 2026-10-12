//! Pane-text effects: clipboard delivery and output navigation.
use super::{PaneTextKind, PaneTextSink};
use crate::app::App;

impl App {
    pub(in crate::app) fn execute_pane_text(&mut self, kind: PaneTextKind, then: PaneTextSink) {
        let Some((mut lines, pane_cwd, agent, history)) =
            self.runtime.pane_tabs.as_mut().map(|tabs| {
                let lines = match kind {
                    PaneTextKind::Visible => tabs.active_mut().visible_lines(),
                    PaneTextKind::Scrollback(n) => tabs.active_mut().recent_lines(n),
                    PaneTextKind::Pickable(n) => tabs.active_mut().pickable_text(n),
                };
                let info = tabs.active_info();
                (
                    lines,
                    info.cwd.clone(),
                    info.agent(),
                    tabs.active().is_scrolling(),
                )
            })
        else {
            if !matches!(then, PaneTextSink::JumpDefault) {
                self.state.flash_error("no pane open");
            }
            return;
        };
        let codex_history = history && agent.kind() == crate::state::sessions::AgentKind::Codex;
        if !(matches!(then, PaneTextSink::Clipboard { .. }) || codex_history) {
            lines.truncate(agent.output_len(&lines));
        }
        match then {
            // A-class: join + copy, byte-identical to the former
            // inline yank sites (`yp`/`ya`).
            PaneTextSink::Clipboard { ok } => {
                let text = lines
                    .iter()
                    .map(|l| l.trim_end())
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.trim().is_empty() {
                    // Empty-flash string is per-kind, byte-identical
                    // to the former inline yank sites for the two
                    // kinds yank actually uses (Visible / Scrollback).
                    // Pickable only ever pairs with `GotoFile`, never
                    // Clipboard — its arm here is for exhaustiveness.
                    self.state.flash_error(match kind {
                        PaneTextKind::Visible | PaneTextKind::Pickable(_) => "pane is empty",
                        PaneTextKind::Scrollback(_) => "pane scrollback is empty",
                    });
                } else {
                    match self.deliver_clipboard(&text) {
                        Ok(()) => self.state.flash_info(ok.success(&text)),
                        Err(e) => {
                            self.state.flash_error(format!("yank failed: {e:#}"));
                        }
                    }
                }
            }
            // C-class: gf/gF. Resolve a path reference from the
            // pickable lines and navigate to it (synchronous, same
            // tick); gF also opens it in the pager at the line.
            PaneTextSink::GotoFile { open_at_line } => {
                self.goto_file_navigate(lines, pane_cwd, open_at_line);
            }
            PaneTextSink::JumpDefault => {
                self.spawn_jump_default(lines, pane_cwd);
            }
        }
    }
}
