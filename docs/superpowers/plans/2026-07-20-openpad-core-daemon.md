# Openpad Core Daemon Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Headless daemon that turns a DOIO KB16-01 into a steering/launch surface for Claude Code, Codex CLI, and Kimi CLI: pad keys dispatch actions to tmux-hosted agents, agent hooks report state back, and the pad's RGB reflects per-agent state.

**Architecture:** Rust workspace with three library crates (`openpad-core` = pure state machine + adapters + LED derivation, no I/O; `openpad-hid` = pad transport; `openpad-dispatch` = tmux/focus delivery) and one binary crate (`openpad-daemon`) wiring them together with an ingest HTTP server on loopback. Hook shims are bash scripts that POST agent events to the daemon.

**Tech Stack:** Rust (stable), `hidapi`, `rdev` (global key capture), `tiny_http` + `serde_json` (ingest), `toml`/`serde` (config + adapters), bash + curl (shims), tmux.

## Global Constraints

- macOS only for v1; keep OS-specifics inside `openpad-hid` and `openpad-dispatch`.
- Config and state live in `~/.config/openpad/`; everything runs locally, no network beyond `127.0.0.1`.
- Ingest listens on `127.0.0.1:7676` (bind loopback explicitly, never `0.0.0.0`).
- **Motion rule:** only `WAITING` may animate on the pad. Spec's "THINKING slow breathe" is overridden by this rule → THINKING renders dim steady blue. (Spec clarification, resolved here.)
- Hook installation is additive: append to existing `~/.claude/settings.json` hook arrays, never rewrite other entries; uninstall removes only openpad's own entries (identified by `openpad` in the command path).
- Pad identity: VID `0xD010` (53264), PID `0x1601` (5633); raw HID interface usagePage `0xFF60`, usage `0x61`.
- Agent action keystrokes in adapter TOMLs are provisional until Task 5's live verification; code must read them from TOML, never hardcode.
- Rust 2021 edition, workspace resolver 2. Test everything in `openpad-core` headlessly; hardware-touching tests are `#[ignore]`.
- End commit messages with: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## File Structure

```
openpad/
├─ Cargo.toml                      # workspace
├─ crates/openpad-core/src/
│   ├─ lib.rs                      # re-exports
│   ├─ state.rs                    # AgentState, StateMachine
│   ├─ adapter.rs                  # Adapter TOML parsing
│   ├─ keymap.rs                   # PhysKey, Action, Keymap
│   └─ led.rs                      # Rgb, LedFrame, derive_frame
├─ crates/openpad-hid/src/lib.rs   # PadLink trait, HidPad, discovery
├─ crates/openpad-dispatch/src/lib.rs  # Dispatcher trait, TmuxDispatcher, FocusDispatcher
├─ crates/openpad-daemon/src/
│   ├─ main.rs                     # CLI (run|listen|doctor|hooks)
│   ├─ ingest.rs                   # tiny_http server
│   ├─ config.rs                   # ~/.config/openpad/config.toml
│   ├─ hooks.rs                    # settings.json install/uninstall transforms
│   ├─ doctor.rs
│   └─ runloop.rs                  # wiring: input → dispatch, ingest → state → LED
├─ adapters/{claude,codex,kimi}.toml
├─ shims/{claude-hook.sh,codex-notify.sh}
├─ layouts/kb16-via.json           # exported from VIA.app (Task 10)
└─ docs/verification.md            # live-TUI findings (Task 5)
```

---

### Task 1: Workspace scaffold

**Files:**
- Create: `Cargo.toml`, `crates/openpad-core/Cargo.toml`, `crates/openpad-core/src/lib.rs`, `.gitignore`

**Interfaces:**
- Produces: compiling empty workspace; crate name `openpad-core` importable in later tasks.

- [ ] **Step 1: Write workspace manifests**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/openpad-core"]
```

`crates/openpad-core/Cargo.toml`:
```toml
[package]
name = "openpad-core"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
toml = "0.8"
```

`crates/openpad-core/src/lib.rs`:
```rust
pub mod adapter;
pub mod keymap;
pub mod led;
pub mod state;
```
(Create empty `state.rs`, `adapter.rs`, `keymap.rs`, `led.rs` files so it compiles.)

`.gitignore`:
```
target/
```

- [ ] **Step 2: Verify it builds and tests run**

Run: `cargo test --workspace`
Expected: `running 0 tests ... test result: ok`

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "chore: scaffold cargo workspace with openpad-core"
```

---

### Task 2: State machine (`openpad-core/src/state.rs`)

**Files:**
- Create: `crates/openpad-core/src/state.rs` (tests inline in `#[cfg(test)]`)

