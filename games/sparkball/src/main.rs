//! Sparkball: roll a ball around an arena and collect every spark before
//! the clock runs out. Mind the sweeper, and do not fall off.
//!
//! Controls: WASD or arrow keys to roll, Space to jump, Escape to pause,
//! F3 for the engine's debug overlay.
//!
//! Run from the repository root with: `cargo run -p sparkball`
//!
//! Options:
//!   --level <file>      scene file under `assets/` (default scenes/arena.ron)
//!   --time <seconds>    time limit (default 90)
//!   --demo              skip the menu and let the game play itself
//!   --exit-after <s>    quit after this many seconds, for automated runs

use engine::egui::{self, Align2, Color32, RichText};
use engine::math::{Quat, Vec3};
use engine::{
    despawn_recursive, find_by_name, Collider, Config, Context, Entity, Game, KeyCode,
    MaterialDesc, Name, Parent, Playback, PointLight, RigidBody, Sound, Transform, Visual,
    WindowConfig,
};

const BALL_RADIUS: f32 = 0.4;
/// Horizontal acceleration while a direction is held, in units/s².
const ACCELERATION: f32 = 22.0;
const MAX_SPEED: f32 = 9.0;
const JUMP_SPEED: f32 = 6.5;
/// A jump pressed slightly before landing still counts.
const JUMP_BUFFER: f32 = 0.15;
/// Below this height the ball has fallen off the arena.
const FALL_HEIGHT: f32 = -6.0;
/// Seconds lost for falling off.
const FALL_PENALTY: f32 = 10.0;
const SWEEPER_SPEED: f32 = 1.1;
const SPARK_COLOR: Vec3 = Vec3::new(0.3, 0.9, 1.0);

/// A spark waiting to be collected.
struct Spark;

/// Makes a spark's visible part hover and turn.
struct Hover {
    phase: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Menu,
    Playing,
    Paused,
    Won,
    Lost,
}

struct Options {
    level: String,
    time_limit: f32,
    demo: bool,
    exit_after: Option<f32>,
}

impl Options {
    fn from_args() -> Self {
        let mut options = Self {
            level: "scenes/arena.ron".to_owned(),
            time_limit: 90.0,
            demo: false,
            exit_after: None,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut number = || args.next().and_then(|value| value.parse::<f32>().ok());
            match arg.as_str() {
                "--demo" => options.demo = true,
                "--time" => options.time_limit = number().unwrap_or(options.time_limit),
                "--exit-after" => options.exit_after = number(),
                "--level" => {
                    if let Some(level) = args.next() {
                        options.level = level;
                    }
                }
                other => eprintln!("ignoring unknown option {other}"),
            }
        }
        options
    }
}

#[derive(Default)]
struct Sounds {
    pickup: Option<Sound>,
    fall: Option<Sound>,
    win: Option<Sound>,
    lose: Option<Sound>,
}

struct Sparkball {
    options: Options,
    state: State,
    sounds: Sounds,

    player: Option<Entity>,
    start: Vec3,
    sweepers: Vec<Entity>,
    sweeper_angle: f32,

    sparks_total: usize,
    sparks_left: usize,
    time_left: f32,
    falls: u32,
    jump_buffer: f32,
    /// The point the camera looks at; trails the ball smoothly.
    camera_focus: Vec3,
    /// Demo mode: how long the ball has barely moved, and how many sparks
    /// to pass over because of it.
    demo_stuck_for: f32,
    demo_skip: usize,
}

impl Sparkball {
    fn new(options: Options) -> Self {
        Self {
            time_left: options.time_limit,
            options,
            state: State::Menu,
            sounds: Sounds::default(),
            player: None,
            start: Vec3::Y,
            sweepers: Vec::new(),
            sweeper_angle: 0.0,
            sparks_total: 0,
            sparks_left: 0,
            falls: 0,
            jump_buffer: 0.0,
            camera_focus: Vec3::ZERO,
            demo_stuck_for: 0.0,
            demo_skip: 0,
        }
    }

