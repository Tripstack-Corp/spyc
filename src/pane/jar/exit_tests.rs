//! The jar binary's exit statuses, each with one meaning (review finding
//! A2): the guard that keeps the holder's busy status apart from every
//! other exit of `spyc-jar`.

use std::path::Path;

/// Each exit status the jar's binary gives means one thing (review finding
/// A2): a jar reads a holder that exits [`spyc_jar::upgrade::HOLDER_BUSY`]
/// before it answers as busy (reason 5), and any other early exit as failed
/// (7). A binary that exits with that status for another reason (an unknown
/// subcommand exits `WIRE_NOT_SERVED`, so a later binary without
/// `hold-handover` would) would be read as busy by every jar of this build.
/// So the holder's status is used at one `exit` only, and it differs from
/// every other status the binary gives: each `std::process::exit(..)` in
/// `crates/spyc-jar/src` and `src/bin/spyc-jar.rs`, 1 for `main`'s error,
/// 101 for a panic, and the shell's 126 and up.
///
/// Read from each file's syntax (`syn`), production code only. An exit is a
/// call of `process::exit` (`std::process` under any name a `use` gives
/// it) or of `exit` under any name a `use` of `std::process::exit`, or a
/// glob of `std::process`, gives it. Its status must be a literal,
/// `HOLDER_BUSY` or `WIRE_NOT_SERVED`; an exit whose status it can't read
/// fails it. `jar::end::end(socket, code)`, which exits with `code`, is read
/// at each of its calls, and its own `exit(code)` is the one exit whose
/// status may be a variable.
///
/// It can't see an exit inside a macro's tokens, `std::process::abort`, or a
/// `main` that returns an `ExitCode`; another helper that takes the status
/// as an argument fails it.
#[test]
fn the_holders_busy_status_is_no_other_exit_of_the_jar() {
    use std::collections::HashSet;
    use syn::visit::Visit;

    use spyc_jar::cli::WIRE_NOT_SERVED;
    use spyc_jar::upgrade::HOLDER_BUSY;

    /// What a file's `use` items call `std::process` and its `exit`.
    #[derive(Default)]
    struct Names {
        process: HashSet<String>,
        exit: HashSet<String>,
    }

    impl Names {
        fn uses(&mut self, prefix: &mut Vec<String>, tree: &syn::UseTree) {
            match tree {
                syn::UseTree::Path(p) => {
                    prefix.push(p.ident.to_string());
                    self.uses(prefix, &p.tree);
                    prefix.pop();
                }
                syn::UseTree::Name(n) => self.name(prefix, &n.ident, &n.ident),
                syn::UseTree::Rename(r) => self.name(prefix, &r.ident, &r.rename),
                syn::UseTree::Group(g) => {
                    for t in &g.items {
                        self.uses(prefix, t);
                    }
                }
                syn::UseTree::Glob(_) => {
                    if prefix.last().is_some_and(|l| l == "process") {
                        self.exit.insert("exit".into());
                    }
                }
            }
        }

        fn name(&mut self, prefix: &[String], ident: &syn::Ident, called: &syn::Ident) {
            let last = prefix.last().map(String::as_str);
            if ident == "process" || (ident == "self" && last == Some("process")) {
                self.process.insert(called.to_string());
            }
            if ident == "exit" && last == Some("process") {
                self.exit.insert(called.to_string());
            }
        }
    }

    impl<'ast> Visit<'ast> for Names {
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            self.uses(&mut Vec::new(), &item.tree);
        }
    }

    struct Scan {
        file: String,
        names: Names,
        /// The function the visit is in.
        function: Option<String>,
        /// Each exit's status, or what it was given instead, and where.
        exits: Vec<(Result<i32, String>, String)>,
        holder: Vec<String>,
        /// Where `jar::end::end` hands its `code` to `exit`.
        helper: Vec<String>,
    }

    impl Scan {
        fn is_exit(&self, path: &syn::Path) -> bool {
            let names: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
            match names.as_slice() {
                [one] => self.names.exit.contains(one),
                [.., module, last] => {
                    last == "exit" && (module == "process" || self.names.process.contains(module))
                }
                [] => false,
            }
        }

        /// A call of `jar::end::end(socket, code)`, which exits with `code`.
        fn is_end(path: &syn::Path) -> bool {
            let names: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
            names.ends_with(&["end".to_string(), "end".to_string()])
        }

        /// Whether `arg` is `end`'s own `code`, handed on to `exit`.
        fn is_ends_code(&self, arg: Option<&syn::Expr>) -> bool {
            self.file.ends_with("jar/end.rs")
                && self.function.as_deref() == Some("end")
                && matches!(arg, Some(syn::Expr::Path(a)) if a.path.is_ident("code"))
        }

        fn status(&mut self, arg: Option<&syn::Expr>, site: String) {
            let status = match arg {
                Some(syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Int(i),
                    ..
                })) => i.base10_parse::<i32>().map_err(|e| e.to_string()),
                Some(syn::Expr::Path(a)) => {
                    let last = a.path.segments.last().map(|s| s.ident.to_string());
                    match last.as_deref() {
                        Some("HOLDER_BUSY") => {
                            self.holder.push(site);
                            return;
                        }
                        Some("WIRE_NOT_SERVED") => Ok(WIRE_NOT_SERVED),
                        _ => Err(format!("a status named {}", last.unwrap_or_default())),
                    }
                }
                _ => Err("a status that is neither a literal nor a name".into()),
            };
            self.exits.push((status, site));
        }
    }

    impl<'ast> Visit<'ast> for Scan {
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            let outer = self.function.replace(item.sig.ident.to_string());
            syn::visit::visit_item_fn(self, item);
            self.function = outer;
        }

        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if let syn::Expr::Path(p) = &*call.func {
                let line = p
                    .path
                    .segments
                    .first()
                    .map_or(0, |s| s.ident.span().start().line);
                let site = format!("{}:{line}", self.file);
                if self.is_exit(&p.path) {
                    if self.is_ends_code(call.args.first()) {
                        self.helper.push(site);
                    } else {
                        self.status(call.args.first(), site);
                    }
                } else if Self::is_end(&p.path) {
                    self.status(call.args.iter().nth(1), site);
                }
            }
            syn::visit::visit_expr_call(self, call);
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![root.join("src/bin/spyc-jar.rs")];
    let mut dirs = vec![root.join("crates/spyc-jar/src")];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                files.push(path);
            }
        }
    }
    // main's `Err`, and a panic.
    let mut others = vec![
        (1, "main returning an error".to_string()),
        (101, "a panic".to_string()),
    ];
    let mut holder_sites = Vec::new();
    let mut helper_sites = Vec::new();
    let mut unread = Vec::new();
    for path in &files {
        let text = crate::guard_support::production_half(&std::fs::read_to_string(path).unwrap());
        let file =
            syn::parse_file(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
        let mut names = Names::default();
        names.visit_file(&file);
        let mut scan = Scan {
            file: path.display().to_string(),
            names,
            function: None,
            exits: Vec::new(),
            holder: Vec::new(),
            helper: Vec::new(),
        };
        scan.visit_file(&file);
        holder_sites.extend(scan.holder);
        helper_sites.extend(scan.helper);
        for (status, site) in scan.exits {
            match status {
                Ok(v) => others.push((v, site)),
                Err(what) => unread.push(format!("{site}: {what}")),
            }
        }
    }
    assert!(
        files.len() > 10 && others.len() >= 6,
        "only {} exits read in {} files: the guard reads the wrong text",
        others.len(),
        files.len()
    );
    assert_eq!(
        helper_sites.len(),
        1,
        "`jar::end::end` hands its code to one exit, read at its callers: {helper_sites:?}"
    );
    assert!(
        unread.is_empty(),
        "exits whose status the guard can't read: {unread:?}"
    );
    assert_eq!(
        holder_sites.len(),
        1,
        "the holder's busy status is given at one exit, the holder's: {holder_sites:?}"
    );
    assert!(
        (2..126).contains(&HOLDER_BUSY),
        "the holder's busy status {HOLDER_BUSY} is one a shell or a signal gives"
    );
    let same: Vec<_> = others.iter().filter(|(v, _)| *v == HOLDER_BUSY).collect();
    assert!(
        same.is_empty(),
        "the holder's busy status {HOLDER_BUSY} is also given by: {same:?}"
    );
}
