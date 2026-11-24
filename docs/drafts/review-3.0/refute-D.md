# Refutation pass: unit D (sessions), finding D1

**SHA:** `444857a2` (`git rev-parse HEAD` in the review worktree). **Platform:** macOS (Apple silicon), the pin's debug binaries at `~/.cache/spyc/target/3.0-9f8124c5/debug/`. Probe files are in `docs/drafts/review-3.0/evidence/refute-D/`.

## D1. A session name of 28 to 35 characters turns `^d` into a session that doesn't come back

**Verdict: `severity`.** The mechanism holds, and it is wider than the finding says, but I'd rate it **medium**, not high.

### Evidence: lines read

- `src/pane/jar/session.rs:146-163` (`link_own`): `resolved = |p| std::fs::canonicalize(p).ok()`, `socket = resolved(given.parent()?)?.join(given.file_name()?)`, then `link(&socket)`. So spyc connects to the **canonical** path, not the one in `SPYC_JAR_SOCKET`.
- `session.rs:168-169` (`link`): `Client::connect(socket, Role::Program, LINK_WAIT, b"")`. In `crates/spyc-jar/src/client.rs:55-70`, `connect` retries **any** `UnixStream::connect` error until `wait` runs out. A path that is too long for `sun_path` fails on every try, so `link` gives up only after `LINK_WAIT` (20 s, `session.rs:34`). After that, `link_own` returns `None` (`session.rs:158-161`).
- `session.rs:220-231` (`start`): the jar is given `place.prepare_sessions()?.join(format!("{name}.sock"))`, built from `socket_dirs(XDG_RUNTIME_DIR, TMPDIR, uid)` (`place.rs:159-178`) with no canonicalization. `crates/spyc-jar/src/pty.rs:70` exports that same uncanonicalized path as `SPYC_JAR_SOCKET`. So the jar binds a path that fits, while spyc connects to one that may not.
- `src/lib.rs:411-416` (`take_terminal`): `link_own()` is the only way a spyc learns it is in its own jar. When it returns `None`, `bootstrap.rs:26-31` leaves `own_jar` unset, so `restore_own_session` (`src/app/detach.rs:340-350`) returns at once. The handover started that spyc with a bare `argv` (`detach.rs:186-192`: `vec![exe]`), so it gets a fresh name and restores nothing.
- `save_released` (`detach.rs:227-234`) writes the manifest before `release_jars`. Recovery is therefore possible, and the finding says so correctly.
- Nothing caps the length of a name. `:name` goes through `session_names::normalize` (`src/state/session_names.rs:60-80`) with no length check (`src/app/state/dispatch.rs:428-450`). Generated names are at most 19 characters (`FENUGREEK_CORIANDER`). The fallback in `unused_session_name` (`project_move.rs:799-817`, up to 30 characters) is used only after 64 random draws all collide, so in practice only a name the user typed hits the gap.
- Arithmetic on this machine: `TMPDIR=$TMPDIR/`, and `/var` is a link to `private/var` (checked with `ls -l /var`). The given prefix `…/T/spyc/sessions/` is 63 bytes and the canonical one 71. With `.sock` and Rust's 103-byte ceiling, the jar binds names of up to 35 characters, while spyc can connect only to names of up to 27. The 28 to 35 gap is right.
- No test reaches it: `tests/session_jar.rs` uses `PAPRIKA_TEST`, and the unit tests of `own_jar_socket` use `/run/u/…` paths that canonicalize to themselves.
- What I tried and could not refute: there is no second link path (`rg -n "link_own|own_jar_socket" src crates` finds only `lib.rs:412`), no length guard on `:name` or `session::start`, and nothing else canonicalizes on the jar side (`rg -n canonicalize src/pane/jar crates/spyc-jar/src` finds only `session.rs:152` and `store.rs:102`, which compares binaries).

### Evidence: command and output (wider than the finding)

I ran `refute-D/p_len.py` (the unit's `p_len.py` and `ud.py`, copied) with `env -u SPYC_CONTEXT -u SPYC_JAR_SOCKET -u SPYC_MCP_SOCK -u SPYC_PANE_ID -u SSH_AUTH_SOCK`. `HOME`, `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `TMPDIR` all pointed into `refute-D/`. `XDG_RUNTIME_DIR=/tmp/rfD` was a link to `refute-D/rt`, so the given path is short and the canonical one is long. The session name was a **12-character** `REFUTE_SHORT`.

```
name len 12 given path len 40 canonical len 145
session jar {'owner_pid': '56550', 'program_pid': '58101'} tab jar {'owner_pid': '', 'program_pid': '57733'}
screen mentions own name: False | orphan offer: ['restore', 'discard', 'Restore']
events CORIANDER_THYME.log … spyc 2.5.0-CURRENT (444857a2) started, pid 58101     (10:04:12)
events REFUTE_SHORT.log … 10:03:52 detached: spyc moves into REFUTE_SHORT's own jar
```

The spyc in the session jar took the fresh name `CORIANDER_THYME`, 20 s after the detach (that is `LINK_WAIT`). The tab's jar was left with no owner and its program running, and the screen offered the orphan prompt. Afterwards both jars refused connections (`spyc-jar describe … Connection refused`), `ps` showed none of my processes left, and I removed the `/tmp/rfD` link.

The narrower form, then: **whenever the canonical sessions path is longer than the one given and only the given one fits, every `^d` handover fails this way, and so does every `:detach project` spyc started with `Begin::Now`. A user-typed name of 28 to 35 characters under macOS's default `TMPDIR` is one way in; a symlinked runtime or temp directory whose real path is long is another, at any name length.** On Linux with `/run/user/N` there is no gap.

### Why medium, not high

- **Narrow trigger.** On a default macOS setup it needs a name the user typed, 28 to 35 characters long. Generated names never get there.
- **Nothing is lost by default.** The manifest is saved first. The orphan prompt offers `r` (restore), and `Esc` leaves the programs running. `spyc -r` brings the session back.
- **The case for keeping it high.** `d` discards with one key and no confirmation (`src/app/orphan_jars.rs:147-170`), and the unit's own run showed typed-ahead keys answering the prompt. A user who types into what they take to be their session can end every agent. That is why this is medium and not low.

**What would make me wrong:** users, or the owner, often pick `:name`s of 28 or more characters, or run with a symlinked `XDG_RUNTIME_DIR` or `TMPDIR`. Or typed-ahead `d` hits the orphan prompt routinely after an attach. In any of those cases high stands.
