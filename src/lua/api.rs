//! The `spyc` global table exposed to Lua scripts.
//!
//! Read functions pull from the [`Bridge`](super::bridge::Bridge)'s context
//! snapshot; mutation/action functions enqueue a [`LuaRequest`] (capped at
//! [`MAX_REQUESTS`]). Every closure captures a clone of the shared bridge `Rc`;
//! none touch the `App`. Registered once per interpreter.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Function, Lua, RegistryKey, Table, Variadic};

use super::bridge::{
    LuaRequest, MAX_REGISTRATIONS, MAX_REQUESTS, RegKind, Registration, SharedBridge,
};

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
