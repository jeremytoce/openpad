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
    e.on_key(PhysKey::Key(Layer::Steer, 0)); // approve
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
fn goto_waiting_jumps_to_the_blocked_agent() {
    let mut e = engine();
    e.on_ingest(ev("claude", "SessionStart", Some("%1")), 0);
    e.on_ingest(ev("codex", "PermissionRequest", Some("%2")), 100); // codex WAITING
    e.on_key(PhysKey::Key(Layer::Steer, 4)); // goto-waiting
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c == "focus %2"), "must jump to the waiting session, got: {calls:?}");
}

#[test]
fn goto_waiting_is_noop_when_nobody_waits() {
    let mut e = engine();
    e.on_ingest(ev("claude", "SessionStart", Some("%1")), 0); // IDLE, pane known
    e.on_key(PhysKey::Key(Layer::Steer, 4));
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
    e.on_key(PhysKey::Key(Layer::Steer, 12)); // mic
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c.starts_with("hotkey ")));
    assert!(!calls.iter().any(|c| c.starts_with("focus ")), "mic dictates into the focused window");
}



#[test]
fn prompt_uses_literal_text_path_into_focused_window() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 11)); // Prompt 1
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    let text_call = calls.iter().find(|c| c.starts_with("text focused "));
    assert!(text_call.is_some(), "prompt must go through send_text to the focused window, got: {calls:?}");
    assert!(text_call.unwrap().contains(' '), "prompt text keeps its spaces");
}

// Adapter auto-selection: the profile follows the focused window.

#[test]
fn focused_pane_selects_that_agents_adapter() {
    let mut e = engine();
    e.on_ingest(ev("codex", "SessionStart", Some("%2")), 0);
    e.dispatcher().context.lock().unwrap().pane = Some("%2".into());
    e.on_key(PhysKey::Key(Layer::Steer, 2)); // reject
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(
        calls.iter().any(|c| c == "send focused Escape"),
        "codex is focused, reject must be codex's Escape, not claude's n: {calls:?}"
    );
}

#[test]
fn window_title_selects_adapter_when_no_pane_match() {
    let mut e = engine();
    e.dispatcher().context.lock().unwrap().title = Some("codex — ~/dev/openpad".into());
    e.on_key(PhysKey::Key(Layer::Steer, 2)); // reject
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c == "send focused Escape"), "title match must pick codex: {calls:?}");
}

#[test]
fn no_context_falls_back_to_selected_agent() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 2)); // reject, nothing focused-identifiable
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c == "send focused n"), "default selected agent is claude: {calls:?}");
}

#[test]
fn focused_agent_lacking_the_verb_is_noop() {
    let mut e = engine();
    e.on_ingest(ev("kimi", "SessionStart", Some("%9")), 0);
    e.dispatcher().context.lock().unwrap().pane = Some("%9".into());
    e.on_key(PhysKey::Key(Layer::Steer, 7)); // plan: kimi doesn't define it
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(
        !calls.iter().any(|c| c.starts_with("send ")),
        "must not send another agent's keystroke into kimi's window: {calls:?}"
    );
}

#[test]
fn discovered_pane_survives_pane_less_events() {
    let mut e = engine();
    e.on_ingest(ev("claude", "SessionStart", Some("%7")), 0);
    e.on_ingest(ev("claude", "Stop", None), 500); // e.g. an IDE session, no pane
    e.on_ingest(ev("claude", "Notification", None), 1_000); // WAITING, pane-less
    e.on_key(PhysKey::Key(Layer::Steer, 4)); // goto-waiting
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c == "focus %7"), "learned pane must survive pane-less events: {calls:?}");
}

#[test]
fn continue_key_types_literal_text() {
    let mut e = engine();
    e.on_key(PhysKey::Key(Layer::Steer, 5)); // continue
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c == "text focused continue\n"), "got: {calls:?}");
}

#[test]
fn encoder1_is_a_menu_knob() {
    let mut e = engine();
    e.on_key(PhysKey::EncoderTurn(0, 1));
    e.on_key(PhysKey::EncoderTurn(0, -1));
    e.on_key(PhysKey::EncoderPush(0));
    let calls = e.dispatcher().calls.lock().unwrap().clone();
    assert_eq!(calls, ["send focused Down", "send focused Up", "send focused Enter"]);
}
