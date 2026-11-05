//! The pager as an object: [`PagerAction`], what an open pager's menu offers,
//! each row with the key that does the same, and [`pager_menu`], the rows a
//! pager's state applies to. The pager's own keys classify through the same
//! enum (`of_capture_key`, `of_ctrl_c`, `of_bracket`, `of_key`), so a key and a
//! click on its row both run [`App::apply_pager_action`]; a key, through
//! [`App::apply_pager_key`], also counts toward its row's hint fading. Motion
//! and search keys stay in the raw match: they move through the pager rather
//! than act on it.
//!
//! `keymap`'s `menu_for` builds the rows of `Action`s, and `keymap` can't name
//! an app type, so a pager's rows are their own kind of popup entry
//! (`view_state::HintEntry::Pager`), drawn, hit-tested and fired as every
//! menu's rows are.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Effect, TaskStatus};

/// What an open pager's menu offers. A live `!` capture's three keys are its
/// own; the task viewer's act on the task it shows; save and close are any
/// pager's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumIter)]
pub enum PagerAction {
    /// `^Z`: a live capture moves to the background tasks, still capturing.
    Background,
    /// `^C`: SIGINT to the command's process group: a live capture's, or the
    /// task viewer's task.
    Interrupt,
    /// `^\`: a live capture is killed and torn down; its output stays.
    Kill,
    /// `:fg N`: the task viewer's task comes back as the live capture.
    Foreground,
    /// `S`: SIGSTOP to the task viewer's task.
    Pause,
    /// `C`: SIGCONT to the task viewer's paused task.
    Continue,
    /// `:task-to-pane N`: the task viewer's task becomes a pane tab.
    ToPane,
    /// `]t`: the task viewer steps to the next task.
    NextTask,
    /// `[t`: the task viewer steps to the previous task.
    PrevTask,
    /// `s`: what the pager holds goes to a file.
    Save,
    /// `q`: the pager closes.
    Close,
}

/// What a pager's menu depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagerFacts {
    /// A live `!` capture streams into the pager and takes every key but its
    /// three, so only those apply.
    pub capture: bool,
    /// The task the pager shows, when it is the task viewer.
    pub task: Option<TaskFacts>,
    /// `s` saves what the pager holds.
    pub saveable: bool,
}

/// What the task viewer's rows depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskFacts {
    /// Its number, as `:fg` and `:task-to-pane` take it.
    pub id: u32,
    /// Its command hasn't exited: it can be interrupted, paused, or moved to
    /// a pane tab.
    pub running: bool,
    /// Stopped by `S` or `:pause`.
    pub paused: bool,
    /// Another background task exists for `]t` and `[t` to step to.
    pub others: bool,
}

