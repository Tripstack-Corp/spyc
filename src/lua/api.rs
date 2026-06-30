//! The `spyc` global table exposed to Lua scripts.
//!
//! Read functions pull from the [`Bridge`](super::bridge::Bridge)'s context
//! snapshot; mutation/action functions enqueue a [`LuaRequest`] (capped at
//! [`MAX_REQUESTS`]). Every closure captures a clone of the shared bridge `Rc`;
//! none touch the `App`. Registered once per interpreter.

use std::rc::Rc;

use mlua::{Lua, Table, Variadic};

use super::bridge::{LuaRequest, MAX_REQUESTS, SharedBridge};

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

/// Install the `spyc` global table into `lua`, backed by `bridge`.
pub fn install(lua: &Lua, bridge: &SharedBridge) -> mlua::Result<()> {
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
