use std::process::Command;
use std::sync::Mutex;

/// What the user is looking at, for adapter auto-selection: the focused
/// terminal's active tmux pane (exact match against discovered agent panes)
/// and the frontmost window title (fallback substring match).
#[derive(Default, Clone)]
pub struct FocusedContext {
    pub pane: Option<String>,
    pub title: Option<String>,
}

/// Everything openpad does to the outside world. All key/text output goes to
/// the focused window (spec revision 2); `focus_pane` moves focus to a
/// hook-discovered tmux pane (goto-waiting).
pub trait Dispatcher: Send {
    /// Whitespace-separated key tokens ("Escape Escape" = two Escape
    /// presses, "S-Tab" = shift-tab); trailing '\n' presses Enter after.
    fn send_keys(&self, keys: &str) -> Result<(), String>;
    /// Literal text: spaces stay spaces; trailing '\n' presses Enter after.
    fn send_text(&self, text: &str) -> Result<(), String>;
    /// Focus a tmux pane by id (e.g. "%5"): window, pane, and client.
    fn focus_pane(&self, pane: &str) -> Result<(), String>;
    fn fire_hotkey(&self, combo: &str) -> Result<(), String>;
    /// Probe what the user is looking at (pane + window title).
    fn focused_context(&self) -> FocusedContext;
}

#[derive(Default)]
pub struct FakeDispatcher {
    pub calls: Mutex<Vec<String>>,
    pub context: Mutex<FocusedContext>,
}

impl Dispatcher for FakeDispatcher {
    fn send_keys(&self, keys: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("send {keys}"));
        Ok(())
    }
    fn send_text(&self, text: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("text {text}"));
        Ok(())
    }
    fn focus_pane(&self, pane: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("focus {pane}"));
        Ok(())
    }
    fn fire_hotkey(&self, combo: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("hotkey {combo}"));
        Ok(())
    }
    fn focused_context(&self) -> FocusedContext {
        self.context.lock().unwrap().clone()
    }
}

/// Build an osascript keystroke command for literal text. Escapes
/// backslashes first, then quotes; trailing '\n' becomes "& return".
pub(crate) fn osascript_keystroke_script(keys: &str) -> String {
    let has_newline = keys.ends_with('\n');
    let text = if has_newline { &keys[..keys.len() - 1] } else { keys };
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    if text.is_empty() && has_newline {
        "tell application \"System Events\" to keystroke return".to_string()
    } else if has_newline {
        format!("tell application \"System Events\" to keystroke \"{escaped}\" & return")
    } else {
        format!("tell application \"System Events\" to keystroke \"{escaped}\"")
    }
}

/// osascript for a single key token: named keys become `key code` commands
/// (with S-/C-/M- modifier prefixes); anything else is typed as text.
pub(crate) fn osascript_key_token_script(tok: &str) -> String {
    let mut mods = Vec::new();
    let mut base = tok;
    loop {
        if let Some(r) = base.strip_prefix("S-") {
            mods.push("shift down");
            base = r;
        } else if let Some(r) = base.strip_prefix("C-") {
            mods.push("control down");
            base = r;
        } else if let Some(r) = base.strip_prefix("M-") {
            mods.push("option down");
            base = r;
        } else {
            break;
        }
    }
    let code = match base {
        "Escape" => Some(53),
        "Enter" | "Return" => Some(36),
        "Tab" => Some(48),
        "Space" => Some(49),
        "Up" => Some(126),
        "Down" => Some(125),
        _ => None,
    };
    match (code, mods.is_empty()) {
        (Some(c), true) => format!("tell application \"System Events\" to key code {c}"),
        (Some(c), false) => format!(
            "tell application \"System Events\" to key code {c} using {{{}}}",
            mods.join(", ")
        ),
        (None, _) => osascript_keystroke_script(tok),
    }
}

pub struct MacDispatcher {
    /// Frontmost apps keys may be synthesized into. Empty = allow all.
    pub terminal_apps: Vec<String>,
}

impl MacDispatcher {
    pub fn new(terminal_apps: Vec<String>) -> Self {
        MacDispatcher { terminal_apps }
    }

    fn osascript(script: &str) -> Result<(), String> {
        Command::new("osascript")
            .args(["-e", script])
            .status()
            .map_err(|e| e.to_string())
            .map(|_| ())
    }