impl PagerAction {
    /// The row's label.
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Background => "background the capture",
            Self::Interrupt => "interrupt the command",
            Self::Kill => "kill the command",
            Self::Foreground => "bring it to the foreground",
            Self::Pause => "pause the task",
            Self::Continue => "continue the task",
            Self::ToPane => "move it to a pane tab",
            Self::NextTask => "next task",
            Self::PrevTask => "previous task",
            Self::Save => "save to a file",
            Self::Close => "close the pager",
        }
    }

    /// The name the row's key is counted under in `learned.json`, which its
    /// menu hint fades on. No `Action`'s `canonical_name` starts `pager_`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Background => "pager_background",
            Self::Interrupt => "pager_interrupt",
            Self::Kill => "pager_kill",
            Self::Foreground => "pager_foreground",
            Self::Pause => "pager_pause",
            Self::Continue => "pager_continue",
            Self::ToPane => "pager_to_pane",
            Self::NextTask => "pager_next_task",
            Self::PrevTask => "pager_prev_task",
            Self::Save => "pager_save",
            Self::Close => "pager_close",
        }
    }

    /// The `:` command a row runs on the task viewer's task, which the pager
    /// has no key for: in a pager `:` starts a line jump.
    pub const fn command(self) -> Option<&'static str> {
        match self {
            Self::Foreground => Some("fg"),
            Self::ToPane => Some("task-to-pane"),
            _ => None,
        }
    }

    /// The key the row shows, as it is typed. A command takes the task's
    /// number, and is typed from the file list once the pager is closed.
    pub fn key(self, task: Option<u32>) -> String {
        let key = match self {
            Self::Background => "^Z",
            Self::Interrupt => "^C",
            Self::Kill => "^\\",
            Self::Pause => "S",
            Self::Continue => "C",
            Self::NextTask => "]t",
            Self::PrevTask => "[t",
            Self::Save => "s",
            Self::Close => "q",
            Self::Foreground | Self::ToPane => {
                let command = self.command().unwrap_or_default();
                let id = task.map(|id| format!(" {id}")).unwrap_or_default();
                return format!(":{command}{id} (list)");
            }
        };
        key.to_string()
    }

    /// What `key` does to a live `!` capture: `^C`, `^\` and `^Z` are
    /// spyc's, and `None` sends any other to the command, so a prompt can be
    /// answered.
    pub const fn of_capture_key(key: KeyEvent) -> Option<Self> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => Some(Self::Interrupt),
            KeyCode::Char('\\') if ctrl => Some(Self::Kill),
            KeyCode::Char('z') if ctrl => Some(Self::Background),
            _ => None,
        }
    }

    /// `^C` in an open pager: the task viewer interrupts its task. Read
    /// before any of the pager's modes, so it works mid-search too.
    pub const fn of_ctrl_c(key: KeyEvent, task_viewer: bool) -> Option<Self> {
        let ctrl_c =
            matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl_c && task_viewer {
            Some(Self::Interrupt)
        } else {
            None
        }
    }

    /// The task `[t` or `]t` steps to, `bracket` being the key typed first.
    pub const fn of_bracket(bracket: char, key: KeyEvent) -> Option<Self> {
        match key.code {
            KeyCode::Char('t') if bracket == '[' => Some(Self::PrevTask),
            KeyCode::Char('t') => Some(Self::NextTask),
            _ => None,
        }
    }

    /// The pager's own keys, once no mode has taken `key`: `q`, `Q` and `Esc`
    /// close it, `S` and `C` pause and continue the task viewer's task, and
    /// `s` saves what can be saved.
    pub const fn of_key(key: KeyEvent, task_viewer: bool, saveable: bool) -> Option<Self> {
        match key.code {
            KeyCode::Char('q' | 'Q') | KeyCode::Esc => Some(Self::Close),
            KeyCode::Char('S') if task_viewer => Some(Self::Pause),
            KeyCode::Char('C') if task_viewer => Some(Self::Continue),
            KeyCode::Char('s') if saveable => Some(Self::Save),
            _ => None,
        }
    }
}

/// The rows of a pager's menu, in the order they are drawn: each only where
/// its action would not be refused. A live capture has its three; the task
/// viewer's rows follow its task; save and close are any other pager's.
pub fn pager_menu(f: PagerFacts) -> Vec<PagerAction> {
    use PagerAction as P;
    if f.capture {
        return vec![P::Background, P::Interrupt, P::Kill];
    }
    let task = |test: fn(TaskFacts) -> bool| f.task.is_some_and(test);
    [
        (task(|t| t.running), P::Interrupt),
        (f.task.is_some(), P::Foreground),
        (task(|t| t.running && !t.paused), P::Pause),
        (task(|t| t.running && t.paused), P::Continue),
        (task(|t| t.running), P::ToPane),
        (task(|t| t.others), P::NextTask),
        (task(|t| t.others), P::PrevTask),
        (f.saveable, P::Save),
        (true, P::Close),
    ]
    .into_iter()
    .filter_map(|(shown, action)| shown.then_some(action))
    .collect()
}

impl App {
    /// `action` fired by its key in the pager: counted toward its row's hint
    /// fading (`menu_hints`), then done as a click on the row does it. The
    /// count's write goes first, as an `Action` key's does.
    pub(in crate::app) fn apply_pager_key(&mut self, action: PagerAction) -> Vec<Effect> {
        let mut effects: Vec<Effect> = self.learn_pager_key(action).into_iter().collect();
        effects.extend(self.apply_pager_action(action));
        effects
    }

    /// Do `action` to the pager that owns input: what its key does there, and
    /// what a click on its menu row does.
    pub(in crate::app) fn apply_pager_action(&mut self, action: PagerAction) -> Vec<Effect> {
        let task = self.active_pager_ref().and_then(|v| v.task_id);
        match action {
            PagerAction::Background => {
                self.background_capture();
                Vec::new()
            }
            PagerAction::Interrupt if self.runtime.pending_capture.is_some() => {
                self.interrupt_capture()
            }
            PagerAction::Interrupt => {
                if let Some(id) = task {
                    match self.interrupt_task(Some(id)) {
                        Ok(msg) | Err(msg) => self.set_active_pager_flash(msg),
                    }
                }
                Vec::new()
            }
            PagerAction::Kill => self.kill_capture(),
            PagerAction::Foreground | PagerAction::ToPane => match (action.command(), task) {
                (Some(command), Some(id)) => self.dispatch_command(&format!("{command} {id}")),
                _ => Vec::new(),
            },
            PagerAction::Pause => task.map_or_else(Vec::new, |id| self.pause_task(Some(id))),
            PagerAction::Continue => task.map_or_else(Vec::new, |id| self.resume_task(Some(id))),
            PagerAction::NextTask | PagerAction::PrevTask => {
                self.cycle_task_viewer(if action == PagerAction::PrevTask {
                    -1
                } else {
                    1
                });
                Vec::new()
            }
            PagerAction::Save => self
                .active_pager_ref()
                .filter(|v| v.saveable)
                .map(|v| {
                    vec![Effect::SavePagerOutput {
                        content: v.save_content(),
                    }]
                })
                .unwrap_or_default(),
            PagerAction::Close => {
                self.close_pager_by_key();
                Vec::new()
            }
        }
    }

