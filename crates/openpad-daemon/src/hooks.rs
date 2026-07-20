use serde_json::{json, Value};

const EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Notification",
    "Stop",
    "StopFailure",
    "SubagentStop",
    "SessionEnd",
];

/// Extracts the "command" string of an openpad hook entry, if this entry is
/// one openpad installed (i.e. its command contains "openpad"). Returns None
/// for entries belonging to other tools.
fn openpad_command(entry: &Value) -> Option<&str> {
    entry
        .get("hooks")?
        .as_array()?
        .iter()
        .find_map(|h| h.get("command").and_then(|c| c.as_str()))
        .filter(|c| c.contains("openpad"))
}

/// `env_prefix` is prepended verbatim to the shim invocation (e.g.
/// `"OPENPAD_INGEST=http://127.0.0.1:9000 "`, including trailing space) when
/// the configured ingest address differs from the default, so the installed
/// hook command still reaches the right ingest server. Pass `""` to install
/// the plain `bash "<shim_path>"` command.
pub fn install_claude_hooks(
    settings_json: &str,
    shim_path: &str,
    env_prefix: &str,
) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(settings_json).map_err(|e| e.to_string())?;
    let target_command = format!("{env_prefix}bash \"{shim_path}\"");
    let hooks = v
        .as_object_mut()
        .ok_or("settings root must be an object")?
        .entry("hooks")
        .or_insert_with(|| json!({}));
    for ev in EVENTS {
        let arr = hooks
            .as_object_mut()
            .ok_or("hooks must be an object")?
            .entry(*ev)
            .or_insert_with(|| json!([]));
        let arr = arr
            .as_array_mut()
            .ok_or("hook event must be an array")?;
        // Drop any existing openpad entry whose command no longer matches the
        // target shim path, so a changed shim path is refreshed rather than
        // silently skipped.
        arr.retain(|e| match openpad_command(e) {
            Some(cmd) => cmd == target_command,
            None => true,
        });
        let already = arr.iter().any(|e| openpad_command(e).is_some());
        if !already {
            arr.push(json!({
                "hooks": [{
                    "type": "command",
                    "command": target_command,
                    "timeout": 3
                }]
            }));
        }
    }
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

