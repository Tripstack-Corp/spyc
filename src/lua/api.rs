//! The `spyc` global table exposed to Lua scripts.
//!
//! Read functions pull from the [`Bridge`](super::bridge::Bridge)'s context
//! snapshot; mutation/action functions enqueue a [`LuaRequest`] (capped at
//! [`MAX_REQUESTS`]). Every closure captures a clone of the shared bridge `Rc`;
//! none touch the `App`. Registered once per interpreter.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mlua::{Function, Lua, RegistryKey, Table, Value, Variadic};
use serde_json::Value as JsonValue;

use super::bridge::{
    LuaRequest, MAX_REGISTRATIONS, MAX_REQUESTS, RegKind, Registration, SharedBridge,
};

/// Cap on results a single live read returns to Lua, mirroring the MCP read
/// tools' defaults (`search_paths` 100, `search_content` 200, `git_log` 20).
/// A script that wants more can loop; these keep a pathological `spyc.read` of
/// a monorepo from flooding the interpreter.
const SEARCH_PATHS_LIMIT: usize = 100;
const SEARCH_CONTENT_LIMIT: usize = 200;
const GIT_LOG_LIMIT: usize = 20;

/// Worker-thread-local store of the Lua callbacks `init.lua` registers via
/// `spyc.map` / `spyc.command` / `spyc.on`. Unlike the per-run
/// [`Bridge`](super::bridge::Bridge), it PERSISTS across runs — a registered
/// callback lives until the next reload, when [`FnRegistry::clear`] drops every
/// key. Held by the worker as `Rc<RefCell<…>>` so the API closures (which
/// insert) and the run driver (which looks up by id) share it.
#[derive(Default)]
pub struct FnRegistry {
    next_id: u64,
    fns: HashMap<u64, RegistryKey>,
}

impl FnRegistry {
    /// Store `key`, returning the id to address it by later.
    fn insert(&mut self, key: RegistryKey) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.fns.insert(id, key);
        id
    }

    /// The `RegistryKey` for `fn_id`, if one is stored.
    pub fn get(&self, fn_id: u64) -> Option<&RegistryKey> {
        self.fns.get(&fn_id)
    }

    /// Drop every stored callback (used on reload, where a fresh `Lua` makes
    /// the old `RegistryKey`s meaningless anyway).
    pub fn clear(&mut self) {
        self.next_id = 0;
        self.fns.clear();
    }
}

/// Shared handle to the persistent callback registry.
pub type SharedFnRegistry = Rc<RefCell<FnRegistry>>;

/// Append a request, or error (aborting the script) once the per-run cap is hit.
fn enqueue(bridge: &SharedBridge, req: LuaRequest) -> mlua::Result<()> {
    let mut b = bridge.borrow_mut();
    if b.requests.len() >= MAX_REQUESTS {
        return Err(mlua::Error::runtime(
            "spyc: too many requests in one script run (limit 10000)",
        ));
    }
    b.requests.push(req);
    Ok(())
}

/// Register a callback: store its `Function` in `fnreg` and record a
/// [`Registration`] of `kind` on the bridge. Errors (aborting the load) once
/// the per-load registration cap is hit.
fn register(
    lua: &Lua,
    bridge: &SharedBridge,
    fnreg: &SharedFnRegistry,
    kind: RegKind,
    f: Function,
) -> mlua::Result<()> {
    if fnreg.borrow().fns.len() >= MAX_REGISTRATIONS {
        return Err(mlua::Error::runtime(
            "spyc: too many registrations in init.lua (limit 10000)",
        ));
    }
    let key = lua.create_registry_value(f)?;
    let fn_id = fnreg.borrow_mut().insert(key);
    bridge
        .borrow_mut()
        .registrations
        .push(Registration { kind, fn_id });
    Ok(())
}

