//! The built-in debug overlay: frame statistics, renderer switches, an
//! entity list and an inspector for the selected entity.

use engine_physics::{Collider, RigidBody};
use engine_render::glam::{EulerRot, Quat, Vec3};
use engine_scene::{despawn_recursive, Entity, Name, Transform};
use engine_ui::egui;

use crate::app::Context;
use crate::assets::ModelPart;
use crate::scene::{DirectionalLight, MeshRenderer, PointLight};

pub(crate) struct DebugUi {
    pub(crate) open: bool,
    selected: Option<Entity>,
    /// Exponentially smoothed frame time, so the readout is legible.
    frame_ms: f32,
    scene_path: String,
    /// Result of the last "Save scene".
    status: String,
}

impl DebugUi {
    pub(crate) fn new(open: bool) -> Self {
        Self {
            open,
            selected: None,
            frame_ms: 16.7,
            scene_path: "scenes/saved.ron".to_owned(),
            status: String::new(),
        }
    }

    pub(crate) fn draw(&mut self, ctx: &mut Context) {
        self.frame_ms += (ctx.time.delta_secs() * 1000.0 - self.frame_ms) * 0.05;
        if !self.open {
            return;
        }
        let ui_ctx = ctx.ui();
        self.stats_window(&ui_ctx, ctx);
        self.entities_window(&ui_ctx, ctx);
        self.inspector_window(&ui_ctx, ctx);
    }

