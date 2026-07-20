# VIA layout for the DOIO KB16-01

openpad talks to the pad over macOS's normal keyboard input path, not raw
HID for keys (raw HID is used only for RGB, see `crates/openpad-hid`). That
means the pad's physical keys have to be programmed in VIA to emit specific
Mac F-keys (F13-F20) with specific modifiers, and openpad's daemon decodes
those F-key + modifier combinations back into logical pad events
(`crates/openpad-daemon/src/input.rs`). This file is the source of truth for
that key programming.

`kb16-via.json` in this directory was generated from DOIO's official VIA
definition (the-via/keyboards, 4x5 matrix: 16 keys in columns 0-3, encoder
pushes at 0,4 / 1,4 / 2,4). Import it in VIA: Save + Load tab, Load, pick
the file. If your VIA build rejects it (layer count or nested Ctrl+Shift
keycodes are the two plausible reasons), fall back to assigning by hand
from the tables below, then re-export over the file and open a PR.
After importing, verify with `openpad listen` (checklist at the bottom).

## Prerequisites

- **Quit the openpad daemon first** (`pkill -f "openpad run"`). The daemon
  holds the pad's raw HID interface exclusively for RGB control, and VIA
  needs that same interface. If VIA shows "NotAllowedError: Failed to open
  the device" or "Received invalid protocol version from device", openpad
  is still running; stop it and, if needed, unplug and replug the pad so
  VIA re-enumerates it. Restart `openpad run` when you're done in VIA.
- A DOIO KB16-01 plugged into a direct USB port (not through a hub).
- [VIA](https://www.caniusevia.com/) desktop app, with the KB16-01's VIA
  definition loaded (VIA ships with common definitions; if the pad doesn't
  show up automatically, use **Settings -> Show Design tab -> Load draft
  definition** and load DOIO's KB16-01 JSON definition).

## Building the layout by hand

VIA shows two layers, 0 and 1, selectable by the tab strip at the top of
the **Keymap** view. For each key you assign, click the physical key in
VIA's on-screen picture, then click the keycode in the panel below (the
**Basic** tab has bare F13-F20). Hold a modifier chip (Shift, Ctrl, Alt)
in the same panel before clicking the F-key to produce combos like
`LSFT(F13)`; layer keys `MO(1)` / `TG(1)` live under the **Layers** tab).

Key numbers below are row-major, top-left to bottom-right, matching VIA's
own numbering for the KB16-01's 4x4 grid:

```
[ 1] [ 2] [ 3] [ 4]
[ 5] [ 6] [ 7] [ 8]
[ 9] [10] [11] [12]
[13] [14] [15] [16]
```

Three rotary encoders (each with a push switch) sit outside this grid;
VIA exposes them as separate CCW / CW / push assignments per layer.

### Layer 0: Steer layer (default)

Verbs act on the focused window; the agent profile is auto-selected from
it (active tmux pane, else window title). Corners are blind-findable:
top-left approve, top-right interrupt, bottom-left mic, bottom-right layer.

