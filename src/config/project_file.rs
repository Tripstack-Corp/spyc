//! A project's own config file under `$XDG_CONFIG_HOME/spyc/projects/` (#99,
//! `docs/archive/PROJECTS_PLAN.md` §1f): which file configures a project,
//! matched by the `home` it names, never by its file name, which is the
//! project's label.
//!
//! A load lists the directory once ([`scan`]) and matches each open project
//! against the list ([`find`]). The first file by name that names a home
//! wins it. A load warns only about what concerns the project it is for: two
//! files naming its home, or a file named for it that names no usable home.

use std::path::{Path, PathBuf};

/// The file that configures a project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectFile {
    pub path: PathBuf,
    /// The file's name without `.toml`: the project's label.
    pub label: String,
}

/// What one file in the directory names.
#[derive(Debug)]
pub enum Named {
    /// A home that exists, canonical.
    Home(PathBuf),
    /// A home that no longer exists, as a removed worktree's: skipped quietly.
    Gone,
    /// No usable home, and why.
    Bad(String),
}

/// One file in the directory.
#[derive(Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub named: Named,
}

impl Entry {
    fn stem(&self) -> String {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn name(&self) -> String {
        format!("projects/{}.toml", self.stem())
    }
}

/// The project files' directory. Under test it is found only through
/// `state::with_config_root`, so no test reads the developer's own files.
pub fn dir() -> Option<PathBuf> {
    #[cfg(test)]
    let root = crate::state::config_root_override();
    #[cfg(not(test))]
    let root = crate::state::config_root();
    root.map(|r| r.join("projects"))
}

/// Every `*.toml` directly in [`dir`], sorted by name, with what each names.
/// Empty when the directory doesn't exist.
pub fn scan() -> Vec<Entry> {
    dir().map(|d| scan_dir(&d)).unwrap_or_default()
}

/// [`scan`] of `dir`.
pub fn scan_dir(dir: &Path) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml") && p.is_file())
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let named = named_in(&path);
            Entry { path, named }
        })
        .collect()
}

/// Just the `home` key: the rest of the file is read by the load proper.
#[derive(serde::Deserialize)]
struct HomeOnly {
    home: Option<String>,
}

fn named_in(path: &Path) -> Named {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => return Named::Bad(format!("reading it: {e}")),
    };
    let home = match toml::from_str::<HomeOnly>(&text) {
        Ok(HomeOnly { home: Some(home) }) => home,
        Ok(HomeOnly { home: None }) => return Named::Bad("it names no `home`".into()),
        Err(e) => return Named::Bad(format!("parsing it: {e}")),
    };
    let expanded = match home.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            let Some(user) = super::home_dir() else {
                return Named::Bad(format!("its home {home} needs $HOME"));
            };
            user.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(&home),
    };
    if !expanded.is_absolute() {
        return Named::Bad(format!("its home {home} is not an absolute path"));
    }
    std::fs::canonicalize(&expanded).map_or(Named::Gone, Named::Home)
}

/// The file in `entries` that names `anchor`, and what this project's load
/// should warn about. Of several naming it, the first by name applies.
pub fn find(entries: &[Entry], anchor: &Path) -> (Option<ProjectFile>, Vec<String>) {
    // The name the project goes by without a file: its anchor's own, not a
    // symlink target's.
    let own_name = anchor.file_name().map(|s| s.to_string_lossy().into_owned());
    let anchor = std::fs::canonicalize(anchor).unwrap_or_else(|_| anchor.to_path_buf());
    let mut warnings = Vec::new();
    let naming: Vec<&Entry> = entries
        .iter()
        .filter(|e| matches!(&e.named, Named::Home(h) if *h == anchor))
        .collect();
    if let [first, _, ..] = naming.as_slice() {
        let names: Vec<String> = naming.iter().map(|e| e.name()).collect();
        warnings.push(format!(
            "{} all name {}; using {}",
            names.join(", "),
            crate::paths::display_tilde(&anchor),
            first.name(),
        ));
    }
    for entry in entries {
        if let Named::Bad(why) = &entry.named
            && own_name.as_deref() == Some(entry.stem().as_str())
        {
            warnings.push(format!("{}: {why}; skipped", entry.name()));
        }
    }
    let file = naming.first().map(|e| ProjectFile {
        path: e.path.clone(),
        label: e.stem(),
    });
    (file, warnings)
}

