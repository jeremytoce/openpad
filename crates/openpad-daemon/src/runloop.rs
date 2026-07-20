use openpad_core::{adapter::Adapter, keymap::{Action, Keymap}, led::derive_frame, state::StateMachine};
use openpad_dispatch::{Dispatcher, Target};
use openpad_hid::PadLink;
use crate::{config::Config, ingest::IngestEvent, input::PhysKey};

pub struct Engine<D: Dispatcher, P: PadLink> {
    cfg: Config,
    adapters: Vec<Adapter>, // parallel to cfg.agents
    keymap: Keymap,
    sm: StateMachine,
    bound: usize, // index into cfg.agents
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
        // Steering actions must reach the bound agent's pane without stealing
        // window focus. Focus jumps happen only on explicit Bind (row-1 keys),
        // EncoderPush(1), and Mic (focus-then-dictate is deliberate) -- never
        // here.
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
                    Action::Broadcast => {
                        self.broadcast = true;
                    }
                    Action::Agent(name) => {
                        if self.broadcast {
                            for i in 0..self.cfg.agents.len() {
                                self.send_action(i, &name);
                            }
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
                            // literal-text path: spaces must not be parsed as key names
                            let msg = format!("{text}\n");
                            let _ = self.dispatcher.send_text(&self.target(self.bound), &msg);
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
            PhysKey::EncoderPush(1) => {
                let _ = self.dispatcher.focus(&self.target(self.bound));
            }
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
