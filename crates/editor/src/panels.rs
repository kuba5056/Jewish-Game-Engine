//! The editor's user interface: toolbar, entity tree and inspector.

use std::collections::HashMap;

use engine::egui::{self, Align2};
use engine::math::{EulerRot, Quat, Vec3, Vec4};
use engine::{
    BodyKind, Collider, Context, DirectionalLight, Entity, MaterialDesc, ModelPart, Name, Parent,
    ParticleEmitter, PointLight, RigidBody, Script, Shape, Transform, Visual, World,
};

use crate::Editor;

/// Something the panels ask the editor to do once they have been drawn.
pub enum Action {
    New,
    Open,
    Save,
    Play,
    Stop,
    Add(NewEntity),
    Select(Option<Entity>),
    Duplicate(Entity),
    Delete(Entity),
}

#[derive(Clone, Copy)]
pub enum NewEntity {
    Empty,
    Cube,
    Sphere,
    Plane,
    Model,
    Sun,
    Lamp,
    Particles,
}

pub fn draw(editor: &mut Editor, ctx: &mut Context) -> Vec<Action> {
    let ui_ctx = ctx.ui();
    let mut actions = Vec::new();
    toolbar(editor, &ui_ctx, &mut actions);
    tree(editor, &ui_ctx, &ctx.world, &mut actions);
    if let Some(entity) = editor.selected.filter(|&e| ctx.world.contains(e)) {
        inspector(editor, &ui_ctx, &mut ctx.world, entity, &mut actions);
    }
    actions
}

fn label_of(world: &World, entity: Entity) -> String {
    match world.get::<&Name>(entity) {
        Ok(name) if !name.0.is_empty() => name.0.clone(),
        _ => format!("Entity {}", entity.id()),
    }
}

// --- Toolbar ----------------------------------------------------------------

fn toolbar(editor: &mut Editor, ui_ctx: &egui::Context, actions: &mut Vec<Action>) {
    egui::Window::new("Toolbar")
        .title_bar(false)
        .resizable(false)
        .anchor(Align2::CENTER_TOP, [0.0, 8.0])
        .show(ui_ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("New").clicked() {
                    actions.push(Action::New);
                }
                ui.add(egui::TextEdit::singleline(&mut editor.path).desired_width(190.0));
                if ui.button("Open").clicked() {
                    actions.push(Action::Open);
                }
                if ui.button("Save").clicked() {
                    actions.push(Action::Save);
                }
                ui.separator();
                if editor.playing.is_some() {
                    if ui.button("Stop").clicked() {
                        actions.push(Action::Stop);
                    }
                } else if ui.button("Play").clicked() {
                    actions.push(Action::Play);
                }
                ui.separator();
                ui.menu_button("Add", |ui| {
                    let kinds = [
                        ("Empty", NewEntity::Empty),
                        ("Cube", NewEntity::Cube),
                        ("Sphere", NewEntity::Sphere),
                        ("Plane", NewEntity::Plane),
                        ("Model", NewEntity::Model),
                        ("Sun", NewEntity::Sun),
                        ("Lamp", NewEntity::Lamp),
                        ("Particles", NewEntity::Particles),
                    ];
                    for (label, kind) in kinds {
                        if ui.button(label).clicked() {
                            actions.push(Action::Add(kind));
                            ui.close();
                        }
                    }
                });
            });
            if !editor.status.is_empty() {
                ui.label(&editor.status);
            }
        });
}

// --- Entity tree ------------------------------------------------------------

