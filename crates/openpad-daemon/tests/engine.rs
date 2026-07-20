use openpad_core::keymap::Layer;
use openpad_daemon::ingest::IngestEvent;
use openpad_daemon::input::PhysKey;
use openpad_daemon::runloop::Engine;
use openpad_dispatch::FakeDispatcher;
use openpad_hid::FakePad;

fn engine() -> Engine<FakeDispatcher, FakePad> {
    Engine::test_fixture() // helper: default config + shipped adapters + fakes
}

fn ev(agent: &str, event: &str, pane: Option<&str>) -> IngestEvent {
    IngestEvent {
        agent: agent.into(),
        event: event.into(),
        detail: None,
        pane: pane.map(String::from),
    }
}

// Spec revision 2: steering acts on the focused window. The safety property
// is visual; approving requires looking at the target.

#[test]
fn approve_synthesizes_into_focused_window() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 4)); // approve
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(
        calls.iter().any(|c| c.starts_with("send focused ")),
        "approve must target the focused window, got: {calls:?}"
    );
    assert!(
        !calls.iter().any(|c| c.starts_with("focus ")),
        "steering must never move focus"
    );
}

#[test]
fn goto_key_focuses_discovered_pane() {
    let mut e = engine();
    e.on_ingest(ev("claude", "SessionStart", Some("%7")), 0);
    e.on_key(PhysKey::Key(Layer::Steer, 0)); // goto claude
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c == "focus %7"), "goto focuses the discovered pane, got: {calls:?}");
}

#[test]
fn goto_without_discovered_pane_is_noop() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 0)); // goto claude, nothing discovered
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.is_empty(), "no pane known yet, nothing to focus, got: {calls:?}");
}

#[test]
fn pane_less_events_do_not_clear_discovery() {
    let mut e = engine();
    e.on_ingest(ev("claude", "SessionStart", Some("%7")), 0);
    e.on_ingest(ev("claude", "Stop", None), 1_000); // e.g. an IDE session
    e.on_key(PhysKey::Key(Layer::Steer, 0));
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c == "focus %7"), "learned pane must survive pane-less events");
}

#[test]
fn goto_waiting_jumps_to_the_blocked_agent() {
    let mut e = engine();
    e.on_ingest(ev("claude", "SessionStart", Some("%1")), 0);
    e.on_ingest(ev("codex", "PermissionRequest", Some("%2")), 100); // codex WAITING
    e.on_key(PhysKey::Key(Layer::Steer, 3)); // goto-waiting
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c == "focus %2"), "must jump to the waiting session, got: {calls:?}");
}

#[test]
fn goto_waiting_is_noop_when_nobody_waits() {
    let mut e = engine();
    e.on_ingest(ev("claude", "SessionStart", Some("%1")), 0); // IDLE, pane known
    e.on_key(PhysKey::Key(Layer::Steer, 3));
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.is_empty(), "nobody is blocked; jumping anywhere would be noise, got: {calls:?}");
}

#[test]
fn ingest_waiting_pulses_pad() {
    let mut e = engine();
    e.on_ingest(ev("claude", "Notification", None), 1_000);
    e.on_tick(1_000);
    e.on_tick(1_600);
    let frames = &e.pad().frames;
    assert!(frames.len() >= 2);
    assert_ne!(frames[frames.len() - 2], frames[frames.len() - 1], "waiting must animate");
}

#[test]
fn mic_fires_hotkey_without_moving_focus() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 8)); // mic
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c.starts_with("hotkey ")));
    assert!(!calls.iter().any(|c| c.starts_with("focus ")), "mic dictates into the focused window");
}

#[test]
fn action_no_adapter_defines_is_noop() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 10)); // branch: no adapter defines it
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.is_empty(), "undefined action must be a complete no-op, got: {calls:?}");
}

#[test]
fn empty_action_is_noop() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 9)); // ask: claude maps it to ""
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.is_empty(), "empty-string adapter action must be a complete no-op, got: {calls:?}");
}

#[test]
fn prompt_uses_literal_text_path_into_focused_window() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Launch, 8)); // Prompt 1
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    let text_call = calls.iter().find(|c| c.starts_with("text focused "));
    assert!(text_call.is_some(), "prompt must go through send_text to the focused window, got: {calls:?}");
    assert!(text_call.unwrap().contains(' '), "prompt text keeps its spaces");
}
