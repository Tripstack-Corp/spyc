#!/usr/bin/env bash
# Shared fixture for the README tour tapes. Source it (never execute it) from a
# tape's hidden setup, with the repo root as cwd:
#
#     source docs/assets/demo/demo-env.sh
#
# It builds a throwaway HOME + a small believable git repo under /tmp and cds
# into the repo, leaving the caller ready to type `spyc`.
#
# Why a private HOME: spyc resolves its trusted config (`$HOME/.spycrc.toml`),
# its Lua scripts (`$HOME/.config/spyc/`) and its whole state dir off HOME. A
# demo recorded against the author's real HOME picks up their theme, their
# session history and their config-load flash — and the Lua loop can't work at
# all without a trusted rc to bind the key from. Repointing HOME makes every
# frame reproducible on any machine and leaves the recorder's own setup alone.
#
# Why the repo has ~14 files across three dirs: a one-file fixture renders a
# commander with an empty listing, which is the opposite of the point. This is
# the smallest tree that still fills the frame and gives every loop something
# real to act on — markdown, syntax, an image, TODOs to pick, a diff to review.

set -euo pipefail

REPO_ROOT="$PWD"
DEMO="${DEMO_DIR:-/tmp/spyc-tour}"
rm -rf "$DEMO"
mkdir -p "$DEMO"

# ── A private HOME: trusted rc, Lua scripts, and a fresh state dir ────────────
export HOME="$DEMO/home"
unset XDG_CONFIG_HOME XDG_STATE_HOME
mkdir -p "$HOME/.config/spyc/lua"

cat > "$HOME/.spycrc.toml" <<'RC'
# The keymap is a TOML array of DSL lines. `lua` is an executing verb, so it
# only binds from the trusted $HOME rc — never a project-local one.
keymap = [
  "map T lua todos",
]

# The branded border pulse is the half of the "an agent needs you" cue that a
# recording can actually capture; the desktop ping is an OS popup outside the
# terminal, so it would only fire on the recorder's own screen.
[notify]
visual = true
desktop = false
RC

cat > "$HOME/.config/spyc/lua/todos.lua" <<'LUA'
-- Pick every file here that still has a TODO in it.
--
-- `search_content` is spyc's own gitignore-aware search, so this sees exactly
-- the tree the file list does — no shelling out to rg. `pick` takes globs
-- matched against the current listing, so we reduce each hit to its basename.
local seen = {}
for _, hit in ipairs(spyc.search_content("TODO")) do
  seen[hit.file:match("[^/]+$")] = true
end

local names = {}
for name in pairs(seen) do
  names[#names + 1] = name
end
table.sort(names)

spyc.pick(table.unpack(names))
spyc.notify("picked " .. #names .. " files with a TODO")
LUA

# ── The project under review ─────────────────────────────────────────────────
P="$DEMO/tour"
mkdir -p "$P/src" "$P/docs" "$P/tests"
cd "$P"

cat > README.md <<'MD'
# tour

A tiny line-oriented log parser, used here to show off spyc's pager.

## Install

```sh
cargo install tour
```

## Usage

Point it at a file and it streams the parsed records to stdout:

| Flag | Default | What it does |
|------|---------|--------------|
| -f | required | File to parse |
| -j | off | Emit JSON instead of text |
| -q | off | Suppress the summary line |

> Records are parsed lazily — a 40 GB log costs one line of memory.

1. Read the file
2. Parse each line into a `Record`
3. Render, in whichever format you asked for

See [the architecture notes](docs/architecture.md) for how the three stages
fit together.
MD

cat > Cargo.toml <<'TOML'
[package]
name = "tour"
version = "0.3.1"
edition = "2024"

[dependencies]
anyhow = "1"
serde_json = "1"
TOML

cat > docs/architecture.md <<'MD'
# Architecture

Three stages, one pass:

- **read** — a buffered reader, one line at a time
- **parse** — `parser::Record::from_line`, no allocation on the happy path
- **render** — text or JSON, chosen once at startup

Nothing buffers the whole file, which is what keeps the memory flat.
MD

cp "$REPO_ROOT/docs/assets/spyc-logo.png" docs/logo.png

# A deterministic binary, so the hex-dump beat renders identical bytes on every
# recording (/dev/urandom would make the frames differ run to run).
mkdir -p data
python3 -c "open('data/index.bin','wb').write(bytes(range(256)) * 3)"

cat > CHANGELOG.md <<'MD'
# Changelog

## 0.3.1

- Trim leading whitespace before parsing a level
- `-q` suppresses the summary line
MD

cat > justfile <<'JF'
check:
    cargo fmt --check && cargo clippy -- -D warnings && cargo test

run FILE:
    cargo run -- -f {{FILE}}
JF

cat > rust-toolchain.toml <<'TOML'
[toolchain]
channel = "1.96.0"
TOML

printf 'BSD 3-Clause License\n\nCopyright (c) 2026, the tour authors\n' > LICENSE

cat > src/main.rs <<'RS'
use anyhow::Result;

mod config;
mod parser;
mod render;

fn main() -> Result<()> {
    let cfg = config::Config::from_args()?;
    let mut out = render::writer(&cfg);

    // TODO: stream stdin when no -f is given

    for line in cfg.input()?.lines() {
        let record = parser::Record::from_line(&line?)?;
        out.write(&record)?;
    }

    out.finish()
}
RS

cat > src/config.rs <<'RS'
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};

/// Everything the run needs, resolved once at startup.
pub struct Config {
    pub path: String,
    pub json: bool,
    pub quiet: bool,
}

impl Config {
    pub fn from_args() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let mut cfg = Self { path: String::new(), json: false, quiet: false };

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-f" => cfg.path = args.next().context("-f needs a path")?,
                "-j" => cfg.json = true,
                "-q" => cfg.quiet = true,
                other => anyhow::bail!("unknown flag: {other}"),
            }
        }
        Ok(cfg)
    }

    // TODO: read the defaults from a config file, not just argv
    pub fn input(&self) -> Result<BufReader<File>> {
        let file = File::open(&self.path)
            .with_context(|| format!("opening {}", self.path))?;
        Ok(BufReader::new(file))
    }
}
RS

