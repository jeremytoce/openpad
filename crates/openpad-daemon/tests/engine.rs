use openpad_daemon::runloop::Engine;
use openpad_daemon::input::PhysKey;
use openpad_daemon::ingest::IngestEvent;
use openpad_core::keymap::Layer;
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
    e.on_ingest(IngestEvent { agent: "claude".into(), event: "Notification".into(), detail: None, pane: None }, 1_000);
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
    assert_eq!(before, after, "missing adapter action must be a complete no-op");
}

#[test]
fn empty_action_is_noop() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 0));      // bind claude
    let before = e.dispatcher().calls.lock().unwrap().len();
    e.on_key(PhysKey::Key(Layer::Steer, 9));      // ask (claude maps this to "")
    let after = e.dispatcher().calls.lock().unwrap().len();
    assert_eq!(before, after, "empty-string adapter action must be a complete no-op");
}

#[test]
fn prompt_uses_literal_text_path() {
    let mut e = openpad_daemon::runloop::Engine::test_fixture();
    e.on_key(openpad_daemon::input::PhysKey::Key(openpad_core::keymap::Layer::Steer, 0)); // bind claude
    e.on_key(openpad_daemon::input::PhysKey::Key(openpad_core::keymap::Layer::Launch, 8)); // Prompt 1
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    let text_call = calls.iter().find(|c| c.starts_with("text claude:0 "));
    assert!(text_call.is_some(), "prompt must go through send_text, got: {calls:?}");
    assert!(text_call.unwrap().contains(' '), "prompt text keeps its spaces");
    assert!(!calls.iter().any(|c| c.starts_with("send claude:0 Summarize")),
        "prompt must not go through the key-token path");
}

#[test]
fn hook_events_teach_the_daemon_where_claude_lives() {
    use openpad_daemon::ingest::IngestEvent;
    use openpad_daemon::input::PhysKey;
    use openpad_core::keymap::Layer;
    let mut e = openpad_daemon::runloop::Engine::test_fixture();
    // a claude session in tmux pane %7 fires SessionStart via the shim
    e.on_ingest(IngestEvent {
        agent: "claude".into(), event: "SessionStart".into(), detail: None,
        pane: Some("%7".into()),
    }, 0);
    e.on_key(PhysKey::Key(Layer::Steer, 0)); // bind claude
    e.on_key(PhysKey::Key(Layer::Steer, 4)); // approve
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c.starts_with("send %7 ")),
        "dispatch must target the discovered pane, got: {calls:?}");
    // an event WITHOUT a pane (e.g. an IDE session) must not clear it
    e.on_ingest(IngestEvent {
        agent: "claude".into(), event: "Stop".into(), detail: None, pane: None,
    }, 1_000);
    e.on_key(PhysKey::Key(Layer::Steer, 4));
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().filter(|c| c.starts_with("send %7 ")).count() >= 2,
        "learned pane must survive pane-less events");
}
