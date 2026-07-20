# Openpad — Design Spec

**Date:** 2026-07-20
**Status:** Approved by Jeremy (brainstorming session)

Open source macropad controller for CLI coding agents (Claude Code, OpenAI Codex CLI, Kimi CLI), driving a DOIO KB16-01 as a physical steering and launch surface, in the spirit of the Work Louder x OpenAI pad.

## Goals

1. **Steer live agent sessions physically**: approve, reject, interrupt, continue without breaking focus.
2. **Launch work**: slash commands, prompt templates, repo/worktree selection on a second layer.
3. **Ambient state display**: pad LEDs reflect each agent's live state (idle / thinking / running / waiting / done / error), driven by agent hooks.
4. **Voice input**: mic key triggers Wispr Flow push-to-talk.
5. **Agent-agnostic core**: adapters are declarative TOML profiles; adding an agent means writing a file, not code.
6. **Open source ergonomics**: single-binary install, no firmware flashing, additive and reversible hook installation.

## Non-goals (v1)

- Custom QMK firmware (stock VIA firmware is sufficient; raw HID 0xFF60 endpoint already exposed).
- Windows/Linux support (macOS first; abstractions should not preclude porting).
- Screen-scraping agent TUIs for state (only where a hook surface is missing, and clearly labeled degraded).
- Built-in speech-to-text (delegated to Wispr Flow).

## Hardware

DOIO KB16-01: 4x4 keys, 3 rotary encoders. Enumerated on this machine as VID `0xD010` (53264), PID `0x1601` (5633), three USB interfaces:

- Keyboard HID (`usagePage 0x01 / usage 0x06`) — input path
- Mouse HID (`0x01/0x02`) — unused
- **Raw HID (`usagePage 0xFF60 / usage 0x61`)** — QMK/VIA protocol endpoint; used for per-key RGB output

Keys are remapped via a shipped **VIA layout JSON** (imported in VIA.app, no flashing) to emit F13–F24 + modifier combos, so keypresses never collide with normal typing.

## Architecture

One long-running local daemon owns the HID handle. All other components talk to the daemon.

```
   DOIO KB16-01
        │  keyboard HID (F13–F24)              ▲ raw HID 0xFF60 (RGB)
        ▼                                      │
┌──────────────────────────────────────────────────────┐
│                   openpad daemon                     │
│  input listener ─▶ state machine ─▶ RGB + HUD output │
│        │                ▲                            │
│        ▼                │ events                     │
│   dispatcher       ingest (localhost socket)         │
└────────┼─────────────────────▲───────────────────────┘
         ▼ tmux send-keys / focus                │
   ┌─────────────┬─────────────┬─────────────┐   │
   │ claude:0    │ codex:0     │ kimi:0      │   │
   └─────────────┴─────────────┴─────────────┘   │
          agent hook shims POST back ────────────┘
```

Components:

- **Input listener**: global capture of the pad's F-key events (macOS event tap / hidapi).
- **State machine**: per-agent state (six states below); the only writer of LED/HUD output.
- **Dispatcher**: delivers actions to agents. Primary path: `tmux send-keys` to named sessions (`claude:0`, `codex:0`, `kimi:0`). Fallback: synthesize keystrokes into the focused window when no tmux binding exists (hybrid targeting).
- **Ingest**: localhost socket (HTTP on loopback) receiving state events from hook shims.
- **Output**: raw HID RGB frames to the pad + HUD/menu-bar updates.

### Adapter profiles (the compatibility seam)

Declarative per-agent TOML; no agent-specific code in core.

```toml
# adapters/claude.toml
[actions]
approve        = "1"
approve_always = "2"
reject         = "3"
interrupt      = "Escape"

[events]
source        = "hooks"        # claude code hook events
Notification  = "WAITING"
PreToolUse    = "RUNNING"
Stop          = "DONE"
```

- **Claude Code**: rich hook surface (`Notification`, `PreToolUse`, `Stop`, `SubagentStop`) → full six-state fidelity.
- **Codex CLI**: `notify` program in `~/.codex/config.toml` fires on turn completion only → reliable `DONE`, no crisp `WAITING`. Rendered honestly as degraded fidelity (dimmer/hatched treatment), not faked.
- **Kimi CLI**: adapter stub in v1; same TOML shape.

**Open verification item:** exact keystrokes for Claude's permission prompt and Codex's approval flow are best guesses and MUST be verified against the live TUIs during implementation (explicit plan task).

## Layer map

Row 1 is the **agent bar** on every layer: each key IS an agent — its LED shows that agent's state; pressing it binds the pad to that agent and focuses its pane. The **All** key broadcasts: while bound to All, steer/launch actions dispatch to every running agent, and its LED shows the most urgent state across agents (ERROR > WAITING > RUNNING > THINKING > DONE > IDLE). Key 16 (`⇧ LAYER`) holds for momentary layer 2, double-taps to lock.