    fn frontmost_app() -> Option<String> {
        let out = Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to get name of first application process whose frontmost is true"])
            .output()
            .ok()?;
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if name.is_empty() { None } else { Some(name) }
    }

    /// Only type into allowlisted apps: a stray approve press with a chat
    /// app focused must do nothing.
    fn allowed(&self) -> bool {
        if self.terminal_apps.is_empty() {
            return true;
        }
        Self::frontmost_app().is_some_and(|app| self.terminal_apps.contains(&app))
    }
}

impl Dispatcher for MacDispatcher {
    fn send_keys(&self, keys: &str) -> Result<(), String> {
        if !self.allowed() {
            return Ok(());
        }
        let (body, enter) = match keys.strip_suffix('\n') {
            Some(b) => (b, true),
            None => (keys, false),
        };
        for tok in body.split_whitespace() {
            Self::osascript(&osascript_key_token_script(tok))?;
        }
        if enter {
            Self::osascript("tell application \"System Events\" to keystroke return")?;
        }
        Ok(())
    }

    fn send_text(&self, text: &str) -> Result<(), String> {
        if !self.allowed() {
            return Ok(());
        }
        Self::osascript(&osascript_keystroke_script(text))
    }

    fn focus_pane(&self, pane: &str) -> Result<(), String> {
        let _ = Command::new("tmux").args(["select-window", "-t", pane]).status();
        let _ = Command::new("tmux").args(["select-pane", "-t", pane]).status();
        let _ = Command::new("tmux").args(["switch-client", "-t", pane]).status();
        Self::osascript("tell application \"iTerm2\" to activate")
    }

    fn fire_hotkey(&self, combo: &str) -> Result<(), String> {
        // combo like "key code 41 using {option down}", from config
        // (wispr_hotkey_osascript).
        Self::osascript(&format!("tell application \"System Events\" to {combo}"))
    }

    fn focused_context(&self) -> FocusedContext {
        // Active tmux pane of the most recently used client; works from
        // outside tmux, errors harmlessly when no server is running.
        let pane = Command::new("tmux")
            .args(["display-message", "-p", "#{pane_id}"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty());
        let title = Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to get name of front window of (first application process whose frontmost is true)"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty());
        FocusedContext { pane, title }
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn fake_dispatcher_records_exact_formats() {
        let d = FakeDispatcher::default();
        d.send_keys("y").unwrap();
        d.send_text("hello world\n").unwrap();
        d.focus_pane("%7").unwrap();
        d.fire_hotkey("key code 41 using {option down}").unwrap();
        assert_eq!(
            d.calls.lock().unwrap().as_slice(),
            ["send y", "text hello world\n", "focus %7", "hotkey key code 41 using {option down}"]
        );
    }

    #[test]
    fn osascript_keystroke_simple_text() {
        assert_eq!(
            osascript_keystroke_script("y"),
            "tell application \"System Events\" to keystroke \"y\""
        );
    }

    #[test]
    fn osascript_keystroke_with_trailing_newline() {
        assert_eq!(
            osascript_keystroke_script("/compact\n"),
            "tell application \"System Events\" to keystroke \"/compact\" & return"
        );
    }

    #[test]
    fn osascript_keystroke_newline_only() {
        assert_eq!(
            osascript_keystroke_script("\n"),
            "tell application \"System Events\" to keystroke return"
        );
    }

    #[test]
    fn osascript_keystroke_escaping_backslash_then_quote() {
        assert_eq!(
            osascript_keystroke_script("path\\\"file\""),
            "tell application \"System Events\" to keystroke \"path\\\\\\\"file\\\"\""
        );
    }

    #[test]
    fn osascript_key_tokens() {
        assert_eq!(
            osascript_key_token_script("Escape"),
            "tell application \"System Events\" to key code 53"
        );
        assert_eq!(
            osascript_key_token_script("S-Tab"),
            "tell application \"System Events\" to key code 48 using {shift down}"
        );
        assert_eq!(
            osascript_key_token_script("Down"),
            "tell application \"System Events\" to key code 125"
        );
        assert_eq!(
            osascript_key_token_script("y"),
            "tell application \"System Events\" to keystroke \"y\""
        );
    }
}