/// The user's file for the project anchored at `anchor`, in `dir`, made if
/// need be (`:project config`): `<label>.toml`, or `<label>-2.toml` and on
/// while the name is another project's. A file of that name that names this
/// project, or no usable home, is returned as it is, to be edited; one is
/// never written over. A new one holds the project's `home`, `~` for the
/// user's home, and must read back as the project's, or it is removed and the
/// call fails: a file the project never goes by would be made again on every
/// call. A label that isn't a plain file name names the file `project`.
pub fn create(dir: &Path, anchor: &Path, label: &str) -> anyhow::Result<PathBuf> {
    use anyhow::Context as _;
    use std::io::Write as _;
    let canonical = std::fs::canonicalize(anchor)
        .with_context(|| format!("the project's home {} has gone", anchor.display()))?;
    let stem = if label.is_empty() || label.contains('/') || label.starts_with('.') {
        "project"
    } else {
        label
    };
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for n in 1.. {
        let name = if n == 1 {
            format!("{stem}.toml")
        } else {
            format!("{stem}-{n}.toml")
        };
        let path = dir.join(name);
        if path.symlink_metadata().is_ok() {
            if !path.is_file() {
                continue;
            }
            match named_in(&path) {
                Named::Home(home) if home != canonical => continue,
                Named::Gone => continue,
                Named::Home(_) | Named::Bad(_) => return Ok(path),
            }
        }
        let home = toml::Value::String(tilde(&canonical));
        let text = format!(
            "# spyc's settings for this project, over its own .spycrc.toml and yours:\n\
             # .spycrc.toml's keys (`spyc --print-config` lists them).\n\
             home = {home}\n"
        );
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| format!("creating {}", path.display()))?;
        file.write_all(text.as_bytes())
            .with_context(|| format!("writing {}", path.display()))?;
        if !matches!(named_in(&path), Named::Home(home) if home == canonical) {
            let _ = std::fs::remove_file(&path);
            anyhow::bail!(
                "{} can't name {} as its home",
                path.display(),
                canonical.display()
            );
        }
        return Ok(path);
    }
    unreachable!("a free name is found: each taken one moves to the next")
}

