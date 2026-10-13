//! Read-only B2 acceptance driver. It observes one explicitly selected rollout;
//! it never launches/resumes Codex or writes its home. Ctrl+C ends the consumer.

use std::path::PathBuf;
use std::sync::mpsc;

use anyhow::{Context, bail};
use spyc::codex_metrics::{Binding, watch::Subscription};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 3 || uuid::Uuid::parse_str(&args[1]).is_err() {
        bail!("usage: codex_metrics CODEX_HOME THREAD_ID EXACT_ROLLOUT");
    }
    let binding = Binding {
        pane_id: "local-acceptance-consumer".to_owned(),
        thread_id: args[1].clone(),
        codex_home: PathBuf::from(&args[0]),
        generation: 1,
    };
    let (tx, rx) = mpsc::channel();
    let subscription = Subscription::start(binding.clone(), PathBuf::from(&args[2]), move || {
        let _ = tx.send(());
    });
    println!("Read-only metrics observer; Ctrl+C to stop. Context/limits remain unknown.");
    while rx.recv().is_ok() {
        let Some(update) = subscription.take() else {
            continue;
        };
        let snapshot = update
            .for_binding(Some(&binding))
            .context("obsolete metrics binding")?;
        let total = |sample: &Option<spyc::codex_metrics::Sample<spyc::codex_metrics::Tokens>>| {
            sample
                .as_ref()
                .and_then(|s| s.value.total)
                .map_or("--".to_owned(), |n| n.to_string())
        };
        println!(
            "source={:?} tail={} request={} turn={} thread={} legacy_thread={} context=-- limits={:?} recent_tools={} exhaustive={} work={:?}",
            update.availability,
            update.tail_only,
            total(&snapshot.request),
            total(&snapshot.turn),
            total(&snapshot.thread),
            total(&snapshot.legacy_thread),
            snapshot.limits.as_ref().map(|sample| sample.value),
            snapshot.tools.len(),
            snapshot.tools_complete,
            subscription.work()
        );
    }
    Ok(())
}
