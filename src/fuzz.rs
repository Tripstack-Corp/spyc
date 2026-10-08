//! Public entry points for the `cargo-fuzz` targets in `fuzz/`.
//!
//! The crate is otherwise all-private modules. Each wrapper takes raw input
//! and discards the result (no internal types leak into the public API), so a
//! fuzz target asserts only the "never panics" property.

/// Drive the pane's byte-processing path with structured escape streams and
/// assert the terminal is still usable afterwards.
///
/// **The property is recovery, not absence of panics.** spyc's
/// `pane::parser_worker` deliberately wraps `process()` in `catch_unwind`,
/// rebuilds a torn parser at its current size, and clears the mutex poison —
/// because the incumbent engine panics on roughly 2.87% of structured-random
/// escape streams at reachable geometries (see `[profile.release]`'s comment
/// and `docs/drafts/VT_ENGINE_SPIKE.md` §5). Asserting "the engine never
/// panics" would therefore fail on the *current* engine for a known and
/// mitigated reason, which trains a reader to ignore a red fuzz job.
///
/// So this asserts what production actually depends on and what holds for
/// any engine: after ANY byte stream, the terminal is still there and still
/// answers questions. That covers the recovery path itself, which is
/// production code no test reaches — the panic branch only runs when the
/// engine panics, so a fuzzer is the only thing that exercises it.
///
/// When the engine flips (`docs/drafts/V2_2_PLAN.md` §8, PR 15) the stricter
/// property becomes assertable: libghostty panicked 0 times in 50,000
/// iterations of the same generator. Tighten the assertion then rather than
/// adding a knowingly-red target now.
///
/// The input is used as a *script* rather than as a PRNG seed, so
/// libFuzzer's coverage feedback can steer the shape of the escape stream
/// instead of steering an opaque hash.
pub fn pane_engine(data: &[u8]) {
    use crate::pane::engine::TerminalScreen as _;

    let Some((&g0, rest)) = data.split_first() else {
        return;
    };
    let Some((&g1, script)) = rest.split_first() else {
        return;
    };
    // Geometry spyc can actually produce: `Pane::resize` and
    // `pane_spawn_size` clamp both dimensions with `.max(1)`, so 1 is
    // reachable and 0 is not. 1-row and 1-column are where the incumbent's
    // live panic classes are, so they must be in range.
    let rows = u16::from(g0 % 40) + 1;
    let cols = u16::from(g1 % 60) + 1;

    let stream = crate::fuzz_support::escape_stream(script);

    let parser = std::sync::Arc::new(std::sync::Mutex::new(
        <crate::pane::PaneEngine as crate::pane::engine::Engine>::new(rows, cols, 200),
    ));

    // **The property is that `process` never panics.**
    //
    // It used to be recovery — feed, `catch_unwind`, rebuild, assert the
    // pane still answers — because the engine underneath was vt100, which
    // panics on valid input at reachable geometries (a one-column grid, a
    // wide char split by a shrink). `parser_worker` still carries that net
    // in production, and should: it is cheap and the next engine's bugs
    // are not known yet. But asserting only recovery against an engine
    // that does not panic asserts almost nothing, so with the flip the
    // target asserts the stronger thing and lets a panic reach libFuzzer.
    //
    // That also removes the hook juggling this target needed: `libfuzzer-sys`
    // installs a panic hook that calls `abort()`, and a hook runs BEFORE
    // unwinding, so `catch_unwind` never saw the panic and the target
    // aborted on its 27th execution reporting a panic production recovers
    // from. With no `catch_unwind` there is nothing to defeat.
    use crate::pane::engine::Engine as _;

    for chunk in stream.chunks(64.max(stream.len() / 8 + 1)) {
        {
            let mut p = parser
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            p.process(chunk);
        }

        // The invariants, checked after every chunk rather than once at the
        // end: a pane that is briefly unusable mid-stream is still a pane
        // the render pass can lock during that window.
        assert!(
            !parser.is_poisoned(),
            "the mutex must never be left poisoned — the render pass locks it"
        );
        // Scoped so the guard drops before the next chunk's lock —
        // clippy's `significant_drop_tightening`, and it is right: holding
        // the render lock across the loop back-edge is not what
        // `parser_worker` does either.
        let guard = parser.lock().expect("not poisoned, just asserted");
        let screen = guard.screen();
        let (r, c) = screen.size();
        assert!(r > 0 && c > 0, "geometry collapsed to {r}x{c}");
        let (cy, cx) = screen.cursor_position();
        // The column bound is `<=`, not `<`, and that is deliberate.
        // `x == cols` is the PENDING-WRAP state every terminal has: after
        // writing the last column the cursor sits past it until the next
        // glyph wraps. Both consumers already guard for it —
        // `app::util::place_pty_cursor_from_screen` returns early on
        // `cx >= rect.width`, and `PaneWidget`'s block cursor is gated on
        // `cx < draw_cols` — so it is normal, not a defect.
        //
        // An earlier version of this assertion used `<` on both axes and
        // fired instantly on a one-column grid. Caught by the gate test
        // below before it could land as a red weekly fuzz job; a target
        // whose first run is red teaches people to ignore it.
        //
        // The ROW bound stays strict: a cursor past the last row is the
        // DECRC-after-resize class that `[profile.release]`'s net exists
        // for, and nothing downstream expects it.
        assert!(
            cy < r,
            "cursor row {cy} past the last row of a {r}x{c} grid"
        );
        assert!(
            cx <= c,
            "cursor column {cx} beyond pending-wrap on a {r}x{c} grid"
        );
        // Every in-bounds cell must answer, and the first out-of-bounds one
        // must not: `line_from_visible_row` breaks its loop on `None`, so a
        // grid that lies about its own extent silently truncates a row.
        for row in 0..r {
            assert!(
                screen.cell_style(row, c.saturating_sub(1)).is_some(),
                "last column of row {row} missing from a {r}x{c} grid"
            );
            assert!(
                screen.cell_style(row, c).is_none(),
                "grid answered past its own last column"
            );
        }
        // These must not panic on any state the stream can produce.
        let _ = screen.contents();
        let _ = screen.contents_between(0, 0, r.saturating_sub(1), c.saturating_sub(1));
        drop(guard);
    }
}

