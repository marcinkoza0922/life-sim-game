//! Pointer and keyboard state, fed from winit events.

use glam::{Vec2, vec2};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key as WKey, NamedKey};

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
    /// Pixels per point.
    pub ppp: f32,
}

impl Default for Input {
    fn default() -> Self {
        Input { pointer: None, down: [false; 3], pressed: [false; 3], released: [false; 3], scroll: Vec2::ZERO, text: String::new(), keys: Vec::new(), ppp: 1.0 }
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
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let key = match &event.logical_key {
                    WKey::Named(NamedKey::Backspace) => Some(Key::Backspace),
                    WKey::Named(NamedKey::Delete) => Some(Key::Delete),
                    WKey::Named(NamedKey::ArrowLeft) => Some(Key::Left),
                    WKey::Named(NamedKey::ArrowRight) => Some(Key::Right),
                    WKey::Named(NamedKey::Home) => Some(Key::Home),
                    WKey::Named(NamedKey::End) => Some(Key::End),
                    WKey::Named(NamedKey::Enter) => Some(Key::Enter),
                    WKey::Named(NamedKey::Escape) => Some(Key::Escape),
                    WKey::Named(NamedKey::Tab) => Some(Key::Tab),
                    _ => None,
                };
                if let Some(k) = key {
                    self.keys.push(k);
                } else if let Some(t) = &event.text {
                    self.text.extend(t.chars().filter(|c| !c.is_control()));
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => self.ppp = *scale_factor as f32,
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
