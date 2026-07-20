#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer { Steer, Launch }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Goto(String),      // focus the agent's discovered pane (LEDs summon, focus targets)
    GotoWaiting,       // jump focus to whichever session is blocked on the user
    Agent(String),     // adapter action name, resolved per bound agent
    Text(String),      // literal text typed into the focused window (+ Enter on trailing newline)
    Mic,               // focus bound pane, then fire Wispr hotkey
    LayerHold,
    Prompt(u8),        // send saved prompt template N
    Shell(String),     // run local command (repo picker etc.)
}

pub struct Keymap {
    steer: [Option<Action>; 16],
    launch: [Option<Action>; 16],
}

impl Keymap {
    pub fn action(&self, layer: Layer, key: u8) -> Option<&Action> {
        let arr = match layer { Layer::Steer => &self.steer, Layer::Launch => &self.launch };
        arr.get(key as usize).and_then(|a| a.as_ref())
    }

    pub fn default_map() -> Keymap {
        use Action::*;
        // Verb-centric steer layer (spec revision 2): every key acts on the
        // focused window; the adapter is auto-selected from it. Corners are
        // blind-findable: TL=approve (most pressed), TR=interrupt (panic),
        // BL=mic (comfortable hold), BR=layer (firmware).
        let mut steer: [Option<Action>; 16] = Default::default();
        steer[0] = Some(Agent("approve".into()));
        steer[1] = Some(Agent("approve_always".into()));
        steer[2] = Some(Agent("reject".into()));
        steer[3] = Some(Agent("interrupt".into()));
        steer[4] = Some(GotoWaiting);
        steer[5] = Some(Text("continue\n".into()));
        steer[6] = Some(Agent("undo".into()));
        steer[7] = Some(Agent("plan".into()));
        steer[8] = Some(Agent("compact".into()));
        steer[9] = Some(Agent("clear".into()));
        steer[10] = Some(Agent("model".into()));
        steer[11] = Some(Prompt(1));
        steer[12] = Some(Mic);
        steer[13] = Some(Prompt(2));
        steer[14] = Some(Prompt(3));
        steer[15] = Some(LayerHold);

        let mut launch: [Option<Action>; 16] = Default::default();
        launch[0] = Some(Agent("review".into()));
        launch[1] = Some(Agent("test".into()));
        launch[2] = Some(Agent("commit".into()));
        launch[3] = Some(Agent("pr".into()));
        launch[4] = Some(Prompt(4));
        launch[5] = Some(Prompt(5));
        launch[6] = Some(Prompt(6));
        launch[7] = Some(Prompt(7));
        launch[8] = Some(Shell("repo".into()));
        launch[9] = Some(Shell("worktree".into()));
        launch[10] = Some(Shell("logs".into()));
        launch[15] = Some(LayerHold);
        Keymap { steer, launch }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_approve_interrupt_mic_layer() {
        let km = Keymap::default_map();
        assert!(matches!(km.action(Layer::Steer, 0), Some(Action::Agent(a)) if a == "approve"));
        assert!(matches!(km.action(Layer::Steer, 3), Some(Action::Agent(a)) if a == "interrupt"));
        assert!(matches!(km.action(Layer::Steer, 12), Some(Action::Mic)));
        assert!(matches!(km.action(Layer::Steer, 15), Some(Action::LayerHold)));
        assert!(matches!(km.action(Layer::Launch, 15), Some(Action::LayerHold)));
    }

    #[test]
    fn steer_has_goto_waiting_and_continue() {
        let km = Keymap::default_map();
        assert!(matches!(km.action(Layer::Steer, 4), Some(Action::GotoWaiting)));
        assert!(matches!(km.action(Layer::Steer, 5), Some(Action::Text(t)) if t == "continue\n"));
    }
}
