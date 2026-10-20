# Keymap — the `keymap` DSL

Part of [Configuring spyc](../../CONFIGURATION.md).

One string per binding in a `keymap = [ ... ]` array. Forms:

| Form | Does |
|------|------|
| `map <KEY> unix <command...>` | run a shell command (`%` = current selection) |
| `map <KEY> command <:cmd...>` | run a `:` command (e.g. `graveyard`, `activity`) |
| `map <KEY> lua <name>` | run `~/.config/spyc/lua/<name>.lua` |
| `map <KEY> prompt <name>` | type the [`[prompts]`](../../CONFIGURATION.md#prompt-templates--prompts) template `<name>` into the active pane tab |
| `map <KEY> patternpick =<glob>` | multi-select files matching a glob |
| `map <KEY> jump =<path>` | jump the file list to a directory |
| `map <KEY> <action>` | any built-in action by [name](#action-names) |
| `map <KEY> <action> =<value>` | an action with a parameter (`harpoon_jump =3`, `set_mark =a`) |
| `unmap <KEY>` | the key does nothing; the same as `map <KEY> noop` |

`<KEY>` is a single char (`f`), a Ctrl-combo (`^P`), or a named key (`<F2>`).
Neither the DSL nor `init.lua`'s `spyc.map` binds a multi-key chord.
Later lines win, so a `map` after an `unmap` of the same key binds it again.
`unmap` of a chord prefix (`g`, `y`, `H`) silences the whole chord, but a
binding for any other key leaves the chords that end in it alone:
`map f unix file %` doesn't change `gf`.

> **`^a` and `^w` are reserved.** spyc intercepts both as chord prefixes, so a
> shell (or tmux) running inside the pane never sees readline's
> `beginning-of-line` / `unix-word-rubout`. If you run an interactive shell as the
> pane child, rebind the prefixes here; inside tmux, keep spyc's `^a` distinct
> from tmux's own prefix (and `set -s escape-time 0` for snappy input).

Several low-frequency features ship as `:` commands **without** a default key so
the keymap stays uncluttered (the graveyard, the activity monitor, `:longlist`,
`:filetype`, `:chmod`), and `--print-config` lists them as commented `command`
lines to enable. Bind the ones you use:

```toml
keymap = [
  "map f unix file %",             # `file` on the cursor/selection
  "map ^P unix ps aux",
  "map H patternpick =*.hpp",
  "map A command activity",        # toggle the activity monitor
  "map ^Y command graveyard",      # recover soft-deleted files
  "map z lua mymacro",             # ~/.config/spyc/lua/mymacro.lua
  "map <F7> git_blame",            # any action by name
  "map <F8> harpoon_jump =2",      # one with a parameter
  "unmap q",                       # silence the reserved macro key
]
```

> Reminder: `unix` / `command` / `lua` / `jump` / `prompt`, and the actions
> marked below, only bind from `~/.spycrc.toml`, not a project file.

## Action names

`map <KEY> <name>` binds any built-in action; these are also the names
[`spyc.action`](lua.md#the-spyc-api) takes, bar `set_mark` and `jump_mark`. The curated
verbs above (`down`, `pick`,
`search`, …) keep working beside them. An action shown with `=N`, `=LETTER` or
`=MODE` needs that parameter (`map <F8> harpoon_jump =2`); the rest take none.

| Name | Does |
|------|------|
| `up` | move up |
| `down` | move down |
| `left` | move left |
| `right` | move right |
| `page_up` | page up |
| `page_down` | page down |
| `goto_first` | top of column |
| `goto_last` | bottom of column |
| `jump_next_git_change` | jump to next git-changed entry |
| `jump_prev_git_change` | jump to prev git-changed entry |
| `enter_or_display` | enter dir / pager on text file |
| `enter_or_edit` | enter dir / editor on file (suspends TUI) |
| `climb` | climb to parent |
| `home` | home directory |
| `toggle_pick` | toggle pick |
| `pick_pattern_prompt` | pick by pattern (prompt) |
| `pick_toggle_all` | pick all / clear |
| `take` | take into inventory |
| `untake` | remove from inventory |
| `drop` | drop from inventory |
| `toggle_inventory_view` | toggle inventory view |
| `empty_inventory` | empty inventory |
| `yank_prompt` | yank visible pane output to clipboard |
| `yank_last_prompt` | yank last typed prompt to clipboard |
| `yank_scrollback` | yank full pane scrollback to clipboard |
| `yank_paths` | yank cursor file path (or picks) to clipboard |
| `toggle_mask =N` | toggle ignore mask `N` (1 or 2) |
| `limit_prompt` | filter file list (glob, ! for picks, empty clears) |
| `command_prompt` | command line (:limit, :!, :!!, :;, etc.) |
| `shell_captured_prompt` | shell command (captured, pager) |
| `shell_foreground_prompt` | shell command (foreground) |
| `start_shell` | start shell |
| `chmod_add =MODE` | add permission bits to the selection: `MODE` is `w` or `x` |
| `search_prompt` | search |
| `search_next` | search next |
| `search_prev` | search previous |
| `jump_prompt` | jump to path (prompt) |
| `copy_prompt` | copy (prompt) |
| `move_prompt` | move (prompt) |
| `remove_prompt` | remove (confirm) |
| `make_dir_prompt` | make directory (prompt) |
| `new_file_prompt` | new file in editor (prompt) |
| `long_list` | long listing |
| `file_type` | file type |
| `sort_cycle` | cycle sort (name/size/mtime/ext) |
| `set_mark =LETTER` | set mark `LETTER` (a to z) |
| `jump_mark =LETTER` | jump to mark `LETTER` |
| `jump_prev_dir` | jump to previous directory |
| `jump_start_dir` | jump to starting directory |
| `jump_project_home` | jump to PROJECT_HOME |
| `jump_worktree_root` | jump the focused column to its worktree/repo root |
| `set_project_home_here` | set PROJECT_HOME to current dir |
| `project_open` | open a directory as a project (`Space n`) |
| `project_close` | close the project in view (`Space x`) |
| `project_list` | list the open projects, to go to one (`Space j`) |
| `project_last` | go to the project you were in last (`Space Space`) |
| `project_next` | go to the next open project, wrapping (`Space ]`) |
| `project_prev` | go to the previous open project, wrapping (`Space [`) |
| `project_by_index =N` | go to open project `N` (1 to 9; `Space 1`..`Space 9`) |
| `set_start_dir_here` | set the start dir (what `jump_start_dir` returns to) to the current dir |
| `edit_in_pane` | open editor in top pane (bottom pane stays visible) |
| `display_in_pane` | open `$PAGER` in the top pane (the bottom pane stays visible) |
| `date` | show date |
| `version` | show version |
| `show_memory` | session info |
| `color_toggle` | toggle colour (mono) |
| `set_env_prompt` | set env var |
| `toggle_activity` | toggle activity monitor |
| `help` | help |
| `about` | about spyc |
| `reload_config` | reload config |
| `toggle_pane` | toggle split pane |
| `resume_pane` | open pane with claude --resume |
| `pane_focus_down` | focus pane (down) |
| `pane_focus_up` | focus list (up) |
| `pane_send_selection` | send selection to pane (`~/.spycrc.toml` only) |
| `pane_send_prefix` | send literal ^a to pane (`~/.spycrc.toml` only) |
| `pane_grow` | grow pane |
| `pane_shrink` | shrink pane |
| `toggle_pane_zoom` | zoom pane (toggle fullscreen) |
| `pane_scroll_enter` | scroll pane history |
| `pane_scroll_save` | save pane scrollback |
| `pane_new_tab` | new pane tab |
| `pane_close_tab` | close pane tab |
| `pane_tab_by_index =N` | switch to pane tab `N` (1 to 9) |
| `pane_next_tab` | next pane tab |
| `pane_prev_tab` | prev pane tab |
| `pane_last_tab` | last pane tab |
| `pane_rename_tab` | rename pane tab |
| `pane_restart_tab` | restart pane tab command |
| `pane_fork_tab` | fork pane tab's conversation |
| `pane_pipe_content` | pipe file contents to pane (`~/.spycrc.toml` only) |
| `pane_pipe_inventory` | pipe inventory contents to pane (`~/.spycrc.toml` only) |
| `vsplit_toggle` | vertical split: open / close |
| `vsplit_toggle_height` | vertical split: full-height / top-only |
| `vsplit_focus_left` | focus left pane (a) |
| `vsplit_focus_right` | focus right pane (b) |
| `toggle_dim` | toggle dimming of the inactive pane |
| `open_second_commander` | open a second file-commander (right column) |
| `close_second_commander` | close the second file-commander |
| `open_graveyard_view` | open graveyard viewer (recover deleted) |
| `quick_select_open` | quick select — pick URL/path/SHA/IP from pane |
| `open_image_gallery` | gallery of images the agent received |
| `harpoon_jump =N` | jump to harpoon slot `N` (1 to 9) |
| `harpoon_append` | harpoon — append cursor file |
| `harpoon_remove` | harpoon — remove cursor file |
| `harpoon_open_menu` | harpoon — open menu |
| `worktree_list` | list git worktrees |
| `worktree_new` | new git worktree |
| `worktree_delete` | delete git worktree |
| `git_diff` | git diff HEAD (staged + unstaged + new) |
| `git_diff_cached` | git diff --cached (staged) |
| `git_diff_unstaged` | git diff (unstaged — since you staged) |
| `git_blame` | git blame (cursor file) |
| `git_restore` | restore deleted file (struck-through row) from git |
| `goto_file` | jump to path in pane output |
| `goto_file_line` | jump to path:line in pane output |
| `redraw` | redraw |
| `quit` | quit |
| `macro_record_reserved` | reserved for macro recording (flashes a hint) |
| `open_task_viewer` | open task viewer (most-recent bg task) |
| `reopen_last_buffer` | reopen the most-recent closed pager buffer |
| `find_file` | find file (project-wide fuzzy) |
| `noop` | nothing: what `unmap` binds |
