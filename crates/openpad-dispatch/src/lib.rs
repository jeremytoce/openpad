use std::process::Command;
use std::sync::Mutex;

pub struct Target {
    pub tmux: Option<String>,
}

pub trait Dispatcher: Send {
    /// Key-token path for adapter action strings: whitespace-separated tmux
    /// key names ("Escape Escape" = two Escape presses, "S-Tab" = shift-tab).
    fn send_keys(&self, t: &Target, keys: &str) -> Result<(), String>;
    /// Literal-text path for prompts: spaces stay spaces, nothing is
    /// interpreted as a key name. Trailing '\n' presses Enter after.
    fn send_text(&self, t: &Target, text: &str) -> Result<(), String>;
    fn focus(&self, t: &Target) -> Result<(), String>;
    fn fire_hotkey(&self, combo: &str) -> Result<(), String>;
}

/// Translate an adapter keystroke string into tmux send-keys args.
/// The (newline-stripped) string is split on whitespace: each token is a
/// separate tmux key argument, so "Escape Escape" presses Escape twice
/// instead of typing the words. Trailing '\n' appends the "Enter" key.
pub fn tmux_args(target: &str, keys: &str) -> Vec<String> {
    let mut out = vec!["send-keys".into(), "-t".into(), target.into()];
    let (body, enter) = match keys.strip_suffix('\n') {
        Some(b) => (b, true),
        None => (keys, false),
    };
    for tok in body.split_whitespace() {
        out.push(tok.into());
    }
    if enter {
        out.push("Enter".into());
    }
    out
}

/// tmux args for literal text: `send-keys -l -- <text>` types the string
/// verbatim (no key-name interpretation). Returns the literal command; the
/// caller sends Enter separately when the text ends in '\n'.
pub fn tmux_text_args(target: &str, text: &str) -> Vec<String> {
    vec![
        "send-keys".into(),
        "-t".into(),
        target.into(),
        "-l".into(),
        "--".into(),
        text.into(),
    ]
}

#[derive(Default)]
pub struct FakeDispatcher {
    pub calls: Mutex<Vec<String>>,
}

impl Dispatcher for FakeDispatcher {
    fn send_keys(&self, t: &Target, keys: &str) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("send {} {}", t.tmux.as_deref().unwrap_or("focused"), keys));
        Ok(())
    }
    fn send_text(&self, t: &Target, text: &str) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("text {} {}", t.tmux.as_deref().unwrap_or("focused"), text));
        Ok(())
    }
    fn focus(&self, t: &Target) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("focus {}", t.tmux.as_deref().unwrap_or("focused")));
        Ok(())
    }
    fn fire_hotkey(&self, combo: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("hotkey {combo}"));
        Ok(())
    }
}

/// Build an osascript keystroke command. Handles escaping and newline conversion.
/// Trailing '\n' becomes "& return" (AppleScript's keystroke return).
/// Escapes backslashes first, then quotes.
pub(crate) fn osascript_keystroke_script(keys: &str) -> String {
    let has_newline = keys.ends_with('\n');
    let text = if has_newline {
        &keys[..keys.len() - 1]
    } else {
        keys
    };

    // Escape backslashes first, then quotes
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"");

    if text.is_empty() && has_newline {
        // Just a newline: send keystroke return
        "tell application \"System Events\" to keystroke return".to_string()
    } else if has_newline {
        // Text with newline: keystroke "text" & return
        format!(
            "tell application \"System Events\" to keystroke \"{}\" & return",
            escaped
        )
    } else {
        // Just text: keystroke "text"
        format!(
            "tell application \"System Events\" to keystroke \"{}\"",
            escaped
        )
    }
}

