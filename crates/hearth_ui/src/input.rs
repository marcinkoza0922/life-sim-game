//! Pointer and keyboard state, fed from winit events.

use glam::{Vec2, vec2};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Enter,
    Escape,
    Tab,
    SelectAll,
    Copy,
    Cut,
    Paste,
}

/// Modifier keys held right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    /// Ctrl, or Cmd on macOS: the shortcut modifier.
    pub command: bool,
    pub alt: bool,
}

#[derive(Clone, Debug)]
pub struct Input {
    /// Pointer position in points.
    pub pointer: Option<Vec2>,
    /// Left, right, middle.
    pub down: [bool; 3],
    pub pressed: [bool; 3],
    pub released: [bool; 3],
    /// Wheel movement in lines (positive y = away from the user).
    pub scroll: Vec2,
    /// Characters typed this frame.
    pub text: String,
    pub keys: Vec<Key>,
    pub mods: Mods,
    /// Pixels per point.
    pub ppp: f32,
}

impl Default for Input {
    fn default() -> Self {
        Input { pointer: None, down: [false; 3], pressed: [false; 3], released: [false; 3], scroll: Vec2::ZERO, text: String::new(), keys: Vec::new(), mods: Mods::default(), ppp: 1.0 }
    }
}

impl Input {
    pub fn on_event(&mut self, ev: &WindowEvent) {
        match ev {
            WindowEvent::CursorMoved { position, .. } => self.pointer = Some(vec2(position.x as f32, position.y as f32) / self.ppp),
            WindowEvent::CursorLeft { .. } => self.pointer = None,
            WindowEvent::MouseInput { state, button, .. } => {
                let i = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    _ => return,
                };
                if *state == ElementState::Pressed {
                    self.down[i] = true;
                    self.pressed[i] = true;
                } else {
                    self.down[i] = false;
                    self.released[i] = true;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.scroll += match delta {
                    MouseScrollDelta::LineDelta(x, y) => vec2(*x, *y),
                    MouseScrollDelta::PixelDelta(p) => vec2(p.x as f32, p.y as f32) / 40.0,
                };
            }
            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                let command = if cfg!(target_os = "macos") { s.contains(ModifiersState::SUPER) } else { s.contains(ModifiersState::CONTROL) };
                self.mods = Mods { shift: s.contains(ModifiersState::SHIFT), command, alt: s.contains(ModifiersState::ALT) };
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                // Ctrl+Alt is AltGr on some layouts: that types characters, it isn't a shortcut
                let shortcut = self.mods.command && !self.mods.alt;
                let key = match &event.logical_key {
                    WKey::Named(NamedKey::Backspace) => Some(Key::Backspace),
                    WKey::Named(NamedKey::Delete) if self.mods.shift => Some(Key::Cut),
                    WKey::Named(NamedKey::Delete) => Some(Key::Delete),
                    WKey::Named(NamedKey::ArrowLeft) => Some(Key::Left),
                    WKey::Named(NamedKey::ArrowRight) => Some(Key::Right),
                    WKey::Named(NamedKey::Home) => Some(Key::Home),
                    WKey::Named(NamedKey::End) => Some(Key::End),
                    WKey::Named(NamedKey::Enter) => Some(Key::Enter),
                    WKey::Named(NamedKey::Escape) => Some(Key::Escape),
                    WKey::Named(NamedKey::Tab) => Some(Key::Tab),
                    WKey::Named(NamedKey::Copy) => Some(Key::Copy),
                    WKey::Named(NamedKey::Cut) => Some(Key::Cut),
                    WKey::Named(NamedKey::Paste) => Some(Key::Paste),
                    WKey::Named(NamedKey::Insert) if self.mods.shift => Some(Key::Paste),
                    WKey::Named(NamedKey::Insert) if self.mods.command => Some(Key::Copy),
                    WKey::Character(c) if shortcut => match c.to_lowercase().as_str() {
                        "a" => Some(Key::SelectAll),
                        "c" => Some(Key::Copy),
                        "x" => Some(Key::Cut),
                        "v" => Some(Key::Paste),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(k) = key {
                    self.keys.push(k);
                } else if !shortcut && let Some(t) = &event.text {
                    self.text.extend(t.chars().filter(|c| !c.is_control()));
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => self.ppp = *scale_factor as f32,
            WindowEvent::Focused(false) => self.mods = Mods::default(),
            _ => {}
        }
    }

    pub(crate) fn end_frame(&mut self) {
        self.pressed = [false; 3];
        self.released = [false; 3];
        self.scroll = Vec2::ZERO;
        self.text.clear();
        self.keys.clear();
    }
}