/// Install the `spyc` global table into `lua`, backed by `bridge` (per-run
/// requests/registrations) and `fnreg` (the persistent callback store the
/// `spyc.map`/`command`/`on` registrars insert into).
pub fn install(lua: &Lua, bridge: &SharedBridge, fnreg: &SharedFnRegistry) -> mlua::Result<()> {
    let spyc = lua.create_table()?;

    // ---- reads (from the current snapshot) ----
    let b = Rc::clone(bridge);
    spyc.set(
        "context",
        lua.create_function(move |lua, ()| context_table(lua, &b))?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "cwd",
        lua.create_function(move |_, ()| {
            Ok(b.borrow()
                .snapshot
                .as_ref()
                .map(|s| s.cwd.to_string_lossy().into_owned()))
        })?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "cursor",
        lua.create_function(move |_, ()| {
            Ok(b.borrow()
                .snapshot
                .as_ref()
                .and_then(|s| s.cursor_file.clone()))
        })?,
    )?;

    // ---- live reads (computed on the worker, returned synchronously) ----
    // These call the same root-scoped, thread-safe readers the off-loop MCP
    // tools use (`src/mcp/readers.rs`, `crate::fs::{finder,grep}`), reading
    // fs/gix directly from the snapshot's read root — no round-trip to the main
    // loop. A read runs as a native call, so the instruction-count hook can't
    // interrupt it mid-read; a pathological search blocks the WORKER (never the
    // UI) until it returns, and the hard ceiling still bounds the whole run once
    // control returns to Lua. All reads RAISE an `mlua::Error` on genuine
    // failure (bad path, not-a-repo file read, invalid regex); "nothing here"
    // (clean tree, empty repo, no matches) is an empty table.
    let b = Rc::clone(bridge);
    spyc.set(
        "worktrees",
        lua.create_function(move |lua, ()| {
            let root = read_root(&b)?;
            json_str_to_lua(lua, &crate::mcp::readers::list_worktrees_json_at(&root))
        })?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "git_status",
        lua.create_function(move |lua, ()| {
            let root = read_root(&b)?;
            json_str_to_lua(lua, &crate::mcp::readers::git_status_json(&root))
        })?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "git_log",
        lua.create_function(move |lua, opts: Option<Table>| {
            let limit = opts
                .as_ref()
                .and_then(|t| t.get::<Option<usize>>("limit").ok().flatten())
                .unwrap_or(GIT_LOG_LIMIT);
            let root = read_root(&b)?;
            json_str_to_lua(lua, &crate::mcp::readers::git_log_json(&root, limit))
        })?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "read",
        lua.create_function(move |_, path: String| {
            let resolved = resolve_read_path(&b, &path)?;
            crate::mcp::readers::read_file_content(&resolved).map_err(mlua::Error::runtime)
        })?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "search_paths",
        lua.create_function(move |lua, query: String| {
            let root = read_root(&b)?;
            let paths = crate::fs::finder::find_paths(&root, &query, SEARCH_PATHS_LIMIT);
            let t = lua.create_table()?;
            for (i, p) in paths.iter().enumerate() {
                t.set(i + 1, p.to_string_lossy().into_owned())?;
            }
            Ok(t)
        })?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "search_content",
        lua.create_function(move |lua, regex: String| {
            let root = read_root(&b)?;
            let hits = crate::fs::grep::search_to_vec(&root, &regex, SEARCH_CONTENT_LIMIT)
                .map_err(mlua::Error::runtime)?;
            let t = lua.create_table()?;
            for (i, m) in hits.iter().enumerate() {
                let row = lua.create_table()?;
                row.set("file", m.path.to_string_lossy().into_owned())?;
                row.set("line", m.line)?;
                row.set("text", m.text.clone())?;
                t.set(i + 1, row)?;
            }
            Ok(t)
        })?,
    )?;

    // ---- mutations / actions (enqueue a request) ----
    let b = Rc::clone(bridge);
    spyc.set(
        "action",
        lua.create_function(move |_, (name, count): (String, Option<u32>)| {
            enqueue(&b, LuaRequest::Action { name, count })
        })?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "cmd",
        lua.create_function(move |_, line: String| enqueue(&b, LuaRequest::Command(line)))?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "navigate",
        lua.create_function(move |_, path: String| enqueue(&b, LuaRequest::Navigate(path)))?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "pick",
        lua.create_function(move |_, globs: Variadic<String>| {
            enqueue(&b, LuaRequest::Pick(globs.into_iter().collect()))
        })?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "clear_picks",
        lua.create_function(move |_, ()| enqueue(&b, LuaRequest::ClearPicks))?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "filter",
        lua.create_function(move |_, glob: Option<String>| enqueue(&b, LuaRequest::Filter(glob)))?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "report_status",
        lua.create_function(move |_, status: String| {
            enqueue(&b, LuaRequest::ReportStatus(status))
        })?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "notify",
        lua.create_function(move |_, msg: String| enqueue(&b, LuaRequest::Notify(msg)))?,
    )?;

    let b = Rc::clone(bridge);
    spyc.set(
        "warn",
        lua.create_function(move |_, msg: String| enqueue(&b, LuaRequest::Warn(msg)))?,
    )?;

    // ---- registration (Tier B, evaluated when init.lua loads) ----
    let b = Rc::clone(bridge);
    let r = Rc::clone(fnreg);
    spyc.set(
        "map",
        lua.create_function(move |lua, (key, f): (String, Function)| {
            register(lua, &b, &r, RegKind::Map(key), f)
        })?,
    )?;

    let b = Rc::clone(bridge);
    let r = Rc::clone(fnreg);
    spyc.set(
        "command",
        lua.create_function(move |lua, (name, f): (String, Function)| {
            register(lua, &b, &r, RegKind::Command(name), f)
        })?,
    )?;

    // Event hooks are recorded but not yet dispatched: the App stores the
    // registration so the Tier-C event seam can later fire it.
    let b = Rc::clone(bridge);
    let r = Rc::clone(fnreg);
    spyc.set(
        "on",
        lua.create_function(move |lua, (event, f): (String, Function)| {
            register(lua, &b, &r, RegKind::Event(event), f)
        })?,
    )?;

    lua.globals().set("spyc", spyc)?;
    Ok(())
}

