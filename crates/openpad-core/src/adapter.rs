use std::collections::BTreeMap;
use serde::Deserialize;
use crate::state::AgentState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fidelity { Full, Degraded }

#[derive(Debug)]
pub struct Adapter {
    pub name: String,
    pub actions: BTreeMap<String, String>,
    pub events: BTreeMap<String, AgentState>,
    pub fidelity: Fidelity,
}

#[derive(Deserialize)]
struct Raw {
    fidelity: Option<String>,
    #[serde(default)] actions: BTreeMap<String, String>,
    #[serde(default)] events: BTreeMap<String, String>,
}

fn state_from(s: &str) -> Result<AgentState, String> {
    Ok(match s {
        "IDLE" => AgentState::Idle,
        "THINKING" => AgentState::Thinking,
        "RUNNING" => AgentState::Running,
        "WAITING" => AgentState::Waiting,
        "DONE" => AgentState::Done,
        "ERROR" => AgentState::Error,
        other => return Err(format!("unknown state '{other}'")),
    })
}

pub fn parse_adapter(name: &str, src: &str) -> Result<Adapter, String> {
    let raw: Raw = toml::from_str(src).map_err(|e| e.to_string())?;
    let mut events = BTreeMap::new();
    for (k, v) in raw.events { events.insert(k, state_from(&v)?); }
    let fidelity = match raw.fidelity.as_deref() {
        Some("full") => Fidelity::Full,
        _ => Fidelity::Degraded,
    };
    Ok(Adapter { name: name.into(), actions: raw.actions, events, fidelity })
}

impl Adapter {
    pub fn keys_for(&self, action: &str) -> Option<&str> {
        self.actions.get(action).map(|s| s.as_str())
    }
    pub fn state_for(&self, event: &str) -> Option<AgentState> {
        self.events.get(event).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AgentState;

    const CLAUDE: &str = r#"
        fidelity = "full"
        [actions]
        approve = "1"
        interrupt = "Escape"
        [events]
        Notification = "WAITING"
        Stop = "DONE"
    "#;

    #[test]
    fn parses_actions_and_events() {
        let a = parse_adapter("claude", CLAUDE).unwrap();
        assert_eq!(a.name, "claude");
        assert_eq!(a.keys_for("approve"), Some("1"));
        assert_eq!(a.state_for("Notification"), Some(AgentState::Waiting));
        assert_eq!(a.state_for("Stop"), Some(AgentState::Done));
        assert!(matches!(a.fidelity, Fidelity::Full));
    }

    #[test]
    fn unknown_event_state_is_error() {
        let bad = "[events]\nX = \"NOT_A_STATE\"\n";
        assert!(parse_adapter("x", bad).is_err());
    }

    #[test]
    fn missing_fidelity_defaults_degraded() {
        let a = parse_adapter("codex", "[actions]\n[events]\n").unwrap();
        assert!(matches!(a.fidelity, Fidelity::Degraded));
    }

    #[test]
    fn shipped_adapters_parse() {
        for (name, src) in [
            ("claude", include_str!("../../../adapters/claude.toml")),
            ("codex", include_str!("../../../adapters/codex.toml")),
            ("kimi", include_str!("../../../adapters/kimi.toml")),
        ] {
            parse_adapter(name, src).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
