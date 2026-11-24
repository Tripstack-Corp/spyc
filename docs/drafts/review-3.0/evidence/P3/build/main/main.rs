// P3 probe wrapper: the same one-line main as spyc's src/bin/spyc-jar.rs at 52ce15b7.
fn main() -> anyhow::Result<()> {
    spyc_jar::cli::main_built("52ce15b7", "p3-cross-build")
}
