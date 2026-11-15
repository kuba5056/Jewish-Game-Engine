//! A starting point for a game on the engine.
//!
//! The scene comes from `assets/scenes/main.ron`. This file adds the part
//! that is specific to the game: moving the player and a small HUD.
//!
//! WASD or the arrow keys roll the ball, Space jumps, F3 opens the debug
//! overlay, Escape quits. Run with `cargo run` from this directory.

use engine::egui;
use engine::math::Vec3;
use engine::{find_by_name, Config, Context, Entity, Game, KeyCode, Transform, WindowConfig};

const SPEED: f32 = 6.0;
const JUMP_SPEED: f32 = 6.0;
/// Seconds a jump press is remembered, so pressing just before landing works.
const JUMP_BUFFER: f32 = 0.15;

#[derive(Default)]
struct MyGame {
    player: Option<Entity>,
    jump_buffer: f32,
}

impl MyGame {
    fn player_position(&self, ctx: &Context) -> Option<Vec3> {
        let transform = ctx.world.get::<&Transform>(self.player?).ok()?;
        Some(transform.translation)
    }
}

impl Game for MyGame {
    /// Once, after the window exists.
    fn init(&mut self, ctx: &mut Context) {
        ctx.renderer.clear_color = Vec3::new(0.22, 0.4, 0.75);
        if let Err(e) = ctx.load_scene("scenes/main.ron") {
            engine::log::error!("{e}");
            ctx.exit();
            return;
        }
        self.player = find_by_name(&ctx.world, "Player").first().copied();
    }

    /// At a fixed 60 Hz, right after each physics step: movement goes here.
    fn fixed_update(&mut self, ctx: &mut Context) {
        let (Some(player), Some(position)) = (self.player, self.player_position(ctx)) else {
            return;
        };
        // `None` until the first physics step has created the body.
        let Some(mut velocity) = ctx.physics.linear_velocity(player) else {
            return;
        };

        let held = |a: KeyCode, b: KeyCode| ctx.input.key_down(a) || ctx.input.key_down(b);
        let axis = |positive: bool, negative: bool| positive as i32 as f32 - negative as i32 as f32;
        let wish = Vec3::new(
            axis(
                held(KeyCode::KeyD, KeyCode::ArrowRight),
                held(KeyCode::KeyA, KeyCode::ArrowLeft),
            ),
            0.0,
            axis(
                held(KeyCode::KeyS, KeyCode::ArrowDown),
                held(KeyCode::KeyW, KeyCode::ArrowUp),
            ),
        )
        .normalize_or_zero();
        velocity.x = wish.x * SPEED;
        velocity.z = wish.z * SPEED;

        self.jump_buffer = (self.jump_buffer - ctx.time.fixed_delta_secs()).max(0.0);
        if self.jump_buffer > 0.0 {
            // A short ray downwards, ignoring the player itself.
            let ground = ctx
                .physics
                .raycast(position, Vec3::NEG_Y, 0.6, Some(player));
            if ground.is_some() {
                velocity.y = JUMP_SPEED;
                self.jump_buffer = 0.0;
            }
        }
        ctx.physics.set_linear_velocity(player, velocity);

        // Fell off the floor: back to the start.
        if position.y < -6.0 {
            if let Ok(mut transform) = ctx.world.get::<&mut Transform>(player) {
                transform.translation = Vec3::new(0.0, 1.0, 3.0);
            }
            ctx.physics.set_linear_velocity(player, Vec3::ZERO);
        }
    }

    /// Once per frame: input that must not be missed, camera, interface.
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.key_pressed(KeyCode::Escape) {
            ctx.exit();
        }
        // One-off presses are read here and remembered: `fixed_update` can
        // run zero or several times in a frame and would miss or repeat them.
        if ctx.input.key_pressed(KeyCode::Space) {
            self.jump_buffer = JUMP_BUFFER;
        }

        if let Some(position) = self.player_position(ctx) {
            ctx.camera.position = position + Vec3::new(0.0, 6.0, 8.0);
            ctx.camera.look_at(position);
        }

        egui::Area::new(egui::Id::unique("hud"))
            .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
            .show(&ctx.ui(), |ui| {
                ui.label(
                    egui::RichText::new("WASD: roll   Space: jump   F3: debug   Esc: quit")
                        .size(16.0)
                        .color(egui::Color32::WHITE),
                );
            });

        // Lets automated runs stop on their own: GAME_SECONDS=3.
        if let Some(limit) = std::env::var("GAME_SECONDS")
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
            title: "My Game".to_owned(),
            ..WindowConfig::default()
        },
        ..Config::default()
    };
    engine::run(config, MyGame::default())
}
