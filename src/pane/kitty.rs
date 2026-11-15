//! Keys for a program that asks for the kitty keyboard protocol (#1176): which
//! process group owns the flags in force, and whether a key goes to the
//! program by them or the legacy way. ARCHITECTURE.md → "Keys for a program
//! that asks for the kitty keyboard protocol".

use super::{Pane, engine};

impl Pane {
    /// The kitty keyboard flags the program pushed, in force on its screen.
    pub fn kitty_keyboard_flags(&self) -> u8 {
        engine::Engine::kitty_keyboard_flags(&*self.lock_parser())
    }

    /// After output: a program that just pushed kitty keyboard flags owns them,
    /// and with none in force nobody does.
    pub(super) fn note_kitty_owner(&mut self) {
        if self.kitty_keyboard_flags() == 0 {
            self.kitty_owner = None;
        } else if self.kitty_owner.is_none() {
            self.kitty_owner = self.foreground_pgrp();
        }
    }

    /// `key` by the kitty keyboard protocol, when the program in the
    /// foreground pushed its flags (#1176); `None` sends it the legacy way.
    pub(super) fn kitty_key(&mut self, key: crossterm::event::KeyEvent) -> Option<Vec<u8>> {
        let flags = self.kitty_keyboard_flags();
        let fg = self.foreground_pgrp();
        let (route, owner) = kitty_route(flags, self.kitty_owner, fg, group_alive);
        self.kitty_owner = owner;
        match route {
            KittyRoute::Legacy => None,
            KittyRoute::EndThenLegacy => {
                engine::Engine::end_kitty_keyboard(&mut *self.lock_parser());
                None
            }
            KittyRoute::Kitty => engine::Engine::encode_kitty_key(&*self.lock_parser(), key),
        }
    }
}

/// How a key reaches a pane's program, by [`kitty_route`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KittyRoute {
    Legacy,
    Kitty,
    /// The flags' owner is gone: pop them, then send this key the legacy way.
    EndThenLegacy,
}

/// Who gets kitty-encoded keys, from the flags in force, the group that pushed
/// them, the pane's foreground group and whether a group is still alive; with
/// the owner to keep.
///
/// SPYC-TRAP(kitty-keys-follow-the-owner): flags outlive a program that
/// crashes or is stopped without popping them, and a shell handed
/// kitty-encoded keys can't read its own Enter. So only the owning group gets
/// them, and the flags are popped once that group is gone. With the foreground
/// unknown (a jar that hasn't said), the program that asked is believed.
pub(super) fn kitty_route(
    flags: u8,
    owner: Option<u32>,
    fg: Option<u32>,
    alive: impl Fn(u32) -> bool,
) -> (KittyRoute, Option<u32>) {
    if flags == 0 {
        return (KittyRoute::Legacy, None);
    }
    match (owner.or(fg), fg) {
        (Some(owner), Some(fg)) if owner != fg => {
            if alive(owner) {
                (KittyRoute::Legacy, Some(owner))
            } else {
                (KittyRoute::EndThenLegacy, None)
            }
        }
        (owner, _) => (KittyRoute::Kitty, owner),
    }
}

/// Whether any process is left in group `pgid`. One that exists but can't be
/// signalled (`EPERM`) is alive.
pub(super) fn group_alive(pgid: u32) -> bool {
    let Some(pid) = i32::try_from(pgid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return false;
    };
    !matches!(
        rustix::process::test_kill_process_group(pid),
        Err(rustix::io::Errno::SRCH)
    )
}