    /// Replaces the world with a fresh copy of the level.
    fn load_level(&mut self, ctx: &mut Context) {
        ctx.world.clear();
        if let Err(e) = ctx.load_scene(&self.options.level) {
            engine::log::error!("{e}");
            ctx.exit();
            return;
        }

        self.start = find_by_name(&ctx.world, "Start")
            .first()
            .and_then(|&start| {
                ctx.world
                    .get::<&Transform>(start)
                    .ok()
                    .map(|t| t.translation)
            })
            .unwrap_or(Vec3::Y);
        self.sweepers = find_by_name(&ctx.world, "Sweeper");
        self.sweeper_angle = 0.0;

        self.player = Some(ctx.world.spawn((
            Name("Player".to_owned()),
            Transform::from_translation(self.start),
            Visual::Sphere {
                radius: BALL_RADIUS,
                material: MaterialDesc {
                    texture: Some("textures/ball.png".to_owned()),
                    roughness: 0.35,
                    ..MaterialDesc::default()
                },
            },
            Collider::sphere(BALL_RADIUS).with_friction(1.0),
            RigidBody {
                // Keeps the ball from spinning on forever once input stops.
                angular_damping: 0.6,
                linear_damping: 0.15,
                ..RigidBody::dynamic()
            },
        )));

        // Level files mark spark positions with bare "Cell" entities; give
        // each one a trigger zone, a light and a glowing, hovering core.
        let cells = find_by_name(&ctx.world, "Cell");
        for (i, &cell) in cells.iter().enumerate() {
            let light = PointLight {
                color: SPARK_COLOR,
                intensity: 2.0,
                range: 5.0,
            };
            let _ = ctx
                .world
                .insert(cell, (Spark, Collider::sphere(0.7).as_sensor(), light));
            ctx.world.spawn((
                Transform::IDENTITY,
                Parent(cell),
                Hover { phase: i as f32 },
                Visual::Cube {
                    size: 0.45,
                    material: MaterialDesc {
                        base_color: [0.0, 0.0, 0.0, 1.0],
                        emissive: (SPARK_COLOR * 5.0).to_array(),
                        ..MaterialDesc::default()
                    },
                },
            ));
        }
        self.sparks_total = cells.len();
        self.sparks_left = cells.len();
        self.time_left = self.options.time_limit;
        self.falls = 0;
        self.jump_buffer = 0.0;
        self.camera_focus = self.start;
    }

    fn start_game(&mut self, ctx: &mut Context) {
        self.load_level(ctx);
        self.state = State::Playing;
    }

    fn player_position(&self, ctx: &Context) -> Option<Vec3> {
        let transform = ctx.world.get::<&Transform>(self.player?).ok()?;
        Some(transform.translation)
    }

