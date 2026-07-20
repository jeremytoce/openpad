use std::process::Command;
use std::sync::Mutex;

pub struct Target {
    pub tmux: Option<String>,
}

pub trait Dispatcher: Send {
    fn send_keys(&self, t: &Target, keys: &str) -> Result<(), String>;
    fn focus(&self, t: &Target) -> Result<(), String>;
    fn fire_hotkey(&self, combo: &str) -> Result<(), String>;
}

/// Translate an adapter keystroke string into tmux send-keys args.
/// Trailing '\n' becomes the tmux key name "Enter"; bare tokens pass through.
pub fn tmux_args(target: &str, keys: &str) -> Vec<String> {
    let mut out = vec!["send-keys".into(), "-t".into(), target.into()];
    if let Some(text) = keys.strip_suffix('\n') {
        out.push(text.into());
        out.push("Enter".into());
    } else {
        out.push(keys.into());
    }
    out
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
                // focused-window fallback: System Events keystroke
                let script = format!(
                    "tell application \"System Events\" to keystroke \"{}\"",
                    keys.replace('"', "\\\"")
                );
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
    fn fake_dispatcher_records_send_keys() {
        let d = FakeDispatcher::default();
        d.send_keys(&Target { tmux: Some("claude:0".into()) }, "1").unwrap();
        assert_eq!(d.calls.lock().unwrap().as_slice(), ["send claude:0 1"]);
    }
}
