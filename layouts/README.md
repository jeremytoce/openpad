# VIA layout for the DOIO KB16-01

openpad talks to the pad over macOS's normal keyboard input path, not raw
HID for keys (raw HID is used only for RGB, see `crates/openpad-hid`). That
means the pad's physical keys have to be programmed in VIA to emit specific
Mac F-keys (F13-F20) with specific modifiers, and openpad's daemon decodes
those F-key + modifier combinations back into logical pad events
(`crates/openpad-daemon/src/input.rs`). This file is the source of truth for
that key programming.

There is no `kb16-via.json` checked into this repo yet. VIA layout export
requires clicking through VIA's GUI with a physical pad attached, which
isn't something that can be scripted or fabricated here. The table below
is complete enough to build the layout by hand in about five minutes.
Once you've built and tested it, export it (see below) and commit
`layouts/kb16-via.json` so nobody else has to redo this step.

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

| Key | VIA keycode | openpad action |
|----:|-------------|-----------------|
| 1 | `F13` | Bind pad to Claude |
| 2 | `F14` | Bind pad to Codex |
| 3 | `F15` | Bind pad to Kimi |
| 4 | `F16` | Broadcast to all agents |
| 5 | `F17` | Approve |
| 6 | `F18` | Approve always (don't ask again) |
| 7 | `F19` | Reject |
| 8 | `F20` | Stop / interrupt |
| 9 | `LSFT(F13)` | Mic (Wispr Flow push-to-talk) |
| 10 | `LSFT(F14)` | Ask |
| 11 | `LSFT(F15)` | Branch |
| 12 | `LSFT(F16)` | Undo |
| 13 | `LSFT(F17)` | Plan mode |
| 14 | `LSFT(F18)` | Compact |
| 15 | `LSFT(F19)` | Clear |
| 16 | `MO(1)` | Hold: momentary Launch layer |

### Layer 1: Launch layer (held via key 16, or locked via `TG(1)`)

| Key | VIA keycode | openpad action |
|----:|-------------|-----------------|
| 1 | `LCTL(F13)` | Bind pad to Claude |
| 2 | `LCTL(F14)` | Bind pad to Codex |
| 3 | `LCTL(F15)` | Bind pad to Kimi |
| 4 | `LCTL(F16)` | Broadcast to all agents |
| 5 | `LCTL(F17)` | `/review` |
| 6 | `LCTL(F18)` | `/test` |
| 7 | `LCTL(F19)` | `/commit` |
| 8 | `LCTL(F20)` | `/pr` |
| 9 | `LCTL(LSFT(F13))` | Prompt 1 |
| 10 | `LCTL(LSFT(F14))` | Prompt 2 |
| 11 | `LCTL(LSFT(F15))` | Prompt 3 |
| 12 | `LCTL(LSFT(F16))` | Prompt 4 |
| 13 | `LCTL(LSFT(F17))` | repo picker *(reserved, Plan 3)* |
| 14 | `LCTL(LSFT(F18))` | worktree picker *(reserved, Plan 3)* |
| 15 | `LCTL(LSFT(F19))` | logs viewer *(reserved, Plan 3)* |
| 16 | `TG(1)` | Toggle: lock/unlock Launch layer |

Row 1 (keys 1-4) is identical on both layers on purpose: bind/broadcast
should always be one tap away, whichever layer you're on.

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

- **Encoder 1** (turn): reserved for transcript scroll, not wired up yet (Plan 2).
- **Encoder 1** (push): reserved, not wired up yet.
- **Encoder 2** (turn): cycles which agent is bound (wraps claude -> codex -> kimi -> claude).
- **Encoder 2** (push): focuses the bound agent's pane. This is the one intentional
  focus-stealing action tied to a steering key. Everything else routes to the
  agent's tmux pane without touching window focus.
- **Encoder 3** (turn): reserved for model-tier switching, not wired up yet (Plan 2).
- **Encoder 3** (push): reserved, not wired up yet.

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