| Key | VIA keycode | openpad action |
|----:|-------------|-----------------|
| 1 | `F13` | Approve |
| 2 | `F14` | Approve always (don't ask again) |
| 3 | `F15` | Reject |
| 4 | `F16` | Stop / interrupt |
| 5 | `F17` | Goto waiting: focus the session blocked on you |
| 6 | `F18` | Continue (types "continue" + Enter) |
| 7 | `F19` | Undo (rewind menu) |
| 8 | `F20` | Plan mode |
| 9 | `LSFT(F13)` | Compact |
| 10 | `LSFT(F14)` | Clear / new chat |
| 11 | `LSFT(F15)` | Model picker |
| 12 | `LSFT(F16)` | Prompt 1 |
| 13 | `LSFT(F17)` | Mic (Wispr Flow push-to-talk) |
| 14 | `LSFT(F18)` | Prompt 2 |
| 15 | `LSFT(F19)` | Prompt 3 |
| 16 | `MO(1)` | Hold: momentary Launch layer |

### Layer 1: Launch layer (held via key 16, or locked via `TG(1)`)

| Key | VIA keycode | openpad action |
|----:|-------------|-----------------|
| 1 | `LCTL(F13)` | `/review` |
| 2 | `LCTL(F14)` | `/test` |
| 3 | `LCTL(F15)` | `/commit` |
| 4 | `LCTL(F16)` | `/pr` |
| 5 | `LCTL(F17)` | Prompt 4 |
| 6 | `LCTL(F18)` | Prompt 5 |
| 7 | `LCTL(F19)` | Prompt 6 |
| 8 | `LCTL(F20)` | Prompt 7 |
| 9 | `LCTL(LSFT(F13))` | repo picker *(reserved, Plan 3)* |
| 10 | `LCTL(LSFT(F14))` | worktree picker *(reserved, Plan 3)* |
| 11 | `LCTL(LSFT(F15))` | logs viewer *(reserved, Plan 3)* |
| 12 | `LCTL(LSFT(F16))` | *(unassigned)* |
| 13 | `LCTL(LSFT(F17))` | *(unassigned)* |
| 14 | `LCTL(LSFT(F18))` | *(unassigned)* |
| 15 | `LCTL(LSFT(F19))` | *(unassigned)* |
| 16 | `TG(1)` | Toggle: lock/unlock Launch layer |

Key 16 is programmed as a layer key (`MO(1)` on layer 0, `TG(1)` on layer
1), not as an F-key combo. The daemon never receives a HID event for it.
The layer switch happens entirely in the pad's firmware. That's why
`Action::LayerHold` in `crates/openpad-core/src/keymap.rs` is a documented
no-op: there is nothing for it to do.

### Encoders

All three encoders are read by the daemon as `Alt`+F-key combos
(`crates/openpad-daemon/src/input.rs`). Program these the same on both
layers. Encoders aren't layer-switched.

| Encoder | CCW | CW | Push |
|---------|-----|----|----|
| 1 | `LALT(F13)` | `LALT(F14)` | `LALT(F19)` |
| 2 | `LALT(F15)` | `LALT(F16)` | `LALT(F20)` |
| 3 | `LALT(F17)` | `LALT(F18)` | `LALT(LSFT(F13))` |

Current daemon behavior per encoder (see `crates/openpad-daemon/src/runloop.rs`):

- **Encoder 1** (turn): menu knob. Sends Up / Down arrows to the focused
  window, so TUI dialogs (permission options, rewind menu, model picker)
  are knob-navigable. Push sends Enter.
- **Encoder 2** (push): toggles the Launch layer in software — click once
  for Launch, click again for Steer. Works alongside the firmware layer
  key (16); no VIA change needed. Turn: reserved (Plan 2).
- **Encoder 3**: reserved (Plan 2, model tier).

## Exporting (do this once you've built and tested the layout)

1. In VIA, with the pad attached and the layout above fully assigned on
   both layers and both encoder rows: go to the **Keymap** view's overflow
   menu (or **Design** tab, depending on VIA version) and choose
   **"Save current layout"**.
2. Save the file as `layouts/kb16-via.json` in this repo.
3. Run `openpad listen` (see root README) and tap every physical key and
   encoder direction/push once, on both layers, confirming each produces
   the expected `PhysKey` printed to the terminal. Record results in
   `docs/verification.md`.
4. Commit `layouts/kb16-via.json`.

## Importing (for anyone re-flashing or setting up a new pad)

1. Open VIA, plug in the KB16-01.
2. If the pad's definition isn't recognized automatically: **Settings ->
   Show Design tab**, then in the new **Design** tab, **Load draft
   definition** and select DOIO's KB16-01 definition JSON.
3. Go to the **Keymap** view, open the overflow/design menu, and choose
   **"Load saved layout"**. Select `layouts/kb16-via.json` from this repo.
4. Confirm both layers and the encoders match the tables above. Run
   `openpad listen` and tap through all 16 keys and 3 encoders to verify.
