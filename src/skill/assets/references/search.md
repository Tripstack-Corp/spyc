# Searching

Four tools, different corpora. Picking the wrong one is the usual mistake.

| Tool | Searches | Use when |
|---|---|---|
| `search_paths` | filenames, fuzzy | you half-remember a name |
| `search_content` | file contents, regex | you know a string or pattern |
| `search_picks` | only the user's current multi-select | they've already narrowed it |
| `search_inventory` | the persistent yank cache | "that file I copied earlier" |

All are gitignore-aware, so they skip `target/`, `node_modules/`, build output —
the noise that makes `grep -r` unusable in a real repo. That also means a
deliberately-ignored file **won't** be found; if you need to search ignored paths,
that's the case for shelling out.

## `search_paths`

Fuzzy filename matching, same engine as spyc's `F` finder. Good for "where is the
worktree module", bad for exact-path lookups — if you already know the path, just
`get_file_content` it.

## `search_content`

Regex over file contents via embedded ripgrep. Returns structured matches with
paths and line numbers, so you can go straight to `get_file_content` or
`navigate_to` without parsing output.

Prefer one broad pattern over several narrow calls — an alternation
(`foo|bar|baz`) in a single call beats three round-trips.

## `search_picks` and `search_inventory`

Both are spyc-specific and have no shell equivalent.

`search_picks` scopes to the files the user multi-selected. When
`get_spyc_context` shows picks, this is almost always what they mean — searching
the whole tree instead is a good way to answer a question they didn't ask.

`search_inventory` searches spyc's persistent cache of yanked files, which is how
you resolve "the file I copied earlier" without making the user re-find it.

## Scoping to another worktree

Every one of these takes an optional `root` (absolute path) to target a worktree
other than the user's focused column. Pass the path `create_worktree` or
`list_worktrees` returned.

Without `root`, you are searching the **user's** column — which, when you're
working in a worktree you just created, is the wrong tree and will look
confusingly like your changes vanished.
