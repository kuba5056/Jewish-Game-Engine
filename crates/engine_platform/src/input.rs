use std::collections::HashSet;

use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

/// Keyboard and mouse state for the current frame.
///
/// Keys are physical (layout-independent) codes, so WASD stays in the same
/// place on any keyboard layout.
#[derive(Debug, Default)]
pub struct Input {
    keys_down: HashSet<KeyCode>,
    keys_pressed: HashSet<KeyCode>,
    keys_released: HashSet<KeyCode>,
    buttons_down: HashSet<MouseButton>,
    buttons_pressed: HashSet<MouseButton>,
    buttons_released: HashSet<MouseButton>,
    cursor: Option<(f32, f32)>,
    mouse_delta: (f32, f32),
    scroll: f32,
}

impl Input {
    /// Is the key currently held?
    pub fn key_down(&self, key: KeyCode) -> bool {
        self.keys_down.contains(&key)
    }

    /// Did the key go down this frame?
    pub fn key_pressed(&self, key: KeyCode) -> bool {
        self.keys_pressed.contains(&key)
    }

    /// Did the key go up this frame?
    pub fn key_released(&self, key: KeyCode) -> bool {
        self.keys_released.contains(&key)
    }

    /// Is the mouse button currently held?
    pub fn button_down(&self, button: MouseButton) -> bool {
        self.buttons_down.contains(&button)
    }

    /// Did the mouse button go down this frame?
    pub fn button_pressed(&self, button: MouseButton) -> bool {
        self.buttons_pressed.contains(&button)
    }

    /// Did the mouse button go up this frame?
    pub fn button_released(&self, button: MouseButton) -> bool {
        self.buttons_released.contains(&button)
    }

    /// Cursor position in window pixels, or `None` while it is outside the window.
    pub fn cursor(&self) -> Option<(f32, f32)> {
        self.cursor
    }

    /// Raw mouse movement this frame (unaffected by the cursor hitting the
    /// window edge), for camera look.
    pub fn mouse_delta(&self) -> (f32, f32) {
        self.mouse_delta
    }

    /// Vertical scroll this frame, in lines; positive is away from the user.
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// Feeds a window event into the state.
    pub fn handle_window_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.set_key(code, event.state == ElementState::Pressed);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.set_button(*button, *state == ElementState::Pressed);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some((position.x as f32, position.y as f32));
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::MouseWheel { delta, .. } => {
                use winit::event::MouseScrollDelta::*;
                self.scroll += match delta {
                    LineDelta(_, y) => *y,
                    // Touchpads report pixels; treat a typical line as 20 px.
                    PixelDelta(p) => p.y as f32 / 20.0,
                };
            }
            // Keys released while unfocused never reach us, so drop held state
            // instead of leaving them stuck.
            WindowEvent::Focused(false) => self.release_all(),
            _ => {}
        }
    }

    /// Adds raw (device-level) mouse motion.
    pub fn add_mouse_motion(&mut self, dx: f32, dy: f32) {
        self.mouse_delta.0 += dx;
        self.mouse_delta.1 += dy;
    }

    /// Clears per-frame state. Call once at the end of every frame.
    pub fn end_frame(&mut self) {
        self.keys_pressed.clear();
        self.keys_released.clear();
        self.buttons_pressed.clear();
        self.buttons_released.clear();
        self.mouse_delta = (0.0, 0.0);
        self.scroll = 0.0;
    }

    fn set_key(&mut self, key: KeyCode, down: bool) {
        if down {
            // OS key repeat sends repeated presses; only the first counts.
            if self.keys_down.insert(key) {
                self.keys_pressed.insert(key);
            }
        } else if self.keys_down.remove(&key) {
            self.keys_released.insert(key);
        }
    }

    fn set_button(&mut self, button: MouseButton, down: bool) {
        if down {
            if self.buttons_down.insert(button) {
                self.buttons_pressed.insert(button);
            }
        } else if self.buttons_down.remove(&button) {
            self.buttons_released.insert(button);
        }
    }

    fn release_all(&mut self) {
        self.keys_released.extend(self.keys_down.drain());
        self.buttons_released.extend(self.buttons_down.drain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_hold_release() {
        let mut input = Input::default();
        input.set_key(KeyCode::KeyW, true);
        assert!(input.key_pressed(KeyCode::KeyW));
        assert!(input.key_down(KeyCode::KeyW));

        input.end_frame();
        input.set_key(KeyCode::KeyW, true); // OS key repeat
        assert!(!input.key_pressed(KeyCode::KeyW));
        assert!(input.key_down(KeyCode::KeyW));

        input.set_key(KeyCode::KeyW, false);
        assert!(input.key_released(KeyCode::KeyW));
        assert!(!input.key_down(KeyCode::KeyW));
    }

    #[test]
    fn losing_focus_releases_everything() {
        let mut input = Input::default();
        input.set_key(KeyCode::Space, true);
        input.set_button(MouseButton::Left, true);
        input.end_frame();

        input.handle_window_event(&WindowEvent::Focused(false));
        assert!(!input.key_down(KeyCode::Space));
        assert!(input.key_released(KeyCode::Space));
        assert!(input.button_released(MouseButton::Left));
    }

    #[test]
    fn per_frame_values_reset() {
        let mut input = Input::default();
        input.add_mouse_motion(3.0, -2.0);
        input.add_mouse_motion(1.0, 1.0);
        assert_eq!(input.mouse_delta(), (4.0, -1.0));
        input.end_frame();
        assert_eq!(input.mouse_delta(), (0.0, 0.0));
    }
}