    /// `^C` in a live capture: SIGINT to its foreground process group, as a
    /// real `^C` in its tty would send, rather than the 0x03 byte a raw-mode
    /// reader ignores.
    fn interrupt_capture(&self) -> Vec<Effect> {
        let Some(capture) = self.runtime.pending_capture.as_ref() else {
            return Vec::new();
        };
        #[cfg(unix)]
        {
            capture
                .host
                .foreground_pgrp()
                .or_else(|| capture.host.process_id())
                .map(|pgrp| {
                    vec![Effect::SignalCapture {
                        pgrp,
                        sig: rustix::process::Signal::INT,
                    }]
                })
                .unwrap_or_default()
        }
        // No signal path off-unix: the 0x03 byte, as `^C` encodes.
        #[cfg(not(unix))]
        {
            let _ = capture;
            vec![Effect::SendToCapture { bytes: vec![0x03] }]
        }
    }

    /// `^\` in a live capture: SIGKILL and tear it down, leaving its pager
    /// open on what it printed, marked interrupted.
    fn kill_capture(&mut self) -> Vec<Effect> {
        let Some(capture) = self.runtime.pending_capture.as_mut() else {
            return Vec::new();
        };
        // Also kill the command's own foreground group: job control
        // under `zsh -i -c` runs it apart from the wrapper shell that
        // Drop/`child.kill()` target, which would otherwise orphan it.
        #[cfg(unix)]
        let fg_pgrp = capture
            .host
            .foreground_pgrp()
            .filter(|&fg| Some(fg) != capture.host.process_id());
        let _ = capture.host.child.kill();
        let _ = capture.host.child.wait();
        // ✗ — same non-clean-termination glyph the status bar uses.
        let title = format!("\u{2717} {} — interrupted", capture.title);
        if let Some(view) = self.view.project_mut().pager.as_mut() {
            view.title = title;
            view.saveable = true;
            view.streaming = false;
        }
        // Clear the wake slot before dropping the host so the EOF
        // close-wake fires through a None slot.
        capture.host.clear_wake_slot();
        self.runtime.pending_capture = None;
        #[cfg(unix)]
        if let Some(pgrp) = fg_pgrp {
            return vec![Effect::SignalCapture {
                pgrp,
                sig: rustix::process::Signal::KILL,
            }];
        }
        Vec::new()
    }

