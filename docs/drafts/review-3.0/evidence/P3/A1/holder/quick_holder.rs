// P3 helper: an upgrade binary whose hold-handover exits at once, as the probe's
// shell wrapper's "exec 0<&-; exit 0" does, but as a native (aarch64, static)
// program, so the emulated x86_64 jar meets a holder that is gone as fast as
// one on a native host. Anything else is passed to the jar under test at
// /tmp/p3real (a link set per round).
use std::os::unix::process::CommandExt;
fn main() {
    let a: Vec<_> = std::env::args_os().collect();
    if a.get(1).is_some_and(|s| s == "hold-handover") {
        return;
    }
    let e = std::process::Command::new("/tmp/p3real").arg0(&a[0]).args(&a[1..]).exec();
    eprintln!("quick_holder: exec /tmp/p3real: {e}");
    std::process::exit(127);
}
