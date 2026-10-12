//! Reading and writing a file in an agent's user configuration without
//! disturbing how the user keeps it.

use std::io;
use std::path::Path;

/// The file's text, `None` when there is no file. A link to a file that doesn't
/// exist is an error, not an absence: writing there would replace the link.
pub(super) fn read(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound && path.is_symlink() => Err(format!(
            "{} is a link to a file that doesn't exist",
            path.display()
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("reading {}: {e}", path.display())),
    }
}

/// Make `path` hold `text`, unless it already does. Returns whether it wrote.
///
/// SPYC-TRAP(user-config-write-through-symlink): a link is written through, so a
/// dotfiles manager's link stays a link, and an existing file keeps its mode.
/// An atomic rename onto `path` would do neither.
pub(super) fn write(path: &Path, text: &str) -> Result<bool, String> {
    if read(path)?.as_deref() == Some(text) {
        return Ok(false);
    }
    let target = if path.is_symlink() {
        std::fs::canonicalize(path).map_err(|e| format!("resolving {}: {e}", path.display()))?
    } else {
        path.to_path_buf()
    };
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let permissions = std::fs::metadata(&target).map_or_else(
        |_| std::os::unix::fs::PermissionsExt::from_mode(0o644),
        |m| m.permissions(),
    );
    crate::fs::write_atomic_with_permissions(&target, text.as_bytes(), permissions)
        .map_err(|e| format!("writing {}: {e}", target.display()))?;
    Ok(true)
}
