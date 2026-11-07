//! `git_diff` with a `base`: what a branch changed since it left that base,
//! through the MCP call an agent makes, over the same fixtures as `gm`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::super::protocol::dispatch;
use super::parse_response;
use crate::context;
use crate::git::test_support::{branch_repo, stale_main_repo};

/// A context file whose focused column is an empty directory beside `repo`,
/// so only an explicit `root` reaches the repo.
fn context_beside(tmp: &Path) -> PathBuf {
    let focused = tmp.join("focused");
    std::fs::create_dir_all(&focused).unwrap();
    let ctx = context::SpycContext {
        root: tmp.to_path_buf(),
        cwd: focused.clone(),
        cursor_file: None,
        picks: vec![],
        inventory: vec![],
        filter: None,
        git_branch: None,
        project_home: Some(focused.clone()),
        search_root: Some(focused),
        session_name: String::new(),
        pid: 0,
        version: String::new(),
        archive_mounts: Vec::new(),
    };
    let ctx_path = context::context_path(tmp);
    context::write_context_file(&ctx_path, &ctx).unwrap();
    ctx_path
}

/// `git_diff` on `repo` with `args` beside its `root`: the reply's text, and
/// whether it is a tool error.
fn git_diff(ctx_path: &Path, repo: &Path, mut args: Value) -> (String, bool) {
    args["root"] = json!(repo.display().to_string());
    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "git_diff", "arguments": args}})
        .to_string(),
        ctx_path,
        None,
    )
    .unwrap();
    let resp = parse_response(&output);
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("a text reply: {resp}"))
        .to_string();
    (text, resp["result"]["isError"] == json!(true))
}

/// The files a diff's `diff --git` headers name, of the fixture's
/// `alpha`–`echo`.
fn files_in(diff: &str) -> Vec<&'static str> {
    ["alpha", "bravo", "charlie", "delta", "echo"]
        .into_iter()
        .filter(|name| diff.contains(&format!("diff --git a/proj/{name}.txt")))
        .collect()
}

/// The headline: since it left `main`, the branch changed `alpha` and
/// `bravo` in a commit, `charlie` in the working tree, and added `delta`
/// untracked; `echo`, which `main` changed after the fork, isn't the branch's.
#[test]
fn git_diff_with_a_base_shows_the_branch_since_it_left_it() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = branch_repo(tmp.path());
    let ctx_path = context_beside(tmp.path());

    let (diff, error) = git_diff(&ctx_path, &repo, json!({"base": "main"}));

    assert!(!error, "{diff}");
    assert_eq!(
        files_in(&diff),
        ["alpha", "bravo", "charlie", "delta"],
        "{diff}"
    );
    assert!(
        diff.contains("+from the branch"),
        "against the base: {diff}"
    );
}

/// A file a branch commit changed and the working tree has since put back as
/// it was at the fork is no change since the fork, so no `diff --git` names it.
#[test]
#[ignore = "red: a file put back as it was at the fork gets a header with no hunks"]
fn git_diff_with_a_base_leaves_out_a_file_put_back_as_it_was_at_the_fork() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = branch_repo(tmp.path());
    std::fs::write(repo.join("proj/alpha.txt"), "alpha\n").unwrap();
    let ctx_path = context_beside(tmp.path());

    let (diff, error) = git_diff(&ctx_path, &repo, json!({"base": "main"}));

    assert!(!error, "{diff}");
    assert_eq!(files_in(&diff), ["bravo", "charlie", "delta"], "{diff}");
}

/// Local `main` lags `origin/main`, which the branch was rebased onto: the
/// change `main` never pulled is other people's work, so `base: "main"`
/// leaves it out.
#[test]
fn git_diff_with_a_stale_local_main_shows_only_the_branchs_own_files() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = stale_main_repo(tmp.path());
    let ctx_path = context_beside(tmp.path());

    let (diff, error) = git_diff(&ctx_path, &repo, json!({"base": "main"}));

    assert!(!error, "{diff}");
    assert_eq!(files_in(&diff), ["alpha"], "{diff}");
}

/// `base` picks the old side, and so do `cached` and `unstaged`: asked for
/// both, the call is an error that says why, never one of the two silently.
#[test]
fn git_diff_refuses_a_base_with_cached_or_unstaged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = branch_repo(tmp.path());
    let ctx_path = context_beside(tmp.path());

    for scope in ["cached", "unstaged"] {
        let (text, error) = git_diff(&ctx_path, &repo, json!({"base": "main", scope: true}));

        assert!(error, "{scope}: {text}");
        assert!(text.contains("base"), "{scope}: names `base`: {text}");
        assert!(text.contains(scope), "{scope}: names it: {text}");
    }
}

/// `paths` narrows a diff since a base as it narrows the others.
#[test]
fn git_diff_with_a_base_still_takes_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = branch_repo(tmp.path());
    let ctx_path = context_beside(tmp.path());

    let args = json!({"base": "main", "paths": ["proj/alpha.txt", "proj/delta.txt"]});
    let (diff, error) = git_diff(&ctx_path, &repo, args);

    assert!(!error, "{diff}");
    assert_eq!(files_in(&diff), ["alpha", "delta"], "{diff}");
}

/// A base that names nothing is an error that names it.
#[test]
fn git_diff_with_an_unknown_base_names_it() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = branch_repo(tmp.path());
    let ctx_path = context_beside(tmp.path());

    let (text, error) = git_diff(&ctx_path, &repo, json!({"base": "nosuch"}));

    assert!(error, "{text}");
    assert!(text.contains("nosuch"), "{text}");
}
