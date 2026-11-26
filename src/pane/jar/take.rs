//! Taking a running jar back, as a restored or lapsed tab's, and ending the
//! program a jar holds before its tab starts again (review 3.0: C2, P1).
//!
//! A tab this spyc owned is taken back for as long as its jar runs. A jar
//! that's silent is asked again, try after try, never given up on after one
//! welcome's wait; one whose lease a connection of this spyc's still holds
//! (the tab's own, closed a moment before) is asked again once the jar says
//! that connection has gone. Only a jar another spyc owns, one this spyc has
//! no token for, or one that refuses it leaves the tab detached, its screen
//! saying which.
//!
//! A restart of a tab whose jar runs ends the program through the jar, and
//! sees the jar go, before anything starts. A jar another spyc owns is never
//! ended from here.

use std::path::Path;
use std::time::{Duration, Instant};

use portable_pty::PtySize;
use spyc_jar::client::{Client, Refusal};
use spyc_jar::proto::{JarSignal, Msg, Role, reason};

use super::link::{self, Watch};
use super::place::{SocketState, socket_state};
use super::{
    ADOPT_WAIT, Adoption, END_HELD_WAIT, ENDING_POLL, Jarred, Place, ReplayGate, Resume, Spec,
    choose, connect, held, offsets, snapshots, tokens,
};

/// How often a wait for a jar to let a connection of this spyc's go asks it.
const LET_GO_POLL: Duration = Duration::from_millis(100);

/// How long a restored tab waits for its jar to let go of a connection of
/// this spyc's, as one an `:upgrade`'s `exec` closed: past the jar's
/// deadline for an owner it hears nothing from.
const LET_GO_RESTORING: Duration =
    spyc_jar::limits::OWNER_DEADLINE.saturating_add(Duration::from_secs(1));

/// A jar another spyc owns: its program is that spyc's, and is never ended
/// or started a second time from here.
#[derive(Debug)]
pub struct OwnedElsewhere(pub Option<u32>);

impl std::fmt::Display for OwnedElsewhere {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "another spyc{} owns this tab's jar; its program runs on there, and nothing was \
             started here",
            by_pid(self.0)
        )
    }
}

impl std::error::Error for OwnedElsewhere {}

fn by_pid(pid: Option<u32>) -> String {
    pid.map_or_else(String::new, |p| format!(" (pid {p})"))
}

/// Who owns a jar, as the jar says.
#[derive(Debug, PartialEq, Eq)]
enum Holder {
    /// A connection of this process: a tab's own, on its way out.
    This,
    /// Another spyc, by its pid when the jar knows it.
    Other(Option<u32>),
    Nobody,
}

/// Who owns the jar at `socket`; `None` when it doesn't answer.
fn holder(socket: &Path) -> Option<Holder> {
    let fields = spyc_jar::client::describe(socket).ok()?;
    let field = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    if !field("owner").is_some_and(|o| o.starts_with("connected")) {
        return Some(Holder::Nobody);
    }
    let pid = field("owner_pid").and_then(|p| p.parse::<u32>().ok());
    Some(if pid == Some(std::process::id()) {
        Holder::This
    } else {
        Holder::Other(pid)
    })
}

/// How a wait for a jar to let go of this spyc's connection ended.
#[derive(Debug, PartialEq, Eq)]
enum LetGo {
    /// The jar said its owner is another, or none: ask it again.
    Answered,
    /// The jar didn't answer, ended, or the tab no longer wants it: ask
    /// again, which finds out which.
    Unanswered,
    /// The jar still said this spyc owns it past its own deadline for an
    /// owner it hears nothing from: a live connection of this process
    /// holds it, never the closed one a retake or a restore waits on.
    StillThis,
}

/// Wait for the jar at `socket` to let go of a connection of this spyc's,
/// `since` the jar first said this spyc held it. Only a restore (`verdict`)
/// concludes anything: from the jar's own answer, never from silence.
fn wait_let_go(socket: &Path, watch: Option<&Watch>, since: Instant, verdict: bool) -> LetGo {
    loop {
        let answer = holder(socket);
        if let Some(found) = let_go_verdict(answer.as_ref(), since.elapsed(), verdict) {
            return found;
        }
        let wanted = watch.is_none_or(Watch::wanted);
        if !wanted || socket_state(socket) != SocketState::Live {
            return LetGo::Unanswered;
        }
        std::thread::sleep(LET_GO_POLL);
    }
}

/// What a jar said when asked who owns it, for [`this_since_after`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Heard {
    This,
    Other,
    Nothing,
}

/// When the jar's present run of "this spyc" answers began, once it has said
/// `heard` at `now`. The run lasts only while each answer says so: any other
/// answer, or none (a jar stopped, starved or parked), ends it, so a silence
/// never counts toward the jar's deadline.
fn this_since_after(since: Option<Instant>, heard: Heard, now: Instant) -> Option<Instant> {
    match heard {
        Heard::This => Some(since.unwrap_or(now)),
        Heard::Other | Heard::Nothing => None,
    }
}

