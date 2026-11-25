//! Which notification an agent's turn gets: the pure decision from its
//! activity's `old → new` edge and the project's `[notify]`, and the words
//! it says them in.

use crate::config::{DesktopVia, NotifyConfig};
use crate::pane::AgentActivity;

/// What a status transition should trigger, after `[notify]` gating +
/// focused-tab suppression. The pure output of [`notification_for_transition`]:
/// `system` is `Some((summary, body))` when the OS notifier fires; `osc9` is
/// `Some(message)` when the terminal-escape channel fires; `bell`/`visual` when
/// those fire. All-off = nothing to do.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct NotifyPlan {
    pub(super) system: Option<(String, String)>,
    pub(super) osc9: Option<String>,
    pub(super) bell: bool,
    pub(super) visual: bool,
}

impl NotifyPlan {
    /// No channel fired — the caller emits nothing.
    pub(super) const fn is_silent(&self) -> bool {
        self.system.is_none() && self.osc9.is_none() && !self.bell && !self.visual
    }
}

/// Which desktop-notification mechanisms fire for `via`, given whether spyc is
/// running over SSH. Returns `(system, osc9)`. `Auto` routes to OSC-9 over SSH
/// (so the ping reaches the *client* terminal, not the remote box) and the OS
/// notifier locally — the "just works over SSH" default. Pure.
pub(super) const fn desktop_delivery(via: DesktopVia, is_ssh: bool) -> (bool, bool) {
    match via {
        DesktopVia::Auto => {
            if is_ssh {
                (false, true)
            } else {
                (true, false)
            }
        }
        DesktopVia::System => (true, false),
        DesktopVia::Osc9 => (false, true),
        DesktopVia::Both => (true, true),
    }
}

/// Decide what a tab's `old → new` activity transition should trigger, given the
/// `[notify]` config, whether this tab is the one the user is actively watching
/// (its pane owns the keyboard), and whether spyc is over SSH. Pure + testable.
///
/// Only a *fresh* transition INTO `Blocked` ("needs me") or `Done` (a finished
/// turn) fires anything; a repeat or any other target is silent, as is a focused
/// tab when `suppress_focused_tab`. Beyond that, each channel decides *which*
/// edge it fires on: `Blocked` fires every enabled channel, but the routine
/// `Done` edge (once per turn) fires a channel only if it opts in via its
/// `*_done` flag — `desktop_done` for the ping, `bell_done` for the bell,
/// `visual_done` for the flash. The default splits by intrusiveness: the quiet
/// desktop ping notifies on `Done`, the interrupting bell + on-screen flash stay
/// `Blocked`-only so they don't fire per turn. The desktop mechanism(s) come from
/// [`desktop_delivery`]. Returns a silent plan when no channel fires.
pub(super) fn notification_for_transition(
    old: AgentActivity,
    new: AgentActivity,
    cfg: &NotifyConfig,
    tab_focused: bool,
    is_ssh: bool,
    who: &Subject,
) -> NotifyPlan {
    // A fresh edge into a notify-worthy state; `is_done` distinguishes the
    // routine finished-turn edge (per-channel opt-in) from `Blocked` (fires all).
    let is_done = match new {
        AgentActivity::Blocked if old != AgentActivity::Blocked => false,
        // A reported error needs the user as a block does: every channel.
        AgentActivity::Error if old != AgentActivity::Error => false,
        AgentActivity::Done if old != AgentActivity::Done => true,
        _ => return NotifyPlan::default(),
    };
    if cfg.suppress_focused_tab && tab_focused {
        return NotifyPlan::default();
    }
    // On a `Done` edge each channel needs its own `*_done` opt-in; `Blocked`
    // (`!is_done`) fires every enabled channel.
    let want_desktop = cfg.desktop && (!is_done || cfg.desktop_done);
    let want_bell = cfg.bell && (!is_done || cfg.bell_done);
    let want_visual = cfg.visual && (!is_done || cfg.visual_done);

    let (want_system, want_osc9) = if want_desktop {
        desktop_delivery(cfg.desktop_via, is_ssh)
    } else {
        (false, false)
    };
    let (system, osc9) = if want_system || want_osc9 {
        let (summary, body) = notification_text(who, new);
        // OSC-9 takes one line; fold the two parts into it.
        let osc9 = want_osc9.then(|| format!("{summary} — {body}"));
        let system = want_system.then_some((summary, body));
        (system, osc9)
    } else {
        (None, None)
    };
    NotifyPlan {
        system,
        osc9,
        bell: want_bell,
        visual: want_visual,
    }
}

/// Who a notification is about: the tab's label and its number in its
/// project, the project, named only while several are open, and what a
/// blocked one waits on ([`crate::pane::TabInfo::waiting_for`]).
pub(super) struct Subject<'a> {
    pub(super) label: &'a str,
    pub(super) tab_number: usize,
    pub(super) project: Option<&'a str>,
    pub(super) blocked_on: Option<std::borrow::Cow<'a, str>>,
}

/// Compose the `(summary, body)` for a transition notification: a glanceable
/// "which agent + what" summary plus the tab locator in the body, each naming
/// the project when [`Subject::project`] does, and a `Blocked` summary what
/// it waits on ("claude needs you: Bash"). Only `Blocked` / `Done` reach here
/// (see [`notification_for_transition`]); the fallback arm is a sane
/// non-panicking default.
pub(super) fn notification_text(who: &Subject, new: AgentActivity) -> (String, String) {
    let (label, tab) = (who.label, who.tab_number);
    let (agent, place) = who.project.map_or_else(
        || (label.to_owned(), format!("tab {tab}")),
        |project| {
            (
                format!("{label} in {project}"),
                format!("{project}: tab {tab}"),
            )
        },
    );
    match new {
        AgentActivity::Blocked => {
            let on = who
                .blocked_on
                .as_ref()
                .map_or_else(String::new, |on| format!(": {on}"));
            (
                format!("{agent} needs you{on}"),
                format!("{place} is blocked"),
            )
        }
        AgentActivity::Done => (format!("{agent} finished"), format!("{place} — done")),
        AgentActivity::Error => (
            format!("{agent} failed"),
            format!("{place} reported an error"),
        ),
        _ => (agent, place),
    }
}
