use egui::{Key, Modifiers, PointerButton};
use smithay::{
    backend::input::MouseButton,
    wayland::seat::{Keysym as KeysymU32, ModifiersState},
};
use std::convert::TryFrom;

/// Converts a set of raw keycodes into [`egui::Key`], if possible.
pub fn convert_key(keys: impl Iterator<Item = KeysymU32>) -> Option<Key> {
    for sym in keys {
        if let Ok(key) = Keysym(sym).try_into() {
            return Some(key);
        }
    }
    None
}

pub struct Keysym(pub KeysymU32);

impl TryFrom<Keysym> for Key {
    type Error = ();

    fn try_from(sym: Keysym) -> Result<Key, ()> {
        use egui::Key::*;
        use smithay::wayland::seat::keysyms::*;

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