/// What one answer from a jar to "who owns you" decides, `held` since the
/// jar first said this spyc: `None` to ask again. A restore concludes
/// another tab of this spyc holds the jar only from the jar still saying so
/// past its deadline; no answer concludes nothing.
fn let_go_verdict(answer: Option<&Holder>, held: Duration, verdict: bool) -> Option<LetGo> {
    match answer {
        Some(Holder::This) if verdict && held >= LET_GO_RESTORING => Some(LetGo::StillThis),
        Some(Holder::This) => None,
        Some(_) => Some(LetGo::Answered),
        None => Some(LetGo::Unanswered),
    }
}

/// One try at the jar at `socket`, as its owner.
enum Try {
    Taken(Jarred),
    /// Another connection holds the jar's lease.
    Owned,
    Refused(String),
    NoToken,
    /// No welcome in time, or the connection lost after one: the jar is
    /// asked again for as long as it runs.
    Silent(String),
}

fn try_once(place: &Place, socket: &Path, size: (u16, u16)) -> Try {
    let Some(token) = tokens::load(place, socket) else {
        return Try::NoToken;
    };
    let (client, welcome) = match Client::connect(socket, Role::Owner, ADOPT_WAIT, &token) {
        Ok(answered) => answered,
        Err(e) => {
            return match e.downcast_ref::<Refusal>() {
                Some(r) if r.reason == reason::OWNER_PRESENT => Try::Owned,
                Some(r) => Try::Refused(r.why.clone()),
                None => Try::Silent(format!("{e:#}")),
            };
        }
    };
    match take(client, welcome, place, socket, size) {
        Ok(jarred) => Try::Taken(jarred),
        Err(e) => Try::Silent(format!("{e:#}")),
    }
}

/// Take back the running jar at `socket` for `spec`'s tab: its program,
/// untouched, or a detached stand-in saying why it can't be. `None` once
/// the jar has gone: a restore then starts the program afresh, a retake ends
/// the tab.
pub(super) fn back(spec: &Spec, socket: &Path, size: (u16, u16)) -> anyhow::Result<Option<Jarred>> {
    let watch = spec.watch.as_ref();
    // When the jar first said a connection of this spyc's holds it.
    let mut this_since: Option<Instant> = None;
    let mut said = String::new();
    let mut say = |line: String| {
        if line != said {
            if let Some(w) = watch {
                w.say(&line);
            }
            said = line;
        }
    };
    loop {
        anyhow::ensure!(
            watch.is_none_or(Watch::wanted),
            "the tab closed while its jar was taken back"
        );
        match socket_state(socket) {
            SocketState::Stale | SocketState::Missing => return Ok(None),
            SocketState::Unknown => {
                return Ok(Some(link::detached(socket, &cant_tell(socket), size)));
            }
            SocketState::Live => {}
        }
        match try_once(&spec.place, socket, size) {
            Try::Taken(jarred) => return Ok(Some(jarred)),
            Try::NoToken => return Ok(Some(link::detached(socket, &no_token(socket), size))),
            Try::Refused(why) => {
                let why = format!(
                    "This tab's jar at {} refused this spyc ({why}). Its program runs on, and \
                     nothing was started here.\n^a x closes this tab.",
                    socket.display()
                );
                return Ok(Some(link::detached(socket, &why, size)));
            }
            Try::Owned => match holder(socket) {
                Some(Holder::Other(pid)) => {
                    let why = owned_elsewhere(pid);
                    return Ok(Some(link::detached(socket, &why, size)));
                }
                Some(Holder::This) => {
                    say(
                        "Its jar still has this spyc's last connection to it; taking it back \
                         once that has gone."
                            .into(),
                    );
                    // A retake waits for as long as the jar runs; a restore whose
                    // jar still says so past its deadline finds a tab of this
                    // spyc's holding it.
                    this_since = this_since_after(this_since, Heard::This, Instant::now());
                    let since = this_since.unwrap_or_else(Instant::now);
                    let heard = match wait_let_go(socket, watch, since, !spec.retake) {
                        LetGo::StillThis => {
                            let why = format!(
                                "Another tab of this spyc has this tab's jar ({}); its program \
                                 runs there.\n^a x closes this tab.",
                                socket.display()
                            );
                            return Ok(Some(link::detached(socket, &why, size)));
                        }
                        LetGo::Answered => Heard::Other,
                        LetGo::Unanswered => Heard::Nothing,
                    };
                    this_since = this_since_after(this_since, heard, Instant::now());
                }
                Some(Holder::Nobody) => {
                    this_since = this_since_after(this_since, Heard::Other, Instant::now());
                }
                None => this_since = this_since_after(this_since, Heard::Nothing, Instant::now()),
            },
            Try::Silent(why) => {
                this_since = this_since_after(this_since, Heard::Nothing, Instant::now());
                say(format!(
                    "Its jar hasn't answered ({why}). It still runs, and this tab is taken back \
                     once it does."
                ));
            }
        }
    }
}

