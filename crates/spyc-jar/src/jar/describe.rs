//! What a jar says about itself (R31): its identity, its program, its owner,
//! its log, for `spyc --jars` and a spyc looking for its jars.

use super::Shared;

/// R31: what this jar is, for `spyc --jars` and a spyc looking for its jars.
pub(super) fn describe(shared: &Shared) -> Vec<(String, String)> {
    let st = shared.lock();
    let id = shared
        .cfg
        .socket
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let mut f = vec![
        ("id", id),
        ("instance", format!("{:016x}", shared.cfg.instance)),
        ("created", shared.cfg.created.to_string()),
        (
            "jar",
            format!(
                "{} {}",
                env!("CARGO_PKG_VERSION"),
                std::env::current_exe().map_or_else(|_| String::new(), |p| p.display().to_string())
            ),
        ),
        ("pid", std::process::id().to_string()),
        ("program", shared.cfg.argv.join(" ")),
        ("program_pid", shared.child_pid.to_string()),
        (
            "cwd",
            shared
                .cfg
                .cwd
                .as_ref()
                .map_or_else(String::new, |p| p.display().to_string()),
        ),
        ("size", format!("{}x{}", st.rows, st.cols)),
        ("overflow", "drop".into()),
        (
            "linger",
            shared.cfg.linger.map_or_else(
                || "until an owner returns".into(),
                |d| format!("{} ms", d.as_millis()),
            ),
        ),
        (
            "owner",
            if st.owner.is_some() {
                "connected".into()
            } else {
                st.ownerless_since.map_or_else(
                    || "none".into(),
                    |t| format!("none for {} s", t.elapsed().as_secs()),
                )
            },
        ),
        (
            "owner_pid",
            st.owner
                .and_then(|id| shared.conns().get(&id).map(|c| c.peer_pid))
                .filter(|p| *p != 0)
                .map_or_else(String::new, |p| p.to_string()),
        ),
        (
            "log",
            format!(
                "{} of {} bytes held, {} printed, from {}",
                st.log.held, st.log.cap, st.log.head, st.log.base
            ),
        ),
        (
            "snapshots",
            st.snapshots
                .iter()
                .map(|s| format!("{}@{}", s.format, s.offset))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    ];
    if let Some((code, signaled)) = st.exit {
        f.push((
            "exit",
            if signaled {
                "signal".into()
            } else {
                code.to_string()
            },
        ));
    }
    let mut f: Vec<(String, String)> = f.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    f.extend(st.info.iter().cloned());
    f
}
