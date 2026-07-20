use crate::{config::Config, ingest::IngestEvent, input::PhysKey};
use openpad_core::{adapter::Adapter, keymap::{Action, Keymap, Layer}, led::derive_frame, state::StateMachine};
use openpad_dispatch::Dispatcher;
use openpad_hid::PadLink;

pub struct Engine<D: Dispatcher, P: PadLink> {
    cfg: Config,
    adapters: Vec<Adapter>,          // parallel to cfg.agents
    panes: Vec<Option<String>>,      // hook-discovered tmux panes, parallel to cfg.agents
    keymap: Keymap,
    sm: StateMachine,
    layer_lock: bool,                // software Launch-layer lock (encoder 2 push)
    dispatcher: D,
    pad: P,
}

impl<D: Dispatcher, P: PadLink> Engine<D, P> {
    pub fn new(cfg: Config, adapters: Vec<Adapter>, dispatcher: D, pad: P) -> Self {
        let names: Vec<&str> = cfg.agents.iter().map(|a| a.name.as_str()).collect();
        let sm = StateMachine::new(&names);
        let panes = vec![None; cfg.agents.len()];
        Engine { cfg, adapters, panes, keymap: Keymap::default_map(), sm, layer_lock: false, dispatcher, pad }
    }

    /// Which agent is the user looking at? Exact match of the focused
    /// terminal's active tmux pane against hook-discovered panes; else a
    /// window-title substring match; else the first configured agent.
    fn focused_agent(&self) -> usize {
        let ctx = self.dispatcher.focused_context();
        if let Some(pane) = &ctx.pane {
            if let Some(i) = self.panes.iter().position(|p| p.as_deref() == Some(pane.as_str())) {
                return i;
            }
        }
        if let Some(title) = &ctx.title {
            let t = title.to_lowercase();
            if let Some(i) = self.cfg.agents.iter().position(|a| t.contains(&a.name.to_lowercase())) {
                return i;
            }
        }
        0
    }

    /// Adapter keystrokes for the focused window, using the focused window's
    /// own agent profile. If that agent's adapter doesn't define the verb,
    /// do nothing: sending another agent's keystroke into this window would
    /// be exactly the wrong-target bug the focused model exists to prevent.
    fn send_action(&self, action: &str) {
        if let Some(keys) = self.adapters[self.focused_agent()].keys_for(action) {
            if !keys.is_empty() {
                let _ = self.dispatcher.send_keys(keys);
            }
        }
    }

    pub fn on_key(&mut self, k: PhysKey) {
        match k {
            PhysKey::Key(layer, key) => {
                // Software layer lock: knob-toggled Launch without touching
                // firmware. Firmware-layer (held key 16) events arrive as
                // Launch already and are unaffected.
                let layer = if self.layer_lock { Layer::Launch } else { layer };
                let Some(action) = self.keymap.action(layer, key).cloned() else { return };
                match action {
                    Action::GotoWaiting => {
                        // Jump to the blocked (or errored) agent with a known
                        // pane; no-op when nobody actually needs the user.
                        use openpad_core::state::{urgency, AgentState};
                        let target = self
                            .sm
                            .snapshot()
                            .iter()
                            .enumerate()
                            .filter(|(i, (_, s))| {
                                self.panes[*i].is_some() && urgency(*s) >= urgency(AgentState::Waiting)
                            })
                            .max_by_key(|(_, (_, s))| urgency(*s))
                            .map(|(i, _)| i);
                        if let Some(i) = target {
                            let _ = self.dispatcher.focus_pane(self.panes[i].as_deref().unwrap());
                        }
                    }
                    Action::Agent(name) => self.send_action(&name),
                    Action::Text(text) => {
                        let _ = self.dispatcher.send_text(&text);
                    }
                    Action::Mic => {
                        let _ = self.dispatcher.fire_hotkey(&self.cfg.wispr_hotkey_osascript);
                    }
                    Action::Prompt(n) => {
                        if let Some(text) = self.cfg.prompts.get(&n) {
                            let _ = self.dispatcher.send_text(&format!("{text}\n"));
                        }
                    }
                }
            }
            PhysKey::EncoderTurn(0, dir) => {
                // Menu knob: TUI dialogs (permission options, rewind, model
                // picker) become knob-navigable. CW = down.
                let _ = self.dispatcher.send_keys(if dir > 0 { "Down" } else { "Up" });
            }
            PhysKey::EncoderPush(0) => {
                let _ = self.dispatcher.send_keys("Enter");
            }
            PhysKey::EncoderPush(1) => {
                self.layer_lock = !self.layer_lock;
            }
            _ => { /* encoder 3 and remaining turns: reserved (Plan 2) */ }
        }
    }

    pub fn on_ingest(&mut self, ev: IngestEvent, now_ms: u64) {
        let Some(i) = self.cfg.agents.iter().position(|a| a.name == ev.agent) else { return };
        // Pane self-discovery: hook events announce the agent's live tmux
        // pane ($TMUX_PANE via the shim). Events without a pane (agent not
        // in tmux) never clear a learned pane.
        if ev.pane.is_some() {
            self.panes[i] = ev.pane;
        }
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
    pub fn dispatcher(&self) -> &D {
        &self.dispatcher
    }
    pub fn pad(&self) -> &P {
        &self.pad
    }
}

#[cfg(any(test, feature = "test-fixtures"))]
impl Engine<openpad_dispatch::FakeDispatcher, openpad_hid::FakePad> {
    pub fn test_fixture() -> Self {
        let cfg = crate::config::parse(crate::config::default_toml()).unwrap();
        let adapters = vec![
            openpad_core::adapter::parse_adapter("claude", include_str!("../../../adapters/claude.toml")).unwrap(),
            openpad_core::adapter::parse_adapter("codex", include_str!("../../../adapters/codex.toml")).unwrap(),
            openpad_core::adapter::parse_adapter("kimi", include_str!("../../../adapters/kimi.toml")).unwrap(),
        ];
        Engine::new(cfg, adapters, openpad_dispatch::FakeDispatcher::default(), openpad_hid::FakePad::default())
    }
}