fn tree(editor: &Editor, ui_ctx: &egui::Context, world: &World, actions: &mut Vec<Action>) {
    // Model parts are recreated from their file, so they are not listed.
    let mut entities: Vec<(Entity, Option<Entity>)> = world
        .query::<(Entity, Option<&Parent>)>()
        .with::<&Transform>()
        .without::<&ModelPart>()
        .iter()
        .map(|(entity, parent)| (entity, parent.map(|p| p.0)))
        .collect();
    entities.sort_by_key(|(entity, _)| entity.to_bits());
    let listed = |entity: Entity| entities.iter().any(|(e, _)| *e == entity);

    // Depth-first from the roots: an entity whose parent is not listed
    // counts as a root, so nothing can go missing from the tree.
    let mut rows: Vec<(Entity, usize)> = Vec::new();
    let mut pending: Vec<(Entity, usize)> = entities
        .iter()
        .rev()
        .filter(|(_, parent)| !parent.is_some_and(listed))
        .map(|(entity, _)| (*entity, 0))
        .collect();
    while let Some((entity, depth)) = pending.pop() {
        // A parent cycle would otherwise list its members forever.
        if rows.iter().any(|(e, _)| *e == entity) {
            continue;
        }
        rows.push((entity, depth));
        let children = entities
            .iter()
            .rev()
            .filter(|(_, parent)| *parent == Some(entity));
        pending.extend(children.map(|(child, _)| (*child, depth + 1)));
    }

    egui::Window::new("Scene")
        .resizable(false)
        .anchor(Align2::LEFT_TOP, [8.0, 8.0])
        .default_width(210.0)
        .show(ui_ctx, |ui| {
            ui.set_min_width(210.0);
            egui::ScrollArea::vertical()
                .max_height(520.0)
                .show(ui, |ui| {
                    for (entity, depth) in rows {
                        let selected = editor.selected == Some(entity);
                        ui.horizontal(|ui| {
                            ui.add_space(depth as f32 * 14.0);
                            if ui
                                .selectable_label(selected, label_of(world, entity))
                                .clicked()
                            {
                                actions.push(Action::Select((!selected).then_some(entity)));
                            }
                        });
                    }
                });
        });
}

// --- Inspector --------------------------------------------------------------

/// Shows an editable copy of a component and writes it back if it changed.
/// Does nothing if the entity does not have the component.
fn edit<T: Clone + PartialEq + Send + Sync + 'static>(
    world: &mut World,
    entity: Entity,
    ui_for: impl FnOnce(&mut T),
) {
    let Some(mut value) = world.get::<&T>(entity).ok().map(|c| (*c).clone()) else {
        return;
    };
    let before = value.clone();
    ui_for(&mut value);
    if value != before {
        let _ = world.insert_one(entity, value);
    }
}

/// A component's heading with a "Remove" button. Returns whether the entity
/// has the component; removes it if the button was clicked.
fn section<T: Send + Sync + 'static>(
    ui: &mut egui::Ui,
    world: &mut World,
    entity: Entity,
    title: &str,
) -> bool {
    if !world.satisfies::<&T>(entity) {
        return false;
    }
    ui.separator();
    let mut remove = false;
    ui.horizontal(|ui| {
        ui.strong(title);
        remove = ui.small_button("Remove").clicked();
    });
    if remove {
        let _ = world.remove_one::<T>(entity);
    }
    !remove
}

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

fn number_row(ui: &mut egui::Ui, label: &str, value: &mut f32, speed: f32) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).speed(speed));
    });
}

fn range_row(ui: &mut egui::Ui, label: &str, range: &mut (f32, f32), speed: f32) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(&mut range.0).speed(speed));
        ui.label("to");
        ui.add(egui::DragValue::new(&mut range.1).speed(speed));
    });
}

/// A text field for a path. Typing only changes a scratch copy; the new
/// value is returned once editing ends, so half-typed paths are never
/// loaded.
fn path_field(
    ui: &mut egui::Ui,
    scratch: &mut HashMap<&'static str, String>,
    key: &'static str,
    current: &str,
) -> Option<String> {
    let buffer = scratch.entry(key).or_insert_with(|| current.to_owned());
    let response = ui.add(egui::TextEdit::singleline(buffer).desired_width(190.0));
    if response.lost_focus() {
        return (buffer != current).then(|| buffer.clone());
    }
    if !response.has_focus() && buffer != current {
        // Changed elsewhere (another selection, undo of a failed load).
        *buffer = current.to_owned();
    }
    None
}

