## Goal Description
The goal is to support "true" mouse scrolling (via trackpad or scroll wheel) inside `spyc`'s alternate screen. 

Currently, `spyc` uses a terminal trick (DEC private mode 1007) that asks the terminal emulator to translate scroll-wheel movements into rapid `Up` and `Down` arrow keystrokes. While this works seamlessly for moving the cursor in the file list, it causes a severe UX problem when focused on the embedded agent pane: scrolling the mouse wheel sends a flurry of `Up` arrows to the shell, cycling through command history instead of scrolling the terminal output (scrollback). Because the application just sees arrow keys, it cannot differentiate a mouse scroll from a legitimate keyboard arrow press.

By opting into `EnableMouseCapture`, we receive explicit `ScrollUp` and `ScrollDown` mouse events. We can route these events to trigger the appropriate actions based on what component is currently focused—solving the pane scrollback issue natively.

## User Review Required
> [!WARNING]
> **Loss of native text selection**
> Capturing the mouse in a terminal application breaks the default click-and-drag text selection provided by your terminal emulator. Users will have to hold a modifier key (like `Shift` on Linux/Windows or `Option/Fn` on macOS) to select text with their mouse while `spyc` is open. Because of this, this feature will be strictly **opt-in** via `.spycrc.toml`.

## Open Questions: "What data will we scroll? Do we need a new buffer?"
**Answer:** We do *not* need to maintain a new buffer!
`spyc` already maintains a robust scrollback buffer for the embedded pane using the `vt100` library (which retains screen history in `vt100::Screen::scrollback()`), and it already has a feature to view it: **Scroll Mode** (triggered manually via `^a v`).

When you press `^a v`, `spyc` captures the `vt100` scrollback and dumps it into a native PagerView overlay. 
**With true mouse scrolling:** If the user scrolls their mouse wheel *up* while focused on the live pane, we will simply synthesize a new `PaneScrollbackMouse` action programmatically. This instantly mounts the scrollback Pager (with UI chrome disabled). Subsequent scroll events will naturally scroll the pager up and down. 

## Proposed Changes

### Configuration
We will introduce a new configuration block in `src/config/mod.rs` to allow users to opt-in to this feature.

#### [MODIFY] src/config/mod.rs
```rust
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(default)]
pub struct TerminalConfig {
    /// Enable true mouse capture. Breaks native terminal text selection (requires Shift/Option to bypass).
    pub mouse_capture: bool,
}
```

#### [MODIFY] src/app/config.rs
To ensure hot-reloading `.spycrc.toml` works, we must apply the changes dynamically when the configuration changes.
```rust
// Inside the ConfigReload handler
if new_config.terminal.mouse_capture != old_config.terminal.mouse_capture {
    if new_config.terminal.mouse_capture {
        effects.push(Effect::RawTerminalCommand(crossterm::event::EnableMouseCapture.to_string()));
    } else {
        effects.push(Effect::RawTerminalCommand(crossterm::event::DisableMouseCapture.to_string()));
        effects.push(Effect::RawTerminalCommand(EnableAlternateScroll.to_string()));
    }
}
```

---

### Terminal Setup
We will modify the terminal setup and teardown routines to conditionally enable mouse capture instead of (or alongside) DEC 1007 alternate scrolling.

#### [MODIFY] src/lib.rs
```diff
-        EnableAlternateScroll,
-        HideMousePointer
+        HideMousePointer
     )?;
+
+    if config.terminal.mouse_capture {
+        execute!(stdout, crossterm::event::EnableMouseCapture)?;
+    } else {
+        execute!(stdout, EnableAlternateScroll)?;
+    }
```
*(A matching `DisableMouseCapture` will be added to `teardown_terminal`)*

---

### MVU Architecture & Actions
To maintain `spyc`'s strict MVU architecture, we will not mutate state directly in the event loop. Instead, we will formalize mouse scrolling as discrete `Action` variants.

