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
