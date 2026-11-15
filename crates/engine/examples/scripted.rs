//! A game whose logic is entirely in script files.
//!
//! This program only opens a window and loads a scene. The scene
//! (`assets/scenes/scripted.ron`) names the scripts in `assets/scripts/`
//! that move the player, spin and collect the coins and slide the platform.
//! Edit a script while this runs and the change takes effect at once.
//!
//! WASD or arrows roll the ball, Space jumps, F3 opens the debug overlay,
//! Escape quits.
//!
//! Run from the repository root with: `cargo run --example scripted`

use engine::math::Vec3;
use engine::{Config, Context, Game, KeyCode, WindowConfig};

struct Scripted;

impl Game for Scripted {
    fn init(&mut self, ctx: &mut Context) {
        ctx.renderer.clear_color = Vec3::new(0.22, 0.4, 0.75);
        if let Err(e) = ctx.load_scene("scenes/scripted.ron") {
            engine::log::error!("{e}");
            ctx.exit();
        }
    }

    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.key_pressed(KeyCode::Escape) {
            ctx.exit();
        }
        // Lets automated runs stop on their own: SCRIPTED_SECONDS=3.
        if let Some(limit) = std::env::var("SCRIPTED_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
        {
            if ctx.time.elapsed().as_secs_f32() >= limit {
                ctx.exit();
            }
        }
    }
}

fn main() -> Result<(), engine::Error> {
    let config = Config {
        window: WindowConfig {
            title: "Scripted".to_owned(),
            ..WindowConfig::default()
        },
        ..Config::default()
    };
    engine::run(config, Scripted)
}
