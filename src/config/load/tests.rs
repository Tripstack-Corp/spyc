//! How a config file with a problem loads. A key this spyc doesn't know
//! (#821) is skipped and named in a warning, and the rest of its file loads.
//! A repo rc that can't load at all (#827) is skipped whole, with a warning,
//! and the user's own `~/.spycrc.toml` still applies.

use std::path::{Path, PathBuf};

use super::super::{Config, DEFAULT_TEMPLATE, with_home_root};

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

/// Whether a warning names `file` and the key path `key`.
fn warns(cfg: &Config, file: &str, key: &str) -> bool {
    cfg.warnings
        .iter()
        .any(|w| w.contains(file) && w.contains(&format!("`{key}`")))
}

#[test]
fn an_unknown_key_is_skipped_and_named_and_the_rest_of_the_file_applies() {
    let tmp = tempfile::tempdir().unwrap();
    let rc = write(
        tmp.path(),
        "rc.toml",
        "[pager]\ntab_width = 2\nfrom_a_newer_spyc = true\n\n[a_newer_table]\nkey = 1\n",
    );
    let cfg = Config::load_from_allowing_unknown(&[Some(&rc)])
        .expect("an unknown key doesn't fail the file");
    assert_eq!(cfg.pager.tab_width, 2, "the rest of the file applies");
    for key in ["pager.from_a_newer_spyc", "a_newer_table"] {
        assert!(warns(&cfg, "rc.toml", key), "{key}: {:?}", cfg.warnings);
    }
    assert_eq!(cfg.warnings.len(), 1, "one per file: {:?}", cfg.warnings);
}

/// The repo's `.spycrc.toml` and the user's `~/.spycrc.toml` are read together,
/// so a key the repo's file has that this spyc doesn't know must not take the
/// user's own settings with it.
#[test]
fn an_unknown_key_in_a_repo_rc_keeps_the_users_own_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, repo) = (tmp.path().join("home"), tmp.path().join("a-repo"));
    write(
        &home,
        ".spycrc.toml",
        "[pager]\ntab_width = 8\nnewer_still = 1\n",
    );
    write(
        &repo,
        ".spycrc.toml",
        "[mouse]\nscroll_lines = 7\nfrom_a_newer_spyc = true\n",
    );
    let cfg = with_home_root(&home, || Config::load_for_project(&repo, &repo, &[]))
        .expect("the repo rc's unknown key doesn't fail the load");
    assert_eq!(cfg.pager.tab_width, 8, "the user's own setting survives");
    assert_eq!(
        cfg.mouse.scroll_lines, 7,
        "the repo rc's other settings apply"
    );
    assert!(
        warns(&cfg, "a-repo/.spycrc.toml", "mouse.from_a_newer_spyc"),
        "{:?}",
        cfg.warnings
    );
    assert!(
        warns(&cfg, "~/.spycrc.toml", "pager.newer_still"),
        "the user's rc is named from home: {:?}",
        cfg.warnings
    );
}

#[test]
fn an_unknown_key_in_a_nested_table_is_named_by_its_full_path() {
    let tmp = tempfile::tempdir().unwrap();
    let rc = write(
        tmp.path(),
        "rc.toml",
        "[[pane.tab]]\ncommand = \"claude\"\nnot_yet = 1\n\n\
         [[scan.patterns]]\nname = \"a\"\nregex = \"a+\"\n\n\
         [[scan.patterns]]\nname = \"b\"\nregex = \"b+\"\nnewer = 2\n\n\
         [colors]\ndir = \"#336699\"\nnot_a_colour_yet = \"red\"\n",
    );
    let cfg = Config::load_from_allowing_unknown(&[Some(&rc)])
        .expect("an unknown key doesn't fail the file");
    assert_eq!(cfg.colors.dir.as_deref(), Some("#336699"));
    assert_eq!(cfg.pane.tabs.len(), 1, "the tab with an unknown key loads");
    assert_eq!(cfg.pane.tabs[0].command, "claude");
    assert_eq!(cfg.scan_patterns.len(), 2, "both patterns load");
    for key in [
        "pane.tab[0].not_yet",
        "scan.patterns[1].newer",
        "colors.not_a_colour_yet",
    ] {
        assert!(warns(&cfg, "rc.toml", key), "{key}: {:?}", cfg.warnings);
    }
}

