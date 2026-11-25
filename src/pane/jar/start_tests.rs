//! A jar's start reaches it whatever else this spyc starts meanwhile (slice
//! A's finding, in slice B). spyc hands each jar its start on its standard
//! input, and the jar reads it to its end. On a pipe, that end comes only
//! once every copy of the pipe's writer is closed, and Rust on macOS makes a
//! pipe with `pipe`, marking it close-on-exec a moment later: a child
//! another thread spawns in that moment keeps a writer for its life, the jar
//! never sees the end of its start, never binds, and its tab fails "didn't
//! answer". The test does by construction what that race does: while a
//! jar's start is under way it hands every pipe writer this process holds to
//! a long-lived child of its own, in a process of its own so no other test's
//! pipes are touched.

// A module of its own, which the jar's source scans read as a test's.
#[cfg(test)]
mod checks {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::super::place::{HOLD, Place};
    use super::super::test_dir::TestDir;

    /// The pipes this process holds open, by descriptor.
    fn pipes() -> Vec<PathBuf> {
        use std::os::unix::fs::FileTypeExt;
        let dir = if Path::new("/proc/self/fd").is_dir() {
            Path::new("/proc/self/fd")
        } else {
            Path::new("/dev/fd")
        };
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.file_type().is_fifo()))
            .collect();
        found.sort();
        found
    }

    /// Run in a process of its own by the tests below: start a jar, a tab's
    /// or a session's as `JAR_TEST_START_LEAK` says, whose start is held up
    /// until every pipe writer this process holds has gone to a long-lived
    /// child, as the race leaves one, then see the jar answer.
    #[test]
    #[ignore = "run in a process of its own: it hands this process's pipes to other processes"]
    fn a_jar_started_while_its_pipes_leak() {
        let Some(kind) = std::env::var_os("JAR_TEST_START_LEAK") else {
            return;
        };
        let dir = TestDir::new("sp");
        let began = dir.path().join("began");
        let go = dir.path().join("go");
        let jar = super::super::warmed(
            std::env::current_exe()
                .unwrap()
                .ancestors()
                .skip(1)
                .map(|d| d.join("spyc-jar"))
                .find(|j| j.is_file())
                .expect("a built spyc-jar: make test builds it"),
        );
        let gated = dir.path().join("gated-jar");
        std::fs::write(
            &gated,
            format!(
                "#!/bin/sh\n: > '{}'\nwhile [ ! -e '{}' ]; do sleep 0.05; done\nexec '{}' \"$@\"\n",
                began.display(),
                go.display(),
                jar.display()
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&gated, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let place = Place {
            binary: gated,
            dirs: vec![
                dir.path().join("spyc"),
                dir.path().join("spyc").join("jars"),
            ],
            tokens: dir.path().join("tokens"),
            hold: HOLD,
            repaint_hold: Duration::from_secs(2),
        };
        let before = pipes();
        // A start longer than a pipe holds, so a writer is still at it once the
        // jar begins, in one argument Linux still takes (`MAX_ARG_STRLEN`,
        // 128 KiB).
        let command = format!("exec cat # {}", "x".repeat(100 * 1024));
        let cwd = dir.path().to_path_buf();
        let starting = std::thread::spawn(move || -> Result<Started, String> {
            if kind == "session" {
                return super::super::session::start_in(
                    &place,
                    "leak",
                    vec!["/bin/sh".into(), "-c".into(), command],
                    &cwd,
                    (24, 80),
                )
                .map(Started::Session)
                .map_err(|e| format!("{e:#}"));
            }
            crate::pane::pty_host::PtyHost::spawn(crate::pane::pty_host::PtySpec {
                project_env: &[],
                command: &command,
                rows: 24,
                cols: 80,
                cwd: &cwd,
                env: &[],
                term: "dumb",
                nudge_winch: false,
                debug_dump: false,
                exec_replace: false,
                capture_launch_env: false,
                jar: Some(super::super::Spec {
                    place,
                    name: "0123456789abcdef".into(),
                    adopt: None,
                    retake: false,
                    exit_linger: None,
                    watch: None,
                }),
                record: None,
            })
            .map(|host| Started::Tab(Box::new(host)))
            .map_err(|e| format!("{e:#}"))
        });
        while !began.exists() {
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut holders = Vec::new();
        for pipe in pipes().into_iter().filter(|p| !before.contains(p)) {
            let Ok(writer) = std::fs::OpenOptions::new().write(true).open(&pipe) else {
                continue;
            };
            holders.push(
                std::process::Command::new("/bin/sleep")
                    .arg("600")
                    .stdout(writer)
                    .spawn()
                    .unwrap(),
            );
        }
        eprintln!("pipe writers handed to other processes: {}", holders.len());
        std::fs::write(&go, "").unwrap();
        let started = starting.join().unwrap();
        for mut holder in holders {
            let _ = holder.kill();
            let _ = holder.wait();
        }
        match started.expect("the jar never saw the end of its start") {
            Started::Tab(mut host) => host.shutdown(Duration::from_millis(500)),
            // The session's jar, which this test started, holds for a
            // terminal: end it.
            Started::Session(socket) => {
                let pid = spyc_jar::client::describe(&socket)
                    .unwrap()
                    .into_iter()
                    .find_map(|(k, v)| (k == "pid").then(|| v.parse::<i32>().unwrap()))
                    .expect("the jar says its pid");
                let _ = rustix::process::kill_process(
                    rustix::process::Pid::from_raw(pid).unwrap(),
                    rustix::process::Signal::TERM,
                );
            }
        }
    }

    /// What a start under way became.
    enum Started {
        Tab(Box<crate::pane::pty_host::PtyHost>),
        Session(PathBuf),
    }

    /// Run [`a_jar_started_while_its_pipes_leak`] for `kind`, and say what it
    /// printed and whether it passed.
    fn leaked_start(kind: &str) -> (bool, String) {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "pane::jar::start_tests::checks::a_jar_started_while_its_pipes_leak",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("JAR_TEST_START_LEAK", kind)
            .output()
            .unwrap();
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (said.contains("1 passed"), said)
    }

    /// The race, by construction: a tab's jar's start still reaches it when
    /// another process holds a copy of every pipe writer this spyc held while
    /// starting it.
    #[test]
    fn a_jars_start_reaches_it_whatever_else_holds_this_spycs_pipes() {
        let (passed, said) = leaked_start("tab");
        assert!(passed, "{said}");
    }

    /// The same for a session's own jar, which `^d` and `:detach project`
    /// start.
    #[test]
    fn a_sessions_jar_start_reaches_it_whatever_else_holds_this_spycs_pipes() {
        let (passed, said) = leaked_start("session");
        assert!(passed, "{said}");
    }

    /// Whether `attrs` hold `#[cfg(test)]`: the parsed form of
    /// `guard_support::production_half`, whose text cut can leave a file syn can't read.
    fn test_only(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| {
            a.path().is_ident("cfg") && a.parse_args::<syn::Ident>().is_ok_and(|i| i == "test")
        })
    }

    /// An item's attributes, for the kinds that carry `#[cfg(test)]` here.
    fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
        match item {
            syn::Item::Fn(i) => &i.attrs,
            syn::Item::Mod(i) => &i.attrs,
            syn::Item::Impl(i) => &i.attrs,
            syn::Item::Const(i) => &i.attrs,
            syn::Item::Static(i) => &i.attrs,
            syn::Item::Use(i) => &i.attrs,
            syn::Item::Struct(i) => &i.attrs,
            syn::Item::Enum(i) => &i.attrs,
            syn::Item::Trait(i) => &i.attrs,
            syn::Item::Type(i) => &i.attrs,
            syn::Item::Macro(i) => &i.attrs,
            _ => &[],
        }
    }

    /// What makes a pipe, or a pair whose reader waits for every copy of its
    /// other end to close: a call or a function named for one, in code or in
    /// a macro's tokens, and the calls a renamed import of one makes.
    fn pipe_makers(src: &str, path: &Path) -> usize {
        use syn::visit::Visit;
        const NAMES: [&str; 7] = [
            "piped",
            "pipe",
            "pipe2",
            "pipe_with",
            "socketpair",
            "openpty",
            "openpt",
        ];
        /// Methods: `Command::output`, `Child::wait_with_output`, and a
        /// PTY system's `openpty`.
        const METHODS: [&str; 3] = ["output", "wait_with_output", "openpty"];
        struct Makers {
            names: std::collections::BTreeSet<String>,
            found: usize,
        }
        impl Makers {
            fn named(&self, segments: &[String]) -> bool {
                match segments {
                    [.., last] if self.names.contains(last) => true,
                    [.., ty, last] => {
                        last == "pair" && matches!(ty.as_str(), "UnixStream" | "UnixDatagram")
                    }
                    _ => false,
                }
            }
        }
        impl<'a> Visit<'a> for Makers {
            fn visit_item(&mut self, item: &'a syn::Item) {
                if !test_only(item_attrs(item)) {
                    syn::visit::visit_item(self, item);
                }
            }
            fn visit_impl_item_fn(&mut self, f: &'a syn::ImplItemFn) {
                if !test_only(&f.attrs) {
                    syn::visit::visit_impl_item_fn(self, f);
                }
            }
            fn visit_use_rename(&mut self, rename: &'a syn::UseRename) {
                if self.names.contains(&rename.ident.to_string()) {
                    self.names.insert(rename.rename.to_string());
                }
            }
            fn visit_path(&mut self, path: &'a syn::Path) {
                let segments: Vec<String> =
                    path.segments.iter().map(|s| s.ident.to_string()).collect();
                if segments.len() > 1 && self.named(&segments) {
                    self.found += 1;
                }
                syn::visit::visit_path(self, path);
            }
            fn visit_expr_call(&mut self, call: &'a syn::ExprCall) {
                if let syn::Expr::Path(callee) = &*call.func
                    && callee.path.segments.len() == 1
                    && self.named(&[callee.path.segments[0].ident.to_string()])
                {
                    self.found += 1;
                }
                syn::visit::visit_expr_call(self, call);
            }
            fn visit_expr_method_call(&mut self, call: &'a syn::ExprMethodCall) {
                let method = call.method.to_string();
                if METHODS.contains(&method.as_str())
                    && (method != "output" || call.args.is_empty())
                {
                    self.found += 1;
                }
                syn::visit::visit_expr_method_call(self, call);
            }
            fn visit_macro(&mut self, mac: &'a syn::Macro) {
                fn walk(makers: &mut Makers, tokens: proc_macro2::TokenStream) {
                    use proc_macro2::{Delimiter, TokenTree};
                    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
                    // The ident before, across a path's `::`, and whether a
                    // `.` came just before.
                    let (mut before, mut dot) = (String::new(), false);
                    for (at, token) in tokens.iter().enumerate() {
                        match token {
                            TokenTree::Group(g) => walk(makers, g.stream()),
                            TokenTree::Ident(i) => {
                                let name = i.to_string();
                                // A method is called, not a field read: `(` next,
                                // and `output` takes no arguments.
                                let called = matches!(
                                    tokens.get(at + 1),
                                    Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis
                                        && (name != "output" || g.stream().is_empty())
                                );
                                if makers.named(&[before, name.clone()])
                                    || (dot && called && METHODS.contains(&name.as_str()))
                                {
                                    makers.found += 1;
                                }
                                (before, dot) = (name, false);
                                continue;
                            }
                            TokenTree::Punct(p) if p.as_char() == ':' => {
                                dot = false;
                                continue;
                            }
                            TokenTree::Punct(p) => {
                                (before, dot) = (String::new(), p.as_char() == '.');
                                continue;
                            }
                            TokenTree::Literal(_) => {}
                        }
                        (before, dot) = (String::new(), false);
                    }
                }
                walk(self, mac.tokens.clone());
            }
        }
        let file = syn::parse_file(src).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if test_only(&file.attrs) {
            return 0;
        }
        let mut makers = Makers {
            names: NAMES.iter().map(ToString::to_string).collect(),
            found: 0,
        };
        // Renamed imports first, then the calls they make.
        makers.visit_file(&file);
        makers.found = 0;
        makers.visit_file(&file);
        makers.found
    }

    /// The same race wherever spyc reads a child's pipe to its end, or a child
    /// reads one from spyc: spyc spawns from many threads, so a child another
    /// thread starts as the pipe is made can keep its writer, and the reader waits
    /// as long as that child lives. No production code makes such a pipe. Found
    /// by `rg -n 'piped|pipe(2|_with)?\(|socketpair|openpty|::pair\(|\.output\(\)|wait_with_output' src crates`,
    /// production halves only; [`pipe_makers`] says what counts. Fails closed:
    /// it must find the code it scans, and each exception must still be there.
    #[test]
    fn no_reader_waits_for_the_end_of_a_pipe_another_spawn_can_hold() {
        /// Where a pipe stays, and why.
        const EXCEPTIONS: [(&str, usize, &str); 7] = [
            (
                "src/clipboard.rs",
                3,
                "the clipboard helpers' three pipes (paste's stdout and stderr, copy's \
                 stdin): filed as #1323",
            ),
            (
                "src/pane/pty_host.rs",
                1,
                "the pane's PTY: portable-pty opens it and marks it close-on-exec after, \
                 the same race, so a child another thread spawns then can keep the \
                 program's terminal open after it ends: filed as #1334",
            ),
            (
                "src/agent/tail/native.rs",
                2,
                "the session-file followers' stop lines: each ends by `shutdown`, \
                 which reaches the reader whoever else holds a copy",
            ),
            (
                "src/mcp/statusline.rs",
                1,
                "`spyc --statusline`, a process of its own that spawns this one child \
                 from its one spawning thread: no other spawn can take the writer",
            ),
            (
                "crates/spyc-jar/src/upgrade.rs",
                4,
                "the jar's handover, slice C's: the blob's pipe, its helper and its one \
                 call (made, then marked close-on-exec: the same race, filed as #1334), \
                 and the socketpairs to the new image and to the holder",
            ),
            (
                "crates/spyc-jar/src/validation.rs",
                3,
                "the jar's validation of a new binary, slice C's: the validator's \
                 socketpair for the blob and its two pipes, stdout and stderr",
            ),
            (
                "crates/spyc-jar/src/pty.rs",
                1,
                "the jar's PTY, opened as the jar starts, before it has a thread that \
                 spawns",
            ),
        ];
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut dirs = vec![root.join("src"), root.join("crates")];
        let (mut scanned, mut found) = (0, std::collections::BTreeMap::new());
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if path.is_dir() {
                    // `harness_tests` is a `#[cfg(test)]` module of `app`.
                    if !matches!(name.as_str(), "tests" | "harness_tests" | "target" | "fuzz") {
                        dirs.push(path);
                    }
                    continue;
                }
                // Test-only modules, by their names.
                if path.extension().is_none_or(|x| x != "rs")
                    || name.ends_with("tests.rs")
                    || name == "test_support.rs"
                {
                    continue;
                }
                scanned += 1;
                let src = std::fs::read_to_string(&path).unwrap();
                let n = pipe_makers(&src, &path);
                if n > 0 {
                    let rel = path.strip_prefix(root).unwrap().display().to_string();
                    found.insert(rel, n);
                }
            }
        }
        assert!(
            scanned > 100,
            "premise: the scan reads the code ({scanned} files)"
        );
        let allowed: std::collections::BTreeMap<String, usize> = EXCEPTIONS
            .iter()
            .map(|(file, n, _)| ((*file).to_string(), *n))
            .collect();
        assert_eq!(
            found, allowed,
            "a pipe a reader waits on the end of, beside spawns on other threads: use a file"
        );
    }

    /// The guard's reading: each way to make a pipe counts, renamed or not.
    #[test]
    fn every_way_to_make_a_pipe_is_counted() {
        for (src, n) in [
            ("fn f() { let (r, w) = std::io::pipe().unwrap(); }", 1),
            ("use std::io::pipe as p; fn f() { let _ = p(); }", 1),
            ("use std::io::pipe; fn f() { let _ = pipe(); }", 1),
            (
                "fn f() { let _ = rustix::pipe::pipe_with(Default::default()); }",
                1,
            ),
            ("fn f() { let _ = nix::unistd::pipe2(0); }", 1),
            ("fn f() { let _ = unsafe { libc::pipe(p) }; }", 1),
            ("fn f() { let _ = UnixStream::pair(); }", 1),
            ("fn f() { c.stdin(Stdio::piped()).spawn(); }", 1),
            (
                "fn f() { let o = c.output(); let o = k.wait_with_output(); }",
                2,
            ),
            ("fn f() { let p = native_pty_system().openpty(size); }", 1),
            ("fn f() { assert!(c.output().is_ok()); }", 1),
            (
                "fn f() { drop(vec![UnixStream::pair(), Ok(std::io::pipe())]); }",
                2,
            ),
            (
                "fn f() { drop(format!(\"{}{}\", t.output, p.pair(1))); }",
                0,
            ),
            (
                "fn f() { drop(vec![c.output(), k.wait_with_output()]); }",
                2,
            ),
            ("fn f() { let pipe = 1; let _ = pipe + w.output(x); }", 0),
        ] {
            assert_eq!(pipe_makers(src, Path::new("t.rs")), n, "{src}");
        }
    }
}
