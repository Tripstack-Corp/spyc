//! The `spyc-jar` binary: the process that holds one pane's program and its
//! PTY and outlives spyc. It lives in spyc's package so every install of spyc
//! carries it; the `spyc-jar` crate is all of it.

fn main() -> anyhow::Result<()> {
    spyc_jar::cli::main()
}
