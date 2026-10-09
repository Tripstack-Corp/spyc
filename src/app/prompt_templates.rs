//! Prompt templates: a named message from `[prompts]` in `~/.spycrc.toml`,
//! typed into the active pane tab with the selection, the inventory or the
//! current directory filled in. A keyboard launcher for the prompts you send an
//! agent over and over.
//!
//! A template is split at its tokens when the key is pressed, which is when a
//! path that can't be typed faithfully is refused. It is rendered when it is
//! delivered, against the receiving pane's cwd — the `^a s` anchor — so each
//! path reads the way the agent's own process resolves it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::App;
use super::effect::{Effect, PaneInput, PaneTarget};

/// A template split at its tokens, with every path it names already checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptText(Vec<Piece>);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(String),
    Paths(Vec<PathBuf>),
}

/// What a template's tokens stand for.
pub(super) struct Sources<'a> {
    /// `%`: the picks, else the cursor row.
    pub selection: &'a [&'a Path],
    /// `%i`: the inventory's picked items, else all of them.
    pub inventory: &'a [&'a Path],
    /// `%d`: the focused column's directory.
    pub dir: &'a Path,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ComposeError {
    /// A path that isn't UTF-8 can't be typed as itself; a lossy copy would
    /// name a different file than the one the user meant.
    NonUtf8(PathBuf),
    /// The token names nothing, so the prompt would be about nothing.
    Empty(&'static str),
}

impl std::fmt::Display for ComposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonUtf8(p) => write!(
                f,
                "prompt: {} isn't a UTF-8 path, so it can't be typed",
                p.display()
            ),
            Self::Empty("%i") => write!(f, "prompt: the inventory is empty, so %i names nothing"),
            Self::Empty(token) => write!(f, "prompt: nothing selected for {token}"),
        }
    }
}

/// Split `template` at its tokens: `%` the selection, `%i` the inventory, `%d`
/// the directory, and `%%` a literal percent sign.
pub(super) fn compose(template: &str, src: &Sources<'_>) -> Result<PromptText, ComposeError> {
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            text.push(ch);
            continue;
        }
        let (token, paths): (&'static str, &[&Path]) = match chars.peek() {
            Some('%') => {
                chars.next();
                text.push('%');
                continue;
            }
            Some('i') => {
                chars.next();
                ("%i", src.inventory)
            }
            Some('d') => {
                chars.next();
                ("%d", std::slice::from_ref(&src.dir))
            }
            _ => ("%", src.selection),
        };
        if paths.is_empty() {
            return Err(ComposeError::Empty(token));
        }
        if let Some(bad) = paths.iter().find(|p| p.to_str().is_none()) {
            return Err(ComposeError::NonUtf8(bad.to_path_buf()));
        }
        if !text.is_empty() {
            pieces.push(Piece::Text(std::mem::take(&mut text)));
        }
        pieces.push(Piece::Paths(
            paths.iter().map(|p| p.to_path_buf()).collect(),
        ));
    }
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
    Ok(PromptText(pieces))
}

impl PromptText {
    /// The text to type into a pane whose cwd is `cwd`.
    pub fn render(&self, cwd: Option<&Path>) -> String {
        let mut out = String::new();
        for piece in &self.0 {
            match piece {
                Piece::Text(t) => out.push_str(t),
                Piece::Paths(paths) => {
                    for (i, p) in paths.iter().enumerate() {
                        if i > 0 {
                            out.push(' ');
                        }
                        out.push_str(&crate::shell::pane_path(p, cwd));
                    }
                }
            }
        }
        out
    }
}

