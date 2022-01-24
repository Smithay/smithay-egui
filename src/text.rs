// This file is a light wrapper around libxkbcommon, see the other file for usage

use xkbcommon::xkb;
pub use xkbcommon::xkb::{keysyms, Keysym};

pub struct KbdInternal {
    keymap: xkb::Keymap,
    state: xkb::State,
}

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
