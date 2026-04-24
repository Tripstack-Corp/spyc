//! Build script: embed git SHA and build timestamp into the binary.

use std::process::Command;

fn main() {
    let sha = run_trimmed(Command::new("git").args(["rev-parse", "--short", "HEAD"]));
    println!("cargo:rustc-env=SPYC_GIT_SHA={sha}");

    let ts = run_trimmed(Command::new("date").args(["-u", "+%Y-%m-%d %H:%M:%S UTC"]));
    println!("cargo:rustc-env=SPYC_BUILD_TIME={ts}");

    let rustc = run_trimmed(Command::new("rustc").arg("--version"));
    println!("cargo:rustc-env=SPYC_RUSTC_VERSION={rustc}");

    // Re-run if git HEAD changes (new commits).
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/");
}

fn run_trimmed(cmd: &mut Command) -> String {
    cmd.output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "unknown".to_string(), |s| s.trim().to_string())
}