**Interfaces:**
- Produces:
  - `pub enum AgentState { Idle, Thinking, Running, Waiting, Done, Error }`
  - `pub struct StateMachine` with `new(agents: &[&str])`, `apply(&mut self, agent: &str, state: AgentState, now_ms: u64)`, `tick(&mut self, now_ms: u64)`, `get(&self, agent: &str) -> AgentState`, `snapshot(&self) -> Vec<(String, AgentState)>` (insertion order), `entered_ms(&self, agent: &str) -> u64`.
  - `pub const DONE_DECAY_MS: u64 = 5_000;`
  - `pub fn urgency(s: AgentState) -> u8` — ERROR=5 > WAITING=4 > RUNNING=3 > THINKING=2 > DONE=1 > IDLE=0.

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_agent_is_idle() {
        let sm = StateMachine::new(&["claude"]);
        assert_eq!(sm.get("nope"), AgentState::Idle);
    }

    #[test]
    fn apply_sets_state_and_entered_time() {
        let mut sm = StateMachine::new(&["claude"]);
        sm.apply("claude", AgentState::Waiting, 1_000);
        assert_eq!(sm.get("claude"), AgentState::Waiting);
        assert_eq!(sm.entered_ms("claude"), 1_000);
    }

    #[test]
    fn done_decays_to_idle_after_5s() {
        let mut sm = StateMachine::new(&["claude"]);
        sm.apply("claude", AgentState::Done, 1_000);
        sm.tick(5_999);
        assert_eq!(sm.get("claude"), AgentState::Done);
        sm.tick(6_001);
        assert_eq!(sm.get("claude"), AgentState::Idle);
    }

    #[test]
    fn error_does_not_decay() {
        let mut sm = StateMachine::new(&["claude"]);
        sm.apply("claude", AgentState::Error, 0);
        sm.tick(60_000);
        assert_eq!(sm.get("claude"), AgentState::Error);
    }

    #[test]
    fn snapshot_preserves_insertion_order() {
        let sm = StateMachine::new(&["claude", "codex", "kimi"]);
        let names: Vec<_> = sm.snapshot().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["claude", "codex", "kimi"]);
    }

    #[test]
    fn urgency_ordering() {
        assert!(urgency(AgentState::Error) > urgency(AgentState::Waiting));
        assert!(urgency(AgentState::Waiting) > urgency(AgentState::Running));
        assert!(urgency(AgentState::Running) > urgency(AgentState::Thinking));
        assert!(urgency(AgentState::Thinking) > urgency(AgentState::Done));
        assert!(urgency(AgentState::Done) > urgency(AgentState::Idle));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p openpad-core`
Expected: FAIL (types not defined)

- [ ] **Step 3: Implement**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState { Idle, Thinking, Running, Waiting, Done, Error }

pub const DONE_DECAY_MS: u64 = 5_000;

pub fn urgency(s: AgentState) -> u8 {
    match s {
        AgentState::Error => 5,
        AgentState::Waiting => 4,
        AgentState::Running => 3,
        AgentState::Thinking => 2,
        AgentState::Done => 1,
        AgentState::Idle => 0,
    }
}

struct Slot { name: String, state: AgentState, entered_ms: u64 }

pub struct StateMachine { slots: Vec<Slot> }

impl StateMachine {
    pub fn new(agents: &[&str]) -> Self {
        Self { slots: agents.iter().map(|a| Slot {
            name: a.to_string(), state: AgentState::Idle, entered_ms: 0 }).collect() }
    }
    fn slot_mut(&mut self, agent: &str) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|s| s.name == agent)
    }
    pub fn apply(&mut self, agent: &str, state: AgentState, now_ms: u64) {
        if let Some(s) = self.slot_mut(agent) { s.state = state; s.entered_ms = now_ms; }
    }
    pub fn tick(&mut self, now_ms: u64) {
        for s in &mut self.slots {
            if s.state == AgentState::Done && now_ms.saturating_sub(s.entered_ms) > DONE_DECAY_MS {
                s.state = AgentState::Idle;
                s.entered_ms = now_ms;
            }
        }
    }
    pub fn get(&self, agent: &str) -> AgentState {
        self.slots.iter().find(|s| s.name == agent).map(|s| s.state).unwrap_or(AgentState::Idle)
    }
    pub fn entered_ms(&self, agent: &str) -> u64 {
        self.slots.iter().find(|s| s.name == agent).map(|s| s.entered_ms).unwrap_or(0)
    }
    pub fn snapshot(&self) -> Vec<(String, AgentState)> {
        self.slots.iter().map(|s| (s.name.clone(), s.state)).collect()
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p openpad-core`
Expected: all PASS

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(core): agent state machine with done-decay and urgency ordering"
```

---

### Task 3: Adapter profiles (`openpad-core/src/adapter.rs` + `adapters/*.toml`)

**Files:**
- Create: `crates/openpad-core/src/adapter.rs`, `adapters/claude.toml`, `adapters/codex.toml`, `adapters/kimi.toml`

**Interfaces:**
- Consumes: `AgentState` from Task 2.
- Produces:
  - `pub struct Adapter { pub name: String, pub actions: BTreeMap<String, String>, pub events: BTreeMap<String, AgentState>, pub fidelity: Fidelity }`
  - `pub enum Fidelity { Full, Degraded }`
  - `pub fn parse_adapter(name: &str, src: &str) -> Result<Adapter, String>`
  - `impl Adapter { pub fn keys_for(&self, action: &str) -> Option<&str>; pub fn state_for(&self, event: &str) -> Option<AgentState> }`

- [ ] **Step 1: Write failing tests** (in `adapter.rs` `#[cfg(test)]`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AgentState;

    const CLAUDE: &str = r#"
        fidelity = "full"
        [actions]
        approve = "1"
        interrupt = "Escape"
        [events]
        Notification = "WAITING"
        Stop = "DONE"
    "#;

    #[test]
    fn parses_actions_and_events() {
        let a = parse_adapter("claude", CLAUDE).unwrap();
        assert_eq!(a.name, "claude");
        assert_eq!(a.keys_for("approve"), Some("1"));
        assert_eq!(a.state_for("Notification"), Some(AgentState::Waiting));
        assert_eq!(a.state_for("Stop"), Some(AgentState::Done));
        assert!(matches!(a.fidelity, Fidelity::Full));
    }

    #[test]
    fn unknown_event_state_is_error() {
        let bad = "[events]\nX = \"NOT_A_STATE\"\n";
        assert!(parse_adapter("x", bad).is_err());
    }

    #[test]
    fn missing_fidelity_defaults_degraded() {
        let a = parse_adapter("codex", "[actions]\n[events]\n").unwrap();
        assert!(matches!(a.fidelity, Fidelity::Degraded));
    }

    #[test]
    fn shipped_adapters_parse() {
        for (name, src) in [
            ("claude", include_str!("../../../adapters/claude.toml")),
            ("codex", include_str!("../../../adapters/codex.toml")),
            ("kimi", include_str!("../../../adapters/kimi.toml")),
        ] {
            parse_adapter(name, src).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p openpad-core adapter` → FAIL

- [ ] **Step 3: Implement parser**

```rust
use std::collections::BTreeMap;
use serde::Deserialize;
use crate::state::AgentState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fidelity { Full, Degraded }

#[derive(Debug)]
pub struct Adapter {
    pub name: String,
    pub actions: BTreeMap<String, String>,
    pub events: BTreeMap<String, AgentState>,
    pub fidelity: Fidelity,
}

#[derive(Deserialize)]
struct Raw {
    fidelity: Option<String>,
    #[serde(default)] actions: BTreeMap<String, String>,
    #[serde(default)] events: BTreeMap<String, String>,
}

fn state_from(s: &str) -> Result<AgentState, String> {
    Ok(match s {
        "IDLE" => AgentState::Idle,
        "THINKING" => AgentState::Thinking,
        "RUNNING" => AgentState::Running,
        "WAITING" => AgentState::Waiting,
        "DONE" => AgentState::Done,
        "ERROR" => AgentState::Error,
        other => return Err(format!("unknown state '{other}'")),
    })
}

pub fn parse_adapter(name: &str, src: &str) -> Result<Adapter, String> {
    let raw: Raw = toml::from_str(src).map_err(|e| e.to_string())?;
    let mut events = BTreeMap::new();
    for (k, v) in raw.events { events.insert(k, state_from(&v)?); }
    let fidelity = match raw.fidelity.as_deref() {
        Some("full") => Fidelity::Full,
        _ => Fidelity::Degraded,
    };
    Ok(Adapter { name: name.into(), actions: raw.actions, events, fidelity })
}

impl Adapter {
    pub fn keys_for(&self, action: &str) -> Option<&str> {
        self.actions.get(action).map(|s| s.as_str())
    }
    pub fn state_for(&self, event: &str) -> Option<AgentState> {
        self.events.get(event).copied()
    }
}
```

- [ ] **Step 4: Write the three shipped adapters** (provisional keystrokes — Task 5 verifies)

`adapters/claude.toml`:
```toml
fidelity = "full"

[actions]
approve        = "1"
approve_always = "2"
reject         = "3"
interrupt      = "Escape"
ask            = ""            # focuses pane; typing is the user's
plan           = "S-Tab"       # tmux send-keys token for shift-tab
compact        = "/compact\n"
clear          = "/clear\n"
undo           = "Escape Escape"

[events]
SessionStart  = "IDLE"
UserPromptSubmit = "THINKING"
PreToolUse    = "RUNNING"
PostToolUse   = "RUNNING"
Notification  = "WAITING"
Stop          = "DONE"
SubagentStop  = "RUNNING"
```

`adapters/codex.toml`:
```toml
fidelity = "degraded"

[actions]
approve   = "y"
reject    = "n"
interrupt = "Escape"
compact   = "/compact\n"
clear     = "/new\n"

[events]
agent-turn-complete = "DONE"
```

`adapters/kimi.toml`:
```toml
fidelity = "degraded"

[actions]
approve   = "y"
reject    = "n"
interrupt = "Escape"

[events]
```

- [ ] **Step 5: Run tests** — `cargo test -p openpad-core` → all PASS

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(core): declarative agent adapters with claude/codex/kimi profiles"
```

---

### Task 4: Keymap + LED derivation (`keymap.rs`, `led.rs`)

**Files:**
- Create: `crates/openpad-core/src/keymap.rs`, `crates/openpad-core/src/led.rs`

**Interfaces:**
- Consumes: `AgentState`, `urgency` (Task 2).
- Produces:
  - `keymap.rs`: `pub enum Layer { Steer, Launch }`; `pub enum Action { Bind(String), Broadcast, Agent(String), Mic, LayerHold, Prompt(u8), Shell(String) }`; `pub struct Keymap` with `pub fn default_map() -> Keymap` and `pub fn action(&self, layer: Layer, key: u8) -> Option<&Action>` (key 0–15, row-major; row 1 = keys 0–3).
  - `led.rs`: `pub struct Rgb(pub u8, pub u8, pub u8)`; `pub fn color_for(s: AgentState) -> Rgb`; `pub fn waiting_level(tick_ms: u64) -> u8` (triangle wave, period 1200ms, 25–255); `pub fn derive_frame(snapshot: &[(String, AgentState)], tick_ms: u64) -> [Rgb; 16]` — keys 0..2 = agents in order, key 3 = most-urgent aggregate, keys 4–15 = dim white if any agent non-idle else off.

- [ ] **Step 1: Write failing tests**

```rust
// led.rs tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AgentState;

    fn snap(states: &[AgentState]) -> Vec<(String, AgentState)> {
        ["claude", "codex", "kimi"].iter().zip(states)
            .map(|(n, s)| (n.to_string(), *s)).collect()
    }

    #[test]
    fn only_waiting_animates() {
        for s in [AgentState::Idle, AgentState::Thinking, AgentState::Running,
                  AgentState::Done, AgentState::Error] {
            let a = derive_frame(&snap(&[s, s, s]), 0);
            let b = derive_frame(&snap(&[s, s, s]), 600);
            assert_eq!(a, b, "{s:?} must not animate");
        }
        let a = derive_frame(&snap(&[AgentState::Waiting; 3]), 0);
        let b = derive_frame(&snap(&[AgentState::Waiting; 3]), 600);
        assert_ne!(a, b, "WAITING must animate");
    }

    #[test]
    fn all_key_shows_most_urgent() {
        let f = derive_frame(&snap(&[AgentState::Done, AgentState::Error, AgentState::Running]), 0);
        assert_eq!(f[3], color_for(AgentState::Error));
    }

    #[test]
    fn waiting_level_is_periodic_triangle() {
        assert_eq!(waiting_level(0), waiting_level(1200));
        assert!(waiting_level(600) > waiting_level(0));
    }
}
```

```rust
// keymap.rs tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_map_row1_binds_agents_on_both_layers() {
        let km = Keymap::default_map();
        for layer in [Layer::Steer, Layer::Launch] {
            assert!(matches!(km.action(layer, 0), Some(Action::Bind(a)) if a == "claude"));
            assert!(matches!(km.action(layer, 3), Some(Action::Broadcast)));
        }
    }

    #[test]
    fn steer_layer_has_approve_and_mic() {
        let km = Keymap::default_map();
        assert!(matches!(km.action(Layer::Steer, 4), Some(Action::Agent(a)) if a == "approve"));
        assert!(matches!(km.action(Layer::Steer, 8), Some(Action::Mic)));
        assert!(matches!(km.action(Layer::Steer, 15), Some(Action::LayerHold)));
    }
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p openpad-core` → FAIL

- [ ] **Step 3: Implement**

```rust
// keymap.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer { Steer, Launch }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Bind(String),      // bind pad to agent + focus its pane
    Broadcast,         // bind to all agents
    Agent(String),     // adapter action name, resolved per bound agent
    Mic,               // focus bound pane, then fire Wispr hotkey
    LayerHold,
    Prompt(u8),        // send saved prompt template N
    Shell(String),     // run local command (repo picker etc.)
}

pub struct Keymap {
    steer: [Option<Action>; 16],
    launch: [Option<Action>; 16],
}

impl Keymap {
    pub fn action(&self, layer: Layer, key: u8) -> Option<&Action> {
        let arr = match layer { Layer::Steer => &self.steer, Layer::Launch => &self.launch };
        arr.get(key as usize).and_then(|a| a.as_ref())
    }

    pub fn default_map() -> Keymap {
        use Action::*;
        let row1 = |arr: &mut [Option<Action>; 16]| {
            arr[0] = Some(Bind("claude".into()));
            arr[1] = Some(Bind("codex".into()));
            arr[2] = Some(Bind("kimi".into()));
            arr[3] = Some(Broadcast);
        };
        let mut steer: [Option<Action>; 16] = Default::default();
        row1(&mut steer);
        steer[4] = Some(Agent("approve".into()));
        steer[5] = Some(Agent("approve_always".into()));
        steer[6] = Some(Agent("reject".into()));
        steer[7] = Some(Agent("interrupt".into()));
        steer[8] = Some(Mic);
        steer[9] = Some(Agent("ask".into()));
        steer[10] = Some(Agent("branch".into()));
        steer[11] = Some(Agent("undo".into()));
        steer[12] = Some(Agent("plan".into()));
        steer[13] = Some(Agent("compact".into()));
        steer[14] = Some(Agent("clear".into()));
        steer[15] = Some(LayerHold);

        let mut launch: [Option<Action>; 16] = Default::default();
        row1(&mut launch);
        launch[4] = Some(Agent("review".into()));
        launch[5] = Some(Agent("test".into()));
        launch[6] = Some(Agent("commit".into()));
        launch[7] = Some(Agent("pr".into()));
        launch[8] = Some(Prompt(1));
        launch[9] = Some(Prompt(2));
        launch[10] = Some(Prompt(3));
        launch[11] = Some(Prompt(4));
        launch[12] = Some(Shell("repo".into()));
        launch[13] = Some(Shell("worktree".into()));
        launch[14] = Some(Shell("logs".into()));
        launch[15] = Some(LayerHold);
        Keymap { steer, launch }
    }
}
```

```rust
// led.rs
use crate::state::{urgency, AgentState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub fn color_for(s: AgentState) -> Rgb {
    match s {
        AgentState::Idle => Rgb(30, 30, 30),
        AgentState::Thinking => Rgb(20, 40, 120),
        AgentState::Running => Rgb(40, 90, 255),
        AgentState::Waiting => Rgb(255, 160, 0),
        AgentState::Done => Rgb(0, 200, 80),
        AgentState::Error => Rgb(220, 30, 30),
    }
}

/// Triangle wave 25..=255, period 1200ms. Drives WAITING pulse only.
pub fn waiting_level(tick_ms: u64) -> u8 {
    let t = (tick_ms % 1200) as i64;
    let half = 600;
    let up = t <= half;
    let frac = if up { t } else { 1200 - t } as f32 / half as f32;
    (25.0 + frac * 230.0) as u8
}

fn scale(c: Rgb, level: u8) -> Rgb {
    let f = level as u16;
    Rgb(((c.0 as u16 * f) / 255) as u8,
        ((c.1 as u16 * f) / 255) as u8,
        ((c.2 as u16 * f) / 255) as u8)
}

pub fn derive_frame(snapshot: &[(String, AgentState)], tick_ms: u64) -> [Rgb; 16] {
    let mut frame = [Rgb(0, 0, 0); 16];
    let render = |s: AgentState| -> Rgb {
        if s == AgentState::Waiting { scale(color_for(s), waiting_level(tick_ms)) }
        else { color_for(s) }
    };
    for (i, (_, s)) in snapshot.iter().take(3).enumerate() { frame[i] = render(*s); }
    let worst = snapshot.iter().map(|(_, s)| *s)
        .max_by_key(|s| urgency(*s)).unwrap_or(AgentState::Idle);
    frame[3] = render(worst);
    let any_active = snapshot.iter().any(|(_, s)| *s != AgentState::Idle);
    if any_active { for k in 4..16 { frame[k] = Rgb(15, 15, 15); } }
    frame
}
```

- [ ] **Step 4: Run tests** — `cargo test -p openpad-core` → all PASS

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(core): default keymap and LED frame derivation with waiting-only motion"
```

---

### Task 5: Live-TUI verification (no code — evidence gathering)

**Files:**
- Create: `docs/verification.md`
- Modify: `adapters/claude.toml`, `adapters/codex.toml` (correct any wrong keystrokes/events)

**Interfaces:**
- Produces: verified adapter TOML values; every later task may trust them.

- [ ] **Step 1: Verify Claude Code permission prompt keys.** In a scratch repo run `claude` with default permission mode, trigger a Bash tool call (`ask it to run ls`), and record: the exact option list and which keystroke selects approve / approve-always / reject. Record in `docs/verification.md`.

- [ ] **Step 2: Verify Claude hook payloads.** Add a temporary hook `{"hooks":{"Notification":[{"hooks":[{"type":"command","command":"tee -a /tmp/openpad-hookdump.json"}]}]}}` to the scratch repo's `.claude/settings.json`; also for `PreToolUse`, `Stop`. Confirm the field name carrying the event (`hook_event_name`) and capture one sample payload per event into `docs/verification.md`.

- [ ] **Step 3: Verify Codex notify.** In `~/.codex/config.toml` set `notify = ["bash", "-c", "echo \"$1\" >> /tmp/codex-notify.log", "--"]`, run a Codex turn, record the JSON argv payload and its `type` values. Verify approval prompt keystrokes in the Codex TUI the same way as Step 1.

- [ ] **Step 4: Verify Wispr Flow hotkey.** Open Wispr Flow settings; record the configured push-to-talk hotkey and whether it can be set to a synthesizable combo (e.g. `F17` or `ctrl+alt+cmd+D`). Record hold-vs-toggle behavior.

- [ ] **Step 5: Correct adapters.** Update `adapters/*.toml` to match findings; run `cargo test -p openpad-core` (shipped-adapter parse test still passes). Note any Claude action that turns out not to exist (e.g. `branch`, `undo`) and remove or reroute it in `Keymap::default_map` if needed.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "docs: record live-TUI verification; correct adapter keystrokes"
```

---

### Task 6: Ingest server (`openpad-daemon/src/ingest.rs`)

**Files:**
- Create: `crates/openpad-daemon/Cargo.toml`, `crates/openpad-daemon/src/main.rs`, `crates/openpad-daemon/src/ingest.rs`
- Modify: root `Cargo.toml` members

**Interfaces:**
- Consumes: nothing from core yet (sends over channel).
- Produces:
  - `pub struct IngestEvent { pub agent: String, pub event: String, pub detail: Option<String> }`
  - `pub fn spawn_ingest(addr: &str, tx: std::sync::mpsc::Sender<IngestEvent>) -> std::io::Result<std::thread::JoinHandle<()>>`
  - HTTP contract: `POST /event?agent=<name>` with a JSON body; the server extracts `hook_event_name` (Claude) else `type` (Codex) else top-level `event` as the event string; `detail` = `tool_name` + compact `tool_input` when present. Responds `204`.

`crates/openpad-daemon/Cargo.toml`:
```toml
[package]
name = "openpad-daemon"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "openpad"
path = "src/main.rs"

[dependencies]
openpad-core = { path = "../openpad-core" }
tiny_http = "0.12"
serde_json = "1"
serde = { version = "1", features = ["derive"] }
toml = "0.8"
```

- [ ] **Step 1: Write failing integration test** `crates/openpad-daemon/tests/ingest.rs`

```rust
use std::sync::mpsc;

#[test]
fn post_event_lands_on_channel() {
    let (tx, rx) = mpsc::channel();
    openpad_daemon::ingest::spawn_ingest("127.0.0.1:17676", tx).unwrap();
    let body = r#"{"hook_event_name":"Notification","tool_name":"Bash","tool_input":{"command":"ls"}}"#;
    let resp = ureq::post("http://127.0.0.1:17676/event?agent=claude").send_string(body);
    assert!(resp.is_ok());
    let ev = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    assert_eq!(ev.agent, "claude");
    assert_eq!(ev.event, "Notification");
    assert!(ev.detail.as_deref().unwrap_or("").contains("Bash"));
}
```
Add to `[dev-dependencies]`: `ureq = "2"`. Expose modules: in `main.rs` add `pub mod ingest;` and make the crate a lib+bin (add `src/lib.rs` with `pub mod ingest;` and keep `main.rs` thin).

- [ ] **Step 2: Run to verify failure** — `cargo test -p openpad-daemon` → FAIL

- [ ] **Step 3: Implement**

```rust
// src/ingest.rs
use std::sync::mpsc::Sender;

pub struct IngestEvent { pub agent: String, pub event: String, pub detail: Option<String> }

pub fn spawn_ingest(addr: &str, tx: Sender<IngestEvent>) -> std::io::Result<std::thread::JoinHandle<()>> {
    let server = tiny_http::Server::http(addr)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::AddrInUse, e.to_string()))?;
    Ok(std::thread::spawn(move || {
        for mut req in server.incoming_requests() {
            let url = req.url().to_string();
            if req.method() != &tiny_http::Method::Post || !url.starts_with("/event") {
                let _ = req.respond(tiny_http::Response::empty(404));
                continue;
            }
            let agent = url.split("agent=").nth(1)
                .map(|s| s.split('&').next().unwrap_or(s).to_string())
                .unwrap_or_default();
            let mut body = String::new();
            let _ = std::io::Read::read_to_string(&mut req.as_reader(), &mut body);
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let event = v.get("hook_event_name").or_else(|| v.get("type")).or_else(|| v.get("event"))
                .and_then(|x| x.as_str()).unwrap_or("").to_string();
            let detail = v.get("tool_name").and_then(|t| t.as_str()).map(|t| {
                let input = v.get("tool_input").map(|i| i.to_string()).unwrap_or_default();
                format!("{t} {input}")
            });
            if !agent.is_empty() && !event.is_empty() {
                let _ = tx.send(IngestEvent { agent, event, detail });
            }
            let _ = req.respond(tiny_http::Response::empty(204));
        }
    }))
}
```

- [ ] **Step 4: Run tests** — `cargo test -p openpad-daemon` → PASS

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(daemon): loopback ingest server for agent hook events"
```

---

### Task 7: Hook shims + hooks install/uninstall

**Files:**
- Create: `shims/claude-hook.sh`, `shims/codex-notify.sh`, `crates/openpad-daemon/src/hooks.rs`

**Interfaces:**
- Consumes: ingest HTTP contract (Task 6).
- Produces:
  - `pub fn install_claude_hooks(settings_json: &str, shim_path: &str) -> Result<String, String>` — pure transform, appends openpad entries to `Notification`/`PreToolUse`/`Stop`/`SessionStart`/`UserPromptSubmit` arrays, idempotent.
  - `pub fn uninstall_claude_hooks(settings_json: &str) -> Result<String, String>` — removes only entries whose command contains `openpad`.
  - CLI: `openpad hooks install|uninstall` (wired in Task 9).

- [ ] **Step 1: Write the shims**

`shims/claude-hook.sh`:
```bash
#!/usr/bin/env bash
# Claude Code hook shim: forwards the hook payload from stdin to openpad.
# Never blocks or fails the agent: 1s timeout, always exit 0.
curl -s -m 1 -X POST "http://127.0.0.1:7676/event?agent=${OPENPAD_AGENT:-claude}" \
     -H 'Content-Type: application/json' --data-binary @- >/dev/null 2>&1
exit 0
```

`shims/codex-notify.sh` (fallback for older Codex versions; current Codex has stdin-JSON hooks — point its `[hooks]` config at `claude-hook.sh` pattern with `OPENPAD_AGENT=codex`, see docs/verification.md):
```bash
#!/usr/bin/env bash
# Codex notify shim: argv[1] is a JSON payload with a "type" field.
curl -s -m 1 -X POST "http://127.0.0.1:7676/event?agent=codex" \
     -H 'Content-Type: application/json' --data-binary "${1:-{}}" >/dev/null 2>&1
exit 0
```
`chmod +x shims/*.sh`

- [ ] **Step 2: Write failing tests for the JSON transforms** (uninstall first, per spec)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const EXISTING: &str = r#"{
      "permissions": {"defaultMode": "auto"},
      "hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "node /gsd.js"}]}]}
    }"#;

    #[test]
    fn uninstall_removes_only_openpad_entries() {
        let installed = install_claude_hooks(EXISTING, "/usr/local/share/openpad/claude-hook.sh").unwrap();
        let removed = uninstall_claude_hooks(&installed).unwrap();
        let orig: serde_json::Value = serde_json::from_str(EXISTING).unwrap();
        let out: serde_json::Value = serde_json::from_str(&removed).unwrap();
        assert_eq!(orig, out);
    }

    #[test]
    fn install_preserves_existing_hooks_and_is_idempotent() {
        let once = install_claude_hooks(EXISTING, "/x/openpad/claude-hook.sh").unwrap();
        let twice = install_claude_hooks(&once, "/x/openpad/claude-hook.sh").unwrap();
        assert_eq!(once, twice);
        let v: serde_json::Value = serde_json::from_str(&once).unwrap();
        let ss = &v["hooks"]["SessionStart"];
        assert!(ss.to_string().contains("gsd.js"), "must keep existing entries");
        assert!(ss.to_string().contains("openpad"));
        assert!(v["hooks"]["Notification"].to_string().contains("openpad"));
    }
}
```

- [ ] **Step 3: Run to verify failure** — `cargo test -p openpad-daemon hooks` → FAIL

- [ ] **Step 4: Implement transforms**

```rust
// src/hooks.rs
use serde_json::{json, Value};

const EVENTS: &[&str] = &["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Notification", "Stop", "SubagentStop"];

pub fn install_claude_hooks(settings_json: &str, shim_path: &str) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(settings_json).map_err(|e| e.to_string())?;
    let hooks = v.as_object_mut().ok_or("settings root must be an object")?
        .entry("hooks").or_insert_with(|| json!({}));
    for ev in EVENTS {
        let arr = hooks.as_object_mut().ok_or("hooks must be an object")?
            .entry(*ev).or_insert_with(|| json!([]));
        let arr = arr.as_array_mut().ok_or("hook event must be an array")?;
        let already = arr.iter().any(|e| e.to_string().contains("openpad"));
        if !already {
            arr.push(json!({"hooks": [{"type": "command", "command": format!("bash \"{shim_path}\""), "timeout": 3}]}));
        }
    }
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

pub fn uninstall_claude_hooks(settings_json: &str) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(settings_json).map_err(|e| e.to_string())?;
    if let Some(hooks) = v.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        let keys: Vec<String> = hooks.keys().cloned().collect();
        for k in keys {
            if let Some(arr) = hooks.get_mut(&k).and_then(|a| a.as_array_mut()) {
                arr.retain(|e| !e.to_string().contains("openpad"));
                if arr.is_empty() { hooks.remove(&k); }
            }
        }
        if hooks.is_empty() { v.as_object_mut().unwrap().remove("hooks"); }
    }
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}
```
Note: `uninstall` removing an event key that existed before install but became empty is impossible — pre-existing entries are retained, so the key only empties if openpad created it. The round-trip test enforces exactly this.

- [ ] **Step 5: Run tests** — `cargo test -p openpad-daemon` → PASS

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(daemon): hook shims and additive install/uninstall transforms"
```

---

### Task 8: HID transport (`openpad-hid`)

**Files:**
- Create: `crates/openpad-hid/Cargo.toml`, `crates/openpad-hid/src/lib.rs`
- Modify: root `Cargo.toml` members

**Interfaces:**
- Consumes: `Rgb` from `openpad-core::led`.
- Produces:
  - `pub trait PadLink: Send { fn send_frame(&mut self, frame: &[Rgb; 16]) -> Result<(), String>; }`
  - `pub struct FakePad { pub frames: Vec<[Rgb; 16]> }` implementing `PadLink` (records frames).
  - `pub struct HidPad` with `pub fn open() -> Result<HidPad, String>` (finds VID 0xD010 / PID 0x1601, usagePage 0xFF60) implementing `PadLink`.
  - `pub const VID: u16 = 0xD010; pub const PID: u16 = 0x1601;`

`crates/openpad-hid/Cargo.toml`:
```toml
[package]
name = "openpad-hid"
version = "0.1.0"
edition = "2021"

[dependencies]
openpad-core = { path = "../openpad-core" }
hidapi = "2"
```

- [ ] **Step 1: Spike — probe RGB granularity (hardware attached).** Write `crates/openpad-hid/examples/probe.rs` that opens the 0xFF60 interface and tries the VIA `custom_set_value` lighting commands (command id `0x07`, channel `id_qmk_rgb_matrix = 3`, value ids: brightness=1, effect=2, effect_speed=3, color=4):

```rust
fn main() -> Result<(), String> {
    let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    let dev = api.device_list()
        .find(|d| d.vendor_id() == 0xD010 && d.product_id() == 0x1601 && d.usage_page() == 0xFF60)
        .ok_or("pad raw-hid interface not found")?
        .open_device(&api).map_err(|e| e.to_string())?;
    // VIA report: [report_id=0x00, command, channel, value_id, data...] padded to 33 bytes
    let mut msg = [0u8; 33];
    msg[1] = 0x07; msg[2] = 3; msg[3] = 2; msg[4] = 1;        // effect = solid color
    dev.write(&msg).map_err(|e| e.to_string())?;
    let mut msg = [0u8; 33];
    msg[1] = 0x07; msg[2] = 3; msg[3] = 4; msg[4] = 28; msg[5] = 255; // color: hue=amber, sat=max
    dev.write(&msg).map_err(|e| e.to_string())?;
    println!("sent solid amber — did the pad change color?");
    Ok(())
}
```
Run: `cargo run -p openpad-hid --example probe` with the pad attached. Record in `docs/verification.md`: whether global color works, and whether any per-key path exists (VIA protocol has no standard per-key set — expected finding is **global-only**, which triggers the documented fallback: whole-pad color = bound agent's state, most-urgent aggregate when broadcast).

- [ ] **Step 2: Write failing test with FakePad**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use openpad_core::led::Rgb;

    #[test]
    fn fake_pad_records_frames() {
        let mut pad = FakePad::default();
        let frame = [Rgb(1, 2, 3); 16];
        pad.send_frame(&frame).unwrap();
        assert_eq!(pad.frames.len(), 1);
        assert_eq!(pad.frames[0][0], Rgb(1, 2, 3));
    }
}
```

- [ ] **Step 3: Run to verify failure**, then implement:

```rust
use openpad_core::led::Rgb;

pub const VID: u16 = 0xD010;
pub const PID: u16 = 0x1601;

pub trait PadLink: Send {
    fn send_frame(&mut self, frame: &[Rgb; 16]) -> Result<(), String>;
}

#[derive(Default)]
pub struct FakePad { pub frames: Vec<[Rgb; 16]> }

impl PadLink for FakePad {
    fn send_frame(&mut self, frame: &[Rgb; 16]) -> Result<(), String> {
        self.frames.push(*frame);
        Ok(())
    }
}

pub struct HidPad { dev: hidapi::HidDevice, last: Option<[Rgb; 16]> }

impl HidPad {
    pub fn open() -> Result<HidPad, String> {
        let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
        let dev = api.device_list()
            .find(|d| d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == 0xFF60)
            .ok_or("openpad: DOIO raw-hid interface not found")?
            .open_device(&api).map_err(|e| e.to_string())?;
        Ok(HidPad { dev, last: None })
    }
    fn via_set(&self, value_id: u8, data: &[u8]) -> Result<(), String> {
        let mut msg = [0u8; 33];
        msg[1] = 0x07; msg[2] = 3; msg[3] = value_id;
        msg[4..4 + data.len()].copy_from_slice(data);
        self.dev.write(&msg).map(|_| ()).map_err(|e| e.to_string())
    }
}

fn rgb_to_hs(c: Rgb) -> (u8, u8) {
    let (r, g, b) = (c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0);
    let max = r.max(g).max(b); let min = r.min(g).min(b); let d = max - min;
    let h = if d == 0.0 { 0.0 }
        else if max == r { 60.0 * (((g - b) / d) % 6.0) }
        else if max == g { 60.0 * ((b - r) / d + 2.0) }
        else { 60.0 * ((r - g) / d + 4.0) };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max == 0.0 { 0.0 } else { d / max };
    ((h / 360.0 * 255.0) as u8, (s * 255.0) as u8)
}

impl PadLink for HidPad {
    /// Global-color fallback: renders key 3 (the most-urgent aggregate) to the whole pad.
    fn send_frame(&mut self, frame: &[Rgb; 16]) -> Result<(), String> {
        if self.last.as_ref() == Some(frame) { return Ok(()); } // skip no-op writes
        let c = frame[3];
        let (h, s) = rgb_to_hs(c);
        let v = (c.0.max(c.1).max(c.2)) as u8;
        self.via_set(2, &[1])?;          // effect: solid color
        self.via_set(4, &[h, s])?;       // hue/sat
        self.via_set(1, &[v])?;          // brightness = value → carries the WAITING pulse
        self.last = Some(*frame);
        Ok(())
    }
}
```
(If Step 1 discovered a per-key path, note it in `docs/verification.md` and file a follow-up; ship global fallback regardless.)

- [ ] **Step 4: Run tests** — `cargo test -p openpad-hid` → PASS. With pad attached: `cargo run -p openpad-hid --example probe` → pad turns amber.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(hid): PadLink trait, FakePad, and VIA global-color transport"
```

---

### Task 9: Dispatcher (`openpad-dispatch`) + input listener + CLI skeleton

**Files:**
- Create: `crates/openpad-dispatch/Cargo.toml`, `crates/openpad-dispatch/src/lib.rs`, `crates/openpad-daemon/src/input.rs`
- Modify: root `Cargo.toml`, `crates/openpad-daemon/src/main.rs`, `crates/openpad-daemon/src/lib.rs`

**Interfaces:**
- Consumes: adapter `keys_for` (Task 3), `Action`/`Layer`/`Keymap` (Task 4).
- Produces:
  - `pub struct Target { pub tmux: Option<String> }` (e.g. `Some("claude:0")`; `None` = focused window)
  - `pub trait Dispatcher: Send { fn send_keys(&self, t: &Target, keys: &str) -> Result<(), String>; fn focus(&self, t: &Target) -> Result<(), String>; fn fire_hotkey(&self, combo: &str) -> Result<(), String>; }`
  - `pub struct FakeDispatcher { pub calls: std::sync::Mutex<Vec<String>> }` implementing `Dispatcher` (records `"send claude:0 1"` style strings).
  - `pub struct MacDispatcher` — tmux via `std::process::Command`; focus = `tmux select-window` + `osascript -e 'tell application "iTerm2" to activate'`; `fire_hotkey` via `osascript` System Events keystroke.
  - `input.rs`: `pub enum PhysKey { Key(openpad_core::keymap::Layer, u8), EncoderTurn(u8, i8), EncoderPush(u8) }`; `pub fn map_key(mods: Mods, code: u32) -> Option<PhysKey>`; `pub struct Mods { pub shift: bool, pub ctrl: bool, pub alt: bool }`; `pub fn spawn_listener(tx: Sender<PhysKey>)` using `rdev::listen`.
  - Physical encoding contract (matches the VIA layout in Task 10): macOS keycodes F13=105 F14=107 F15=113 F16=106 F17=64 F18=79 F19=80 F20=90. Steer keys 0–7 = plain F13–F20; steer keys 8–15 = shift+F13–F20; launch layer = same with ctrl added; encoders: alt+F13/F14 = enc1 CCW/CW, alt+F15/F16 = enc2, alt+F17/F18 = enc3; alt+F19, alt+F20, alt+shift+F13 = pushes 1–3.

- [ ] **Step 1: Write failing tests for `map_key`** (pure function — the only testable part of input)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use openpad_core::keymap::Layer;

    const NONE: Mods = Mods { shift: false, ctrl: false, alt: false };
    const SHIFT: Mods = Mods { shift: true, ctrl: false, alt: false };
    const CTRL: Mods = Mods { shift: false, ctrl: true, alt: false };
    const ALT: Mods = Mods { shift: false, ctrl: false, alt: true };

    #[test]
    fn plain_f13_is_steer_key0() {
        assert_eq!(map_key(NONE, 105), Some(PhysKey::Key(Layer::Steer, 0)));
    }
    #[test]
    fn shift_f13_is_steer_key8() {
        assert_eq!(map_key(SHIFT, 105), Some(PhysKey::Key(Layer::Steer, 8)));
    }
    #[test]
    fn ctrl_f20_is_launch_key7() {
        assert_eq!(map_key(CTRL, 90), Some(PhysKey::Key(Layer::Launch, 7)));
    }
    #[test]
    fn alt_f14_is_encoder1_cw() {
        assert_eq!(map_key(ALT, 107), Some(PhysKey::EncoderTurn(0, 1)));
    }
    #[test]
    fn unmapped_key_is_none() {
        assert_eq!(map_key(NONE, 0), None); // 'a'
    }
}
```

- [ ] **Step 2: Write failing test for dispatcher command formatting** (FakeDispatcher + a pure `tmux_args` helper)

```rust
#[test]
fn tmux_send_keys_args() {
    assert_eq!(
        tmux_args("claude:0", "Escape"),
        vec!["send-keys", "-t", "claude:0", "Escape"]
    );
    assert_eq!(
        tmux_args("claude:0", "/compact\n"),
        vec!["send-keys", "-t", "claude:0", "/compact", "Enter"]
    );
}
```

- [ ] **Step 3: Run to verify failures**, then implement:

```rust
// openpad-dispatch/src/lib.rs
use std::process::Command;
use std::sync::Mutex;

pub struct Target { pub tmux: Option<String> }

pub trait Dispatcher: Send {
    fn send_keys(&self, t: &Target, keys: &str) -> Result<(), String>;
    fn focus(&self, t: &Target) -> Result<(), String>;
    fn fire_hotkey(&self, combo: &str) -> Result<(), String>;
}

/// Translate an adapter keystroke string into tmux send-keys args.
/// Trailing '\n' becomes the tmux key name "Enter"; bare tokens pass through.
pub fn tmux_args(target: &str, keys: &str) -> Vec<String> {
    let mut out = vec!["send-keys".into(), "-t".into(), target.into()];
    if let Some(text) = keys.strip_suffix('\n') {
        out.push(text.into());
        out.push("Enter".into());
    } else {
        out.push(keys.into());
    }
    out
}

#[derive(Default)]
pub struct FakeDispatcher { pub calls: Mutex<Vec<String>> }

impl Dispatcher for FakeDispatcher {
    fn send_keys(&self, t: &Target, keys: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("send {} {}", t.tmux.as_deref().unwrap_or("focused"), keys));
        Ok(())
    }
    fn focus(&self, t: &Target) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("focus {}", t.tmux.as_deref().unwrap_or("focused")));
        Ok(())
    }
    fn fire_hotkey(&self, combo: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("hotkey {combo}"));
        Ok(())
    }
}

pub struct MacDispatcher;

impl Dispatcher for MacDispatcher {
    fn send_keys(&self, t: &Target, keys: &str) -> Result<(), String> {
        match &t.tmux {
            Some(target) => {
                let args = tmux_args(target, keys);
                let status = Command::new("tmux").args(&args).status().map_err(|e| e.to_string())?;
                if status.success() { Ok(()) } else { Err(format!("tmux exited {status}")) }
            }
            None => {
                // focused-window fallback: System Events keystroke
                let script = format!("tell application \"System Events\" to keystroke \"{}\"", keys.replace('"', "\\\""));
                Command::new("osascript").args(["-e", &script]).status().map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }
    fn focus(&self, t: &Target) -> Result<(), String> {
        if let Some(target) = &t.tmux {
            let win = target.split(':').next().unwrap_or(target);
            Command::new("tmux").args(["switch-client", "-t", win]).status().map_err(|e| e.to_string())?;
        }
        Command::new("osascript").args(["-e", "tell application \"iTerm2\" to activate"]).status().map_err(|e| e.to_string())?;
        Ok(())
    }
    fn fire_hotkey(&self, combo: &str) -> Result<(), String> {
        // combo like "key code 64 using {control down, option down}"; exact value comes
        // from config (wispr_hotkey_osascript), recorded during Task 5 verification.
        let script = format!("tell application \"System Events\" to {combo}");
        Command::new("osascript").args(["-e", &script]).status().map_err(|e| e.to_string())?;
        Ok(())
    }
}
```

```rust
// openpad-daemon/src/input.rs
use openpad_core::keymap::Layer;
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mods { pub shift: bool, pub ctrl: bool, pub alt: bool }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysKey {
    Key(Layer, u8),
    EncoderTurn(u8, i8), // (encoder 0..2, -1 ccw / +1 cw)
    EncoderPush(u8),
}

const FKEYS: [u32; 8] = [105, 107, 113, 106, 64, 79, 80, 90]; // F13..F20 mac keycodes

pub fn map_key(mods: Mods, code: u32) -> Option<PhysKey> {
    let idx = FKEYS.iter().position(|&c| c == code)? as u8;
    if mods.alt {
        return Some(match (idx, mods.shift) {
            (0, false) => PhysKey::EncoderTurn(0, -1),
            (1, false) => PhysKey::EncoderTurn(0, 1),
            (2, false) => PhysKey::EncoderTurn(1, -1),
            (3, false) => PhysKey::EncoderTurn(1, 1),
            (4, false) => PhysKey::EncoderTurn(2, -1),
            (5, false) => PhysKey::EncoderTurn(2, 1),
            (6, false) => PhysKey::EncoderPush(0),
            (7, false) => PhysKey::EncoderPush(1),
            (0, true) => PhysKey::EncoderPush(2),
            _ => return None,
        });
    }
    let layer = if mods.ctrl { Layer::Launch } else { Layer::Steer };
    let key = if mods.shift { idx + 8 } else { idx };
    Some(PhysKey::Key(layer, key))
}

pub fn spawn_listener(tx: Sender<PhysKey>) {
    std::thread::spawn(move || {
        let mods = std::sync::Arc::new(std::sync::Mutex::new(Mods { shift: false, ctrl: false, alt: false }));
        let m = mods.clone();
        let _ = rdev::listen(move |ev| {
            use rdev::{EventType, Key};
            let mut mods = m.lock().unwrap();
            match ev.event_type {
                EventType::KeyPress(Key::ShiftLeft | Key::ShiftRight) => mods.shift = true,
                EventType::KeyRelease(Key::ShiftLeft | Key::ShiftRight) => mods.shift = false,
                EventType::KeyPress(Key::ControlLeft | Key::ControlRight) => mods.ctrl = true,
                EventType::KeyRelease(Key::ControlLeft | Key::ControlRight) => mods.ctrl = false,
                EventType::KeyPress(Key::Alt | Key::AltGr) => mods.alt = true,
                EventType::KeyRelease(Key::Alt | Key::AltGr) => mods.alt = false,
                EventType::KeyPress(k) => {
                    if let Some(code) = rdev_code(k) {
                        if let Some(pk) = map_key(*mods, code) { let _ = tx.send(pk); }
                    }
                }
                _ => {}
            }
        });
    });
}

fn rdev_code(k: rdev::Key) -> Option<u32> {
    // rdev reports F13+ as Unknown(mac keycode) on macOS
    match k {
        rdev::Key::Unknown(c) => Some(c),
        _ => None,
    }
}
```
Add `rdev = "0.5"` to `openpad-daemon` dependencies and `openpad-dispatch = { path = "../openpad-dispatch" }`. Add `pub mod input;` to lib.

**Manual check** (needs Accessibility permission granted to the terminal): `openpad listen` subcommand (add to `main.rs`) that prints each `PhysKey` received — press pad keys and confirm mapping. If `rdev` fails to surface F13+ as `Unknown(code)`, fall back to a CGEventTap via the `core-graphics` crate — but verify with `listen` first before adding that dependency.

- [ ] **Step 4: Run tests** — `cargo test --workspace` → PASS

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: dispatcher with tmux/focus paths and pad input mapping"
```

---

### Task 10: Config + run loop + `openpad run`

**Files:**
- Create: `crates/openpad-daemon/src/config.rs`, `crates/openpad-daemon/src/runloop.rs`
- Modify: `crates/openpad-daemon/src/main.rs`, `crates/openpad-daemon/src/lib.rs`

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `config.rs`: `pub struct Config { pub agents: Vec<AgentCfg>, pub prompts: BTreeMap<u8, String>, pub wispr_hotkey_osascript: String, pub ingest_addr: String }`; `pub struct AgentCfg { pub name: String, pub adapter: String, pub tmux: Option<String> }`; `pub fn load(path: &Path) -> Result<Config, String>`; `pub fn default_toml() -> &'static str`; default path `~/.config/openpad/config.toml`, written on first run.
  - `runloop.rs`: `pub struct Engine<D: Dispatcher, P: PadLink>` with `pub fn new(cfg, adapters, dispatcher, pad) -> Engine`, `pub fn on_key(&mut self, k: PhysKey)`, `pub fn on_ingest(&mut self, ev: IngestEvent, now_ms: u64)`, `pub fn on_tick(&mut self, now_ms: u64)` — owns `StateMachine`, `Keymap`, bound-agent index, broadcast flag. Fully testable with fakes.

Default `config.toml` content (embedded via `default_toml()`):
```toml
ingest_addr = "127.0.0.1:7676"
# osascript fragment fired for the Mic key; set to match Wispr Flow's push-to-talk
# hotkey (see docs/verification.md, Task 5 Step 4)
wispr_hotkey_osascript = "key code 41 using {option down}"  # Option+; — verified PTT binding on this machine

[[agents]]
name = "claude"
adapter = "claude"
tmux = "claude:0"

[[agents]]
name = "codex"
adapter = "codex"
tmux = "codex:0"

[[agents]]
name = "kimi"
adapter = "kimi"
tmux = "kimi:0"

[prompts]
1 = "Summarize the current state of this task and what remains."
2 = "Run the test suite and fix any failures."
3 = "Review the last diff for bugs before I commit."
4 = "Continue."
```

- [ ] **Step 1: Write failing Engine tests** (`crates/openpad-daemon/tests/engine.rs`)

```rust
use openpad_daemon::runloop::Engine;
use openpad_daemon::input::PhysKey;
use openpad_daemon::ingest::IngestEvent;
use openpad_core::keymap::Layer;
use openpad_core::state::AgentState;
use openpad_dispatch::FakeDispatcher;
use openpad_hid::FakePad;

fn engine() -> Engine<FakeDispatcher, FakePad> {
    Engine::test_fixture() // helper: default config + shipped adapters + fakes
}

#[test]
fn approve_dispatches_bound_agent_keystroke() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 0));      // bind claude
    e.on_key(PhysKey::Key(Layer::Steer, 4));      // approve
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c == "focus claude:0"));
    assert!(calls.iter().any(|c| c.starts_with("send claude:0 ")), "approve keystroke sent");
}

#[test]
fn broadcast_sends_to_all_agents_with_action() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 3));      // All
    e.on_key(PhysKey::Key(Layer::Steer, 7));      // interrupt (all three adapters define it)
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    for t in ["claude:0", "codex:0", "kimi:0"] {
        assert!(calls.iter().any(|c| c.contains(&format!("send {t}"))), "{t} missing");
    }
}

#[test]
fn ingest_waiting_pulses_pad() {
    let mut e = engine();
    e.on_ingest(IngestEvent { agent: "claude".into(), event: "Notification".into(), detail: None }, 1_000);
    e.on_tick(1_000);
    e.on_tick(1_600);
    let frames = &e.pad().frames;
    assert!(frames.len() >= 2);
    assert_ne!(frames[frames.len() - 2], frames[frames.len() - 1], "waiting must animate");
}

#[test]
fn mic_focuses_then_fires_hotkey_in_order() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 0));      // bind claude
    e.on_key(PhysKey::Key(Layer::Steer, 8));      // mic
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    let f = calls.iter().position(|c| c == "focus claude:0").unwrap();
    let h = calls.iter().position(|c| c.starts_with("hotkey ")).unwrap();
    assert!(f < h, "must focus before firing Wispr hotkey");
}

#[test]
fn unknown_action_for_agent_is_noop_not_error() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 2));      // bind kimi (no 'plan' action)
    let before = e.dispatcher().calls.lock().unwrap().len();
    e.on_key(PhysKey::Key(Layer::Steer, 12));     // plan
    let after = e.dispatcher().calls.lock().unwrap().len();
    assert_eq!(before + 1, after, "only the focus from bind; no send for missing action");
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p openpad-daemon engine` → FAIL

- [ ] **Step 3: Implement `config.rs` and `runloop.rs`**

```rust
// runloop.rs (core logic; trimmed to the decision structure)
use openpad_core::{adapter::Adapter, keymap::{Action, Keymap, Layer}, led::derive_frame, state::{AgentState, StateMachine}};
use openpad_dispatch::{Dispatcher, Target};
use openpad_hid::PadLink;
use crate::{config::Config, ingest::IngestEvent, input::PhysKey};

pub struct Engine<D: Dispatcher, P: PadLink> {
    cfg: Config,
    adapters: Vec<Adapter>,          // parallel to cfg.agents
    keymap: Keymap,
    sm: StateMachine,
    bound: usize,                    // index into cfg.agents
    broadcast: bool,
    dispatcher: D,
    pad: P,
}

impl<D: Dispatcher, P: PadLink> Engine<D, P> {
    pub fn new(cfg: Config, adapters: Vec<Adapter>, dispatcher: D, pad: P) -> Self {
        let names: Vec<&str> = cfg.agents.iter().map(|a| a.name.as_str()).collect();
        let sm = StateMachine::new(&names);
        Engine { cfg, adapters, keymap: Keymap::default_map(), sm, bound: 0, broadcast: false, dispatcher, pad }
    }

    fn target(&self, idx: usize) -> Target {
        Target { tmux: self.cfg.agents[idx].tmux.clone() }
    }

    fn send_action(&self, idx: usize, action: &str) {
        if let Some(keys) = self.adapters[idx].keys_for(action) {
            if !keys.is_empty() {
                let _ = self.dispatcher.send_keys(&self.target(idx), keys);
            }
        }
    }

    pub fn on_key(&mut self, k: PhysKey) {
        match k {
            PhysKey::Key(layer, key) => {
                let Some(action) = self.keymap.action(layer, key).cloned() else { return };
                match action {
                    Action::Bind(name) => {
                        if let Some(i) = self.cfg.agents.iter().position(|a| a.name == name) {
                            self.bound = i;
                            self.broadcast = false;
                            let _ = self.dispatcher.focus(&self.target(i));
                        }
                    }
                    Action::Broadcast => { self.broadcast = true; }
                    Action::Agent(name) => {
                        if self.broadcast {
                            for i in 0..self.cfg.agents.len() { self.send_action(i, &name); }
                        } else {
                            self.send_action(self.bound, &name);
                        }
                    }
                    Action::Mic => {
                        let _ = self.dispatcher.focus(&self.target(self.bound));
                        let _ = self.dispatcher.fire_hotkey(&self.cfg.wispr_hotkey_osascript);
                    }
                    Action::Prompt(n) => {
                        if let Some(text) = self.cfg.prompts.get(&n) {
                            let msg = format!("{text}\n");
                            let _ = self.dispatcher.send_keys(&self.target(self.bound), &msg);
                        }
                    }
                    Action::Shell(_) | Action::LayerHold => { /* layer handled on-pad; shell in later plan */ }
                }
            }
            PhysKey::EncoderTurn(1, dir) => {
                let n = self.cfg.agents.len();
                self.bound = (self.bound as i64 + dir as i64).rem_euclid(n as i64) as usize;
                self.broadcast = false;
            }
            PhysKey::EncoderPush(1) => { let _ = self.dispatcher.focus(&self.target(self.bound)); }
            _ => { /* enc 0 (scroll) and enc 2 (model tier) in later plan */ }
        }
    }

    pub fn on_ingest(&mut self, ev: IngestEvent, now_ms: u64) {
        let Some(i) = self.cfg.agents.iter().position(|a| a.name == ev.agent) else { return };
        if let Some(state) = self.adapters[i].state_for(&ev.event) {
            self.sm.apply(&ev.agent, state, now_ms);
        }
    }

    pub fn on_tick(&mut self, now_ms: u64) {
        self.sm.tick(now_ms);
        let frame = derive_frame(&self.sm.snapshot(), now_ms);
        let _ = self.pad.send_frame(&frame);
    }

    // test accessors
    pub fn dispatcher(&self) -> &D { &self.dispatcher }
    pub fn pad(&self) -> &P { &self.pad }
}
```
Add `Engine::test_fixture()` under `#[cfg(any(test, feature = "test-fixtures"))]` (enable the feature in dev-dependencies for the integration test) constructing: `Config` from `config::default_toml()`, adapters parsed from `include_str!("../../../adapters/*.toml")`, `FakeDispatcher::default()`, `FakePad::default()`.