/// A detached tab's screen when another spyc owns its jar.
fn owned_elsewhere(pid: Option<u32>) -> String {
    format!(
        "This tab's program runs in its jar for another spyc{}, so nothing runs here.\n^a R \
         won't start it here while that spyc has it; ^a x closes this tab and leaves the \
         program running there.",
        by_pid(pid)
    )
}

/// A detached tab's screen when this spyc has no token for its jar.
pub(super) fn no_token(socket: &Path) -> String {
    format!(
        "This tab's program still runs in its jar at {}, but this spyc has no token for that \
         jar, so it can't take it back or end it. Nothing was started here, and ^a R won't \
         start it beside it.\n`spyc --jars` lists the jar and its pid; ^a x closes this tab.",
        socket.display()
    )
}

/// A detached tab's screen when nothing can tell whether its jar runs.
fn cant_tell(socket: &Path) -> String {
    format!(
        "Can't tell whether this tab's jar at {} still runs, so its program wasn't started \
         again, and ^a R won't start it while that can't be told.\n^a x closes this tab.",
        socket.display()
    )
}

/// This spyc has no token for the jar at the path it names, so can't end
/// its program there.
#[derive(Debug)]
pub struct NoToken(pub std::path::PathBuf);

impl std::fmt::Display for NoToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "this spyc has no token for the jar at {}, so can't end its program; nothing was \
             started",
            self.0.display()
        )
    }
}

impl std::error::Error for NoToken {}

/// Another spyc, by its pid when the jar knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OtherSpyc(pub Option<u32>);

/// Another spyc that owns the jar at `socket`, as the jar says within
/// `wait`; `None` when the jar names this spyc, no one, or doesn't answer
/// in time.
pub fn owned_elsewhere_now(socket: &Path, wait: Duration) -> Option<OtherSpyc> {
    let fields = spyc_jar::client::describe_within(socket, wait).ok()?;
    let field = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    if !field("owner").is_some_and(|o| o.starts_with("connected")) {
        return None;
    }
    let pid = field("owner_pid").and_then(|p| p.parse::<u32>().ok());
    (pid != Some(std::process::id())).then_some(OtherSpyc(pid))
}

/// A jar that welcomed this spyc as its owner, taken: its snapshot chosen
/// and its replay asked for.
fn take(
    mut client: Client,
    welcome: Msg,
    place: &Place,
    socket: &Path,
    (rows, cols): (u16, u16),
) -> anyhow::Result<Jarred> {
    let Msg::Welcome {
        child_pid,
        base,
        head,
        rows: had_rows,
        cols: had_cols,
        ..
    } = welcome
    else {
        anyhow::bail!("expected a welcome, got {welcome:?}");
    };
    client.send(&Msg::FetchSnapshot)?;
    let mut fetched = Vec::new();
    loop {
        match client.recv(ADOPT_WAIT)? {
            Some(Msg::Snapshot(Some(s))) => fetched.push(s),
            Some(Msg::Snapshot(None)) => break,
            Some(_) => {}
            None => anyhow::bail!("{} listed no snapshots", socket.display()),
        }
    }
    let snapshot = choose(&fetched, head, |blob| {
        snapshots::unstamp(blob).is_some_and(crate::pane::engine_ghostty::snapshot_decodes)
    })
    .cloned();
    // What the pane holds reaches the snapshot's offset; the jar holds bytes
    // from `base`. Past a gap the first byte handed over follows a cut.
    let from = snapshot.as_ref().map_or(base, |s| s.offset);
    let offsets = if snapshot.is_none() && base > 0 {
        offsets::Offsets::after_cut(base)
    } else {
        offsets::Offsets::new(from)
    };
    let replay = from.max(base);
    let resume = Resume {
        pid: child_pid,
        replay,
        offsets,
        gate: ReplayGate::until(head),
    };
    let jarred = connect(client, resume, socket, (rows, cols), place.repaint_hold)?;
    if (had_rows, had_cols) != (rows, cols) {
        jarred.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
    }
    Ok(Jarred {
        adoption: Some(Adoption { snapshot }),
        ..jarred
    })
}

