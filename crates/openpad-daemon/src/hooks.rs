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

pub fn install_claude_hooks(settings_json: &str, shim_path: &str) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(settings_json).map_err(|e| e.to_string())?;
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
        let already = arr.iter().any(|e| e.to_string().contains("openpad"));
        if !already {
            arr.push(json!({
                "hooks": [{
                    "type": "command",
                    "command": format!("bash \"{}\"", shim_path),
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
        let keys: Vec<String> = hooks.keys().cloned().collect();
        for k in keys {
            if let Some(arr) = hooks.get_mut(&k).and_then(|a| a.as_array_mut()) {
                arr.retain(|e| !e.to_string().contains("openpad"));
                if arr.is_empty() {
                    hooks.remove(&k);
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
        let installed = install_claude_hooks(EXISTING, "/usr/local/share/openpad/claude-hook.sh").unwrap();
        let removed = uninstall_claude_hooks(&installed).unwrap();
        let orig: serde_json::Value = serde_json::from_str(EXISTING).unwrap();
        let out: serde_json::Value = serde_json::from_str(&removed).unwrap();
        assert_eq!(orig, out);
    }

    #[test]
    fn install_preserves_existing_hooks_and_is_idempotent() {
        let once = install_claude_hooks(EXISTING, "/x/openpad/claude-hook.sh").unwrap();
        let twice = install_claude_hooks(&once, "/x/openpad/claude-hook.sh").unwrap();
        assert_eq!(once, twice);
        let v: serde_json::Value = serde_json::from_str(&once).unwrap();
        let ss = &v["hooks"]["SessionStart"];
        assert!(ss.to_string().contains("gsd.js"), "must keep existing entries");
        assert!(ss.to_string().contains("openpad"));
        assert!(v["hooks"]["Notification"].to_string().contains("openpad"));
    }
}
