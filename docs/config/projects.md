# Per-project files — `~/.config/spyc/projects/`

Part of [Configuring spyc](../../CONFIGURATION.md).

Your settings for one project go in a file of your own, beside the rest of
spyc's config, never in the project's directory:

```
~/.config/spyc/projects/<name>.toml        # $XDG_CONFIG_HOME/spyc/projects/ when that is set
```

```toml
home = "~/src/spyc"          # the project this file configures

[pane]
tabs = ["claude", "zsh"]     # this project's startup tabs

[colors]
dir = "#88c0d0"              # tint each project, so a glance says which one
```

## Which project a file configures

- **Its `home` names the project:** the directory the project is anchored at, its
  `PROJECT_HOME`, else its start directory. `~` is your home; anything else must
  be an absolute path. The file's name doesn't decide which project it is for.
- **The file's name is the project's name.** `work.toml` calls its project
  `work` in the status bar, `Space j`, notifications, the terminal title and the
  `-r` picker. A project with no file is named for its directory, as before.
- **Two files naming one home:** the first by name applies, and spyc warns,
  naming both.
- **A file whose home has gone** (a worktree removed after its merge, say) is
  skipped quietly. One that doesn't parse, or names no usable home, is warned
  about in the project it is named after.
- **A project needs no file.** `Space n` on a directory with none creates
  nothing.

## Making one

**`:project config`** opens the project's file in your editor, in the top
pane as `V` does, so the agents below stay in view. With no file yet, it makes
`<name>.toml`, named for the project, with its `home` filled in (`~` for your
home), and the project goes by it from then on; a name another project's file
has gets `-2`. A file is never written over: one of that name that names this
project, or no usable home, is opened as it is. Saving it reloads it. (A
directory called `config` is `:project ./config`.)

## How it layers

A project's config is three files, each overriding the one before, per field:

1. `~/.spycrc.toml`, your defaults;
2. the repo's own `<start dir>/.spycrc.toml`, if it ships one;
3. the project's file here.

A repo's `.spycrc.toml` that can't load is skipped whole, with a warning,
and the other two still apply.

Keymap bindings append, so a later file's binding wins its key. Startup tabs
replace: the last file that lists any is the list, so a project file's tabs
open in place of a repo's, without the repo's consent prompt. `[[ignore_masks]]`
add up across the files, so a project file can't turn off a group your
`~/.spycrc.toml` turned on.

## Trust

The file is yours, as `~/.spycrc.toml` is, so it is honoured in full:
executing bindings, `[prompts]`, `[clipboard] command` and startup tabs all
work from it. The one exception is `codex_mcp`, which reaches every project
and is read from `~/.spycrc.toml` alone. A repo's `.spycrc.toml` is someone else's text, and those stay
off there. A `home` key anywhere but here is ignored, with a warning.

## When it is read

When a project opens (`Space n`, `spyc -r`), when you edit any file here or
press `^R`, and when the project moves: `Space P`, `:project <path>`,
`:project clear`, `gS`, or `-r` restoring it somewhere else read again the
file that names its new place.
