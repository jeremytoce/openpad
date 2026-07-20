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

#[test]
fn get_event_is_404() {
    let (tx, _rx) = mpsc::channel();
    openpad_daemon::ingest::spawn_ingest("127.0.0.1:17677", tx).unwrap();
    let resp = ureq::get("http://127.0.0.1:17677/event?agent=claude").call();
    let err = resp.unwrap_err();
    assert_eq!(err.into_response().map(|r| r.status()), Some(404));
}

#[test]
fn post_other_path_is_404() {
    let (tx, _rx) = mpsc::channel();
    openpad_daemon::ingest::spawn_ingest("127.0.0.1:17678", tx).unwrap();
    let resp = ureq::post("http://127.0.0.1:17678/other?agent=claude").send_string("{}");
    let err = resp.unwrap_err();
    assert_eq!(err.into_response().map(|r| r.status()), Some(404));
}

#[test]
fn malformed_json_body_yields_204_and_no_send() {
    let (tx, rx) = mpsc::channel();
    openpad_daemon::ingest::spawn_ingest("127.0.0.1:17679", tx).unwrap();
    let resp = ureq::post("http://127.0.0.1:17679/event?agent=claude").send_string("not json");
    assert!(resp.is_ok());
    assert_eq!(resp.unwrap().status(), 204);
    assert!(rx.recv_timeout(std::time::Duration::from_millis(300)).is_err());
}

#[test]
fn missing_agent_param_yields_204_and_no_send() {
    let (tx, rx) = mpsc::channel();
    openpad_daemon::ingest::spawn_ingest("127.0.0.1:17680", tx).unwrap();
    let body = r#"{"hook_event_name":"Notification"}"#;
    let resp = ureq::post("http://127.0.0.1:17680/event").send_string(body);
    assert!(resp.is_ok());
    assert_eq!(resp.unwrap().status(), 204);
    assert!(rx.recv_timeout(std::time::Duration::from_millis(300)).is_err());
}

#[test]
fn agent_param_is_parsed_not_substring_matched() {
    let (tx, rx) = mpsc::channel();
    openpad_daemon::ingest::spawn_ingest("127.0.0.1:17681", tx).unwrap();
    let body = r#"{"hook_event_name":"Notification"}"#;
    let resp = ureq::post("http://127.0.0.1:17681/event?fakeagent=notclaude&agent=claude").send_string(body);
    assert!(resp.is_ok());
    let ev = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    assert_eq!(ev.agent, "claude");
}

#[test]
fn agent_param_is_percent_decoded() {
    let (tx, rx) = mpsc::channel();
    openpad_daemon::ingest::spawn_ingest("127.0.0.1:17683", tx).unwrap();
    let body = r#"{"hook_event_name":"Notification"}"#;
    let resp = ureq::post("http://127.0.0.1:17683/event?agent=claude%2Dcode").send_string(body);
    assert!(resp.is_ok());
    let ev = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    assert_eq!(ev.agent, "claude-code");
}

#[test]
fn detail_has_no_trailing_space_when_tool_input_absent() {
    let (tx, rx) = mpsc::channel();
    openpad_daemon::ingest::spawn_ingest("127.0.0.1:17682", tx).unwrap();
    let body = r#"{"hook_event_name":"Notification","tool_name":"Bash"}"#;
    let resp = ureq::post("http://127.0.0.1:17682/event?agent=claude").send_string(body);
    assert!(resp.is_ok());
    let ev = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    assert_eq!(ev.detail.as_deref(), Some("Bash"));
}