    /// The direction the player wants to roll in, on the ground plane.
    fn wish_direction(&self, ctx: &Context, position: Vec3) -> Vec3 {
        if self.options.demo {
            // Demo mode: head straight for a spark, nearest first. Each
            // time the ball gets stuck it moves on to the next nearest.
            let mut offsets: Vec<Vec3> = ctx
                .world
                .query::<&Transform>()
                .with::<&Spark>()
                .iter()
                .map(|transform| (transform.translation - position).with_y(0.0))
                .collect();
            offsets.sort_by(|a, b| a.length_squared().total_cmp(&b.length_squared()));
            return offsets
                .get(self.demo_skip % offsets.len().max(1))
                .map_or(Vec3::ZERO, |offset| offset.normalize_or_zero());
        }
        let held = |a: KeyCode, b: KeyCode| ctx.input.key_down(a) || ctx.input.key_down(b);
        let axis = |positive: bool, negative: bool| positive as i32 as f32 - negative as i32 as f32;
        // The camera never turns, so screen directions are world directions.
        Vec3::new(
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
        .normalize_or_zero()
    }

    fn play(ctx: &mut Context, sound: &Option<Sound>, volume: f32) {
        if let Some(sound) = sound {
            let playback = Playback {
                volume,
                ..Playback::default()
            };
            ctx.audio.play_with(sound, playback);
        }
    }

    fn simulate_player(&mut self, ctx: &mut Context, dt: f32) {
        let (Some(player), Some(position)) = (self.player, self.player_position(ctx)) else {
            return;
        };

        if position.y < FALL_HEIGHT {
            self.falls += 1;
            self.time_left -= FALL_PENALTY;
            Self::play(ctx, &self.sounds.fall, 0.7);
            // Writing the Transform teleports the body; it keeps its speed
            // unless that is cleared as well.
            if let Ok(mut transform) = ctx.world.get::<&mut Transform>(player) {
                transform.translation = self.start;
            }
            ctx.physics.set_linear_velocity(player, Vec3::ZERO);
            ctx.physics.set_angular_velocity(player, Vec3::ZERO);
            return;
        }

        let Some(mut velocity) = ctx.physics.linear_velocity(player) else {
            return; // Not in the simulation until its first step.
        };
        if self.options.demo {
            let stuck = velocity.with_y(0.0).length() < 0.5;
            self.demo_stuck_for = if stuck { self.demo_stuck_for + dt } else { 0.0 };
            if self.demo_stuck_for > 1.0 {
                self.demo_stuck_for = 0.0;
                self.demo_skip += 1;
            }
        }
        let wish = self.wish_direction(ctx, position);
        let horizontal = velocity.with_y(0.0) + wish * ACCELERATION * dt;
        // Only steering is capped, so a shove from the sweeper can still
        // send the ball flying faster than it could roll by itself.
        let capped = if horizontal.length() > MAX_SPEED && wish != Vec3::ZERO {
            horizontal.clamp_length_max(velocity.with_y(0.0).length().max(MAX_SPEED))
        } else {
            horizontal
        };
        velocity.x = capped.x;
        velocity.z = capped.z;

        self.jump_buffer = (self.jump_buffer - dt).max(0.0);
        if self.jump_buffer > 0.0 {
            let ground =
                ctx.physics
                    .raycast(position, Vec3::NEG_Y, BALL_RADIUS + 0.1, Some(player));
            if ground.is_some() {
                velocity.y = JUMP_SPEED;
                self.jump_buffer = 0.0;
            }
        }
        ctx.physics.set_linear_velocity(player, velocity);
    }

    fn collect_sparks(&mut self, ctx: &mut Context) {
        let Some(player) = self.player else { return };
        let touched: Vec<Entity> = ctx
            .physics
            .collisions()
            .iter()
            .filter(|collision| collision.started)
            .filter_map(|collision| collision.other(player))
            .filter(|&other| ctx.world.satisfies::<&Spark>(other))
            .collect();
        for spark in touched {
            // Two contacts with the same spark can arrive in one frame.
            if !ctx.world.contains(spark) {
                continue;
            }
            despawn_recursive(&mut ctx.world, spark);
            self.sparks_left = self.sparks_left.saturating_sub(1);
            Self::play(ctx, &self.sounds.pickup, 0.8);
        }
    }

    fn update_camera(&mut self, ctx: &mut Context) {
        let dt = ctx.time.delta_secs();
        // The menu camera sits far back, so shadows must reach further.
        ctx.renderer.shadow_distance = if self.state == State::Menu {
            45.0
        } else {
            22.0
        };
        if self.state == State::Menu {
            let angle = ctx.time.elapsed().as_secs_f32() * 0.15;
            ctx.camera.position = Vec3::new(angle.sin() * 22.0, 13.0, angle.cos() * 22.0 + 4.0);
            ctx.camera.look_at(Vec3::new(0.0, 0.0, 4.0));
            return;
        }
        if let Some(position) = self.player_position(ctx) {
            // Frame-rate independent smoothing; do not follow a fall down.
            let target = position.with_y(position.y.max(0.0));
            self.camera_focus += (target - self.camera_focus) * (1.0 - (-8.0 * dt).exp());
        }
        ctx.camera.position = self.camera_focus + Vec3::new(0.0, 10.0, 9.0);
        ctx.camera.look_at(self.camera_focus);
    }

    fn draw_hud(&self, ui_ctx: &egui::Context) {
        egui::Area::new(egui::Id::unique("sparkball hud"))
            .anchor(Align2::CENTER_TOP, [0.0, 14.0])
            .show(ui_ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_black_alpha(150))
                    .corner_radius(8.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        let collected = self.sparks_total - self.sparks_left;
                        let time = self.time_left.max(0.0).ceil();
                        let time_color = if time <= 10.0 {
                            Color32::from_rgb(255, 110, 90)
                        } else {
                            Color32::WHITE
                        };
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("Sparks {collected}/{}", self.sparks_total))
                                    .size(22.0)
                                    .color(Color32::from_rgb(120, 230, 255)),
                            );
                            ui.add_space(18.0);
                            ui.label(
                                RichText::new(format!("Time {time:.0}"))
                                    .size(22.0)
                                    .color(time_color),
                            );
                        });
                    });
            });
    }

    /// A centred panel with a title, a line of text and buttons. Returns the
    /// index of the button clicked this frame.
    fn draw_panel(
        ui_ctx: &egui::Context,
        title: &str,
        text: &str,
        buttons: &[&str],
    ) -> Option<usize> {
        let mut clicked = None;
        egui::Window::new(title)
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui_ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new(title).size(36.0).strong());
                    ui.add_space(6.0);
                    ui.label(RichText::new(text).size(16.0));
                    ui.add_space(14.0);
                    for (i, label) in buttons.iter().enumerate() {
                        let button = egui::Button::new(RichText::new(*label).size(20.0));
                        if ui.add_sized([220.0, 38.0], button).clicked() {
                            clicked = Some(i);
                        }
                        ui.add_space(6.0);
                    }
                });
            });
        clicked
    }

    fn draw_ui(&mut self, ctx: &mut Context) {
        let ui_ctx = ctx.ui();
        if self.state != State::Menu {
            self.draw_hud(&ui_ctx);
        }
        let panel = match self.state {
            State::Playing => return,
            State::Menu => Self::draw_panel(
                &ui_ctx,
                "Sparkball",
                "Collect every spark before time runs out.\nWASD to roll, Space to jump, Esc to pause.",
                &["Play", "Quit"],
            ),
            State::Paused => Self::draw_panel(&ui_ctx, "Paused", "", &["Resume", "Restart", "Quit"])
                // Shifted so that index 1 is always "start a new game".
                .map(|i| if i == 0 { 2 } else { i - 1 }),
            State::Won => {
                let used = self.options.time_limit - self.time_left;
                let text = format!("All sparks collected in {used:.1} s, with {} falls.", self.falls);
                Self::draw_panel(&ui_ctx, "You win!", &text, &["Play again", "Quit"])
            }
            State::Lost => {
                let text = format!("{} sparks were left.", self.sparks_left);
                Self::draw_panel(&ui_ctx, "Out of time", &text, &["Try again", "Quit"])
            }
        };
        match panel {
            Some(0) => self.start_game(ctx),
            Some(1) => ctx.exit(),
            Some(_) => self.state = State::Playing, // "Resume"
            None => {}
        }
    }
}