`main.rs` subcommands: `openpad run` (spawn ingest → channel, spawn listener → channel, `HidPad::open()` with warning fallback to a `NullPad` if absent, 100ms tick loop with `Instant`-derived `now_ms`, `MacDispatcher`), `openpad listen` (Task 9), `openpad hooks install|uninstall` (Task 7 transforms applied to `~/.claude/settings.json` with a timestamped backup written first, plus Codex `notify` line printed for manual addition), `openpad doctor` (Task 11).

- [ ] **Step 4: Run tests** — `cargo test --workspace` → all PASS

- [ ] **Step 5: End-to-end smoke (pad + tmux attached).** `tmux new-session -d -s claude && tmux send-keys -t claude:0 'claude' Enter`, run `openpad hooks install`, `openpad run`, ask Claude something that triggers a permission prompt, confirm: pad turns amber and pulses; pressing key 5 (approve) approves it; pad goes blue then green. Record outcome in `docs/verification.md`.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(daemon): engine run loop wiring pad, ingest, dispatch, and RGB"
```

---

### Task 11: `openpad doctor`

**Files:**
- Create: `crates/openpad-daemon/src/doctor.rs`
- Modify: `main.rs`, `lib.rs`

**Interfaces:**
- Consumes: `openpad_hid::{VID, PID}`, config loading, hooks transforms.
- Produces: `pub struct Check { pub name: String, pub ok: bool, pub hint: String }`; `pub fn run_checks(settings_json: Option<&str>, hid_present: bool, tmux_ok: bool, port_free: bool) -> Vec<Check>` (pure — inputs gathered by the CLI wrapper); CLI prints ✓/✗ per check with hint, exit 1 if any failed.

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_good_yields_all_ok() {
        let checks = run_checks(Some(r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"bash /x/openpad/claude-hook.sh"}]}]}}"#), true, true, true);
        assert!(checks.iter().all(|c| c.ok));
    }

    #[test]
    fn missing_pad_and_hooks_reported_with_hints() {
        let checks = run_checks(Some("{}"), false, true, true);
        let pad = checks.iter().find(|c| c.name == "pad").unwrap();
        assert!(!pad.ok && pad.hint.contains("USB"));
        let hooks = checks.iter().find(|c| c.name == "claude-hooks").unwrap();
        assert!(!hooks.ok && hooks.hint.contains("openpad hooks install"));
    }
}
```

