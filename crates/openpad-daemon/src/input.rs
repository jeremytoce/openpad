use openpad_core::keymap::Layer;
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysKey {
    Key(Layer, u8),
    EncoderTurn(u8, i8), // (encoder 0..2, -1 ccw / +1 cw)
    EncoderPush(u8),
}

const FKEYS: [u32; 8] = [105, 107, 113, 106, 64, 79, 80, 90]; // F13..F20 mac keycodes

pub fn map_key(mods: Mods, code: u32) -> Option<PhysKey> {
    let idx = FKEYS.iter().position(|&c| c == code)? as u8;
    if mods.alt {
        return Some(match (idx, mods.shift) {
            (0, false) => PhysKey::EncoderTurn(0, -1),
            (1, false) => PhysKey::EncoderTurn(0, 1),
            (2, false) => PhysKey::EncoderTurn(1, -1),
            (3, false) => PhysKey::EncoderTurn(1, 1),
            (4, false) => PhysKey::EncoderTurn(2, -1),
            (5, false) => PhysKey::EncoderTurn(2, 1),
            (6, false) => PhysKey::EncoderPush(0),
            (7, false) => PhysKey::EncoderPush(1),
            (0, true) => PhysKey::EncoderPush(2),
            _ => return None,
        });
    }
    let layer = if mods.ctrl { Layer::Launch } else { Layer::Steer };
    let key = if mods.shift { idx + 8 } else { idx };
    Some(PhysKey::Key(layer, key))
}

/// Classify one input event: update modifier state, and if it's a mapped
/// pad key, emit the PhysKey (on press) and report it as swallowable.
/// Shared by the grab (consuming) and listen (passive fallback) paths.
fn classify(ev: &rdev::EventType, mods: &mut Mods, tx: &Sender<PhysKey>) -> bool {
    use rdev::{EventType, Key};
    match ev {
        EventType::KeyPress(Key::ShiftLeft | Key::ShiftRight) => mods.shift = true,
        EventType::KeyRelease(Key::ShiftLeft | Key::ShiftRight) => mods.shift = false,
        EventType::KeyPress(Key::ControlLeft | Key::ControlRight) => mods.ctrl = true,
        EventType::KeyRelease(Key::ControlLeft | Key::ControlRight) => mods.ctrl = false,
        EventType::KeyPress(Key::Alt | Key::AltGr) => mods.alt = true,
        EventType::KeyRelease(Key::Alt | Key::AltGr) => mods.alt = false,
        EventType::KeyPress(k) => {
            if let Some(pk) = rdev_code(*k).and_then(|c| map_key(*mods, c)) {
                let _ = tx.send(pk);
                return true; // swallow: pad key handled
            }
        }
        EventType::KeyRelease(k) => {
            // swallow the matching key-up of consumed pad keys
            if rdev_code(*k).and_then(|c| map_key(*mods, c)).is_some() {
                return true;
            }
        }
        _ => {}
    }
    false
}

pub fn spawn_listener(tx: Sender<PhysKey>) {
    std::thread::spawn(move || {
        let mods = std::sync::Arc::new(std::sync::Mutex::new(Mods { shift: false, ctrl: false, alt: false }));

        // Primary: active grab (CGEventTap). Mapped pad keys are CONSUMED so
        // their F-key escape sequences never reach the focused terminal, a
        // prerequisite of the focused-window steering model.
        let m = mods.clone();
        let tx_grab = tx.clone();
        let grab_result = rdev::grab(move |ev| {
            if classify(&ev.event_type, &mut m.lock().unwrap(), &tx_grab) {
                None
            } else {
                Some(ev)
            }
        });

        // Fallback: passive listen (pad works, but F-key sequences leak into
        // focused terminals). Happens when grab is denied (Accessibility /
        // Input Monitoring not granted to this binary).
        if grab_result.is_err() {
            eprintln!("openpad: event grab unavailable (grant Accessibility + Input Monitoring); falling back to passive listen; pad keys will leak escape codes into focused terminals");
            let _ = rdev::listen(move |ev| {
                classify(&ev.event_type, &mut mods.lock().unwrap(), &tx);
            });
        }
    });
}

fn rdev_code(k: rdev::Key) -> Option<u32> {
    // rdev reports F13+ as Unknown(mac keycode) on macOS
    match k {
        rdev::Key::Unknown(c) => Some(c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpad_core::keymap::Layer;

    const NONE: Mods = Mods { shift: false, ctrl: false, alt: false };
    const SHIFT: Mods = Mods { shift: true, ctrl: false, alt: false };
    const CTRL: Mods = Mods { shift: false, ctrl: true, alt: false };
    const ALT: Mods = Mods { shift: false, ctrl: false, alt: true };

    #[test]
    fn plain_f13_is_steer_key0() {
        assert_eq!(map_key(NONE, 105), Some(PhysKey::Key(Layer::Steer, 0)));
    }
    #[test]
    fn shift_f13_is_steer_key8() {
        assert_eq!(map_key(SHIFT, 105), Some(PhysKey::Key(Layer::Steer, 8)));
    }
    #[test]
    fn ctrl_f20_is_launch_key7() {
        assert_eq!(map_key(CTRL, 90), Some(PhysKey::Key(Layer::Launch, 7)));
    }
    #[test]
    fn alt_f14_is_encoder1_cw() {
        assert_eq!(map_key(ALT, 107), Some(PhysKey::EncoderTurn(0, 1)));
    }
    #[test]
    fn unmapped_key_is_none() {
        assert_eq!(map_key(NONE, 0), None); // 'a'
    }
}