/// End the program the jar at `socket` holds, through the jar, and see the
/// jar go: what a restart of its tab does before it starts anything (C2).
/// A jar another spyc owns is never ended from here ([`OwnedElsewhere`]);
/// one this spyc can't end in time leaves the restart undone.
pub fn end_held(place: &Place, socket: &Path, watch: Option<&Watch>) -> anyhow::Result<()> {
    let until = Instant::now() + END_HELD_WAIT;
    if let Some(w) = watch {
        w.say("Ending its program in its jar first…");
    }
    loop {
        if held(Some(socket)).is_none() {
            return Ok(());
        }
        let left = until.saturating_duration_since(Instant::now());
        let Some(token) = tokens::load(place, socket) else {
            // This spyc can't end it, but a jar already ending (a live
            // tab's restart has killed its program over the tab's own
            // connection) goes by itself: wait for that, not for a token.
            if left.is_zero() {
                return Err(NoToken(socket.to_path_buf()).into());
            }
            std::thread::sleep(LET_GO_POLL);
            continue;
        };
        anyhow::ensure!(
            !left.is_zero(),
            "its program still runs in its jar at {}, so nothing was started",
            socket.display()
        );
        match Client::connect(socket, Role::Owner, left, &token) {
            Ok((client, _)) => return end_through(client, socket, until),
            Err(e) => match e.downcast_ref::<Refusal>() {
                Some(r) if r.reason == reason::OWNER_PRESENT => match holder(socket) {
                    Some(Holder::Other(pid)) => return Err(OwnedElsewhere(pid).into()),
                    _ => std::thread::sleep(LET_GO_POLL),
                },
                _ => {
                    return Err(e.context(format!(
                        "end the program in the jar at {}; nothing was started",
                        socket.display()
                    )));
                }
            },
        }
    }
}

/// End the program of the jar `client` owns, and see the jar go, by
/// `until`.
fn end_through(mut client: Client, socket: &Path, until: Instant) -> anyhow::Result<()> {
    client.send(&Msg::Signal {
        signal: JarSignal::Kill as i32,
    })?;
    // From the newest byte: only the exit matters now.
    client.send(&Msg::Replay { from: u64::MAX })?;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        anyhow::ensure!(
            !left.is_zero(),
            "its program didn't end in its jar at {}; nothing was started",
            socket.display()
        );
        match client.recv(left)? {
            Some(Msg::Exited { .. }) => break,
            Some(Msg::Output { offset, bytes }) => {
                let _ = client.send(&Msg::Ack {
                    offset: offset + bytes.len() as u64,
                });
            }
            Some(_) | None => {}
        }
    }
    client.send(&Msg::AckExit)?;
    // The jar ends once its exit is collected.
    while held(Some(socket)).is_some() {
        anyhow::ensure!(
            Instant::now() < until,
            "its program ended, but its jar at {} still runs; nothing was started",
            socket.display()
        );
        std::thread::sleep(ENDING_POLL);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A restore concludes another tab of this spyc holds its jar only from
    /// the jar still naming this spyc past its deadline: silence concludes
    /// nothing, however long, and a retake never concludes it.
    #[test]
    fn only_the_jars_answer_past_its_deadline_says_another_tab_holds_it() {
        let past = LET_GO_RESTORING + Duration::from_secs(1);
        assert_eq!(
            let_go_verdict(Some(&Holder::This), past, true),
            Some(LetGo::StillThis)
        );
        assert_eq!(
            let_go_verdict(Some(&Holder::This), Duration::ZERO, true),
            None
        );
        assert_eq!(let_go_verdict(Some(&Holder::This), past, false), None);
        assert_eq!(let_go_verdict(None, past, true), Some(LetGo::Unanswered));
        assert_eq!(
            let_go_verdict(Some(&Holder::Nobody), past, true),
            Some(LetGo::Answered)
        );
        assert_eq!(
            let_go_verdict(Some(&Holder::Other(Some(7))), past, true),
            Some(LetGo::Answered)
        );
    }

    /// A jar that said "this spyc", then was silent past its deadline
    /// (stopped, starved, parked), and on waking still names the old
    /// connection whose end it hasn't read yet, isn't taken to hold another
    /// tab of this spyc: the silence isn't the jar's answer. When it then
    /// says "no owner", the tab is asked for again and taken back.
    #[test]
    fn a_jars_first_answer_after_a_silence_is_judged_on_its_own() {
        let t0 = Instant::now();
        let woke = t0 + LET_GO_RESTORING + Duration::from_secs(1);
        let since = this_since_after(None, Heard::This, t0);
        let since = this_since_after(since, Heard::Nothing, woke);
        let since = this_since_after(since, Heard::This, woke).expect("it said this spyc");
        assert_ne!(
            let_go_verdict(Some(&Holder::This), woke - since, true),
            Some(LetGo::StillThis),
            "a silence counted toward the jar's deadline"
        );
        assert_eq!(
            let_go_verdict(Some(&Holder::Nobody), woke - since, true),
            Some(LetGo::Answered),
            "\"no owner\" sends the retake back to the jar"
        );
        assert_eq!(this_since_after(Some(since), Heard::Other, woke), None);
    }
}
