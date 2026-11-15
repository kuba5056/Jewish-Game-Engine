//! Opens a window and logs the frame rate once per second. Escape quits.
//!
//! Run with: `cargo run --example hello_window`

use engine::{Config, Context, Game, KeyCode, WindowConfig};

#[derive(Default)]
struct Hello {
    fixed_steps: u64,
    frames_this_second: u32,
    second_timer: f32,
}

impl Game for Hello {
    fn init(&mut self, ctx: &mut Context) {
        let (w, h) = ctx.window_size();
        engine::log::info!("hello from the engine, drawable area {w}x{h}");
    }

    fn fixed_update(&mut self, _ctx: &mut Context) {
        self.fixed_steps += 1;
    }

    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.key_pressed(KeyCode::Escape) {
            ctx.exit();
        }

        self.frames_this_second += 1;
        self.second_timer += ctx.time.delta_secs();
        if self.second_timer >= 1.0 {
            engine::log::info!(
                "{} fps, {} fixed steps so far",
                self.frames_this_second,
                self.fixed_steps
            );
            self.frames_this_second = 0;
            self.second_timer -= 1.0;
        }

        // Lets automated runs stop on their own: HELLO_WINDOW_SECONDS=3.
        if let Some(limit) = std::env::var("HELLO_WINDOW_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
        {
            if ctx.time.elapsed().as_secs_f32() >= limit {
                ctx.exit();
            }
        }
    }

    fn shutdown(&mut self, _ctx: &mut Context) {
        engine::log::info!("bye, ran {} fixed steps", self.fixed_steps);
    }
}

fn main() -> Result<(), engine::Error> {
    let config = Config {
        window: WindowConfig {
            title: "Hello Window".to_owned(),
            ..WindowConfig::default()
        },
        ..Config::default()
    };
    engine::run(config, Hello::default())
}
