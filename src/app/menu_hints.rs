//! A menu's hint fades as the key is learned. A row fired by a click or `Enter`
//! names the key that does the same (`mouse::name_the_key`) until the action
//! has been fired by its own key `[ui] menu_hints_fade_after` times. Only a key
//! counts, so clicking a row can't silence the lesson it is there to give. The
//! counts and the first-menu flag are the Model's `state.learned`; each change
//! returns an `Effect::SaveLearned` for the executor to write `learned.json`.
//!
//! Also `:hints`, which lists what has faded, or with `reset` forgets it all.

use super::{App, Effect};
use crate::config::{MenuHints, UiConfig};
use crate::keymap::Action;
use crate::state::Learned;

impl App {
    /// Count `action` as fired by its key, toward its menu hint's fade, and
    /// return the write the count needs if it moved.
    pub(super) fn learn_key(&mut self, action: &Action) -> Option<Effect> {
        let cap = self.state.active_project().config.ui.count_cap()?;
        self.state
            .learned
            .record(action.canonical_name(), cap)
            .then(|| Effect::SaveLearned(self.state.learned.clone()))
    }

    /// Note that an object's menu has opened, which retires the status bar's
    /// `. menu`, and return the write that needs the first time.
    pub(super) fn learn_menu(&mut self) -> Option<Effect> {
        if !self.state.active_project().config.ui.teaches() {
            return None;
        }
        self.state
            .learned
            .see_menu()
            .then(|| Effect::SaveLearned(self.state.learned.clone()))
    }

    /// Whether a menu row for the action named `name` still names its key.
    pub(super) fn names_the_key(&self, name: &str) -> bool {
        let ui = self.state.active_project().config.ui;
        ui.names_the_key(self.state.learned.count(name))
    }

    /// Whether the status bar still shows `. menu`: no object's menu has ever
    /// opened, and the menus teach.
    pub(super) fn teaches_the_menu_key(&self) -> bool {
        !self.state.learned.menu_seen && self.state.active_project().config.ui.teaches()
    }
}

/// The executor's half of `Effect::SaveLearned`. A failed write is logged, not
/// flashed: the next change writes the whole file again.
pub(super) fn persist(learned: &Learned) {
    if let Err(e) = learned.save() {
        crate::spyc_debug!("learned.json: {e:#}");
    }
}

/// `:hints` lists the actions whose menu hint has faded; `:hints reset` forgets
/// every count and the first-menu flag, so the hints and `. menu` return.
pub(super) fn cmd_hints(app: &mut App, args: &str) -> Vec<Effect> {
    match args {
        "" => {
            let ui = app.state.active_project().config.ui;
            let lines = hints_lines(&app.state.learned, ui);
            app.set_pager(crate::ui::pager::PagerView::new_plain("hints", lines));
            Vec::new()
        }
        "reset" => {
            app.state.learned = Learned::default();
            app.state
                .flash_info("hints: every count forgotten; the menus name their keys again");
            vec![Effect::SaveLearned(Learned::default())]
        }
        _ => {
            app.state.flash_error("usage: :hints [reset]");
            Vec::new()
        }
    }
}

/// `:hints`'s pager: each faded action as its label and its `.spycrc.toml`
/// name, sorted by label. A name this spyc doesn't know is left out: it has no
/// row in this spyc's menus.
fn hints_lines(learned: &Learned, ui: UiConfig) -> Vec<String> {
    let threshold = ui.menu_hints_fade_after;
    match ui.menu_hints {
        MenuHints::Off => return vec!["Menu hints are off: `[ui] menu_hints = \"off\"`.".into()],
        MenuHints::Always => {
            return vec!["Menu hints never fade: `[ui] menu_hints = \"always\"`.".into()];
        }
        MenuHints::Fade => {}
    }
    let mut rows: Vec<(&str, &str)> = learned
        .faded(threshold)
        .filter_map(|name| {
            Some((
                crate::keymap::action::action_from_name(name)?.describe(),
                name,
            ))
        })
        .collect();
    if rows.is_empty() {
        return vec![format!(
            "No menu hint has faded yet. A menu stops naming an action's key once \
             that key has fired it {threshold} times."
        )];
    }
    rows.sort_unstable();
    let width = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let mut lines = vec![
        format!("Faded menu hints: each action below has been fired by its key {threshold} times."),
        "`:hints reset` brings them back.".into(),
        String::new(),
    ];
    lines.extend(
        rows.iter()
            .map(|(label, name)| format!("{label:<width$}  {name}")),
    );
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_lines_name_what_has_faded_by_label_and_name() {
        let mut learned = Learned::default();
        learned.counts.insert("goto_last".into(), 3);
        learned.counts.insert("goto_first".into(), 3);
        learned.counts.insert("toggle_pick".into(), 2);
        learned.counts.insert("a_future_action".into(), 3);
        let lines = hints_lines(&learned, UiConfig::default());
        let rows: Vec<&str> = lines.iter().skip(3).map(String::as_str).collect();
        assert_eq!(
            rows,
            [
                "bottom of column  goto_last",
                "top of column     goto_first"
            ]
        );
    }

    #[test]
    fn hints_lines_say_when_nothing_has_faded() {
        let lines = hints_lines(&Learned::default(), UiConfig::default());
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with("No menu hint has faded yet"),
            "{lines:?}"
        );
    }
}