pub fn uninstall_claude_hooks(settings_json: &str) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(settings_json).map_err(|e| e.to_string())?;
    if let Some(hooks) = v.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        // Only touch keys that openpad installs into (EVENTS). Keys outside
        // EVENTS are left completely untouched, even if their serialized form
        // happens to contain the substring "openpad" (e.g. a third-party
        // hook command) -- the additive invariant requires we never damage
        // entries we did not install.
        for ev in EVENTS {
            if let Some(arr) = hooks.get_mut(*ev).and_then(|a| a.as_array_mut()) {
                arr.retain(|e| !e.to_string().contains("openpad"));
                // NOTE (documented cosmetic edge case): if this key was
                // already an empty array before install (e.g. the user had
                // "Stop": []), or becomes empty once openpad's entry is
                // removed, the key itself is removed here. A pre-existing
                // empty array under an EVENTS key does not survive an
                // install/uninstall round-trip byte-for-byte, though no
                // hook behavior is affected (an empty array and a missing
                // key are equivalent to Claude Code).
                if arr.is_empty() {
                    hooks.remove(*ev);
                }
            }
        }
        if hooks.is_empty() {
            v.as_object_mut().unwrap().remove("hooks");
        }
    }
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXISTING: &str = r#"{
      "permissions": {"defaultMode": "auto"},
      "hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "node /gsd.js"}]}]}
    }"#;

    #[test]
    fn uninstall_removes_only_openpad_entries() {
        let installed =
            install_claude_hooks(EXISTING, "/usr/local/share/openpad/claude-hook.sh", "").unwrap();
        let removed = uninstall_claude_hooks(&installed).unwrap();
        let orig: serde_json::Value = serde_json::from_str(EXISTING).unwrap();
        let out: serde_json::Value = serde_json::from_str(&removed).unwrap();
        assert_eq!(orig, out);
    }

    #[test]
    fn install_preserves_existing_hooks_and_is_idempotent() {
        let once = install_claude_hooks(EXISTING, "/x/openpad/claude-hook.sh", "").unwrap();
        let twice = install_claude_hooks(&once, "/x/openpad/claude-hook.sh", "").unwrap();
        assert_eq!(once, twice);
        let v: serde_json::Value = serde_json::from_str(&once).unwrap();
        let ss = &v["hooks"]["SessionStart"];
        assert!(ss.to_string().contains("gsd.js"), "must keep existing entries");
        assert!(ss.to_string().contains("openpad"));
        assert!(v["hooks"]["Notification"].to_string().contains("openpad"));
    }

    #[test]
    fn uninstall_scoped_to_openpad_events() {
        // A custom, non-EVENTS key whose serialized form happens to contain the
        // substring "openpad" must survive uninstall completely unchanged, since
        // uninstall must only ever touch keys in EVENTS.
        let settings = r#"{
          "hooks": {"MyCustomEvent": [{"hooks": [{"type": "command", "command": "openpad-lookalike"}]}]}
        }"#;
        let out = uninstall_claude_hooks(settings).unwrap();
        let orig: serde_json::Value = serde_json::from_str(settings).unwrap();
        let out_v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(orig, out_v, "keys outside EVENTS must be left completely untouched");
    }

    #[test]
    fn install_refreshes_stale_shim_path() {
        let once = install_claude_hooks(EXISTING, "/old/openpad/claude-hook.sh", "").unwrap();
        let refreshed = install_claude_hooks(&once, "/new/openpad/claude-hook.sh", "").unwrap();
        let v: serde_json::Value = serde_json::from_str(&refreshed).unwrap();
        for ev in EVENTS {
            let arr = v["hooks"][*ev]
                .as_array()
                .unwrap_or_else(|| panic!("missing array for {ev}"));
            let openpad_entries: Vec<_> = arr
                .iter()
                .filter(|e| e.to_string().contains("openpad"))
                .collect();
            assert_eq!(
                openpad_entries.len(),
                1,
                "event {ev} should have exactly one openpad entry"
            );
            let s = openpad_entries[0].to_string();
            assert!(s.contains("/new/"), "event {ev} should point at /new/");
            assert!(!s.contains("/old/"), "event {ev} should not reference /old/");
        }

        let twice = install_claude_hooks(&refreshed, "/new/openpad/claude-hook.sh", "").unwrap();
        assert_eq!(refreshed, twice, "idempotency must hold once refreshed to /new/");
    }

    /// A changed `env_prefix` (e.g. the ingest address moving away from the
    /// default) must be treated the same as a changed shim path: the stale
    /// openpad entry is replaced, not duplicated, and re-running with the
    /// new prefix is idempotent.
    #[test]
    fn install_refreshes_stale_env_prefix() {
        let once = install_claude_hooks(EXISTING, "/x/openpad/claude-hook.sh", "").unwrap();
        let refreshed = install_claude_hooks(
            &once,
            "/x/openpad/claude-hook.sh",
            "OPENPAD_INGEST=http://127.0.0.1:9000 ",
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&refreshed).unwrap();
        for ev in EVENTS {
            let arr = v["hooks"][*ev]
                .as_array()
                .unwrap_or_else(|| panic!("missing array for {ev}"));
            let openpad_entries: Vec<_> = arr
                .iter()
                .filter(|e| e.to_string().contains("openpad"))
                .collect();
            assert_eq!(
                openpad_entries.len(),
                1,
                "event {ev} should have exactly one openpad entry"
            );
            let s = openpad_entries[0].to_string();
            assert!(
                s.contains("OPENPAD_INGEST=http://127.0.0.1:9000"),
                "event {ev} should carry the new env prefix"
            );
        }

        let twice = install_claude_hooks(
            &refreshed,
            "/x/openpad/claude-hook.sh",
            "OPENPAD_INGEST=http://127.0.0.1:9000 ",
        )
        .unwrap();
        assert_eq!(refreshed, twice, "idempotency must hold once refreshed to the new prefix");
    }
}
