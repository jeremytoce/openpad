#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState { Idle, Thinking, Running, Waiting, Done, Error }

pub const DONE_DECAY_MS: u64 = 5_000;

pub fn urgency(s: AgentState) -> u8 {
    match s {
        AgentState::Error => 5,
        AgentState::Waiting => 4,
        AgentState::Running => 3,
        AgentState::Thinking => 2,
        AgentState::Done => 1,
        AgentState::Idle => 0,
    }
}

struct Slot { name: String, state: AgentState, entered_ms: u64 }

pub struct StateMachine { slots: Vec<Slot> }

impl StateMachine {
    pub fn new(agents: &[&str]) -> Self {
        Self { slots: agents.iter().map(|a| Slot {
            name: a.to_string(), state: AgentState::Idle, entered_ms: 0 }).collect() }
    }
    fn slot_mut(&mut self, agent: &str) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|s| s.name == agent)
    }
    pub fn apply(&mut self, agent: &str, state: AgentState, now_ms: u64) {
        if let Some(s) = self.slot_mut(agent) { s.state = state; s.entered_ms = now_ms; }
    }
    pub fn tick(&mut self, now_ms: u64) {
        for s in &mut self.slots {
            if s.state == AgentState::Done && now_ms.saturating_sub(s.entered_ms) > DONE_DECAY_MS {
                s.state = AgentState::Idle;
                s.entered_ms = now_ms;
            }
        }
    }
    pub fn get(&self, agent: &str) -> AgentState {
        self.slots.iter().find(|s| s.name == agent).map(|s| s.state).unwrap_or(AgentState::Idle)
    }
    pub fn entered_ms(&self, agent: &str) -> u64 {
        self.slots.iter().find(|s| s.name == agent).map(|s| s.entered_ms).unwrap_or(0)
    }
    pub fn snapshot(&self) -> Vec<(String, AgentState)> {
        self.slots.iter().map(|s| (s.name.clone(), s.state)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_agent_is_idle() {
        let sm = StateMachine::new(&["claude"]);
        assert_eq!(sm.get("nope"), AgentState::Idle);
    }

    #[test]
    fn apply_sets_state_and_entered_time() {
        let mut sm = StateMachine::new(&["claude"]);
        sm.apply("claude", AgentState::Waiting, 1_000);
        assert_eq!(sm.get("claude"), AgentState::Waiting);
        assert_eq!(sm.entered_ms("claude"), 1_000);
    }

    #[test]
    fn done_decays_to_idle_after_5s() {
        let mut sm = StateMachine::new(&["claude"]);
        sm.apply("claude", AgentState::Done, 1_000);
        sm.tick(5_999);
        assert_eq!(sm.get("claude"), AgentState::Done);
        sm.tick(6_001);
        assert_eq!(sm.get("claude"), AgentState::Idle);
    }

    #[test]
    fn error_does_not_decay() {
        let mut sm = StateMachine::new(&["claude"]);
        sm.apply("claude", AgentState::Error, 0);
        sm.tick(60_000);
        assert_eq!(sm.get("claude"), AgentState::Error);
    }

    #[test]
    fn snapshot_preserves_insertion_order() {
        let sm = StateMachine::new(&["claude", "codex", "kimi"]);
        let names: Vec<_> = sm.snapshot().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["claude", "codex", "kimi"]);
    }

    #[test]
    fn urgency_ordering() {
        assert!(urgency(AgentState::Error) > urgency(AgentState::Waiting));
        assert!(urgency(AgentState::Waiting) > urgency(AgentState::Running));
        assert!(urgency(AgentState::Running) > urgency(AgentState::Thinking));
        assert!(urgency(AgentState::Thinking) > urgency(AgentState::Done));
        assert!(urgency(AgentState::Done) > urgency(AgentState::Idle));
    }
}
