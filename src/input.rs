// This file is a light wrapper around libxkbcommon, see the other file for usage

use egui::{Key, Modifiers, PointerButton};
use smithay::{
    backend::input::MouseButton,
    input::keyboard::{Keysym as KeysymU32, ModifiersState},
};
use xkbcommon::xkb;
pub use xkbcommon::xkb::{keysyms, Keysym};

use std::convert::TryFrom;

pub struct KbdInternal {
    keymap: xkb::Keymap,
    state: xkb::State,
}
// SAFETY: This is OK, because all parts of xkb will remain on the same thread
unsafe impl Send for KbdInternal {}

// focus_hook does not implement debug, so we have to impl Debug manually
impl std::fmt::Debug for KbdInternal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KbdInternal")
            .field("keymap", &self.keymap.get_raw_ptr())
            .field("state", &self.state.get_raw_ptr())
            .finish()
    }
}

impl KbdInternal {
    pub fn new() -> Option<KbdInternal> {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            "",
            "",
            "",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )?;
        let state = xkb::State::new(&keymap);
        Some(KbdInternal { keymap, state })
    }

    // return true if modifier state has changed
    pub fn key_input(&mut self, keycode: u32, pressed: bool) {
        let direction = match pressed {
            true => xkb::KeyDirection::Down,
            false => xkb::KeyDirection::Up,
        };

        // update state (keycode is already offset by 8)
        self.state.update_key(keycode, direction);
    }

    pub fn get_utf8(&self, keycode: u32) -> String {
        self.state.key_get_utf8(keycode)
    }
}

/// Converts a set of raw keycodes into [`egui::Key`], if possible.
pub fn convert_key(keys: impl Iterator<Item = KeysymU32>) -> Option<Key> {
    for sym in keys {
        if let Ok(key) = KeysymConv(sym).try_into() {
            return Some(key);
        }
    }
    None
}

pub struct KeysymConv(pub KeysymU32);

impl TryFrom<KeysymConv> for Key {
    type Error = ();

    fn try_from(sym: KeysymConv) -> Result<Key, ()> {
        use egui::Key::*;
        use smithay::input::keyboard::keysyms::*;

        #[allow(non_upper_case_globals)]
        Ok(match sym.0 {
            KEY_Down => ArrowDown,
            KEY_Left => ArrowLeft,
            KEY_Right => ArrowRight,
            KEY_Up => ArrowUp,
            KEY_Escape => Escape,
            KEY_Tab => Tab,
            KEY_BackSpace => Backspace,
            KEY_Return => Enter,
            KEY_space => Space,
            KEY_Insert => Insert,
            KEY_Delete => Delete,
            KEY_Home => Home,
            KEY_End => End,
            KEY_Page_Up => PageUp,
            KEY_Page_Down => PageDown,
            KEY_0 => Num0,
            KEY_1 => Num1,
            KEY_2 => Num2,
            KEY_3 => Num3,
            KEY_4 => Num4,
            KEY_5 => Num5,
            KEY_6 => Num6,
            KEY_7 => Num7,
            KEY_8 => Num8,
            KEY_9 => Num9,
            KEY_a => A,
            KEY_b => B,
            KEY_c => C,
            KEY_d => D,
            KEY_e => E,
            KEY_f => F,
            KEY_g => G,
            KEY_h => H,
            KEY_i => I,
            KEY_j => J,
            KEY_k => K,
            KEY_l => L,
            KEY_m => M,
            KEY_n => N,
            KEY_o => O,
            KEY_p => P,
            KEY_q => Q,
            KEY_r => R,
            KEY_s => S,
            KEY_t => T,
            KEY_u => U,
            KEY_v => V,
            KEY_w => W,
            KEY_x => X,
            KEY_y => Y,
            KEY_z => Z,
            _ => {
                return Err(());
            }
        })
    }
}

/// Convert from smithay's [`ModifiersState`] to egui's [`Modifiers`]
pub fn convert_modifiers(modifiers: ModifiersState) -> Modifiers {
    ModifiersWrapper(modifiers).into()
}

pub struct ModifiersWrapper(pub ModifiersState);

impl From<ModifiersWrapper> for Modifiers {
    fn from(modifiers: ModifiersWrapper) -> Modifiers {
        Modifiers {
            alt: modifiers.0.alt,
            ctrl: modifiers.0.ctrl,
            shift: modifiers.0.shift,
            mac_cmd: if cfg!(target_os = "macos") {
                modifiers.0.logo
            } else {
                false
            },
            command: if cfg!(target_os = "macos") {
                modifiers.0.logo
            } else {
                modifiers.0.ctrl
            },
        }
    }
}

/// Convert from smithay's [`MouseButton`] to egui's [`PointerButton`], if possible
pub fn convert_button(button: MouseButton) -> Option<PointerButton> {
    ButtonWrapper(button).try_into().ok()
}

pub struct ButtonWrapper(pub MouseButton);

impl TryFrom<ButtonWrapper> for PointerButton {
    type Error = ();

    fn try_from(button: ButtonWrapper) -> Result<PointerButton, ()> {
        Ok(match button.0 {
            MouseButton::Left => PointerButton::Primary,
            MouseButton::Middle => PointerButton::Middle,
            MouseButton::Right => PointerButton::Secondary,
            _ => {
                return Err(());
            }
        })
    }
}