- [ ] **Step 2: Run to verify failure**, then implement:

```rust
pub struct Check { pub name: String, pub ok: bool, pub hint: String }

fn check(name: &str, ok: bool, hint: &str) -> Check {
    Check { name: name.into(), ok, hint: hint.into() }
}

pub fn run_checks(settings_json: Option<&str>, hid_present: bool, tmux_ok: bool, port_free: bool) -> Vec<Check> {
    let hooks_ok = settings_json.map(|s| s.contains("openpad")).unwrap_or(false);
    vec![
        check("pad", hid_present, "DOIO KB16-01 not found on USB. Plug it in (direct port, not a hub) and re-run."),
        check("tmux", tmux_ok, "tmux server not reachable. Start your agent sessions: tmux new -s claude"),
        check("ingest-port", port_free, "127.0.0.1:7676 already in use — is another openpad running?"),
        check("claude-hooks", hooks_ok, "Claude hooks not installed. Run: openpad hooks install"),
    ]
}
```
CLI wrapper in `main.rs` gathers inputs: hidapi enumeration for the pad, `tmux has-session` exit status, `TcpListener::bind("127.0.0.1:7676")` for the port (skip when the daemon itself is running), read `~/.claude/settings.json`.

- [ ] **Step 3: Run tests** — `cargo test --workspace` → PASS. Manual: `openpad doctor` prints a sensible report.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(daemon): doctor with actionable per-check hints"
```

---

### Task 12: VIA layout + README

**Files:**
- Create: `layouts/kb16-via.json`, `layouts/README.md`, `README.md`

- [ ] **Step 1: Build the layout in VIA.app** (pad attached): assign per the Task 9 physical encoding contract — layer 0: keys 1–8 = F13–F20, keys 9–16 = S(F13)–S(F20); layer 1 (activated by pad-side `MO(1)` on key 16 — replaces S(F20) there; key 16 layer 1 = `TG(1)` for lock): C(F13)–C(F20) and C(S(F13))–C(S(F19)); encoders: A(F13)/A(F14), A(F15)/A(F16), A(F17)/A(F18) for turns, A(F19), A(F20), A(S(F13)) for pushes. Export via VIA's "Save current layout" → `layouts/kb16-via.json`.
  Note: key 16 carries `MO(1)`/`TG(1)` on-pad, so the daemon never sees LayerHold — `Action::LayerHold` remains a no-op by design.

- [ ] **Step 2: Write `layouts/README.md`** — import instructions (VIA.app → Design tab → Load draft definition if needed → Layers → Load saved layout), plus the full key/legend table for both layers.

- [ ] **Step 3: Write root `README.md`** — what openpad is, hardware supported, quickstart (`cargo install` path for now, VIA import, `openpad hooks install`, tmux session naming convention, `openpad run`, `openpad doctor`), state/color legend, Codex fidelity caveat, uninstall.

- [ ] **Step 4: Manual verification** — with the layout imported and daemon running, every physical key produces its expected `PhysKey` in `openpad listen`. Record in `docs/verification.md`.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "docs: VIA layout export, import guide, and project README"
```

---

## Deferred to Plan 2 (UI) and Plan 3 (packaging)

- Tauri HUD (legend overlay + pending-tool-call display), menu bar item, config window (local-only requirement carries over), transcript scroll encoder, model-tier encoder, `Action::Shell` implementations (repo/worktree/logs), WAITING `detail` surfacing (ingest already captures it), per-key RGB if the Task 8 spike found a path, brew formula + signing.

## Self-Review Notes

- Spec coverage: steering ✓ (Task 10), launch layer ✓ (keymap + prompts; Shell deferred, noted), state/LED ✓ (Tasks 2/4/8), voice ✓ (Mic action, Task 10), adapters ✓ (Task 3), additive hooks ✓ (Task 7), doctor ✓ (Task 11), VIA layout ✓ (Task 12), HUD/config UI → Plan 2 by scope decision.
- Motion-rule contradiction in spec (THINKING breathe) resolved in Global Constraints: WAITING-only motion.
- Type consistency: `PhysKey`/`Mods` (Task 9) consumed by Engine (Task 10); `IngestEvent` (Task 6) consumed by Engine; `FakeDispatcher.calls` string format matches engine tests; `derive_frame` signature identical in Tasks 4 and 10.