/// Normalize one raw archive member name and discard the result.
///
/// This is the boundary that makes zip-slip impossible — every member name
/// in a downloaded archive is attacker-controlled — so it asserts the
/// normalizer never panics and never returns a path that could escape.
pub fn normalize_archive_name(raw: &str) {
    if let Ok(name) = crate::archive::index::normalize(raw) {
        assert!(
            !name.inner.starts_with('/') && !name.inner.split('/').any(|p| p == ".."),
            "normalized name must stay inside the mount: {:?}",
            name.inner
        );
    }
}

/// Drive a whole container through the mount path and assert nothing it
/// writes lands outside the staging root.
///
/// [`normalize_archive_name`] covers one member *name*; this covers the
/// parsers that eat bytes — zip central directories, tar headers, and the
/// gz/zst streams — plus the extraction those parsers feed. Names are only
/// half the containment story: a member is also free to be a *symlink*, and
/// a link the extractor creates redirects where a later member physically
/// lands. That composition is invisible to any check that reasons about one
/// name at a time, so the property asserted here is positional — where the
/// bytes ended up — not lexical.
///
/// Containment is not the only lie an archive tells. A seekable member is
/// also read back under a byte ceiling, because a zip understating its
/// declared size defeated a cap that trusted the declaration — a property
/// about *how many* bytes arrive, which no amount of asserting where they
/// land can reach.
///
/// The first input byte picks the container flavour and the rest is its
/// content, so one corpus serves all four formats.
pub fn archive_container(data: &[u8]) {
    use crate::archive::ArchiveFormat;

    // Small enough to keep executions fast; large enough that the entry cap
    // and the extract budget are both reachable rather than theoretical.
    const CAP: usize = 512;
    const BUDGET: u64 = 1 << 20;
    // A per-read ceiling in the same spirit as the MCP tools' 100 KB, small
    // enough that a modest corpus entry can reach it.
    const READ_CAP: u64 = 64 * 1024;

    let Some((&flavor, body)) = data.split_first() else {
        return;
    };
    let name = match flavor % 4 {
        0 => "input.zip",
        1 => "input.tar",
        2 => "input.tar.gz",
        _ => "input.tar.zst",
    };
    let Some(format) = crate::archive::detect(name, body) else {
        return;
    };
    let Ok(sandbox) = tempfile::tempdir() else {
        return;
    };
    let root = sandbox.path();
    let archive = root.join(name);
    if std::fs::write(&archive, body).is_err() {
        return;
    }
    // Staging sits several levels down so a `..` escape lands back inside
    // the sandbox, where the walk below can see it. Rooting staging at the
    // sandbox top would let the interesting case climb out unobserved.
    let staging = root.join("mnt/a/b/staging");

    match format {
        ArchiveFormat::Zip | ArchiveFormat::Tar => {
            // Indexing writes nothing; materializing each member is where a
            // seekable container touches the filesystem.
            if let Ok(indexed) = crate::archive::read::index_seekable(&archive, format, CAP) {
                for entry in &indexed.index.entries {
                    let _ = crate::archive::read::materialize(&archive, entry, &staging);
                    // A read cap bounds the BYTES, not the archive's claim
                    // about them. A zip understating its uncompressed size
                    // walked straight past the MCP 100 KB ceiling and handed
                    // back the real stream (HIGH-2), and nothing here would
                    // have noticed: this target asserted where bytes LAND
                    // and never how many arrive. `materialize` is uncapped
                    // by design — the mount budget is its ceiling — so the
                    // question goes to the function the readers call.
                    if let Ok(bytes) =
                        crate::archive::read::member_bytes_within(&archive, entry, READ_CAP)
                    {
                        assert!(
                            bytes.len() as u64 <= READ_CAP,
                            "{}: {} bytes came back under a {READ_CAP}-byte cap",
                            entry.inner,
                            bytes.len()
                        );
                    }
                }
            }
        }
        _ => {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let _ = crate::archive::read::stream_mount(
                &archive, format, &staging, BUDGET, CAP, &cancel,
            );
        }
    }

    assert_contained(root, &staging, &archive);
}

