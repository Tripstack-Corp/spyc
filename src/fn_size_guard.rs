//! Guard: how long a function may get.
//!
//! [`crate::size_guard`]'s ratchet, one level down. A function's body stays
//! within [`FUNCTION`]'s limit, counted the way clippy's `too_many_lines` counts:
//! lines that hold code, so blank lines and comments are free. Every function
//! over the limit when the guard landed is pinned at its length then and may
//! only shrink. Clippy's lint stays off in `Cargo.toml` because it can only say
//! a function is too long, not that a long one grew.
//!
//! Test code is not counted: whole-file test modules, `#[cfg(test)]` items and
//! `#[test]` functions. A table of cases is long without being hard to follow.
//!
//! A function is named `path::Type::name`, with any inline `mod`, trait impl
//! (`<Type as Trait>`) or enclosing function in between, so a pin follows the
//! function and not a line number.

use std::path::Path;

use proc_macro2::LineColumn;
use syn::visit::Visit;

use crate::size_guard::{EXEMPT, Rule, is_test_module, rust_sources};

/// Code lines in a function body, from the line after its `{` to the line
/// before its `}`.
const FUNCTION: Rule = Rule {
    list: "FUNCTION",
    unit: "lines of code",
    limit: 100,
    slack: 10,
    advice: "Extract a cohesive helper",
    pins: &[
        ("src/app/actions.rs::App::apply_inner", 360),
        ("src/app/activity_diagnostics.rs::activity_dump_lines", 167),
        ("src/app/agent_status.rs::App::settle_agent_activity", 141),
        ("src/app/archive.rs::App::apply_one_archive_outcome", 134),
        ("src/app/archive_route.rs::classify", 142),
        ("src/app/bootstrap.rs::App::new", 266),
        ("src/app/effect/executor.rs::App::run_effects", 356),
        ("src/app/file_ops.rs::App::apply_one_file_outcome", 144),
        ("src/app/file_ops.rs::run_file_op", 162),
        ("src/app/key_dispatch/mod.rs::App::handle_key", 172),
        (
            "src/app/key_dispatch/prompts.rs::App::handle_prompt_key",
            160,
        ),
        (
            "src/app/key_dispatch/prompts.rs::App::handle_vi_prompt_key",
            144,
        ),
        ("src/app/mcp.rs::App::execute_mcp_command", 386),
        ("src/app/mouse/mod.rs::App::handle_mouse", 136),
        ("src/app/pager_handler/file_view.rs::build_pager_view", 151),
        (
            "src/app/pager_handler/motion.rs::App::handle_pager_motion",
            329,
        ),
        (
            "src/app/pager_handler/pickers.rs::App::handle_pager_history_editor",
            170,
        ),
        ("src/app/prompt.rs::App::dispatch_prompt", 162),
        ("src/app/prompt.rs::App::tab_complete_path", 146),
        (
            "src/app/render/activity_hud.rs::App::render_activity_hud",
            157,
        ),
        (
            "src/app/render/chrome.rs::App::render_pane_status_line",
            196,
        ),
        ("src/app/render/inner.rs::App::render_inner", 141),
        ("src/app/render/mod.rs::App::compute_layout", 183),
        ("src/app/render/overlays.rs::App::render_image_gallery", 129),
        ("src/app/session_restore.rs::App::restore_session", 126),
        ("src/app/state/apply.rs::AppState::apply", 380),
        ("src/app/state/dispatch.rs::AppState::dispatch_command", 201),
        ("src/config/load.rs::Config::merge_file", 179),
        ("src/git/diff_model/build.rs::build_index_change_file", 114),
        ("src/git/diff_model/build.rs::build_tree_change_file", 120),
        ("src/keymap/action.rs::Action::canonical_name", 119),
        ("src/keymap/action.rs::Action::describe", 119),
        ("src/keymap/action.rs::action_from_name", 118),
        ("src/keymap/resolver/mod.rs::Resolver::feed", 456),
        ("src/lua/api.rs::install", 170),
        ("src/mcp/protocol.rs::handle_tools_call", 420),
        (
            "src/pane/engine_ghostty/mod.rs::GhosttyScreen::fill_from_render_state",
            111,
        ),
        ("src/ui/line_edit.rs::LineEditor::feed_normal", 122),
        ("src/ui/markdown/renderer.rs::Renderer::start_tag", 135),
    ],
};

