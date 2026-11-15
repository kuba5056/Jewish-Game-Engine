//! Loads a scene from a file and shows the debug overlay.
//!
//! F3 toggles the overlay. Click an entity in the list to inspect and edit
//! it; "Save scene" writes the world back to a file under `assets/`.
//! Escape quits.
//!
//! Run from the repository root with: `cargo run --example scene`

use engine::math::Vec3;
use engine::{Config, Context, Game, KeyCode, WindowConfig};

struct SceneDemo;

impl Game for SceneDemo {
    fn init(&mut self, ctx: &mut Context) {
        ctx.renderer.clear_color = Vec3::new(0.25, 0.45, 0.8);
        ctx.camera.position = Vec3::new(1.5, 3.0, 8.0);
        ctx.camera.look_at(Vec3::new(0.0, 1.0, 0.0));

        // SCENE_FILE picks another scene, e.g. one saved from the overlay.
        let path = std::env::var("SCENE_FILE").unwrap_or_else(|_| "scenes/demo.ron".to_owned());
        match ctx.load_scene(&path) {
            Ok(entities) => engine::log::info!("loaded {} entities from {path}", entities.len()),
            Err(e) => {
                engine::log::error!("{e}");
                ctx.exit();
            }
        }
    }

    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.key_pressed(KeyCode::Escape) {
            ctx.exit();
        }
        // Lets automated runs stop on their own: SCENE_SECONDS=3.
        if let Some(limit) = std::env::var("SCENE_SECONDS")
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
            title: "Scene".to_owned(),
            ..WindowConfig::default()
        },
        show_debug_overlay: true,
        ..Config::default()
    };
    engine::run(config, SceneDemo)
}