#### [MODIFY] src/keymap/action.rs
```rust
pub enum Action {
    // ...
    ScrollUpMouse(usize),
    ScrollDownMouse(usize),
    PaneScrollbackMouse,
}
```

#### [MODIFY] src/app/run.rs
Intercept `Event::Mouse` in the main loop and map it to our new actions. We silently ignore clicks for now, leaving room for future click-to-focus implementations.
```rust
// Inside dispatch_effective() match arm:
Event::Mouse(mouse_ev) => {
    use crossterm::event::{MouseEventKind, MouseEvent};
    use crate::keymap::action::Action;
    
    let action = match mouse_ev.kind {
        MouseEventKind::ScrollUp => {
            if self.view.focus == Focus::Pane && self.active_pager_ref().is_none() {
                Some(Action::PaneScrollbackMouse)
            } else {
                Some(Action::ScrollUpMouse(3))
            }
        },
        MouseEventKind::ScrollDown => Some(Action::ScrollDownMouse(3)),
        _ => None, // Future: Handle clicks here
    };
    
    if let Some(act) = action {
        let effects = self.apply_action(act)?;
        self.run_effects(effects, terminal, foreground_exec);
    }
}
```

---

### Event Handling & UX Polish
We will implement the actions to handle the two critical UX requests:
1. **Disable Pager Chrome:** When mounting via mouse, disable line numbers and EOF markers.
2. **Auto-Exit on Bottom:** Exit the pager if the user scrolls down *past* the bottom.

#### [MODIFY] src/app/pane_scroll.rs
Modify `install_lower_pane_scroll_view` to accept a `via_mouse` boolean:
```rust
pub(crate) fn install_lower_pane_scroll_view(
    // ...
    via_mouse: bool,
) {
    // ... setup PagerView ...
    if via_mouse {
        view.show_line_numbers = false;
        view.streaming = true; // suppresses EOF marker and tilde fill
    } else {
        view.show_line_numbers = true;
    }
}
```

#### [MODIFY] src/app/actions.rs
Handle the new mouse actions to enforce the auto-exit heuristic safely.
```rust
Action::PaneScrollbackMouse => {
    // Calls install_lower_pane_scroll_view(..., via_mouse: true)
    self.mount_scroll_pager_via_mouse(); 
}
Action::ScrollDownMouse(n) => {
    if self.view.focus == Focus::Pane {
        if let Some(view) = self.active_pager_mut() {
            if view.pane_scroll {
                let viewport = self.pager_viewport();
                let was_at_bottom = view.scroll >= view.scroll_max(viewport);
                
                // Only exit if they were ALREADY at the bottom before this scroll event
                if was_at_bottom {
                    self.close_pane_scroll_pager();
                    return Ok(Vec::new());
                }
            }
        }
    }
    // Otherwise, just scroll down normally
    self.apply_action(Action::Down(n))?
}
Action::ScrollUpMouse(n) => self.apply_action(Action::Up(n))?
```

## Verification Plan

### Automated Tests
* Run `cargo test --all-targets` to ensure the input routing changes do not break existing pure routing tests.
* Ensure `config` deserialization tests pass with the new `[terminal]` section.

### Manual Verification
1. Open `.spycrc.toml` and set `[terminal] mouse_capture = true`.
2. Launch `spyc`.
3. With focus on the file list, scroll the mouse wheel and verify it jumps by 3 lines per tick.
4. **Seamless Mount Test:** Move focus to the bottom Agent pane. Execute `ls -la` to fill the screen. Scroll the mouse wheel *up*. Verify that `spyc` immediately mounts the scrollback pager WITHOUT line numbers or `[EOF]` markers, and scrolls up.
5. **Auto-Exit Test:** Scroll the mouse wheel *down* until the pager stops moving (hitting the bottom). Scroll down *once more*. Verify that this final scroll past the bottom automatically closes the pager and returns you to the live pane.
6. **Native Selection Tests:** Try to click-and-drag text to verify native selection is intercepted. Hold `Option` (macOS) and click-and-drag to verify native selection can be bypassed.
