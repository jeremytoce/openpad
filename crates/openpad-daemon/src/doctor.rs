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
