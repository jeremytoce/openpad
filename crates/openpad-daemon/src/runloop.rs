use openpad_core::{adapter::Adapter, keymap::{Action, Keymap}, led::derive_frame, state::StateMachine};
use openpad_dispatch::{Dispatcher, Target};
use openpad_hid::PadLink;
use crate::{config::Config, ingest::IngestEvent, input::PhysKey};

pub struct Engine<D: Dispatcher, P: PadLink> {
    cfg: Config,
    adapters: Vec<Adapter>, // parallel to cfg.agents
    keymap: Keymap,
    sm: StateMachine,
    selected: usize, // encoder-selected agent for goto (dispatch never uses it)
    dispatcher: D,
    pad: P,
}

impl<D: Dispatcher, P: PadLink> Engine<D, P> {
    pub fn new(cfg: Config, adapters: Vec<Adapter>, dispatcher: D, pad: P) -> Self {
        let names: Vec<&str> = cfg.agents.iter().map(|a| a.name.as_str()).collect();
        let sm = StateMachine::new(&names);
        Engine { cfg, adapters, keymap: Keymap::default_map(), sm, selected: 0, dispatcher, pad }
    }

    /// Steering always acts on the focused window. The safety property is
    /// visual: you approve what you are looking at. (Spec revision 2.)
    fn focused() -> Target {
        Target { tmux: None }
    }

    fn pane_target(&self, idx: usize) -> Target {
        Target { tmux: self.cfg.agents[idx].tmux.clone() }
    }

    /// Which agent is the user looking at? Exact match of the focused
    /// terminal's active tmux pane against hook-discovered panes; else a
    /// window-title substring match; else the encoder-selected agent.
    fn focused_agent(&self) -> usize {
        let ctx = self.dispatcher.focused_context();
        if let Some(pane) = &ctx.pane {
            if let Some(i) = self
                .cfg
                .agents
                .iter()
                .position(|a| a.tmux.as_deref() == Some(pane.as_str()))
            {
                return i;
            }
        }
        if let Some(title) = &ctx.title {
            let t = title.to_lowercase();
            if let Some(i) = self.cfg.agents.iter().position(|a| t.contains(&a.name.to_lowercase())) {
                return i;
            }
        }
        self.selected
    }

    /// Adapter keystrokes for the focused window, using the focused window's
    /// own agent profile. If that agent's adapter doesn't define the verb,
    /// do nothing: sending another agent's keystroke into this window would
    /// be exactly the wrong-target bug the focused model exists to prevent.
    fn send_action_focused(&self, action: &str) {
        let idx = self.focused_agent();
        if let Some(keys) = self.adapters[idx].keys_for(action) {
            if !keys.is_empty() {
                let _ = self.dispatcher.send_keys(&Self::focused(), keys);
            }
        }
    }

    pub fn on_key(&mut self, k: PhysKey) {
        match k {
            PhysKey::Key(layer, key) => {
                let Some(action) = self.keymap.action(layer, key).cloned() else { return };
                match action {
                    Action::Goto(name) => {
                        if let Some(i) = self.cfg.agents.iter().position(|a| a.name == name) {
                            self.selected = i;
                            if self.cfg.agents[i].tmux.is_some() {
                                let _ = self.dispatcher.focus(&self.pane_target(i));
                            }
                        }
                    }
                    Action::GotoWaiting => {
                        // jump to the blocked (or errored) agent with a known
                        // pane; no-op when nobody actually needs the user
                        use openpad_core::state::{urgency, AgentState};
                        let snapshot = self.sm.snapshot();
                        let target = snapshot
                            .iter()
                            .enumerate()
                            .filter(|(i, (_, s))| {
                                self.cfg.agents[*i].tmux.is_some()
                                    && urgency(*s) >= urgency(AgentState::Waiting)
                            })
                            .max_by_key(|(_, (_, s))| urgency(*s));
                        if let Some((i, _)) = target {
                            self.selected = i;
                            let _ = self.dispatcher.focus(&self.pane_target(i));
                        }
                    }
                    Action::Agent(name) => self.send_action_focused(&name),
                    Action::Mic => {
                        // fires into the focused window, like everything else
                        let _ = self.dispatcher.fire_hotkey(&self.cfg.wispr_hotkey_osascript);
                    }
                    Action::Prompt(n) => {
                        if let Some(text) = self.cfg.prompts.get(&n) {
                            // literal-text path: spaces must not be parsed as key names
                            let msg = format!("{text}\n");
                            let _ = self.dispatcher.send_text(&Self::focused(), &msg);
                        }
                    }
                    Action::Shell(_) | Action::LayerHold => { /* layer handled on-pad; shell in later plan */ }
                }
            }
            PhysKey::EncoderTurn(1, dir) => {
                let n = self.cfg.agents.len();
                self.selected = (self.selected as i64 + dir as i64).rem_euclid(n as i64) as usize;
            }
            PhysKey::EncoderPush(1) => {
                if self.cfg.agents[self.selected].tmux.is_some() {
                    let _ = self.dispatcher.focus(&self.pane_target(self.selected));
                }
            }
            _ => { /* enc 0 (scroll) and enc 2 (model tier) in later plan */ }
        }
    }

    pub fn on_ingest(&mut self, ev: IngestEvent, now_ms: u64) {
        let Some(i) = self.cfg.agents.iter().position(|a| a.name == ev.agent) else { return };
        // Pane self-discovery: hook events announce the agent's live tmux
        // pane ($TMUX_PANE via the shim). This overrides any configured
        // static target, so no session-naming convention is needed. Events
        // without a pane (agent not in tmux) never clear a learned target.
        if let Some(pane) = &ev.pane {
            if self.cfg.agents[i].tmux.as_deref() != Some(pane.as_str()) {
                self.cfg.agents[i].tmux = Some(pane.clone());
            }
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
    /// Default config + shipped adapters + fakes, for tests exercising the Engine
    /// decision logic without touching real hardware, tmux, or osascript.
    pub fn test_fixture() -> Self {
        let cfg = crate::config::parse(crate::config::default_toml())
            .expect("default_toml() must parse");
        let adapters = vec![
            openpad_core::adapter::parse_adapter("claude", include_str!("../../../adapters/claude.toml")).unwrap(),
            openpad_core::adapter::parse_adapter("codex", include_str!("../../../adapters/codex.toml")).unwrap(),
            openpad_core::adapter::parse_adapter("kimi", include_str!("../../../adapters/kimi.toml")).unwrap(),
        ];
        Engine::new(cfg, adapters, openpad_dispatch::FakeDispatcher::default(), openpad_hid::FakePad::default())
    }
}
