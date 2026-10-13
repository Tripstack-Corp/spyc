//! Read-only acceptance driver for following one Claude Code transcript. It
//! observes one explicitly selected session file; it never launches Claude Code
//! or searches the Claude home. Ctrl+C ends the consumer.

use std::path::PathBuf;
use std::sync::mpsc;

use anyhow::{Context, bail};
use spyc::claude_metrics::{Binding, Sample, watch::Subscription};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 3 || uuid::Uuid::parse_str(&args[1]).is_err() {
        bail!("usage: claude_metrics CLAUDE_HOME SESSION_ID EXACT_TRANSCRIPT");
    }
    let binding = Binding {
        pane_id: "local-acceptance-consumer".to_owned(),
        session_id: args[1].clone(),
        claude_home: PathBuf::from(&args[0]),
        generation: 1,
    };
    let (tx, rx) = mpsc::channel();
    let subscription = Subscription::start(binding.clone(), PathBuf::from(&args[2]), move || {
        let _ = tx.send(());
    });
    println!("Read-only metrics observer; Ctrl+C to stop. Context % remains unknown.");
    while rx.recv().is_ok() {
        let Some(update) = subscription.take() else {
            continue;
        };
        let snapshot = update
            .for_binding(Some(&binding))
            .context("obsolete metrics binding")?;
        let value = |sample: Option<&Sample<u64>>| {
            sample.map_or_else(|| "--".to_owned(), |s| s.value.to_string())
        };
        let mode = snapshot.mode.as_ref().map(|sample| &sample.value);
        let label = |field: Option<&Option<String>>| {
            field.and_then(Option::as_deref).unwrap_or("--").to_owned()
        };
        println!(
            "source={:?} tail={} model={} effort={} permission={} messages={} totals={:?} \
             context_tokens={} compactions={} plan={:?} recent_tools={} exhaustive={} \
             subagents={} background={} work={:?}",
            update.availability,
            update.tail_only,
            label(mode.map(|m| &m.model)),
            label(mode.map(|m| &m.effort)),
            label(mode.map(|m| &m.permission_mode)),
            snapshot.messages,
            snapshot.totals,
            value(snapshot.context_tokens.as_ref()),
            snapshot.compactions,
            snapshot.plan.as_ref().map(|p| p.value),
            snapshot.tools.len(),
            snapshot.tools_complete,
            snapshot.subagent_calls,
            snapshot.background_calls,
            subscription.work(),
        );
    }
    Ok(())
}
