# Launch prep — spyc 2.0

Working doc for the pre-2.0 launch hygiene pass. Captures decisions
to make, work to do, and the criteria we're trying to hit before
calling 2.0 a "public beta, daily-driver-ready" release.

The framing is benchmarked against Yazi
(github.com/sxyazi/yazi → 37k stars), used as the gold-standard
example of a reputable, install-and-rely-on TUI tool. The MCP /
Claude-Code pairing remains spyc's differentiator and is not
something Yazi has — keep it front and centre.

## Launch goal

A 2.0 release that someone reading the repo cold can trust enough
to make their daily file manager. Not a promotion blitz — just
enough signal to feel "this is real, maintained, and works for me."

## Open decisions

- [ ] **GitHub org account.** Etraveli vs Tripstack vs personal
  (`@derekmarshall`). Whichever we pick becomes the canonical home
  (`github.com/<org>/spyc`); everything downstream — Cargo.toml's
  `repository =`, `.github/` workflows, Homebrew tap namespace,
  Discord/Slack pointers — keys off this. Decision blocks all
  github-side work below.
- [ ] **License footer.** Already BSD-3-Clause in `Cargo.toml`;
  confirm we're still good with that for the public release and
  that the LICENSE file is present at repo root.
- [ ] **Status statement wording.** Default proposal:
  *"Public beta, daily-driver-ready. macOS and Linux."* (Lifted
  from Yazi's "Public beta, can be used as a daily driver" — sets
  expectations explicitly without overpromising.)

## Top 3 — required for 2.0

### 1. GitHub move
- [ ] Create the public repo under the chosen org.
- [ ] Push history (preserve commits, tags, signed commits).
- [ ] Update `Cargo.toml` `repository = "https://github.com/<org>/spyc"`.
- [ ] Update README links, INSTALL.md links, any internal cross-refs.
- [ ] Decide whether Bitbucket stays as a private mirror or is
      retired entirely. (Recommendation: retire — single source of
      truth is simpler.)
- [ ] Set up branch protection on `main` (block force-push, require
      status checks if/when CI lands).

### 2. Demo capture at top of README
- [ ] Record a 30–60s asciinema or MP4 showing the full Claude
      pairing loop. Suggested script:
  1. Launch spyc, top bar visible (`🌶️ | spyc | spice-name | path`).
  2. Press `F`, type a fragment, Enter — fuzzy-finds and cursors a file.
  3. `:grep <pattern>` — streaming results in the pager.
  4. `^\` to focus the Claude pane.
  5. Ask Claude something like "what files am I picking?" — it
     calls `get_spyc_context` and answers.
  6. Claude references a path; user presses `gf` to jump to it.
  7. Quit — terminal title restoration visible.
- [ ] Drop into README right after the headline value-prop.
- [ ] Update the existing `screen_shot.png` link or replace it.

### 3. Release pipeline + binaries
- [ ] Add `.github/workflows/release.yml` — on tag push, build with
      `release-static` profile for:
  - macOS arm64 (Apple Silicon)
  - macOS x86_64 (Intel)
  - Linux x86_64 (musl, statically linked)
  - Linux arm64 (musl)
- [ ] Attach binaries to GitHub Releases automatically.
- [ ] Sign release artifacts (sigstore or `gh attestation`) — nice
      to have, defer if it gets complex.
- [ ] **Homebrew tap**: `brew tap <org>/spyc && brew install spyc`.
      Auto-update the formula's URL+sha from the release workflow.
- [ ] AUR `spyc-bin` package — defer to post-2.0 unless someone
      volunteers; AUR is community-maintained and we don't have
      to own it.

## Cheap wins — batch with the launch pass

### 4. README hygiene
- [ ] Remove the stale `v1.21.1` status line (line 14). Replace
      with the agreed status statement.
- [ ] Confirm the screenshot/demo placement is the *first* media
      element after the value prop.
- [ ] Re-read the headline — make sure it sells the Claude angle
      in one sentence.
- [ ] Confirm the keybinding tables match the current keymap
      (we keep these in sync per CLAUDE.md, but spot-check before
      launch).

### 5. `.github/` scaffolding
- [ ] `.github/ISSUE_TEMPLATE/bug.yml` — repro steps, version,
      OS, terminal, expected vs actual.
- [ ] `.github/ISSUE_TEMPLATE/feature.yml` — what / why / would
      you use this yourself.
- [ ] `.github/PULL_REQUEST_TEMPLATE.md` — short: what changed,
      why, how tested.
- [ ] `SECURITY.md` — one line: "report to <email>". No bug
      bounty, no fancy process.
- [ ] `CODE_OF_CONDUCT.md` — Contributor Covenant boilerplate.
- [ ] `CONTRIBUTING.md` — already exists; review and refresh.

### 6. "Coming from X" migration page
- [ ] One `MIGRATION.md` (or section in README) with three small
      keybind-comparison tables: ranger → spyc, lf → spyc, Yazi →
      spyc. ~10 keybinds each — most-used motions, picks, common
      commands. Plus a one-paragraph "things spyc has that the
      others don't" note about the Claude/MCP integration.

## Deferred (explicitly not 2.0)

- Dedicated docs site (Astro/Starlight/Docusaurus). The existing
  Markdown reads fine on GitHub. Revisit if docs grow past
  comfortable single-file size.
- Blog / release-flavored marketing posts. CHANGELOG.md is enough.
  A single Show HN post at 2.0 is plenty.
- Windows support. Real engineering, separate scope.
- Discord / Matrix / discussion forum. GitHub Discussions can be
  enabled post-launch if there's traffic to handle; a chat channel
  is a maintenance commitment we don't want pre-2.0.
- Sponsorship buttons. Skip until traction warrants it.
- Plugin/extension system. Out of thesis (per ROADMAP non-goals).

## Done-criteria for 2.0 launch

A user landing on the GitHub repo cold should be able to:

1. Watch a 30-second demo in the README and understand what spyc
   does and why it's different.
2. Install via `brew install <org>/spyc/spyc` *or* download a
   pre-built binary from Releases — no Rust toolchain required.
3. Read FEATURES.md and INSTALL.md without hitting broken links
   or stale version numbers.
4. File a bug or feature request via templated issues.
5. See an active recent release on the Releases page (within
   the last ~30 days) and a current CHANGELOG entry.
6. Read the CHANGELOG and see a clear 2.0 entry that says "what
   changed since 1.x and what stability we promise going forward."

## Sequencing

Items in roughly the order they should be tackled, post GitHub-
account decision:

1. GitHub move (blocks everything else).
2. README hygiene + status statement (read for first impression).
3. Demo capture (highest-leverage signal).
4. `.github/` scaffolding (cheap, parallelizable).
5. Release pipeline + first tagged 2.0 binary build.
6. Homebrew tap + formula.
7. Migration page.

CHANGELOG entry for 2.0 itself is the last thing, written once
the binaries are out and we've used our own builds for a few
days.
