//! launchd LaunchAgent management: `openpad service install|uninstall|start|stop|status`.
//!
//! The daemon runs as a per-user LaunchAgent (starts at login, restarts on
//! crash, no terminal needed). `stop` boots the agent out of launchd without
//! deleting the plist (needed before VIA edits, since KeepAlive would revive
//! a plainly-killed process); `start` bootstraps it back.

pub const LABEL: &str = "com.openpad.daemon";

/// Pure plist generation; `binary` is the absolute path launchd will exec.
pub fn plist(binary: &str, log_path: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>run</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{log_path}</string>
    <key>StandardErrorPath</key>
    <string>{log_path}</string>
</dict>
</plist>
"#
    )
}

pub fn plist_path(home: &str) -> String {
    format!("{home}/Library/LaunchAgents/{LABEL}.plist")
}

pub fn log_path(home: &str) -> String {
    format!("{home}/Library/Logs/openpad.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_contains_binary_run_and_keepalive() {
        let p = plist("/Users/x/.local/bin/openpad", "/Users/x/Library/Logs/openpad.log");
        assert!(p.contains("<string>/Users/x/.local/bin/openpad</string>"));
        assert!(p.contains("<string>run</string>"));
        assert!(p.contains("<key>KeepAlive</key>\n    <true/>"));
        assert!(p.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(p.contains(LABEL));
    }

    #[test]
    fn paths_are_under_home() {
        assert_eq!(
            plist_path("/Users/x"),
            "/Users/x/Library/LaunchAgents/com.openpad.daemon.plist"
        );
        assert_eq!(log_path("/Users/x"), "/Users/x/Library/Logs/openpad.log");
    }
}
