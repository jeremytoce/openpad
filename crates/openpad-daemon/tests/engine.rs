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