/// `path` as a `home` names it: `~` for the user's home, as [`find`] reads it
/// back, else as it is.
pub(super) fn tilde(path: &Path) -> String {
    match super::home_dir().and_then(|home| path.strip_prefix(&home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _tmp: tempfile::TempDir,
        dir: PathBuf,
        home: PathBuf,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let (dir, home) = (base.join("projects"), base.join("src").join("app"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        Fixture {
            _tmp: tmp,
            dir,
            home,
        }
    }

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::write(dir.join(name), text).unwrap();
    }

    /// The first file by name that names a home is that project's, and the
    /// warning names every file that does.
    #[test]
    fn the_first_file_by_name_wins_a_home() {
        let f = fixture();
        let home = format!("home = {:?}\n", f.home.display().to_string());
        write(&f.dir, "zed.toml", &home);
        write(&f.dir, "alpha.toml", &home);
        write(&f.dir, "notes.txt", &home);
        let (file, warnings) = find(&scan_dir(&f.dir), &f.home);
        assert_eq!(file.map(|f| f.label).as_deref(), Some("alpha"));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("projects/alpha.toml, projects/zed.toml all name "),
            "{warnings:?}"
        );
    }

    /// `~` is the user's home.
    #[test]
    fn a_tilde_home_is_the_users() {
        let f = fixture();
        let user = f.home.parent().unwrap().parent().unwrap().to_path_buf();
        write(&f.dir, "app.toml", "home = \"~/src/app\"\n");
        let (file, _) = super::super::with_home_root(&user, || find(&scan_dir(&f.dir), &f.home));
        assert_eq!(file.map(|f| f.label).as_deref(), Some("app"));
    }

    /// A file that names no usable home is skipped. It is warned about only in
    /// the project it is named for, and a home that has gone is skipped
    /// quietly.
    #[test]
    fn an_unusable_file_warns_only_the_project_it_is_named_for() {
        let f = fixture();
        write(&f.dir, "app.toml", "home = [\n");
        write(&f.dir, "other.toml", "[layout]\n");
        write(&f.dir, "rel.toml", "home = \"src/app\"\n");
        write(&f.dir, "gone.toml", "home = \"/no/such/dir/anywhere\"\n");
        let entries = scan_dir(&f.dir);
        let (file, warnings) = find(&entries, &f.home);
        assert_eq!(file, None);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("projects/app.toml: parsing it:"),
            "{warnings:?}"
        );
        assert!(matches!(entries[2].named, Named::Bad(ref w) if w.contains("no `home`")));
        assert!(matches!(entries[3].named, Named::Bad(ref w) if w.contains("not an absolute")));
        assert!(matches!(entries[1].named, Named::Gone));
    }

    /// The user's own file for a project wins over the repo's rc, which wins
    /// over `~/.spycrc.toml`: its colours where it sets them, and its startup
    /// tabs outright, with nothing held for consent.
    #[test]
    fn a_project_file_wins_over_the_repos_rc_and_the_users() {
        let f = fixture();
        let user = f.home.parent().unwrap().join("user");
        std::fs::create_dir_all(&user).unwrap();
        write(
            &user,
            ".spycrc.toml",
            "[colors]\ndir = \"#111111\"\nfile = \"#aaaaaa\"\n[pane]\ntabs = [\"a\"]\n",
        );
        write(
            &f.home,
            ".spycrc.toml",
            "[colors]\ndir = \"#222222\"\n[pane]\ntabs = [\"b\"]\n",
        );
        let home = format!("home = {:?}\n", f.home.display().to_string());
        write(
            &f.dir,
            "app.toml",
            &format!("{home}[colors]\ndir = \"#333333\"\n[pane]\ntabs = [\"c\"]\n"),
        );
        let files = scan_dir(&f.dir);
        let cfg = super::super::with_home_root(&user, || {
            super::super::Config::load_for_project(&f.home, &f.home, &files)
        })
        .expect("the three load");
        assert_eq!(cfg.colors.dir.as_deref(), Some("#333333"));
        assert_eq!(cfg.colors.file.as_deref(), Some("#aaaaaa"), "the user's");
        let tabs: Vec<&str> = cfg.pane.tabs.iter().map(|t| t.command.as_str()).collect();
        assert_eq!(tabs, ["c"]);
        assert!(
            cfg.pane.project_tabs.is_none(),
            "the repo's held for consent"
        );
        assert_eq!(cfg.project.map(|p| p.label).as_deref(), Some("app"));
        assert_eq!(cfg.sources.len(), 3);
    }

    /// A repo's rc can't name the project it configures: its `home` is a
    /// warning, and the rest of it loads.
    #[test]
    fn a_home_in_a_repos_rc_is_a_warning() {
        let f = fixture();
        let user = f.home.parent().unwrap().join("user");
        std::fs::create_dir_all(&user).unwrap();
        write(
            &f.home,
            ".spycrc.toml",
            "home = \"/x\"\n[colors]\ndir = \"#222222\"\n",
        );
        let cfg = super::super::with_home_root(&user, || {
            super::super::Config::load_for_project(&f.home, &f.home, &[])
        })
        .expect("the rc loads");
        assert_eq!(cfg.colors.dir.as_deref(), Some("#222222"));
        assert!(
            cfg.warnings.iter().any(|w| w.contains("`home` ignored")),
            "{:?}",
            cfg.warnings
        );
    }

    /// A repo rc this spyc can't read doesn't take the user's own file for the
    /// project with it: that rc is skipped, with a warning, and the file still
    /// applies and names the project.
    #[test]
    fn a_broken_repo_rc_keeps_the_users_file() {
        let f = fixture();
        let user = f.home.parent().unwrap().join("user");
        std::fs::create_dir_all(&user).unwrap();
        write(&f.home, ".spycrc.toml", "[layout\n");
        let home = format!("home = {:?}\n", f.home.display().to_string());
        write(
            &f.dir,
            "app.toml",
            &format!("{home}[colors]\ndir = \"#333333\"\n"),
        );
        let files = scan_dir(&f.dir);
        let cfg = super::super::with_home_root(&user, || {
            super::super::Config::load_for_project(&f.home, &f.home, &files)
        })
        .expect("the user's file still loads");
        assert_eq!(cfg.project.map(|p| p.label).as_deref(), Some("app"));
        assert_eq!(cfg.colors.dir.as_deref(), Some("#333333"));
        assert!(
            cfg.warnings.iter().any(|w| w.contains("parsing")),
            "{:?}",
            cfg.warnings
        );
        let alone = super::super::with_home_root(&user, || {
            super::super::Config::load_for_project(&f.home, &f.home, &[])
        });
        assert!(
            alone.is_err(),
            "with no file of the user's, the error stands"
        );
    }

    /// `create` makes the directory and a file named for the project, holding
    /// its home, which `find` then matches.
    #[test]
    fn create_makes_a_file_naming_the_projects_home() {
        let f = fixture();
        let dir = f.dir.join("made");
        let path = create(&dir, &f.home, "app").expect("made");
        assert_eq!(path, dir.join("app.toml"));
        let (file, warnings) = find(&scan_dir(&dir), &f.home);
        assert_eq!(file.map(|f| f.path), Some(path));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// A name another project's file has moves on to `-2`; one naming this
    /// project, or no usable home, is the one to edit, never written over.
    #[test]
    fn create_never_writes_over_a_file() {
        let f = fixture();
        let other = f.home.parent().unwrap().join("other");
        std::fs::create_dir_all(&other).unwrap();
        write(
            &f.dir,
            "app.toml",
            &format!("home = {:?}\n", other.display().to_string()),
        );
        let made = create(&f.dir, &f.home, "app").expect("made");
        assert_eq!(made, f.dir.join("app-2.toml"));
        assert_eq!(
            create(&f.dir, &f.home, "app").unwrap(),
            made,
            "its own file again"
        );
        write(&f.dir, "gone.toml", "home = \"/no/such/dir/anywhere\"\n");
        assert_eq!(
            create(&f.dir, &f.home, "gone").unwrap(),
            f.dir.join("gone-2.toml")
        );
        write(&f.dir, "broken.toml", "home = [\n");
        assert_eq!(
            create(&f.dir, &f.home, "broken").unwrap(),
            f.dir.join("broken.toml")
        );
        let text = std::fs::read_to_string(f.dir.join("broken.toml")).unwrap();
        assert_eq!(text, "home = [\n", "written over");
    }

    /// A home under the user's own is written with `~`, and reads back.
    #[test]
    fn create_writes_the_users_home_as_tilde() {
        let f = fixture();
        let user = f.home.parent().unwrap().to_path_buf();
        let path = super::super::with_home_root(&user, || create(&f.dir, &f.home, "app")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("home = \"~/app\""), "{text}");
        let (file, _) = super::super::with_home_root(&user, || find(&scan_dir(&f.dir), &f.home));
        assert_eq!(file.map(|f| f.path), Some(path));
    }

    /// A gone home makes no file: one the project could never go by would be
    /// made again on every call.
    #[test]
    fn create_makes_nothing_for_a_gone_home() {
        let f = fixture();
        let err = create(&f.dir, &f.home.join("gone"), "gone").unwrap_err();
        assert!(format!("{err:#}").contains("has gone"), "{err:#}");
        assert_eq!(std::fs::read_dir(&f.dir).unwrap().count(), 0);
    }

    /// A label that isn't a plain file name, as the root's `/` is, names the
    /// file `project`, inside the directory; a directory where a file would be
    /// is a name taken.
    #[test]
    fn create_keeps_to_the_directory_and_to_files() {
        let f = fixture();
        assert_eq!(
            create(&f.dir, &f.home, "/").unwrap(),
            f.dir.join("project.toml")
        );
        std::fs::create_dir(f.dir.join("app.toml")).unwrap();
        assert_eq!(
            create(&f.dir, &f.home, "app").unwrap(),
            f.dir.join("app-2.toml")
        );
    }

    /// No directory is no file, and nothing to say.
    #[test]
    fn a_missing_directory_is_quiet() {
        let f = fixture();
        let (file, warnings) = find(&scan_dir(&f.dir.join("nope")), &f.home);
        assert_eq!((file, warnings.len()), (None, 0));
    }
}
