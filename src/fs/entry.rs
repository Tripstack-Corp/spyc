use std::fs::Metadata;
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    Executable,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub mtime: SystemTime,
}

impl Entry {
    pub fn from_dir_entry(dir_entry: &std::fs::DirEntry) -> std::io::Result<Self> {
        let path = dir_entry.path();
        let name = dir_entry.file_name().to_string_lossy().into_owned();
        let md = dir_entry.metadata()?;
        Ok(Self::from_parts(path, name, &md))
    }

    fn from_parts(path: PathBuf, name: String, md: &Metadata) -> Self {
        let kind = if md.is_dir() {
            EntryKind::Dir
        } else if md.file_type().is_symlink() {
            EntryKind::Symlink
        } else if md.is_file() {
            if is_executable(md) {
                EntryKind::Executable
            } else {
                EntryKind::File
            }
        } else {
            EntryKind::Other
        };
        let size = md.len();
        let mtime = md.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        Self {
            path,
            name,
            kind,
            size,
            mtime,
        }
    }

    #[allow(dead_code)]
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }

    /// Display name with a trailing `/` for directories or `*` for
    /// executables (both are classic spy / `ls -F` conventions).
    pub fn display_name(&self) -> String {
        match self.kind {
            EntryKind::Dir => format!("{}/", self.name),
            EntryKind::Executable => format!("{}*", self.name),
            _ => self.name.clone(),
        }
    }
}

#[cfg(unix)]
fn is_executable(md: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    md.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_md: &Metadata) -> bool {
    false
}
