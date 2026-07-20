# openpad

An open-source macropad controller for CLI coding agents: Claude Code,
Codex CLI, and Kimi. Built on the DOIO KB16-01. Inspired by the Work
Louder x OpenAI pad, but not tied to one vendor or one agent.

openpad turns a 16-key + 3-encoder macropad into a steering wheel for
agents you run in tmux: approve or reject tool calls, interrupt, branch,
undo, launch slash commands, push-to-talk into Wispr Flow, and see at a
glance (via the pad's RGB) whether an agent is idle, thinking, running,
waiting on you, done, or errored. All without stealing window focus from
whatever you're looking at.

## How it works

- **State feedback**: openpad installs additive hooks into Claude Code
  (and, when present, Codex) that POST lifecycle events to a small local
  ingest server. The daemon turns those events into per-agent state and
  renders it to the pad's RGB.
- **Steering without focus stealing**: pad keys send keystrokes straight
  into the bound agent's tmux pane over `tmux send-keys`, addressed by
  session name. Your foreground window never changes unless you explicitly
  ask to focus (binding an agent, or the encoder's "focus pane" push).
- **Voice**: a dedicated Mic key focuses the bound agent's pane, then fires
  the Wispr Flow push-to-talk hotkey.
- **Adapters are data, not code**: each agent's key bindings and
  event-to-state mapping live in a declarative TOML file under
  `adapters/`. Adding support for a new agent means writing a TOML file,
  not touching Rust.

## Status

Pre-release. This is the v1 core daemon for macOS: input decoding, agent
state tracking, RGB rendering, tmux dispatch, hook installation, and a
`doctor` health check are implemented and unit-tested. Hardware
communication (raw HID open/write to the KB16-01) has been verified with a
probe program on real hardware; a full human smoke test with the pad
running end-to-end alongside live Claude Code / Codex sessions is still
pending (see `docs/verification.md`). Treat this as "should work, wants a
few more hours of real-world use before you trust it in the middle of a
task."

Supported hardware: DOIO KB16-01 only, for now. The RGB and raw-HID paths
are written against its VIA report layout; other VIA-compatible pads with
16+ keys and rotary encoders could work with adjustments but haven't been
tried.

## Quickstart

### 1. Build

```bash
cargo build --release
```

The binary is `target/release/openpad`. A packaged install path (Homebrew
formula, code signing) is planned but not done; for now, build from
source or run via `cargo run -p openpad-daemon --release -- <command>`.)

### 2. Program and import the VIA layout

The pad's keys need to be programmed in VIA to emit specific Mac F-keys
(F13-F20) with modifiers, which the daemon decodes back into pad actions.
See `layouts/README.md` for the full key table and import/export steps.

### 3. Install agent hooks

```bash
openpad hooks install
```

This installs openpad's hooks additively into `~/.claude/settings.json`
(backing up the existing file first) and drops shim scripts under
`~/.local/share/openpad/`. It never removes or overwrites hooks you
already have configured for other tools. Codex CLI has no additive hooks
API yet, so `hooks install` prints the one line to add by hand to
`~/.codex/config.toml`.

### 4. Start your agent sessions in tmux

openpad addresses agents by tmux session name. The default config expects
`claude:0`, `codex:0`, and `kimi:0`. That is, a session named after the agent,
with the agent running in window 0:

```bash
tmux new -s claude
# inside the session:
claude
```

Repeat for `codex` / `kimi` sessions as needed. You don't have to run all
three. Openpad only lights up and steers agents whose tmux sessions
actually exist.

### 5. Run the daemon

```bash
openpad run
```

On first run this writes a default config to
`~/.config/openpad/config.toml` (agent list, tmux session names, ingest
port, prompt templates, Wispr Flow hotkey). Edit it to change any of
those. If no pad is attached, `openpad run` still runs (steering and hooks
work) but skips RGB output.

### 6. Troubleshooting: `openpad doctor`

```bash
openpad doctor
```

Checks whether the pad is on USB, tmux is reachable, the ingest port is
free (or already held by openpad itself), and Claude hooks are installed.
It prints a one-line hint for anything that's wrong.

### Debugging pad input

```bash
openpad listen
```

Prints every decoded pad event (`Key`, `EncoderTurn`, `EncoderPush`) as you
press it: useful for confirming a VIA layout was programmed correctly, or
for wiring in new key assignments.

## State / color legend

| State | Color | Motion |
|-------|-------|--------|
| IDLE | dim white | static |
| THINKING | dim blue | static |
| RUNNING | blue | static |
| WAITING | amber | **pulsing** |
| DONE | green | static, decays to IDLE after 5s |
| ERROR | red | static |

WAITING is the only state that animates (a slow triangle-wave pulse). The
pad only draws your eye in motion when an agent is actually waiting on
you. Every other state is a solid, steady color so it doesn't compete for
attention.

## Agent fidelity

- **Claude Code**: full fidelity. Claude's hooks system (`PreToolUse`,
  `PostToolUse`, `Notification`, `Stop`, etc.) gives openpad an accurate
  read on every state transition, including WAITING when a permission
  prompt is open.
- **Codex CLI**: full fidelity. Codex ships its own hooks system, and its
  `PermissionRequest` hook maps directly to an accurate WAITING light.
  The same fidelity as Claude. Older Codex versions without the hooks
  system fall back to the argv-based `notify` callback, which only fires
  on turn completion (no WAITING signal, but still gets you DONE/ERROR).
- **Kimi**: stub adapter with degraded fidelity (a few key bindings, no
  event mapping yet, so it always shows IDLE). Contributions welcome; see
  `adapters/kimi.toml` and the other two adapters as a template.

Because adapters are plain TOML (`adapters/*.toml`), adding or fixing an
agent's fidelity is a data change, not a code change.

## Uninstall

```bash
openpad hooks uninstall
```

Removes only the hook entries openpad added to `~/.claude/settings.json`
(backing up the file first). Anything else in your settings is left
untouched.

## License

MIT. See `LICENSE`.