/// A file with many unknown keys, a repo rc's thousands among them, names the
/// first few and counts the rest, so the flash stays readable.
#[test]
fn many_unknown_keys_name_the_first_few_and_count_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let rc = write(
        tmp.path(),
        "rc.toml",
        "[pager]\na = 1\nb = 2\nc = 3\nd = 4\ne = 5\n",
    );
    let cfg =
        Config::load_from_allowing_unknown(&[Some(&rc)]).expect("unknown keys don't fail the file");
    assert_eq!(cfg.warnings.len(), 1, "{:?}", cfg.warnings);
    let warning = &cfg.warnings[0];
    for key in ["`pager.a`", "`pager.b`", "`pager.c`", "and 2 more"] {
        assert!(warning.contains(key), "{key}: {warning}");
    }
    assert!(!warning.contains("`pager.d`"), "{warning}");
}

/// Only an unknown key is skipped: a file that isn't TOML, or a known key with
/// a value of the wrong type, still fails, as a typo in a value always has.
#[test]
fn a_syntax_error_or_a_wrong_type_still_fails_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    for body in ["[pager\n", "[pager]\ntab_width = \"four\"\n"] {
        let rc = write(tmp.path(), "rc.toml", body);
        assert!(Config::load_from(&[Some(&rc)]).is_err(), "{body:?}");
    }
}

/// `spyc --print-config`'s template uses only keys this spyc knows. With an
/// unknown key a warning rather than an error, this is what catches a
/// misspelt table header in it.
#[test]
fn the_default_template_names_no_unknown_key() {
    assert!(
        DEFAULT_TEMPLATE.contains("\n[pager]\n"),
        "the template has tables"
    );
    let tmp = tempfile::tempdir().unwrap();
    let rc = write(tmp.path(), "template.toml", DEFAULT_TEMPLATE);
    let cfg = Config::load_from(&[Some(&rc)]).expect("the template loads");
    assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
}

/// A repo rc that can't load, for any reason, is skipped whole: the user's own
/// `~/.spycrc.toml` applies, nothing of the repo rc does, not even the
/// `[colors]` the merge takes in before it reaches a bad `keymap` line, and a
/// warning names it.
#[test]
fn a_repo_rc_that_cannot_load_is_skipped_whole_and_the_users_own_settings_apply() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, ".spycrc.toml", "[pager]\ntab_width = 8\n");
    for (why, broken) in [
        ("not-toml", "[layout\n"),
        ("wrong-type", "[mouse]\nscroll_lines = \"many\"\n"),
        ("missing-required-key", "[[scan.patterns]]\nname = \"a\"\n"),
        ("bad-keymap-line", "keymap = [\"map z no_such_action\"]\n"),
    ] {
        let repo = tmp.path().join(why);
        write(
            &repo,
            ".spycrc.toml",
            &format!("{broken}\n[colors]\ndir = \"#123456\"\n"),
        );
        let cfg = with_home_root(&home, || Config::load_for_project(&repo, &repo, &[]))
            .unwrap_or_else(|e| panic!("{why}: the repo rc fails the load: {e:#}"));
        assert_eq!(cfg.pager.tab_width, 8, "{why}: the user's own setting");
        assert_eq!(cfg.colors.dir, None, "{why}: nothing of the repo rc");
        assert!(
            cfg.warnings
                .iter()
                .any(|w| w.contains(&format!("{why}/.spycrc.toml")) && w.contains("skipped")),
            "{why}: {:?}",
            cfg.warnings
        );
    }
}

/// The user's own `~/.spycrc.toml` that can't load is still the error it was:
/// skipping it would quietly lose everything they set.
#[test]
fn a_users_rc_that_cannot_load_still_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, repo) = (tmp.path().join("home"), tmp.path().join("repo"));
    write(&home, ".spycrc.toml", "[layout\n");
    write(&repo, ".spycrc.toml", "[pager]\ntab_width = 3\n");
    assert!(with_home_root(&home, || Config::load_for_project(&repo, &repo, &[])).is_err());
}
