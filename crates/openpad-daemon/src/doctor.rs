use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub struct Check {
    pub name: String,
    pub ok: bool,
    pub hint: String,
}

fn check(name: &str, ok: bool, hint: &str) -> Check {
    Check {
        name: name.into(),
        ok,
        hint: hint.into(),
    }
}

pub fn run_checks(
    settings_json: Option<&str>,
    hid_present: bool,
    tmux_ok: bool,
    port_free: bool,
) -> Vec<Check> {
    let hooks_ok = settings_json.map(|s| s.contains("openpad")).unwrap_or(false);
    vec![
        check(
            "pad",
            hid_present,
            "DOIO KB16-01 not found on USB. Plug it in (direct port, not a hub) and re-run.",
        ),
        check(
            "tmux",
            tmux_ok,
            "tmux server not reachable. Start your agent sessions: tmux new -s claude",
        ),
        check(
            "ingest-port",
            port_free,
            "127.0.0.1:7676 already in use — is another openpad running?",
        ),
        check(
            "claude-hooks",
            hooks_ok,
            "Claude hooks not installed. Run: openpad hooks install",
        ),
    ]
}

/// Impure I/O companion to `run_checks` (which stays pure). Probes whether
/// the process currently holding `addr` is an openpad-compatible ingest
/// server, so the CLI wrapper can tell "our own daemon owns this port"
/// (healthy) apart from "some unrelated process owns this port" (unhealthy).
///
/// Issues a minimal `GET /event` over a raw TCP connection with a short
/// timeout. openpad's ingest server (see `ingest::spawn_ingest`) only
/// handles `POST /event` and replies 404 to anything else, including GET —
/// so ANY well-formed `HTTP/1.x ...` status line in the reply is sufficient
/// evidence that an openpad ingest server answered. No response, a refused
/// connection, or non-HTTP garbage means something else owns the port.
pub fn port_holder_is_openpad(addr: &str) -> bool {
    let Some(sock_addr) = addr.to_socket_addrs().ok().and_then(|mut a| a.next()) else {
        return false;
    };
    let timeout = Duration::from_millis(500);

    let mut stream = match TcpStream::connect_timeout(&sock_addr, timeout) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    if stream.write_all(b"GET /event HTTP/1.0\r\n\r\n").is_err() {
        return false;
    }

    let mut buf = [0u8; 64];
    let n = match stream.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n == 0 {
        return false;
    }
    String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_good_yields_all_ok() {
        let checks = run_checks(
            Some(r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"bash /x/openpad/claude-hook.sh"}]}]}}"#),
            true,
            true,
            true,
        );
        assert!(checks.iter().all(|c| c.ok));
    }

    #[test]
    fn missing_pad_and_hooks_reported_with_hints() {
        let checks = run_checks(Some("{}"), false, true, true);
        let pad = checks.iter().find(|c| c.name == "pad").unwrap();
        assert!(!pad.ok && pad.hint.contains("USB"));
        let hooks = checks.iter().find(|c| c.name == "claude-hooks").unwrap();
        assert!(!hooks.ok && hooks.hint.contains("openpad hooks install"));
    }
}