/// osascript for a single key token in the focused-window fallback.
/// Named keys become `key code` commands (with modifier prefixes S-/C-/M-
/// handled); anything else is typed as literal text.
pub(crate) fn osascript_key_token_script(tok: &str) -> String {
    let (mods, base) = {
        let mut mods = Vec::new();
        let mut rest = tok;
        loop {
            if let Some(r) = rest.strip_prefix("S-") {
                mods.push("shift down");
                rest = r;
            } else if let Some(r) = rest.strip_prefix("C-") {
                mods.push("control down");
                rest = r;
            } else if let Some(r) = rest.strip_prefix("M-") {
                mods.push("option down");
                rest = r;
            } else {
                break;
            }
        }
        (mods, rest)
    };
    let code = match base {
        "Escape" => Some(53),
        "Enter" | "Return" => Some(36),
        "Tab" => Some(48),
        "Space" => Some(49),
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

pub struct MacDispatcher;

impl Dispatcher for MacDispatcher {
    fn send_keys(&self, t: &Target, keys: &str) -> Result<(), String> {
        match &t.tmux {
            Some(target) => {
                let args = tmux_args(target, keys);
                let status = Command::new("tmux")
                    .args(&args)
                    .status()
                    .map_err(|e| e.to_string())?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("tmux exited {status}"))
                }
            }
            None => {
                // focused-window fallback: each key token becomes its own
                // System Events command (named keys mapped to key codes).
                let (body, enter) = match keys.strip_suffix('\n') {
                    Some(b) => (b, true),
                    None => (keys, false),
                };
                for tok in body.split_whitespace() {
                    let script = osascript_key_token_script(tok);
                    Command::new("osascript")
                        .args(["-e", &script])
                        .status()
                        .map_err(|e| e.to_string())?;
                }
                if enter {
                    Command::new("osascript")
                        .args(["-e", "tell application \"System Events\" to keystroke return"])
                        .status()
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            }
        }
    }
    fn send_text(&self, t: &Target, text: &str) -> Result<(), String> {
        match &t.tmux {
            Some(target) => {
                let (body, enter) = match text.strip_suffix('\n') {
                    Some(b) => (b, true),
                    None => (text, false),
                };
                let status = Command::new("tmux")
                    .args(&tmux_text_args(target, body))
                    .status()
                    .map_err(|e| e.to_string())?;
                if !status.success() {
                    return Err(format!("tmux exited {status}"));
                }
                if enter {
                    let status = Command::new("tmux")
                        .args(["send-keys", "-t", target, "Enter"])
                        .status()
                        .map_err(|e| e.to_string())?;
                    if !status.success() {
                        return Err(format!("tmux exited {status}"));
                    }
                }
                Ok(())
            }
            None => {
                let script = osascript_keystroke_script(text);
                Command::new("osascript")
                    .args(["-e", &script])
                    .status()
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }
    fn focus(&self, t: &Target) -> Result<(), String> {
        if let Some(target) = &t.tmux {
            let win = target.split(':').next().unwrap_or(target);
            Command::new("tmux")
                .args(["switch-client", "-t", win])
                .status()
                .map_err(|e| e.to_string())?;
        }
        Command::new("osascript")
            .args(["-e", "tell application \"iTerm2\" to activate"])
            .status()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn fire_hotkey(&self, combo: &str) -> Result<(), String> {
        // combo like "key code 64 using {control down, option down}"; exact value comes
        // from config (wispr_hotkey_osascript), recorded during Task 5 verification.
        let script = format!("tell application \"System Events\" to {combo}");
        Command::new("osascript")
            .args(["-e", &script])
            .status()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn tmux_send_keys_args() {
        assert_eq!(
            tmux_args("claude:0", "Escape"),
            vec!["send-keys", "-t", "claude:0", "Escape"]
        );
        assert_eq!(
            tmux_args("claude:0", "/compact\n"),
            vec!["send-keys", "-t", "claude:0", "/compact", "Enter"]
        );
    }

    #[test]
    fn tmux_multi_key_action_splits_into_tokens() {
        // "Escape Escape" must press Escape twice, not type the words
        assert_eq!(
            tmux_args("claude:0", "Escape Escape"),
            vec!["send-keys", "-t", "claude:0", "Escape", "Escape"]
        );
    }

    #[test]
    fn tmux_text_args_are_literal() {
        assert_eq!(
            tmux_text_args("claude:0", "Run the test suite."),
            vec!["send-keys", "-t", "claude:0", "-l", "--", "Run the test suite."]
        );
    }

    #[test]
    fn fake_dispatcher_send_text_records_exact_format() {
        let d = FakeDispatcher::default();
        d.send_text(&Target { tmux: Some("claude:0".into()) }, "hello world\n").unwrap();
        assert_eq!(d.calls.lock().unwrap().as_slice(), ["text claude:0 hello world\n"]);
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
            osascript_key_token_script("y"),
            "tell application \"System Events\" to keystroke \"y\""
        );
    }

    #[test]
    fn fake_dispatcher_records_send_keys() {
        let d = FakeDispatcher::default();
        d.send_keys(&Target { tmux: Some("claude:0".into()) }, "1").unwrap();
        assert_eq!(d.calls.lock().unwrap().as_slice(), ["send claude:0 1"]);
    }

    #[test]
    fn fake_dispatcher_focus_records_exact_format() {
        let d = FakeDispatcher::default();
        d.focus(&Target { tmux: Some("work:0".into()) }).unwrap();
        assert_eq!(d.calls.lock().unwrap().as_slice(), ["focus work:0"]);
    }

    #[test]
    fn fake_dispatcher_hotkey_records_exact_format() {
        let d = FakeDispatcher::default();
        d.fire_hotkey("key code 64 using {control down}").unwrap();
        assert_eq!(d.calls.lock().unwrap().as_slice(), ["hotkey key code 64 using {control down}"]);
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
    fn osascript_keystroke_escaped_quotes() {
        assert_eq!(
            osascript_keystroke_script("say \"hi\""),
            "tell application \"System Events\" to keystroke \"say \\\"hi\\\"\""
        );
    }

    #[test]
    fn osascript_keystroke_escaped_backslash() {
        assert_eq!(
            osascript_keystroke_script("a\\b"),
            "tell application \"System Events\" to keystroke \"a\\\\b\""
        );
    }

    #[test]
    fn osascript_keystroke_backslash_and_quote() {
        assert_eq!(
            osascript_keystroke_script("path\\\"file\""),
            "tell application \"System Events\" to keystroke \"path\\\\\\\"file\\\"\""
        );
    }
}
