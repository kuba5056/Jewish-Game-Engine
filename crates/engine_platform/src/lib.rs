//! Windowing and input, built on `winit` (X11/Wayland on Linux, Win32 on Windows).

mod input;

pub use input::Input;
pub use winit;
pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;

/// Settings for the main window.
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Game".to_owned(),
            width: 1280,
            height: 720,
            resizable: true,
        }
    }
}
