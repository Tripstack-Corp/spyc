// P3 probe wrapper: the same one-line main as spyc's src/bin/spyc-jar.rs at 444857a2.
fn main() -> anyhow::Result<()> {
    spyc_jar::cli::main_built("444857a2", "p3-cross-build")
}