/// Every non-test function under the size guard's roots:
/// `(name, code lines, line of its `fn`)`.
fn functions(repo: &Path) -> Vec<(String, usize, usize)> {
    let mut found = Vec::new();
    for (path, src) in rust_sources(repo) {
        if is_test_module(&path) || EXEMPT.iter().any(|(exempt, _)| *exempt == path) {
            continue;
        }
        found.extend(functions_in(&path, &src));
    }
    found
}

/// The functions in one file, each named and counted.
fn functions_in(path: &str, src: &str) -> Vec<(String, usize, usize)> {
    let file = syn::parse_file(src).unwrap_or_else(|e| panic!("parsing {path}: {e}"));
    let mut finder = Finder {
        lines: src.lines().collect(),
        scope: vec![path.to_string()],
        found: Vec::new(),
    };
    finder.visit_file(&file);
    // Two functions can share a name (`#[cfg(unix)]` / `#[cfg(windows)]`
    // variants); number the repeats so each keeps its own pin.
    let mut seen = std::collections::HashMap::new();
    finder
        .found
        .into_iter()
        .map(|(name, lines, at)| {
            let n = seen.entry(name.clone()).or_insert(0);
            *n += 1;
            if *n == 1 {
                (name, lines, at)
            } else {
                (format!("{name}#{n}"), lines, at)
            }
        })
        .collect()
}

struct Finder<'a> {
    lines: Vec<&'a str>,
    /// The file, then each enclosing `mod`, impl, trait and function.
    scope: Vec<String>,
    found: Vec<(String, usize, usize)>,
}

impl Finder<'_> {
    fn record(&mut self, sig: &syn::Signature, body: &syn::Block) {
        let braces = body.brace_token.span;
        let snippet = self.snippet(braces.open().start(), braces.close().end());
        let qualified = format!("{}::{}", self.scope.join("::"), sig.ident);
        let at = sig.fn_token.span.start().line;
        self.found.push((qualified, code_lines(&snippet), at));
    }

    /// The source text from `start` up to `end`, the way a span covers it.
    fn snippet(&self, start: LineColumn, end: LineColumn) -> String {
        let mut out = String::new();
        for line in start.line..=end.line {
            let text = self.lines[line - 1];
            let from = if line == start.line { start.column } else { 0 };
            let to = if line == end.line {
                end.column
            } else {
                text.chars().count()
            };
            out.extend(text.chars().skip(from).take(to.saturating_sub(from)));
            if line != end.line {
                out.push('\n');
            }
        }
        out
    }

    fn within(&mut self, name: String, visit: impl FnOnce(&mut Self)) {
        self.scope.push(name);
        visit(self);
        self.scope.pop();
    }
}

impl<'ast> Visit<'ast> for Finder<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !is_test_only(&item.attrs) {
            self.within(item.ident.to_string(), |f| {
                syn::visit::visit_item_mod(f, item);
            });
        }
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if !is_test_only(&item.attrs) {
            self.within(impl_name(item), |f| {
                syn::visit::visit_item_impl(f, item);
            });
        }
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        if !is_test_only(&item.attrs) {
            self.within(item.ident.to_string(), |f| {
                syn::visit::visit_item_trait(f, item);
            });
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !is_test_only(&item.attrs) {
            self.record(&item.sig, &item.block);
            self.within(item.sig.ident.to_string(), |f| {
                syn::visit::visit_item_fn(f, item);
            });
        }
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !is_test_only(&item.attrs) {
            self.record(&item.sig, &item.block);
            self.within(item.sig.ident.to_string(), |f| {
                syn::visit::visit_impl_item_fn(f, item);
            });
        }
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if is_test_only(&item.attrs) {
            return;
        }
        if let Some(body) = &item.default {
            self.record(&item.sig, body);
        }
        self.within(item.sig.ident.to_string(), |f| {
            syn::visit::visit_trait_item_fn(f, item);
        });
    }
}

/// `#[test]`, or `#[cfg(test)]` exactly: `#[cfg(not(test))]` is production.
fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            || (attr.path().is_ident("cfg")
                && matches!(&attr.meta, syn::Meta::List(list) if list.tokens.to_string() == "test"))
    })
}