cat > src/parser.rs <<'RS'
use anyhow::{Result, bail};

/// One parsed log line.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub level: Level,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warn,
    Info,
}

impl Record {
    /// Parse `LEVEL target: message`. Borrows until the last moment.
    pub fn from_line(line: &str) -> Result<Self> {
        let (level, rest) = line.split_once(' ').unwrap_or((line, ""));
        let (target, message) = rest.split_once(": ").unwrap_or((rest, ""));

        let level = match level {
            "ERROR" => Level::Error,
            "WARN" => Level::Warn,
            "INFO" => Level::Info,
            other => bail!("unknown level: {other}"),
        };

        // TODO: keep the original span so a bad line can point at the column
        Ok(Self {
            level,
            target: target.to_string(),
            message: message.to_string(),
        })
    }
}
RS

cat > src/render.rs <<'RS'
use crate::config::Config;
use crate::parser::Record;
use anyhow::Result;

pub trait Writer {
    fn write(&mut self, record: &Record) -> Result<()>;
    fn finish(self: Box<Self>) -> Result<()>;
}

pub fn writer(cfg: &Config) -> Box<dyn Writer> {
    // TODO: a --color=never flag, for when stdout is a pipe
    if cfg.json {
        Box::new(Json { count: 0 })
    } else {
        Box::new(Text { count: 0, quiet: cfg.quiet })
    }
}

struct Text {
    count: usize,
    quiet: bool,
}

struct Json {
    count: usize,
}
RS

cat > tests/parser.rs <<'RS'
use tour::parser::{Level, Record};

#[test]
fn parses_a_well_formed_line() {
    let r = Record::from_line("INFO net: connected").unwrap();
    assert_eq!(r.level, Level::Info);
    assert_eq!(r.target, "net");
}

#[test]
fn rejects_an_unknown_level() {
    assert!(Record::from_line("LOUD net: hi").is_err());
}
RS

# ── Commit it, then leave a real local edit so the git gutter has something to
#    show and `gd` has a diff to render. ──────────────────────────────────────
git init -q -b main
git config user.email demo@spyc.dev
git config user.name "spyc demo"
git add -A
git commit -qm "parse, then render"

# The edit under review: a fixed bug plus its test.
python3 - <<'PY'
import re, pathlib
p = pathlib.Path("src/parser.rs")
s = p.read_text()
s = s.replace(
    '        let (level, rest) = line.split_once(\' \').unwrap_or((line, ""));',
    '        let line = line.trim_start();\n'
    '        let (level, rest) = line.split_once(\' \').unwrap_or((line, ""));',
)
s = s.replace(
    "        // TODO: keep the original span so a bad line can point at the column\n",
    "",
)
p.write_text(s)
PY

cat >> tests/parser.rs <<'RS'

#[test]
fn tolerates_leading_whitespace() {
    assert!(Record::from_line("   INFO net: indented").is_ok());
}
RS
