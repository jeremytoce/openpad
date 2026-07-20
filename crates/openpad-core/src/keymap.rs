#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer { Steer, Launch }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Goto(String),      // focus the agent's discovered pane (LEDs summon, focus targets)
    GotoWaiting,       // jump focus to whichever session is blocked on the user
    Agent(String),     // adapter action name, resolved per bound agent
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
        let row1 = |arr: &mut [Option<Action>; 16]| {
            arr[0] = Some(Goto("claude".into()));
            arr[1] = Some(Goto("codex".into()));
            arr[2] = Some(Goto("kimi".into()));
            arr[3] = Some(GotoWaiting);
        };
        let mut steer: [Option<Action>; 16] = Default::default();
        row1(&mut steer);
        steer[4] = Some(Agent("approve".into()));
        steer[5] = Some(Agent("approve_always".into()));
        steer[6] = Some(Agent("reject".into()));
        steer[7] = Some(Agent("interrupt".into()));
        steer[8] = Some(Mic);
        steer[9] = Some(Agent("ask".into()));
        steer[10] = Some(Agent("branch".into()));
        steer[11] = Some(Agent("undo".into()));
        steer[12] = Some(Agent("plan".into()));
        steer[13] = Some(Agent("compact".into()));
        steer[14] = Some(Agent("clear".into()));
        steer[15] = Some(LayerHold);

        let mut launch: [Option<Action>; 16] = Default::default();
        row1(&mut launch);
        launch[4] = Some(Agent("review".into()));
        launch[5] = Some(Agent("test".into()));
        launch[6] = Some(Agent("commit".into()));
        launch[7] = Some(Agent("pr".into()));
        launch[8] = Some(Prompt(1));
        launch[9] = Some(Prompt(2));
        launch[10] = Some(Prompt(3));
        launch[11] = Some(Prompt(4));
        launch[12] = Some(Shell("repo".into()));
        launch[13] = Some(Shell("worktree".into()));
        launch[14] = Some(Shell("logs".into()));
        launch[15] = Some(LayerHold);
        Keymap { steer, launch }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_map_row1_binds_agents_on_both_layers() {
        let km = Keymap::default_map();
        for layer in [Layer::Steer, Layer::Launch] {
            assert!(matches!(km.action(layer, 0), Some(Action::Goto(a)) if a == "claude"));
            assert!(matches!(km.action(layer, 3), Some(Action::GotoWaiting)));
        }
    }

    #[test]
    fn steer_layer_has_approve_and_mic() {
        let km = Keymap::default_map();
        assert!(matches!(km.action(Layer::Steer, 4), Some(Action::Agent(a)) if a == "approve"));
        assert!(matches!(km.action(Layer::Steer, 8), Some(Action::Mic)));
        assert!(matches!(km.action(Layer::Steer, 15), Some(Action::LayerHold)));
    }
}
