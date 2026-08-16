use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ptr::null_mut;
use std::rc::{Rc, Weak};

use anyhow::{Context, bail};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey, VK_F1,
};
use windows_sys::Win32::UI::WindowsAndMessaging::WM_HOTKEY;

use crate::platform::api::{GlobalShortcutHost, GlobalShortcutRegistration, Shortcut, ShortcutKey};

type Callback = Rc<RefCell<Box<dyn FnMut() + 'static>>>;

const FIRST_REGISTRATION_ID: i32 = 1;
const LAST_REGISTRATION_ID: i32 = 0xBFFF;

/// Registers thread-level hotkeys and dispatches them from the owning event loop.
///
/// `handle_message` must be called on the registering thread for every message
/// taken from that thread's Win32 message queue.
pub struct Host {
    callbacks: Rc<RefCell<HashMap<i32, Callback>>>,
    next_id: Cell<i32>,
}

impl Host {
    pub fn new() -> Self {
        Self {
            callbacks: Rc::new(RefCell::new(HashMap::new())),
            next_id: Cell::new(FIRST_REGISTRATION_ID),
        }
    }

    /// Returns `true` when the message belongs to a registered hotkey.
    pub fn handle_message(&self, message: u32, wparam: usize) -> bool {
        if message != WM_HOTKEY {
            return false;
        }

        let Ok(id) = i32::try_from(wparam) else {
            return false;
        };
        let callback = self.callbacks.borrow().get(&id).cloned();
        let Some(callback) = callback else {
            return false;
        };

        if let Ok(mut callback) = callback.try_borrow_mut() {
            callback();
        }
        true
    }

    fn next_registration_id(&self) -> anyhow::Result<i32> {
        let callbacks = self.callbacks.borrow();
        let start = self.next_id.get();
        let mut candidate = start;

        loop {
            if !callbacks.contains_key(&candidate) {
                self.next_id.set(if candidate == LAST_REGISTRATION_ID {
                    FIRST_REGISTRATION_ID
                } else {
                    candidate + 1
                });
                return Ok(candidate);
            }

            candidate = if candidate == LAST_REGISTRATION_ID {
                FIRST_REGISTRATION_ID
            } else {
                candidate + 1
            };
            if candidate == start {
                bail!("no Windows global shortcut registration IDs remain");
            }
        }
    }
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalShortcutHost for Host {
    fn register_global_shortcut(
        &self,
        shortcut: Shortcut,
        callback: Box<dyn FnMut() + 'static>,
    ) -> anyhow::Result<Box<dyn GlobalShortcutRegistration>> {
        let (modifiers, virtual_key) = map_shortcut(shortcut)?;
        let id = self.next_registration_id()?;

        let registered = unsafe { RegisterHotKey(null_mut(), id, modifiers, virtual_key) };
        if registered == 0 {
            return Err(std::io::Error::last_os_error()).with_context(|| {
                format!("failed to register Windows global shortcut {shortcut:?}")
            });
        }

        self.callbacks
            .borrow_mut()
            .insert(id, Rc::new(RefCell::new(callback)));

        Ok(Box::new(Registration {
            id,
            callbacks: Rc::downgrade(&self.callbacks),
        }))
    }
}

struct Registration {
    id: i32,
    callbacks: Weak<RefCell<HashMap<i32, Callback>>>,
}

impl GlobalShortcutRegistration for Registration {}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(callbacks) = self.callbacks.upgrade() {
            let removed = callbacks.borrow_mut().remove(&self.id);
            drop(removed);
        }
        unsafe {
            UnregisterHotKey(null_mut(), self.id);
        }
    }
}

fn map_shortcut(shortcut: Shortcut) -> anyhow::Result<(u32, u32)> {
    let mut modifiers = 0;
    if shortcut.modifiers.control {
        modifiers |= MOD_CONTROL;
    }
    if shortcut.modifiers.alt {
        modifiers |= MOD_ALT;
    }
    if shortcut.modifiers.shift {
        modifiers |= MOD_SHIFT;
    }
    if shortcut.modifiers.logo {
        modifiers |= MOD_WIN;
    }

    let virtual_key = match shortcut.key {
        ShortcutKey::Character(character) if character.is_ascii_alphabetic() => {
            character.to_ascii_uppercase() as u32
        }
        ShortcutKey::Character(character) if character.is_ascii_digit() => character as u32,
        ShortcutKey::Character(character) => {
            bail!("character {character:?} has no layout-independent Windows virtual key")
        }
        ShortcutKey::Function(number @ 1..=24) => u32::from(VK_F1) + u32::from(number - 1),
        ShortcutKey::Function(number) => {
            bail!("Windows global shortcut function key F{number} is outside F1-F24")
        }
    };

    Ok((modifiers, virtual_key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::api::ShortcutModifiers;

    #[test]
    fn maps_control_alt_s() {
        let shortcut = Shortcut {
            modifiers: ShortcutModifiers {
                control: true,
                alt: true,
                shift: false,
                logo: false,
            },
            key: ShortcutKey::Character('s'),
        };

        let (modifiers, virtual_key) = map_shortcut(shortcut).unwrap();

        assert_eq!(modifiers, MOD_CONTROL | MOD_ALT);
        assert_eq!(virtual_key, u32::from(b'S'));
    }
}