/// `Type` for an inherent impl, `<Type as Trait>` for a trait impl.
fn impl_name(item: &syn::ItemImpl) -> String {
    let last = |path: &syn::Path| {
        path.segments
            .last()
            .map_or_else(|| "_".to_string(), |s| s.ident.to_string())
    };
    let ty = match &*item.self_ty {
        syn::Type::Path(p) => last(&p.path),
        _ => "_".to_string(),
    };
    match &item.trait_ {
        Some((_, trait_path, _)) => format!("<{ty} as {}>", last(trait_path)),
        None => ty,
    }
}

/// clippy's `too_many_lines` count for a braced body: the lines between the
/// braces that hold any code, so a line that is blank, a `//` comment, or inside
/// a `/* */` comment is free.
fn code_lines(body: &str) -> usize {
    let inner = body
        .strip_prefix('{')
        .and_then(|b| b.strip_suffix('}'))
        .unwrap_or(body)
        .trim();
    let mut count = 0;
    let mut in_comment = false;
    for mut line in inner.lines() {
        let mut code = false;
        loop {
            line = line.trim_start();
            if line.is_empty() {
                break;
            }
            if in_comment {
                if let Some(i) = line.find("*/") {
                    line = &line[i + 2..];
                    in_comment = false;
                    continue;
                }
            } else {
                let multi = line.find("/*").unwrap_or(line.len());
                let single = line.find("//").unwrap_or(line.len());
                code |= multi > 0 && single > 0;
                if multi < single {
                    line = &line[multi + 2..];
                    in_comment = true;
                    continue;
                }
            }
            break;
        }
        count += usize::from(code);
    }
    count
}

#[test]
fn no_function_grows_past_its_limit() {
    let functions = functions(Path::new(env!("CARGO_MANIFEST_DIR")));
    // A walk that finds nothing, or a counter that counts nothing, passes everything.
    assert!(
        functions.len() > 1000
            && functions
                .iter()
                .any(|(name, lines, _)| name == "src/lib.rs::run" && *lines > 0),
        "found {} functions; the walk or the count is broken",
        functions.len()
    );
    let mut problems: Vec<String> = functions
        .iter()
        .filter_map(|(name, lines, _)| FUNCTION.verdict(name, *lines))
        .collect();
    for (pin, _) in FUNCTION.pins {
        if !functions.iter().any(|(name, ..)| name == pin) {
            problems.push(format!(
                "{pin}: pinned in src/fn_size_guard.rs but not found. Delete it."
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "function-length ratchet (src/fn_size_guard.rs):\n{}",
        problems.join("\n")
    );
}

#[test]
fn code_lines_count_like_clippy() {
    let body = "{\n    let a = 1;\n\n    // a comment\n    /* a block\n       comment */\n    let b = 2; // trailing\n    /* inline */ let c = 3;\n}";
    // `a`, `b` and `c`: the braces, the blank line and both comments are free.
    assert_eq!(code_lines(body), 3);
    assert_eq!(code_lines("{ one(); }"), 1);
    assert_eq!(code_lines("{}"), 0);
}

#[test]
fn functions_are_named_by_where_they_live_and_tests_are_skipped() {
    let src = r"
fn free() { one(); }
mod inner { fn nested() { two(); } }
struct S;
impl S { fn method(&self) { fn helper() { three(); } } }
impl std::fmt::Display for S {
    fn fmt(&self, _: &mut std::fmt::Formatter) -> std::fmt::Result { Ok(()) }
}
trait T { fn provided(&self) { four(); } fn required(&self); }
#[cfg(test)] mod tests { fn in_tests() { five(); } }
#[test] fn a_test() { six(); }
#[cfg(not(test))] fn production() { seven(); }
#[cfg(unix)] fn twice() { eight(); }
#[cfg(windows)] fn twice() { nine(); }
";
    let names: Vec<String> = functions_in("x.rs", src)
        .into_iter()
        .map(|(name, ..)| name)
        .collect();
    assert_eq!(
        names,
        [
            "x.rs::free",
            "x.rs::inner::nested",
            "x.rs::S::method",
            "x.rs::S::method::helper",
            "x.rs::<S as Display>::fmt",
            "x.rs::T::provided",
            "x.rs::production",
            "x.rs::twice",
            "x.rs::twice#2",
        ]
    );
}

#[test]
fn pins_are_sorted_with_one_entry_per_function() {
    assert!(
        FUNCTION.pins.windows(2).all(|pair| pair[0].0 < pair[1].0),
        "keep FUNCTION sorted by name, one entry per function"
    );
}
