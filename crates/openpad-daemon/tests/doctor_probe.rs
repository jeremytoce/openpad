// Integration tests for the doctor port-holder probe: verifies that
// `port_holder_is_openpad` correctly distinguishes "our own ingest server
// holds this port" from "nothing (or something else) is listening here".

#[test]
fn probes_true_when_openpad_ingest_holds_the_port() {
    let (tx, _rx) = std::sync::mpsc::channel();
    openpad_daemon::ingest::spawn_ingest("127.0.0.1:17677", tx)
        .expect("failed to spawn ingest server for test");
    assert!(openpad_daemon::doctor::port_holder_is_openpad("127.0.0.1:17677"));
}

#[test]
fn probes_false_when_nothing_is_listening() {
    assert!(!openpad_daemon::doctor::port_holder_is_openpad("127.0.0.1:17999"));
}