/// Every path under `root` must be the archive itself or sit inside
/// `staging` — anything else was written through an escape.
///
/// Walks with `symlink_metadata` so a link is judged by where it *is*, not
/// by what it points at, and reports the link target when one escapes: a
/// contained link aimed outside the mount is the step before a later member
/// is written through it, so it is worth failing on in its own right.
fn assert_contained(root: &std::path::Path, staging: &std::path::Path, archive: &std::path::Path) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if path == archive {
                continue;
            }
            assert!(
                path.starts_with(staging),
                "member escaped the staging root: {} (staging {})",
                path.display(),
                staging.display()
            );
            if meta.is_symlink() {
                let target = std::fs::read_link(&path).unwrap_or_default();
                assert!(
                    !target.is_absolute(),
                    "staged symlink points at an absolute path: {} -> {}",
                    path.display(),
                    target.display()
                );
                // Resolve against the link's *canonical* parent and then let
                // the filesystem answer. Joined, NOT folded: cancelling `..`
                // against the preceding component is a paper operation, and
                // it is wrong exactly when that component is itself a link —
                // `d/link1/../x` reads as `d/x` on paper while `open()`
                // follows `link1` first and the `..` climbs out of wherever
                // that landed. Folding here would reproduce the production
                // bug inside the assertion and certify the escape this
                // target exists to catch.
                let base = path
                    .parent()
                    .and_then(|p| std::fs::canonicalize(p).ok())
                    .unwrap_or_else(|| staging.to_path_buf());
                let canonical_staging =
                    std::fs::canonicalize(staging).unwrap_or_else(|_| staging.to_path_buf());
                // A dangling target is legal in an archive, so ask the
                // deepest ancestor that resolves — the directory the link
                // would actually reach through.
                let joined = base.join(&target);
                let mut probe = joined.as_path();
                let landed = loop {
                    if let Ok(real) = std::fs::canonicalize(probe) {
                        break Some(real);
                    }
                    match probe.parent() {
                        Some(p) => probe = p,
                        None => break None,
                    }
                };
                assert!(
                    landed
                        .as_ref()
                        .is_some_and(|r| r.starts_with(&canonical_staging)),
                    "staged symlink points outside the staging root: {} -> {}",
                    path.display(),
                    target.display()
                );
            }
        }
    }
}