    /// The pager that owns input as a menu target, with its title. A picker
    /// (worktrees, projects, sessions, history) has none: its keys are its
    /// own, and `q` may be typed into it.
    pub(in crate::app) fn pager_facts(&self) -> Option<(PagerFacts, String)> {
        let view = self.active_pager_ref().filter(|v| v.picker.is_none())?;
        let capture = self.runtime.pending_capture.is_some()
            && matches!(
                self.active_pager_slot(),
                super::PagerSlot::Modal | super::PagerSlot::Top
            );
        let tasks = &self.runtime.background_tasks.tasks;
        let task = view.task_id.and_then(|id| {
            let t = tasks.iter().find(|t| t.id == id)?;
            Some(TaskFacts {
                id,
                running: matches!(t.status, TaskStatus::Running),
                paused: t.paused,
                others: tasks.len() > 1,
            })
        });
        let facts = PagerFacts {
            capture,
            task,
            saveable: view.saveable,
        };
        Some((facts, view.title.trim().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{PagerAction, PagerFacts, TaskFacts, pager_menu};
    use crate::app::command_table::COMMAND_TABLE;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use strum::IntoEnumIterator;

    /// Every pager a menu can be for: a live capture, the task viewer on each
    /// kind of task, and any other pager, saveable or not.
    fn every_pager() -> Vec<PagerFacts> {
        let mut out = vec![PagerFacts {
            capture: true,
            task: None,
            saveable: false,
        }];
        for saveable in [false, true] {
            out.push(PagerFacts {
                capture: false,
                task: None,
                saveable,
            });
            for bits in 0..8u8 {
                let bit = |n: u8| bits & (1 << n) != 0;
                out.push(PagerFacts {
                    capture: false,
                    task: Some(TaskFacts {
                        id: 3,
                        running: bit(0),
                        paused: bit(1),
                        others: bit(2),
                    }),
                    saveable,
                });
            }
        }
        out
    }

    /// The keystrokes a row's key names: `^x` is `x` with Ctrl, and each other
    /// character is one key.
    fn keystrokes(keys: &str) -> Vec<KeyEvent> {
        if let Some(c) = keys.strip_prefix('^') {
            let c = c.chars().next().expect("a key follows `^`");
            return vec![KeyEvent::new(
                KeyCode::Char(c.to_ascii_lowercase()),
                KeyModifiers::CONTROL,
            )];
        }
        keys.chars()
            .map(|c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
            .collect()
    }

    /// What `keys` do in an open pager that holds no search, jump or
    /// selection: `^C` is read first, a `[` or `]` waits for the key after
    /// it, and the rest reach the pager's own keys, as `dispatch_pager_key`
    /// orders them.
    fn in_the_pager(keys: &[KeyEvent], f: PagerFacts) -> Option<PagerAction> {
        let task_viewer = f.task.is_some();
        let mut bracket = None;
        let mut done = None;
        for &key in keys {
            done = PagerAction::of_ctrl_c(key, task_viewer).or_else(|| {
                if let Some(b) = bracket.take() {
                    return PagerAction::of_bracket(b, key);
                }
                match key.code {
                    KeyCode::Char(c @ ('[' | ']')) => {
                        bracket = Some(c);
                        None
                    }
                    _ => PagerAction::of_key(key, task_viewer, f.saveable),
                }
            });
        }
        done
    }

    /// Each action is on some pager's menu: a variant no menu offers fails
    /// here until a pager offers it.
    #[test]
    fn every_pager_action_is_on_some_pagers_menu() {
        let offered: Vec<PagerAction> = every_pager().into_iter().flat_map(pager_menu).collect();
        let mut checked = 0;
        for action in PagerAction::iter() {
            assert!(
                offered.contains(&action),
                "{action:?} is on no pager's menu: add it to `pager_menu`"
            );
            checked += 1;
        }
        assert!(checked >= 11, "only {checked} pager actions checked");
    }

    /// Each pager row's key does what a click on the row does, so the menu
    /// teaches a key that works there: fed through the capture's keys or the
    /// pager's, it is the row's action, and a `:` row names the command the
    /// row runs, with the task's number.
    #[test]
    fn every_pager_rows_key_fires_its_action() {
        let mut checked = 0;
        for f in every_pager() {
            let id = f.task.map(|t| t.id);
            for action in pager_menu(f) {
                let key = action.key(id);
                if let Some(typed) = key.strip_suffix(" (list)") {
                    let typed = typed
                        .strip_prefix(':')
                        .expect("a `(list)` row is a command");
                    let (name, args) = typed.split_once(' ').unwrap_or((typed, ""));
                    assert_eq!(Some(name), action.command(), "{key}: the row's command");
                    assert!(
                        COMMAND_TABLE.iter().any(|c| c.name == name),
                        "{key}: no such command"
                    );
                    assert_eq!(args, id.map(|n| n.to_string()).unwrap_or_default(), "{key}");
                } else {
                    let strokes = keystrokes(&key);
                    let fired = if f.capture {
                        let [stroke] = strokes[..] else {
                            panic!("{key}: a capture's key is one keystroke");
                        };
                        PagerAction::of_capture_key(stroke)
                    } else {
                        in_the_pager(&strokes, f)
                    };
                    assert_eq!(fired, Some(action), "{key} in {f:?}");
                }
                checked += 1;
            }
        }
        assert!(checked > 80, "only {checked} rows checked");
    }

    /// A live capture's menu has its three rows and nothing else: every other
    /// key is the command's.
    #[test]
    fn a_live_captures_menu_is_its_three_keys() {
        let capture = PagerFacts {
            capture: true,
            task: None,
            saveable: true,
        };
        assert_eq!(
            pager_menu(capture),
            [
                PagerAction::Background,
                PagerAction::Interrupt,
                PagerAction::Kill
            ]
        );
    }

    /// No `Action` shares a pager action's name, so neither's count fades the
    /// other's hint.
    #[test]
    fn no_action_shares_a_pager_actions_name() {
        for action in PagerAction::iter() {
            assert!(
                crate::keymap::action::action_from_name(action.name()).is_none(),
                "{} is an `Action`'s name",
                action.name()
            );
        }
    }
}
