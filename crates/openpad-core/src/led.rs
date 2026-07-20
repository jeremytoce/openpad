use crate::state::{urgency, AgentState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub fn color_for(s: AgentState) -> Rgb {
    match s {
        AgentState::Idle => Rgb(30, 30, 30),
        AgentState::Thinking => Rgb(20, 40, 120),
        AgentState::Running => Rgb(40, 90, 255),
        AgentState::Waiting => Rgb(255, 160, 0),
        AgentState::Done => Rgb(0, 200, 80),
        AgentState::Error => Rgb(220, 30, 30),
    }
}

/// Triangle wave 25..=255, period 1200ms. Drives WAITING pulse only.
pub fn waiting_level(tick_ms: u64) -> u8 {
    let t = (tick_ms % 1200) as i64;
    let half = 600;
    let up = t <= half;
    let frac = if up { t } else { 1200 - t } as f32 / half as f32;
    (25.0 + frac * 230.0) as u8
}

fn scale(c: Rgb, level: u8) -> Rgb {
    let f = level as u16;
    Rgb(((c.0 as u16 * f) / 255) as u8,
        ((c.1 as u16 * f) / 255) as u8,
        ((c.2 as u16 * f) / 255) as u8)
}

pub fn derive_frame(snapshot: &[(String, AgentState)], tick_ms: u64) -> [Rgb; 16] {
    let mut frame = [Rgb(0, 0, 0); 16];
    let render = |s: AgentState| -> Rgb {
        if s == AgentState::Waiting { scale(color_for(s), waiting_level(tick_ms)) }
        else { color_for(s) }
    };
    for (i, (_, s)) in snapshot.iter().take(3).enumerate() { frame[i] = render(*s); }
    let worst = snapshot.iter().map(|(_, s)| *s)
        .max_by_key(|s| urgency(*s)).unwrap_or(AgentState::Idle);
    frame[3] = render(worst);
    let any_active = snapshot.iter().any(|(_, s)| *s != AgentState::Idle);
    if any_active { for k in 4..16 { frame[k] = Rgb(15, 15, 15); } }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AgentState;

    fn snap(states: &[AgentState]) -> Vec<(String, AgentState)> {
        ["claude", "codex", "kimi"].iter().zip(states)
            .map(|(n, s)| (n.to_string(), *s)).collect()
    }

    #[test]
    fn only_waiting_animates() {
        for s in [AgentState::Idle, AgentState::Thinking, AgentState::Running,
                  AgentState::Done, AgentState::Error] {
            let a = derive_frame(&snap(&[s, s, s]), 0);
            let b = derive_frame(&snap(&[s, s, s]), 600);
            assert_eq!(a, b, "{s:?} must not animate");
        }
        let a = derive_frame(&snap(&[AgentState::Waiting; 3]), 0);
        let b = derive_frame(&snap(&[AgentState::Waiting; 3]), 600);
        assert_ne!(a, b, "WAITING must animate");
    }

    #[test]
    fn all_key_shows_most_urgent() {
        let f = derive_frame(&snap(&[AgentState::Done, AgentState::Error, AgentState::Running]), 0);
        assert_eq!(f[3], color_for(AgentState::Error));
    }

    #[test]
    fn waiting_level_is_periodic_triangle() {
        assert_eq!(waiting_level(0), waiting_level(1200));
        assert!(waiting_level(600) > waiting_level(0));
    }
}