fn inspector(
    editor: &mut Editor,
    ui_ctx: &egui::Context,
    world: &mut World,
    entity: Entity,
    actions: &mut Vec<Action>,
) {
    let scratch = &mut editor.text;
    egui::Window::new("Inspector")
        .resizable(false)
        .anchor(Align2::RIGHT_TOP, [-8.0, 8.0])
        .default_width(290.0)
        .show(ui_ctx, |ui| {
            ui.set_min_width(290.0);
            egui::ScrollArea::vertical()
                .max_height(700.0)
                .show(ui, |ui| {
                    // Name: an empty field removes the component.
                    let mut name = world
                        .get::<&Name>(entity)
                        .map_or(String::new(), |n| n.0.clone());
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        if ui.text_edit_singleline(&mut name).changed() {
                            if name.is_empty() {
                                let _ = world.remove_one::<Name>(entity);
                            } else {
                                let _ = world.insert_one(entity, Name(name.clone()));
                            }
                        }
                    });
                    parent_row(ui, world, entity);
                    ui.horizontal(|ui| {
                        if ui.button("Duplicate").clicked() {
                            actions.push(Action::Duplicate(entity));
                        }
                        if ui.button("Delete").clicked() {
                            actions.push(Action::Delete(entity));
                        }
                    });

                    ui.separator();
                    ui.strong("Transform");
                    edit::<Transform>(world, entity, |transform| transform_rows(ui, transform));

                    if section::<Visual>(ui, world, entity, "Visual") {
                        edit::<Visual>(world, entity, |visual| visual_rows(ui, scratch, visual));
                    }
                    if section::<DirectionalLight>(ui, world, entity, "Directional light") {
                        edit::<DirectionalLight>(world, entity, |light| {
                            color_row(ui, &mut light.color);
                            number_row(ui, "Intensity", &mut light.intensity, 0.05);
                            ui.label("Shines along the entity's forward direction.");
                        });
                    }
                    if section::<PointLight>(ui, world, entity, "Point light") {
                        edit::<PointLight>(world, entity, |light| {
                            color_row(ui, &mut light.color);
                            number_row(ui, "Intensity", &mut light.intensity, 0.1);
                            number_row(ui, "Range", &mut light.range, 0.1);
                        });
                    }
                    if section::<Collider>(ui, world, entity, "Collider") {
                        edit::<Collider>(world, entity, |collider| collider_rows(ui, collider));
                    }
                    if section::<RigidBody>(ui, world, entity, "Rigid body") {
                        edit::<RigidBody>(world, entity, |body| body_rows(ui, body));
                    }
                    if section::<ParticleEmitter>(ui, world, entity, "Particles") {
                        edit::<ParticleEmitter>(world, entity, |emitter| {
                            particle_rows(ui, emitter)
                        });
                    }
                    if section::<Script>(ui, world, entity, "Script") {
                        edit::<Script>(world, entity, |script| {
                            if let Some(path) = path_field(ui, scratch, "script", &script.path) {
                                script.path = path;
                            }
                        });
                    }

                    ui.separator();
                    add_component_menu(ui, world, entity);
                });
        });
}

fn parent_row(ui: &mut egui::Ui, world: &mut World, entity: Entity) {
    let current = world.get::<&Parent>(entity).ok().map(|p| p.0);
    let mut candidates: Vec<Entity> = world
        .query::<Entity>()
        .with::<&Transform>()
        .without::<&ModelPart>()
        .iter()
        .filter(|&other| other != entity)
        .collect();
    candidates.sort_by_key(|e| e.to_bits());

    let mut choice = current;
    ui.horizontal(|ui| {
        ui.label("Parent");
        let shown = current.map_or("(none)".to_owned(), |p| label_of(world, p));
        egui::ComboBox::from_id_salt("parent")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut choice, None, "(none)");
                for candidate in candidates {
                    ui.selectable_value(&mut choice, Some(candidate), label_of(world, candidate));
                }
            });
    });
    if choice != current {
        match choice {
            Some(parent) => {
                let _ = world.insert_one(entity, Parent(parent));
            }
            None => {
                let _ = world.remove_one::<Parent>(entity);
            }
        }
    }
}

