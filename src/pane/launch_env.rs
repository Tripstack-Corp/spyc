//! Bounded home metadata captured after the pane shell has loaded its rc.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Read;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const MAX_RECORD_BYTES: u64 = 64 * 1024;

/// Only the home variables, never the full environment or command arguments.
#[derive(Clone, Debug, Default)]
pub struct LaunchEnvironment {
    pub home: Option<OsString>,
    pub codex_home: Option<OsString>,
}

impl LaunchEnvironment {
    /// Incomplete, missing or oversized records stay unbound; no process-env fallback.
    pub fn read(path: &Path) -> Option<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(MAX_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return None;
        }
        let fields: Vec<_> = bytes.split(|&byte| byte == 0).collect();
        let [home, codex_home, end] = fields.as_slice() else {
            return None;
        };
        if !end.is_empty() {
            return None;
        }
        Some(Self {
            home: (!home.is_empty()).then(|| OsString::from_vec(home.to_vec())),
            codex_home: Some(OsString::from_vec(codex_home.to_vec())),
        })
    }
}

/// Directory ownership prevents a late shell from recreating a closed pane's record.
pub struct RecordFile {
    file: tempfile::NamedTempFile,
    _dir: tempfile::TempDir,
}

impl RecordFile {
    pub fn path(&self) -> &Path {
        self.file.path()
    }
}

/// Prefix the shell's command after rc loading while retaining its original exec.
/// Explicit per-pane home values outrank rc values, preserving restore/fork identity.
/// The private file stays owned by the host and disappears when its pane closes.
pub fn prepare(args: &mut [String], env: &[(&str, &str)]) -> anyhow::Result<RecordFile> {
    let dir = tempfile::Builder::new()
        .prefix("spyc-launch-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let record = tempfile::NamedTempFile::new_in(dir.path())?;
    let path = record
        .path()
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("launch metadata path is not UTF-8"))?;
    let mut prefix = String::new();
    for &(key, value) in env {
        if matches!(key, "HOME" | "CODEX_HOME") {
            let _ = write!(
                prefix,
                "export {key}={}; ",
                crate::shell::shell_quote(value)
            );
        }
    }
    let _ = write!(
        prefix,
        "command printf '%s\\0%s\\0' \"${{HOME-}}\" \"${{CODEX_HOME-}}\" > {}; ",
        crate::shell::shell_quote(path)
    );
    let command = args
        .last_mut()
        .ok_or_else(|| anyhow::anyhow!("launch metadata needs a shell command"))?;
    *command = format!("{prefix}{command}");
    Ok(RecordFile {
        file: record,
        _dir: dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_and_oversized_launch_records_never_supply_a_home() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        for body in [
            b"/home\0unfinished".to_vec(),
            b"/home\0/custom\0extra".to_vec(),
            {
                let mut bytes = b"/home\0".to_vec();
                bytes.extend(vec![b'x'; MAX_RECORD_BYTES as usize]);
                bytes.push(0);
                bytes
            },
        ] {
            std::fs::write(tmp.path(), body).unwrap();
            assert!(LaunchEnvironment::read(tmp.path()).is_none());
        }
        std::fs::write(tmp.path(), b"/home\0/custom\0").unwrap();
        let env = LaunchEnvironment::read(tmp.path()).unwrap();
        assert_eq!(env.home.as_deref(), Some(std::ffi::OsStr::new("/home")));
        assert_eq!(
            env.codex_home.as_deref(),
            Some(std::ffi::OsStr::new("/custom"))
        );
    }
    #[test]
    fn closed_launch_records_cannot_be_recreated_by_a_late_shell() {
        let mut args = vec!["-c".to_string(), "true".to_string()];
        let record = prepare(&mut args, &[]).unwrap();
        let path = record.path().to_path_buf();
        let parent = path.parent().unwrap().to_path_buf();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
            0o700
        );
        drop(record);
        assert!(!parent.exists());
        let _ = std::process::Command::new("/bin/sh")
            .args(args)
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(
            !path.exists(),
            "the late shell cannot leave an orphaned record"
        );
    }
}
