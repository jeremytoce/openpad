use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

pub struct Config {
    pub agents: Vec<AgentCfg>,
    pub prompts: BTreeMap<u8, String>,
    pub wispr_hotkey_osascript: String,
    pub ingest_addr: String,
    /// Frontmost apps steering keys may synthesize into. A press with any
    /// other app focused is a no-op (a stray approve must never type "y"
    /// into Slack).
    pub terminal_apps: Vec<String>,
}

pub struct AgentCfg {
    pub name: String,
    pub adapter: String,
    pub tmux: Option<String>,
}

#[derive(Deserialize)]
struct RawAgent {
    name: String,
    adapter: String,
    #[serde(default)]
    tmux: Option<String>,
}

fn default_terminal_apps() -> Vec<String> {
    ["iTerm2", "Terminal", "WezTerm", "Alacritty", "kitty", "Warp", "Ghostty", "Code", "Cursor"]
        .into_iter().map(String::from).collect()
}

#[derive(Deserialize)]
struct Raw {
    ingest_addr: String,
    wispr_hotkey_osascript: String,
    #[serde(default = "default_terminal_apps")]
    terminal_apps: Vec<String>,
    #[serde(default)]
    agents: Vec<RawAgent>,
    #[serde(default)]
    prompts: BTreeMap<String, String>,
}

/// Embedded default config, written to `~/.config/openpad/config.toml` on first run.
pub fn default_toml() -> &'static str {
    r#"ingest_addr = "127.0.0.1:7676"
# osascript fragment fired for the Mic key; set to match Wispr Flow's push-to-talk
# hotkey (see docs/verification.md, Task 5 Step 4)
wispr_hotkey_osascript = "key code 41 using {option down}"  # Option+; — verified PTT binding on this machine

# Steering keys act on the focused window; agents in tmux additionally
# self-announce their pane via hooks, which powers the goto keys.
# terminal_apps is the allowlist of frontmost apps keys may type into:
# terminal_apps = ["iTerm2", "Terminal", "WezTerm", "Alacritty", "kitty", "Warp", "Ghostty", "Code", "Cursor"]

[[agents]]
name = "claude"
adapter = "claude"

[[agents]]
name = "codex"
adapter = "codex"

[[agents]]
name = "kimi"
adapter = "kimi"

[prompts]
1 = "Summarize the current state of this task and what remains."
2 = "Run the test suite and fix any failures."
3 = "Review the last diff for bugs before I commit."
4 = "Continue."
"#
}

/// Parse config from a TOML source string (used by both `load` and tests).
pub fn parse(src: &str) -> Result<Config, String> {
    let raw: Raw = toml::from_str(src).map_err(|e| e.to_string())?;
    let mut prompts = BTreeMap::new();
    for (k, v) in raw.prompts {
        let n: u8 = k
            .parse()
            .map_err(|_| format!("invalid prompt key '{k}': must be 0-255"))?;
        prompts.insert(n, v);
    }
    Ok(Config {
        agents: raw
            .agents
            .into_iter()
            .map(|a| AgentCfg { name: a.name, adapter: a.adapter, tmux: a.tmux })
            .collect(),
        prompts,
        wispr_hotkey_osascript: raw.wispr_hotkey_osascript,
        terminal_apps: raw.terminal_apps,
        ingest_addr: raw.ingest_addr,
    })
}

/// Default config path: `~/.config/openpad/config.toml`.
pub fn default_path() -> Option<std::path::PathBuf> {
    dirs_home().map(|h| h.join(".config").join("openpad").join("config.toml"))
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

pub fn load(path: &Path) -> Result<Config, String> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| format!("reading {}: {e}", path.display()))?;
    parse(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_toml_parses() {
        let cfg = parse(default_toml()).unwrap();
        assert_eq!(cfg.agents.len(), 3);
        assert_eq!(cfg.agents[0].name, "claude");
        assert_eq!(cfg.agents[0].tmux, None, "panes are discovered via hooks, not configured");
        assert_eq!(cfg.ingest_addr, "127.0.0.1:7676");
        assert!(cfg.terminal_apps.iter().any(|a| a == "iTerm2"), "allowlist has defaults");
        assert_eq!(cfg.prompts.get(&1).map(|s| s.as_str()), Some("Summarize the current state of this task and what remains."));
    }

    #[test]
    fn load_reads_from_disk() {
        let dir = std::env::temp_dir().join(format!("openpad-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, default_toml()).unwrap();
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.agents.len(), 3);
        std::fs::remove_dir_all(&dir).ok();
    }
}