/// Parse one keymap-DSL line and discard the result. See
/// `fuzz/fuzz_targets/dsl_parse.rs`.
pub fn parse_keymap_line(line: &str) {
    let _ = crate::config::dsl::parse(line);
}

/// Render arbitrary markdown to styled lines and discard the result — the
/// fuzz target asserts the renderer never panics on adversarial markdown
/// (it ingests untrusted file content via the pager).
pub fn render_markdown(source: &str) {
    let _ = crate::ui::markdown::render(source, &crate::ui::theme::Theme::default(), Some(80));
}

/// Syntax-highlight arbitrary content (as if it were a Rust file) and
/// discard the result — asserts the highlighter never panics.
pub fn highlight(content: &str) {
    let _ = crate::ui::syntax::highlight_to_lines("fuzz.rs", content);
}

/// Word-wrap arbitrary text at `width`, asserting the wrap invariant.
///
/// Every returned byte range must land on char boundaries and be
/// sliceable — a mid-codepoint range would panic the pager's actual
/// slicing, which is the bug class this catches.
pub fn word_wrap(text: &str, width: usize) {
    for (start, end) in crate::ui::wrap::word_wrap_ranges(text, width) {
        assert!(
            start <= end && end <= text.len(),
            "wrap range out of bounds: ({start},{end}) len {}",
            text.len()
        );
        assert!(
            text.is_char_boundary(start) && text.is_char_boundary(end),
            "wrap range splits a codepoint: ({start},{end}) in {text:?}"
        );
        let _ = &text[start..end]; // must not panic
    }
}

/// Expand `~` / `$VAR` / `${VAR}` in an arbitrary path string and discard
/// the result — asserts the path expander never panics on adversarial
/// variable syntax.
pub fn expand_path(input: &str) {
    let _ = crate::paths::expand(input);
}

/// Expand a `%`-template (the `unix CMD` substitution) against a couple of
/// fixed target paths and discard the result — asserts the template parser
/// never panics on arbitrary `%`/escape syntax.
pub fn expand_percent(template: &str) {
    let _ = crate::shell::expand_percent(
        template,
        &[
            std::path::Path::new("/tmp/a.rs"),
            std::path::Path::new("/tmp/b c.txt"),
        ],
    );
}

#[cfg(test)]
mod fuzz_target_registration_tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    fn repo() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn on_disk() -> BTreeSet<String> {
        std::fs::read_dir(repo().join("fuzz/fuzz_targets"))
            .expect("read fuzz_targets")
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                (p.extension()? == "rs").then(|| p.file_stem()?.to_str().map(String::from))?
            })
            .collect()
    }

    /// A target absent from `fuzz/Cargo.toml` builds nowhere; one absent from the
    /// weekly matrix builds but never runs.
    ///
    /// Both failures are silent — `archive_name` sat outside the matrix from the
    /// day it was added, so the one target covering attacker-controlled archive
    /// names had never executed in CI. Three lists, one source of truth.
    #[test]
    fn every_fuzz_target_is_registered_and_scheduled() {
        let targets = on_disk();
        assert!(!targets.is_empty(), "no fuzz targets found");

        let manifest =
            std::fs::read_to_string(repo().join("fuzz/Cargo.toml")).expect("read fuzz/Cargo.toml");
        let workflow = std::fs::read_to_string(repo().join(".github/workflows/fuzz.yml"))
            .expect("read fuzz.yml");

        for target in &targets {
            assert!(
                manifest.contains(&format!("name = \"{target}\"")),
                "fuzz target {target} has no [[bin]] in fuzz/Cargo.toml"
            );
            assert!(
                workflow.contains(&format!("- {target}")),
                "fuzz target {target} is missing from the fuzz.yml matrix — it \
                 would build but never run"
            );
        }
    }
}