    fn stats_window(&mut self, ui_ctx: &egui::Context, ctx: &mut Context) {
        egui::Window::new("Debug")
            .default_pos([12.0, 12.0])
            .default_width(230.0)
            .show(ui_ctx, |ui| {
                ui.label(format!(
                    "{:.0} fps ({:.1} ms)",
                    1000.0 / self.frame_ms.max(0.001),
                    self.frame_ms
                ));
                ui.label(format!(
                    "{} entities, {} meshes drawn",
                    ctx.world.len(),
                    ctx.renderer.queued_draws()
                ));
                ui.label(format!("{} particles", ctx.renderer.queued_particles()));
                ui.separator();
                ui.checkbox(&mut ctx.renderer.shadows, "Shadows");
                ui.add(egui::Slider::new(&mut ctx.renderer.exposure, 0.1..=4.0).text("Exposure"));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("File");
                    ui.text_edit_singleline(&mut self.scene_path);
                });
                if ui.button("Save scene").clicked() {
                    self.status = match ctx.save_scene(&self.scene_path) {
                        Ok(()) => format!("Saved {}", self.scene_path),
                        Err(e) => e.to_string(),
                    };
                }
                if !self.status.is_empty() {
                    ui.label(&self.status);
                }
            });
    }

    fn entities_window(&mut self, ui_ctx: &egui::Context, ctx: &mut Context) {
        // Model parts are listed indented under a note rather than hidden:
        // they are often exactly what needs inspecting.
        let mut entities: Vec<(Entity, String)> = ctx
            .world
            .query::<(Entity, Option<&Name>, Option<&ModelPart>)>()
            .with::<&Transform>()
            .iter()
            .map(|(entity, name, part)| {
                let label = match name {
                    Some(name) => name.0.clone(),
                    None => format!("Entity {}", entity.id()),
                };
                let indent = if part.is_some() { "    " } else { "" };
                (entity, format!("{indent}{label}"))
            })
            .collect();
        entities.sort_by_key(|(entity, _)| entity.to_bits());

        egui::Window::new("Entities")
            .default_pos([12.0, 250.0])
            .default_width(230.0)
            .show(ui_ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for (entity, label) in &entities {
                            let selected = self.selected == Some(*entity);
                            if ui.selectable_label(selected, label).clicked() {
                                self.selected = (!selected).then_some(*entity);
                            }
                        }
                    });
            });
    }

    fn inspector_window(&mut self, ui_ctx: &egui::Context, ctx: &mut Context) {
        let Some(entity) = self.selected.filter(|&e| ctx.world.contains(e)) else {
            self.selected = None;
            return;
        };
        let title = match ctx.world.get::<&Name>(entity) {
            Ok(name) => name.0.clone(),
            Err(_) => format!("Entity {}", entity.id()),
        };
        let mut despawn = false;

        egui::Window::new("Inspector")
            .default_pos([260.0, 12.0])
            .default_width(260.0)
            .show(ui_ctx, |ui| {
                ui.heading(title);
                let world = &ctx.world;

                if let Ok(mut transform) = world.get::<&mut Transform>(entity) {
                    ui.separator();
                    ui.strong("Transform");
                    vec3_row(ui, "Position", &mut transform.translation, 0.05);
                    // Shown as yaw/pitch/roll in degrees; converted back only
                    // when edited, so merely looking never alters the rotation.
                    let (yaw, pitch, roll) = transform.rotation.to_euler(EulerRot::YXZ);
                    let mut degrees =
                        Vec3::new(pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees());
                    // Rounded for display; adding zero turns "-0" into "0".
                    degrees = (degrees * 10.0).round() / 10.0 + Vec3::ZERO;
                    if vec3_row(ui, "Rotation", &mut degrees, 1.0) {
                        transform.rotation = Quat::from_euler(
                            EulerRot::YXZ,
                            degrees.y.to_radians(),
                            degrees.x.to_radians(),
                            degrees.z.to_radians(),
                        );
                    }
                    vec3_row(ui, "Scale", &mut transform.scale, 0.02);
                }

                if let Ok(mut renderer) = world.get::<&mut MeshRenderer>(entity) {
                    ui.separator();
                    ui.strong("Material");
                    let material = &mut renderer.material;
                    let mut rgb = material.base_color.truncate().to_array();
                    ui.horizontal(|ui| {
                        ui.label("Colour");
                        if ui.color_edit_button_rgb(&mut rgb).changed() {
                            material.base_color = Vec3::from(rgb).extend(material.base_color.w);
                        }
                    });
                    ui.add(egui::Slider::new(&mut material.metallic, 0.0..=1.0).text("Metallic"));
                    ui.add(egui::Slider::new(&mut material.roughness, 0.0..=1.0).text("Roughness"));
                    vec3_row(ui, "Emissive", &mut material.emissive, 0.05);
                }

                if let Ok(mut light) = world.get::<&mut DirectionalLight>(entity) {
                    ui.separator();
                    ui.strong("Directional light");
                    color_row(ui, &mut light.color);
                    drag_row(ui, "Intensity", &mut light.intensity, 0.05);
                }

                if let Ok(mut light) = world.get::<&mut PointLight>(entity) {
                    ui.separator();
                    ui.strong("Point light");
                    color_row(ui, &mut light.color);
                    drag_row(ui, "Intensity", &mut light.intensity, 0.1);
                    drag_row(ui, "Range", &mut light.range, 0.1);
                }

                // Physics settings apply when a body is created, so they are
                // shown but not editable here.
                if let Ok(collider) = world.get::<&Collider>(entity) {
                    ui.separator();
                    ui.strong("Collider");
                    ui.label(format!("{:?}", collider.shape));
                    if let Ok(body) = world.get::<&RigidBody>(entity) {
                        ui.label(format!("{:?} body", body.kind));
                    } else {
                        ui.label("Static");
                    }
                    if let Some(velocity) = ctx.physics.linear_velocity(entity) {
                        ui.label(format!("Speed {:.2}", velocity.length()));
                    }
                }

                ui.separator();
                despawn = ui.button("Despawn").clicked();
            });

        if despawn {
            despawn_recursive(&mut ctx.world, entity);
            self.selected = None;
        }
    }
}

/// A labelled row of three draggable numbers. Returns whether any changed.
fn vec3_row(ui: &mut egui::Ui, label: &str, value: &mut Vec3, speed: f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        for axis in [&mut value.x, &mut value.y, &mut value.z] {
            changed |= ui.add(egui::DragValue::new(axis).speed(speed)).changed();
        }
    });
    changed
}

fn drag_row(ui: &mut egui::Ui, label: &str, value: &mut f32, speed: f32) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::DragValue::new(value)
                .speed(speed)
                .range(0.0..=f32::MAX),
        );
    });
}

fn color_row(ui: &mut egui::Ui, color: &mut Vec3) {
    let mut rgb = color.to_array();
    ui.horizontal(|ui| {
        ui.label("Colour");
        if ui.color_edit_button_rgb(&mut rgb).changed() {
            *color = Vec3::from(rgb);
        }
    });
}