/// The root a live read scopes to: the snapshot's `search_root` (the focused
/// column's worktree root) when set, else its `cwd`. Errors (aborting the read)
/// if no snapshot is installed — which never happens for a real run.
fn read_root(bridge: &SharedBridge) -> mlua::Result<PathBuf> {
    let b = bridge.borrow();
    let s = b
        .snapshot
        .as_ref()
        .ok_or_else(|| mlua::Error::runtime("spyc: no context available"))?;
    Ok(s.search_root.clone().unwrap_or_else(|| s.cwd.clone()))
}

/// Resolve a `spyc.read` path argument: an absolute path as-is, else relative
/// to the snapshot's `cwd` (matching the MCP `get_file_content` contract that
/// relative paths resolve against spyc's cwd).
fn resolve_read_path(bridge: &SharedBridge, path: &str) -> mlua::Result<PathBuf> {
    let raw = Path::new(path);
    if raw.is_absolute() {
        return Ok(raw.to_path_buf());
    }
    let b = bridge.borrow();
    let s = b
        .snapshot
        .as_ref()
        .ok_or_else(|| mlua::Error::runtime("spyc: no context available"))?;
    Ok(s.cwd.join(raw))
}

/// Parse a reader's JSON string and convert it to a Lua value. The readers
/// (`git_status_json`, `git_log_json`, `list_worktrees_json_at`) already emit
/// JSON, so each read fn is just `reader(root) -> String -> here`. A parse
/// failure raises (it means a reader returned malformed JSON — a bug, not a
/// user error).
fn json_str_to_lua(lua: &Lua, json: &str) -> mlua::Result<Value> {
    let v: JsonValue = serde_json::from_str(json)
        .map_err(|e| mlua::Error::runtime(format!("spyc: reader returned bad JSON: {e}")))?;
    json_to_lua(lua, &v)
}

/// Convert a `serde_json::Value` to an `mlua::Value`: object → table keyed by
/// string, array → 1-based sequence table, string/number/bool → the Lua scalar,
/// null → `nil`. The single marshaling point for every JSON-producing reader.
fn json_to_lua(lua: &Lua, v: &JsonValue) -> mlua::Result<Value> {
    Ok(match v {
        JsonValue::Null => Value::Nil,
        JsonValue::Bool(b) => Value::Boolean(*b),
        JsonValue::Number(n) => {
            // Prefer an exact integer; fall back to float for fractional /
            // out-of-range values. `as_f64` never fails for a JSON number.
            if let Some(i) = n.as_i64() {
                Value::Integer(i)
            } else {
                Value::Number(n.as_f64().unwrap_or(0.0))
            }
        }
        JsonValue::String(s) => Value::String(lua.create_string(s)?),
        JsonValue::Array(arr) => {
            let t = lua.create_table()?;
            for (i, item) in arr.iter().enumerate() {
                t.set(i + 1, json_to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
        JsonValue::Object(map) => {
            let t = lua.create_table()?;
            for (k, item) in map {
                t.set(k.as_str(), json_to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
    })
}

/// Build the `spyc.context()` table from the current snapshot (empty table when
/// no snapshot is set).
fn context_table(lua: &Lua, bridge: &SharedBridge) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    let b = bridge.borrow();
    let Some(s) = b.snapshot.as_ref() else {
        return Ok(t);
    };
    t.set("cwd", s.cwd.to_string_lossy().into_owned())?;
    t.set("cursor_file", s.cursor_file.clone())?;
    t.set("filter", s.filter.clone())?;
    t.set("git_branch", s.git_branch.clone())?;
    t.set(
        "project_home",
        s.project_home
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
    )?;
    t.set("session_name", s.session_name.clone())?;
    t.set("version", s.version.clone())?;
    let picks = lua.create_table()?;
    for (i, p) in s.picks.iter().enumerate() {
        picks.set(i + 1, p.to_string_lossy().into_owned())?;
    }
    t.set("picks", picks)?;
    Ok(t)
}
