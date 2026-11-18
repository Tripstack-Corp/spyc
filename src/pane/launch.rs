//! Starting a pane: its program on a PTY of spyc's own, or in a jar when the
//! tab asks for one, with the environment every pane's program gets.

use std::path::Path;

use super::pty_host::{PtyHost, PtySpec};

use super::{Pane, PaneEngine, PaneWake, engine, jar};

/// What a pane tab runs, where, and whether in a jar.
pub struct Launch<'a> {
    pub command: &'a str,
    pub rows: u16,
    pub cols: u16,
    pub cwd: &'a Path,
    pub context_path: &'a Path,
    pub extra_env: &'a [(&'a str, &'a str)],
    /// The project's `:setenv` variables.
    pub project_env: &'a [(String, String)],
    pub jar: Option<jar::Spec>,
}

impl Pane {
    /// Spawn `command` in a fresh pty of `rows × cols`, with `cwd` as
    /// the working directory. `context_path` points at *App's* live
    /// context file (the one the main loop writes to) so the child's
    /// `SPYC_CONTEXT` always resolves to a real file regardless of
    /// where the pane itself spawns — App writes one canonical file
    /// (`context::process_context_path`), but a pane can spawn in any
    /// subdir, and recomputing from `cwd` would point at a path nobody
    /// writes. Tests' spawn: a project's tab or overlay goes through
    /// [`Self::launch`] with its `:setenv` variables.
    #[cfg(test)]
    pub fn spawn(
        command: &str,
        rows: u16,
        cols: u16,
        cwd: &Path,
        context_path: &Path,
        wake: PaneWake,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_env(command, rows, cols, cwd, context_path, &[], wake)
    }

    #[cfg(test)]
    pub fn spawn_with_env(
        command: &str,
        rows: u16,
        cols: u16,
        cwd: &Path,
        context_path: &Path,
        extra_env: &[(&str, &str)],
        wake: PaneWake,
    ) -> anyhow::Result<Self> {
        Self::launch(
            Launch {
                command,
                rows,
                cols,
                cwd,
                context_path,
                extra_env,
                project_env: &[],
                jar: None,
            },
            wake,
        )
    }

    /// Spawn a pane as `launch` describes: in a jar when it asks for one and
    /// one can be started.
    pub fn launch(launch: Launch, wake: PaneWake) -> anyhow::Result<Self> {
        let Launch {
            command,
            rows,
            cols,
            cwd,
            context_path,
            extra_env,
            project_env,
            jar,
        } = launch;
        // Pane env: tell child processes (e.g. Claude CLI's MCP server)
        // where this spyc instance's context file lives, and advertise
        // truecolor support so apps that negotiate it (bat, fzf, delta,
        // …) don't silently downgrade to 256-colour. TERM=xterm-256color
        // alone doesn't signal 24-bit; COLORTERM=truecolor is the de
        // facto standard pair.
        let context_str = context_path.to_string_lossy().into_owned();
        let mut env: Vec<(&str, &str)> = Vec::with_capacity(extra_env.len() + 2);
        env.push((crate::context::CONTEXT_ENV_VAR, context_str.as_str()));
        env.push(("COLORTERM", "truecolor"));
        env.extend(extra_env.iter().copied());

        let mut host = PtyHost::spawn(PtySpec {
            command,
            rows,
            cols,
            cwd,
            env: &env,
            project_env,
            term: "xterm-256color",
            // Send SIGWINCH right after spawn so rc-file shells
            // (p10k / oh-my-zsh) re-query the pty size and render
            // their first prompt at the right geometry.
            nudge_winch: true,
            debug_dump: std::env::var("SPYC_PTY_DEBUG").is_ok(),
            // Interactive pane: exec the agent so it replaces the rc-sourcing
            // shell (no job-control wrapper to fight `^z` suspend/resume).
            exec_replace: true,
            capture_launch_env: !extra_env.is_empty(),
            jar,
        })?;
        // A program taken back from its jar: its pane starts from the jar's
        // snapshot, and the replay brings it up to date.
        let parser = match host.take_adoption().and_then(|a| a.snapshot) {
            Some(snapshot) => jar::snapshots::engine_from(&snapshot, (rows, cols)),
            None => <PaneEngine as engine::Engine>::new(rows, cols, 10_000),
        };
        Ok(Self::adopt(host, parser, wake))
    }

    /// A tab's stand-in pane, around `jar`'s stand-ins (`jar::starting`,
    /// `jar::unstarted`): `starting` while the tab's jar starts or is taken
    /// back.
    pub fn standing_in(
        jar: jar::Jarred,
        starting: bool,
        (rows, cols): (u16, u16),
        wake: PaneWake,
    ) -> anyhow::Result<Self> {
        let host = PtyHost::around_jar(jar, (rows, cols))?;
        let parser = <PaneEngine as engine::Engine>::new(rows, cols, 10_000);
        let mut pane = Self::adopt(host, parser, wake);
        pane.starting = starting;
        Ok(pane)
    }
}