**Layer 1 — STEER**

| | | | |
|---|---|---|---|
| Claude (state) | Codex (state) | Kimi (state) | All (state) |
| ✓ Approve | ✓✓ Approve-always | ✕ Reject | ⏸ Stop/interrupt |
| 🎤 Mic (Wispr) | 💬 Ask | ⤢ Branch/handoff | ↩ Undo |
| ◔ Plan mode | ⌦ Compact | ⟲ Clear | ⇧ LAYER |

**Layer 2 — LAUNCH**

| | | | |
|---|---|---|---|
| Claude (state) | Codex (state) | Kimi (state) | All (state) |
| /review | /test | /commit | /pr |
| Prompt 1 | Prompt 2 | Prompt 3 | Prompt 4 |
| Repo picker | Worktree | Logs | ⇧ LAYER |

**Encoders:**

1. Scroll transcript; push = jump to latest.
2. Cycle bound agent; push = focus its pane.
3. Thinking budget / model tier; push = layer lock.

## State model

| State | Color | Behavior |
|---|---|---|
| `IDLE` | dim white | ambient |
| `THINKING` | blue | slow breathe |
| `RUNNING` | blue | steady |
| `WAITING` | amber | **pulse** |
| `DONE` | green | 5s, fades to idle |
| `ERROR` | red | steady until acknowledged |

**Motion rule (hard constraint): only `WAITING` may animate.** Motion exclusively means "you are blocking something." Everything else is ambient, or the pad becomes peripheral-vision noise and loses all signal value.

## Targeting model

- **Primary**: tmux-addressed named sessions. Deterministic; enables steering a background agent while looking elsewhere; enables one pad driving all three agents simultaneously.
- **Fallback**: focused-window keystroke injection when no tmux binding exists (keeps the tool usable for non-tmux users).
- **Mic key exception — focus-then-dictate**: the mic key first focuses the bound agent's pane, then fires Wispr Flow's push-to-talk hotkey. Wispr types into the focused app, so the focus jump is what routes dictation correctly. The jump is accepted as honest feedback about where words are going.

## UI surface

1. **HUD** (seen most): always-on-top overlay; fades in on pad touch or `⇧ LAYER` hold, fades out ~1.5s after. Shows the current layer's key legend (the pad has no printed legends) plus a status line with the bound agent, its state, and **the pending tool call when WAITING**. That last line is the safety mechanism: a physical approve key must never fire blind.
2. **Menu bar item**: connection status, active agent, pause toggle.
3. **Config window**: visual keymap editor (click a key on a rendered pad, pick an action), adapter profile editing, Wispr hotkey setting, prompt template slots.

## Stack

- **Rust + Tauri v2**: one signed binary containing daemon, tray, HUD, config UI.
- `hidapi` for pad I/O; `tmux` via `std::process`; macOS event tap for input capture.
- Rationale: distribution. `brew install openpad` yielding one binary; avoids `node-hid` native-rebuild support burden on contributors.
- Hook shims stay language-agnostic: tiny scripts that POST JSON to the loopback socket; contributors can write shims in anything.

## Repo layout

```
openpad/
├─ crates/openpad-core/     state machine, adapter parsing (no I/O — fully testable)
├─ crates/openpad-hid/      pad transport, RGB frames
├─ crates/openpad-dispatch/ tmux, focus, keystroke synthesis
├─ app/                     Tauri: HUD, config, tray
├─ adapters/                claude.toml, codex.toml, kimi.toml
├─ shims/                   hook scripts (claude hooks, codex notify)
└─ layouts/                 VIA JSON for KB16-01
```

## Testing strategy

- HID and tmux behind traits with fakes; `openpad-core` has zero I/O and carries the real coverage (state transitions, adapter parsing, LED frame derivation). Runs headless in CI with no pad.
- `VirtualPad` fake: replay event sequences, assert on resulting LED frames.
- `openpad doctor`: human-in-the-loop verifier — pad enumeration, tmux reachability, hook installation, Wispr hotkey presence; prints what's broken.

## Install / uninstall

- Hook installation is **additive**: appends to existing `settings.json` hook arrays, never rewrites (must coexist with Jeremy's GSD hook suite). Codex: adds/updates `notify` in `~/.codex/config.toml`.
- `openpad uninstall` cleanly removes only openpad's own entries. **Written before the installer, not after.**
- VIA layout: shipped JSON, imported by the user in VIA.app.

## Open items carried into planning

1. Verify Claude Code permission-prompt keystrokes and hook payload shapes against the live TUI.
2. Verify Codex CLI approval keys and `notify` config against the live TUI.
3. Verify Wispr Flow hotkey configurability (settable/stable global hotkey we can synthesize).
4. Confirm KB16-01 VIA RGB control granularity (per-key vs zones) over raw HID.