fn transform_rows(ui: &mut egui::Ui, transform: &mut Transform) {
    vec3_row(ui, "Position", &mut transform.translation, 0.05);
    // Shown as pitch/yaw/roll in degrees, and converted back only when
    // edited, so looking at a rotation never alters it.
    let (yaw, pitch, roll) = transform.rotation.to_euler(EulerRot::YXZ);
    let mut degrees = Vec3::new(pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees());
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

fn color_row(ui: &mut egui::Ui, color: &mut Vec3) {
    let mut rgb = color.to_array();
    ui.horizontal(|ui| {
        ui.label("Colour");
        if ui.color_edit_button_rgb(&mut rgb).changed() {
            *color = Vec3::from(rgb);
        }
    });
}

fn visual_rows(
    ui: &mut egui::Ui,
    scratch: &mut HashMap<&'static str, String>,
    visual: &mut Visual,
) {
    const KINDS: [&str; 4] = ["Cube", "Sphere", "Plane", "Model"];
    let kind = match visual {
        Visual::Cube { .. } => 0,
        Visual::Sphere { .. } => 1,
        Visual::Plane { .. } => 2,
        Visual::Model { .. } => 3,
    };
    let mut chosen = kind;
    ui.horizontal(|ui| {
        ui.label("Kind");
        egui::ComboBox::from_id_salt("visual kind")
            .selected_text(KINDS[kind])
            .show_ui(ui, |ui| {
                for (index, label) in KINDS.iter().enumerate() {
                    ui.selectable_value(&mut chosen, index, *label);
                }
            });
    });
    if chosen != kind {
        // Keep the material when switching between the built-in shapes.
        let material = match visual {
            Visual::Cube { material, .. }
            | Visual::Sphere { material, .. }
            | Visual::Plane { material, .. } => material.clone(),
            Visual::Model { .. } => MaterialDesc::default(),
        };
        *visual = match chosen {
            0 => Visual::Cube {
                size: 1.0,
                material,
            },
            1 => Visual::Sphere {
                radius: 0.5,
                material,
            },
            2 => Visual::Plane {
                size: 4.0,
                uv_repeat: 1.0,
                material,
            },
            _ => Visual::Model {
                path: "models/robot.gltf".to_owned(),
            },
        };
    }

    let material = match visual {
        Visual::Cube { size, material } => {
            number_row(ui, "Size", size, 0.02);
            material
        }
        Visual::Sphere { radius, material } => {
            number_row(ui, "Radius", radius, 0.02);
            material
        }
        Visual::Plane {
            size,
            uv_repeat,
            material,
        } => {
            number_row(ui, "Size", size, 0.05);
            number_row(ui, "Texture repeat", uv_repeat, 0.05);
            material
        }
        Visual::Model { path } => {
            ui.horizontal(|ui| {
                ui.label("File");
                if let Some(new) = path_field(ui, scratch, "model", path) {
                    *path = new;
                }
            });
            return;
        }
    };
    ui.horizontal(|ui| {
        ui.label("Colour");
        ui.color_edit_button_rgba_unmultiplied(&mut material.base_color);
    });
    ui.add(egui::Slider::new(&mut material.metallic, 0.0..=1.0).text("Metallic"));
    ui.add(egui::Slider::new(&mut material.roughness, 0.0..=1.0).text("Roughness"));
    let mut emissive = Vec3::from(material.emissive);
    if vec3_row(ui, "Emissive", &mut emissive, 0.05) {
        material.emissive = emissive.to_array();
    }
    ui.horizontal(|ui| {
        ui.label("Texture");
        let current = material.texture.clone().unwrap_or_default();
        if let Some(new) = path_field(ui, scratch, "texture", &current) {
            material.texture = (!new.is_empty()).then_some(new);
        }
    });
}

fn collider_rows(ui: &mut egui::Ui, collider: &mut Collider) {
    const SHAPES: [&str; 3] = ["Box", "Sphere", "Capsule"];
    let kind = match collider.shape {
        Shape::Box { .. } => 0,
        Shape::Sphere { .. } => 1,
        Shape::Capsule { .. } => 2,
    };
    let mut chosen = kind;
    ui.horizontal(|ui| {
        ui.label("Shape");
        egui::ComboBox::from_id_salt("collider shape")
            .selected_text(SHAPES[kind])
            .show_ui(ui, |ui| {
                for (index, label) in SHAPES.iter().enumerate() {
                    ui.selectable_value(&mut chosen, index, *label);
                }
            });
    });
    if chosen != kind {
        collider.shape = match chosen {
            0 => Shape::Box {
                half_extents: Vec3::splat(0.5),
            },
            1 => Shape::Sphere { radius: 0.5 },
            _ => Shape::Capsule {
                half_height: 0.5,
                radius: 0.3,
            },
        };
    }
    match &mut collider.shape {
        Shape::Box { half_extents } => {
            vec3_row(ui, "Half size", half_extents, 0.02);
        }
        Shape::Sphere { radius } => number_row(ui, "Radius", radius, 0.02),
        Shape::Capsule {
            half_height,
            radius,
        } => {
            number_row(ui, "Half height", half_height, 0.02);
            number_row(ui, "Radius", radius, 0.02);
        }
    }
    ui.add(egui::Slider::new(&mut collider.friction, 0.0..=1.5).text("Friction"));
    ui.add(egui::Slider::new(&mut collider.restitution, 0.0..=1.0).text("Bounce"));
    number_row(ui, "Density", &mut collider.density, 0.05);
    ui.checkbox(&mut collider.sensor, "Sensor (detects, does not block)");
    ui.label("The Transform's scale also applies to the shape.");
}

fn body_rows(ui: &mut egui::Ui, body: &mut RigidBody) {
    let label = |kind: BodyKind| match kind {
        BodyKind::Dynamic => "Dynamic",
        BodyKind::Kinematic => "Kinematic",
        BodyKind::Fixed => "Fixed",
    };
    ui.horizontal(|ui| {
        ui.label("Kind");
        egui::ComboBox::from_id_salt("body kind")
            .selected_text(label(body.kind))
            .show_ui(ui, |ui| {
                for kind in [BodyKind::Dynamic, BodyKind::Kinematic, BodyKind::Fixed] {
                    ui.selectable_value(&mut body.kind, kind, label(kind));
                }
            });
    });
    vec3_row(ui, "Start velocity", &mut body.linear_velocity, 0.05);
    number_row(ui, "Linear damping", &mut body.linear_damping, 0.01);
    number_row(ui, "Angular damping", &mut body.angular_damping, 0.01);
    number_row(ui, "Gravity scale", &mut body.gravity_scale, 0.02);
    ui.checkbox(&mut body.lock_rotation, "Lock rotation");
    ui.checkbox(
        &mut body.continuous_collision,
        "Continuous collision (fast objects)",
    );
}

fn particle_rows(ui: &mut egui::Ui, emitter: &mut ParticleEmitter) {
    ui.horizontal(|ui| {
        ui.label("Preset");
        if ui.button("Fire").clicked() {
            *emitter = ParticleEmitter::fire();
        }
        if ui.button("Smoke").clicked() {
            *emitter = ParticleEmitter::smoke();
        }
        if ui.button("Default").clicked() {
            *emitter = ParticleEmitter::default();
        }
    });
    ui.checkbox(&mut emitter.emitting, "Emitting");
    number_row(ui, "Per second", &mut emitter.rate, 0.5);
    range_row(ui, "Lifetime", &mut emitter.lifetime, 0.02);
    range_row(ui, "Speed", &mut emitter.speed, 0.05);
    vec3_row(ui, "Direction", &mut emitter.direction, 0.02);
    ui.add(egui::Slider::new(&mut emitter.spread, 0.0..=180.0).text("Spread°"));
    number_row(ui, "Spawn radius", &mut emitter.radius, 0.01);
    vec3_row(ui, "Gravity", &mut emitter.gravity, 0.05);
    number_row(ui, "Drag", &mut emitter.drag, 0.02);
    range_row(ui, "Size", &mut emitter.size, 0.01);
    // Plain numbers rather than a colour picker: values above 1 make
    // particles glow, which a picker cannot express.
    for (label, color) in [
        ("Start RGBA", &mut emitter.color_start),
        ("End RGBA", &mut emitter.color_end),
    ] {
        let mut rgba = color.to_array();
        ui.horizontal(|ui| {
            ui.label(label);
            for channel in &mut rgba {
                ui.add(egui::DragValue::new(channel).speed(0.02).range(0.0..=20.0));
            }
        });
        *color = Vec4::from(rgba);
    }
    ui.checkbox(&mut emitter.additive, "Additive (glowing)");
}

fn add_component_menu(ui: &mut egui::Ui, world: &mut World, entity: Entity) {
    ui.menu_button("Add component", |ui| {
        // One entry per component the entity does not have yet.
        fn entry<T: Send + Sync + 'static>(
            ui: &mut egui::Ui,
            world: &mut World,
            entity: Entity,
            label: &str,
            make: impl FnOnce() -> T,
        ) {
            if !world.satisfies::<&T>(entity) && ui.button(label).clicked() {
                let _ = world.insert_one(entity, make());
                ui.close();
            }
        }
        entry(ui, world, entity, "Visual", || Visual::Cube {
            size: 1.0,
            material: MaterialDesc::default(),
        });
        entry(
            ui,
            world,
            entity,
            "Directional light",
            DirectionalLight::default,
        );
        entry(ui, world, entity, "Point light", PointLight::default);
        entry(ui, world, entity, "Collider", Collider::default);
        entry(ui, world, entity, "Rigid body", RigidBody::default);
        entry(ui, world, entity, "Particles", ParticleEmitter::default);
        entry(ui, world, entity, "Script", || {
            Script::new("scripts/coin.rhai")
        });
    });
}
