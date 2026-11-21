//! The SSH agent a pane's program reaches (#904). A tab's program keeps the
//! environment it started with, and a session outlives the login that
//! started it: once its terminal attaches from a new SSH login, the
//! `SSH_AUTH_SOCK` the program was given names an agent that has gone, and
//! its `git push` fails.
//!
//! So each tab is given a link of its own, `<runtime>/spyc/ssh-agents/<pane
//! id>`, as its `SSH_AUTH_SOCK`, and spyc points it at the agent of the
//! terminal it has now. The pane id is the tab's for good: a restore, a
//! handover, an upgrade and a project's move keep it. `spyc attach NAME`
//! points `<NAME>.session` there at its own terminal's agent before it
//! attaches, and the session's spyc, told a terminal attached, repoints its
//! tabs' links there (`app::proc`).
//!
//! A link only ever names a socket of this user's, in a directory no one else
//! can write to, and the links' directory is this user's alone (`0700`).

use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::Context;

/// Where the links are: beside the jars' sockets.
pub fn links_dir() -> Option<PathBuf> {
    Some(
        super::jar::place::this_users_sessions()
            .parent()?
            .join("ssh-agents"),
    )
}

/// The link a tab's program is given as its `SSH_AUTH_SOCK`.
pub fn tab_link(dir: &Path, pane_id: &str) -> PathBuf {
    dir.join(pane_id)
}

/// The link `spyc attach NAME` points at its terminal's agent.
pub fn session_link(dir: &Path, session: &str) -> PathBuf {
    dir.join(format!("{session}.session"))
}

/// The agent `agent` names, checked: a socket of this user's, in a
/// directory only its owner or root can change. Symlinks are followed, so
/// another link of spyc's gives the agent it points at.
pub fn checked(agent: &Path) -> anyhow::Result<PathBuf> {
    let real = std::fs::canonicalize(agent)
        .with_context(|| format!("the SSH agent {} isn't there", agent.display()))?;
    let meta = std::fs::metadata(&real).with_context(|| format!("read {}", real.display()))?;
    anyhow::ensure!(
        meta.file_type().is_socket(),
        "{} isn't a socket",
        real.display()
    );
    let me = rustix::process::getuid().as_raw();
    anyhow::ensure!(
        meta.uid() == me,
        "{} belongs to another user",
        real.display()
    );
    let dir = real.parent().context("the agent's directory")?;
    let dmeta = std::fs::metadata(dir).with_context(|| format!("read {}", dir.display()))?;
    let mode = dmeta.permissions().mode();
    let sticky = mode & 0o1000 != 0;
    anyhow::ensure!(
        (dmeta.uid() == me || dmeta.uid() == 0) && (mode & 0o022 == 0 || sticky),
        "another user could replace the agent in {}",
        dir.display()
    );
    Ok(real)
}

/// Make the links' directory, this user's alone; refuse one that isn't.
pub fn prepare(dir: &Path) -> anyhow::Result<()> {
    match std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
    {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e).with_context(|| format!("make {}", dir.display())),
    }
    let meta = std::fs::symlink_metadata(dir).with_context(|| format!("read {}", dir.display()))?;
    anyhow::ensure!(
        meta.is_dir()
            && meta.uid() == rustix::process::getuid().as_raw()
            && meta.permissions().mode() & 0o777 == 0o700,
        "{} isn't this user's alone",
        dir.display()
    );
    Ok(())
}

/// Point `link` at `agent`, already [`checked`], in one step: a program
/// reading it sees the old agent or the new, never none.
pub fn point(link: &Path, agent: &Path) -> anyhow::Result<()> {
    let dir = link.parent().context("the link's directory")?;
    prepare(dir)?;
    if std::fs::read_link(link).is_ok_and(|now| now == agent) {
        return Ok(());
    }
    let name = link
        .file_name()
        .context("the link's name")?
        .to_string_lossy();
    let tmp = dir.join(format!(".{name}.{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    std::os::unix::fs::symlink(agent, &tmp)
        .with_context(|| format!("link {} to {}", tmp.display(), agent.display()))?;
    std::fs::rename(&tmp, link).with_context(|| format!("move {} into place", link.display()))
}

/// The agent `link` points at, when it does and that agent is [`checked`].
pub fn pointed_at(link: &Path) -> Option<PathBuf> {
    std::fs::read_link(link).ok()?;
    checked(link).ok()
}

/// The agent this process's `SSH_AUTH_SOCK` names, when it has one and it
/// is [`checked`]; why not, when it has one that isn't.
pub fn from_env() -> Option<anyhow::Result<PathBuf>> {
    let agent = std::env::var_os("SSH_AUTH_SOCK").filter(|a| !a.is_empty())?;
    Some(checked(Path::new(&agent)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_dir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap()
    }

    #[test]
    fn a_link_follows_each_agent_it_is_pointed_at() {
        let agents = private_dir();
        let (a, b) = (agents.path().join("a.sock"), agents.path().join("b.sock"));
        let _la = std::os::unix::net::UnixListener::bind(&a).unwrap();
        let _lb = std::os::unix::net::UnixListener::bind(&b).unwrap();
        let links = private_dir();
        let dir = links.path().join("ssh-agents");
        let link = tab_link(&dir, "pane-1");
        point(&link, &checked(&a).unwrap()).unwrap();
        assert_eq!(pointed_at(&link), Some(std::fs::canonicalize(&a).unwrap()));
        point(&link, &checked(&b).unwrap()).unwrap();
        assert_eq!(pointed_at(&link), Some(std::fs::canonicalize(&b).unwrap()));
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        // A link to a link gives the agent behind it.
        assert_eq!(checked(&link).unwrap(), std::fs::canonicalize(&b).unwrap());
    }

    #[test]
    fn only_a_socket_of_this_users_where_no_one_else_can_write_is_an_agent() {
        let agents = private_dir();
        let file = agents.path().join("not-a-socket");
        std::fs::write(&file, "").unwrap();
        assert!(checked(&file).is_err());
        assert!(checked(&agents.path().join("missing")).is_err());
        let open = private_dir();
        std::fs::set_permissions(open.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let sock = open.path().join("a.sock");
        let _l = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let err = checked(&sock).unwrap_err().to_string();
        assert!(err.contains("another user could replace"), "{err}");
    }

    #[test]
    fn a_links_directory_others_can_read_is_refused() {
        let base = private_dir();
        let dir = base.path().join("ssh-agents");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(prepare(&dir).is_err());
    }
}