impl App {
    /// `map KEY prompt <name>` / `:prompt <name>`: type the template into the
    /// active pane tab and focus the pane. It is never submitted: the text is
    /// the user's to check and finish, and Enter sends it.
    pub(super) fn send_prompt(&mut self, name: &str) -> Vec<Effect> {
        if self.runtime.pane_tabs.is_none() {
            self.state.flash_info("no pane open (Ctrl-\\ to open one)");
            return Vec::new();
        }
        let Some(template) = self.state.config.prompts.get(name) else {
            self.state.flash_error(format!(
                "no prompt named '{name}' in [prompts] (:prompt lists them)"
            ));
            return Vec::new();
        };
        let selection = self.state.selection_paths();
        let picked: HashSet<String> = self.state.inventory.selected_ids().into_iter().collect();
        let inventory: Vec<&Path> = self
            .state
            .inventory
            .items()
            .filter(|item| picked.contains(&item.id))
            .map(|item| item.orig_path.as_path())
            .collect();
        let composed = compose(
            template,
            &Sources {
                selection: &selection,
                inventory: &inventory,
                dir: &self.state.cur().listing.dir,
            },
        );
        match composed {
            Ok(text) => {
                self.set_pane_focus(true);
                vec![Effect::SendToPane {
                    target: PaneTarget::Active,
                    input: PaneInput::Prompt(text),
                    on_ok: Some(format!("prompt '{name}' typed — Enter sends it")),
                    err_prefix: Some("prompt failed"),
                }]
            }
            Err(e) => {
                self.state.flash_error(e.to_string());
                Vec::new()
            }
        }
    }

    /// `:prompt <name>` types a template; a bare `:prompt` lists them.
    pub(super) fn cmd_prompt(&mut self, args: &str) -> Vec<Effect> {
        let name = args.trim();
        if !name.is_empty() {
            return self.send_prompt(name);
        }
        if self.state.config.prompts.is_empty() {
            self.state
                .flash_info("no prompts yet: define them under [prompts] in ~/.spycrc.toml");
        } else {
            let names: Vec<&str> = self
                .state
                .config
                .prompts
                .keys()
                .map(String::as_str)
                .collect();
            self.state
                .flash_info(format!("prompts: {}", names.join(", ")));
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> &Path {
        Path::new(s)
    }

    fn render(template: &str, src: &Sources<'_>, cwd: Option<&str>) -> String {
        compose(template, src)
            .expect("composes")
            .render(cwd.map(Path::new))
    }

    #[test]
    fn each_token_names_its_source() {
        let src = Sources {
            selection: &[p("/r/a.rs"), p("/r/b.rs")],
            inventory: &[p("/r/notes.md")],
            dir: p("/r"),
        };
        assert_eq!(
            render("review % against %i in %d, 100%% done", &src, None),
            "review /r/a.rs /r/b.rs against /r/notes.md in /r, 100% done"
        );
    }

    /// The `^a s` anchor: under the receiving pane's cwd a path goes out
    /// relative, and the directory itself as `.`; anything else stays
    /// absolute.
    #[test]
    fn paths_read_the_way_the_receiving_pane_resolves_them() {
        let src = Sources {
            selection: &[p("/r/src/a.rs"), p("/elsewhere/b.rs")],
            inventory: &[],
            dir: p("/r"),
        };
        assert_eq!(
            render("% from %d", &src, Some("/r")),
            "src/a.rs /elsewhere/b.rs from ."
        );
    }

    #[test]
    fn a_path_with_a_space_stays_one_word() {
        let src = Sources {
            selection: &[p("/r/my file.rs")],
            inventory: &[],
            dir: p("/r"),
        };
        assert_eq!(render("see %", &src, None), "see '/r/my file.rs'");
    }

    #[test]
    fn text_passes_through_verbatim() {
        let src = Sources {
            selection: &[p("/r/a.rs")],
            inventory: &[],
            dir: p("/r"),
        };
        assert_eq!(
            render("line one\nline two, 50%%i, then %", &src, None),
            "line one\nline two, 50%i, then /r/a.rs"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_path_is_refused_where_it_is_named() {
        use std::os::unix::ffi::OsStrExt;
        let bad = Path::new(std::ffi::OsStr::from_bytes(b"/r/\xff.rs"));
        let src = Sources {
            selection: &[bad],
            inventory: &[p("/r/notes.md")],
            dir: p("/r"),
        };
        assert_eq!(
            compose("explain %", &src),
            Err(ComposeError::NonUtf8(bad.to_path_buf()))
        );
        assert_eq!(render("summarize %i", &src, None), "summarize /r/notes.md");
    }

    #[test]
    fn a_token_with_nothing_behind_it_is_refused() {
        let src = Sources {
            selection: &[],
            inventory: &[],
            dir: p("/r"),
        };
        assert_eq!(compose("explain %", &src), Err(ComposeError::Empty("%")));
        assert_eq!(
            compose("summarize %i", &src),
            Err(ComposeError::Empty("%i"))
        );
        assert_eq!(render("what's in %d?", &src, None), "what's in /r?");
    }
}