impl Game for Sparkball {
    fn init(&mut self, ctx: &mut Context) {
        ctx.renderer.clear_color = Vec3::new(0.22, 0.4, 0.75);

        let mut load = |path: &str| {
            ctx.load_sound(path)
                .map_err(|e| engine::log::warn!("{e}"))
                .ok()
        };
        self.sounds = Sounds {
            pickup: load("sounds/pickup.wav"),
            fall: load("sounds/fall.wav"),
            win: load("sounds/win.wav"),
            lose: load("sounds/lose.wav"),
        };

        // The level doubles as the backdrop of the menu.
        self.load_level(ctx);
        if self.options.demo {
            self.state = State::Playing;
        }
    }

    fn fixed_update(&mut self, ctx: &mut Context) {
        if self.state == State::Paused || self.state == State::Lost {
            return;
        }
        let dt = ctx.time.fixed_delta_secs();

        // Kinematic bodies are moved by writing their Transform.
        self.sweeper_angle += SWEEPER_SPEED * dt;
        for &sweeper in &self.sweepers {
            if let Ok(mut transform) = ctx.world.get::<&mut Transform>(sweeper) {
                transform.rotation = Quat::from_rotation_y(self.sweeper_angle);
            }
        }

        if self.state != State::Playing {
            return;
        }
        self.simulate_player(ctx, dt);
        self.collect_sparks(ctx);

        self.time_left -= dt;
        if self.sparks_total > 0 && self.sparks_left == 0 {
            self.state = State::Won;
            Self::play(ctx, &self.sounds.win, 0.8);
        } else if self.time_left <= 0.0 {
            self.state = State::Lost;
            Self::play(ctx, &self.sounds.lose, 0.8);
        }
    }

    fn update(&mut self, ctx: &mut Context) {
        let elapsed = ctx.time.elapsed().as_secs_f32();
        if self
            .options
            .exit_after
            .is_some_and(|limit| elapsed >= limit)
        {
            ctx.exit();
        }

        if ctx.input.key_pressed(KeyCode::Escape) {
            self.state = match self.state {
                State::Playing => State::Paused,
                State::Paused => State::Playing,
                other => other,
            };
        }
        // Remembered rather than read in fixed_update, which may run zero or
        // several times in one frame and would miss or repeat the press.
        if ctx.input.key_pressed(KeyCode::Space) {
            self.jump_buffer = JUMP_BUFFER;
        }
        // The world freezes behind the pause and game-over panels.
        ctx.physics.paused = matches!(self.state, State::Paused | State::Lost);

        for (transform, hover) in ctx.world.query_mut::<(&mut Transform, &Hover)>() {
            let t = elapsed * 2.0 + hover.phase;
            transform.translation.y = t.sin() * 0.15;
            transform.rotation = Quat::from_rotation_y(t) * Quat::from_rotation_x(0.6);
        }

        self.update_camera(ctx);
        self.draw_ui(ctx);
    }
}

fn main() -> Result<(), engine::Error> {
    let config = Config {
        window: WindowConfig {
            title: "Sparkball".to_owned(),
            ..WindowConfig::default()
        },
        ..Config::default()
    };
    engine::run(config, Sparkball::new(Options::from_args()))
}
