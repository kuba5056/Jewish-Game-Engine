//! Scenes as data: describing entities in a file and turning the world back
//! into one.
//!
//! A scene file lists entities with the components that can be written
//! down: name, transform, parent, lights, physics and a [`Visual`]. GPU
//! resources are not stored; a `Visual` says what to show and the engine
//! creates the mesh and material for it.

use std::collections::HashMap;
use std::path::Path;

use engine_assets::AssetError;
use engine_physics::{Collider, RigidBody};
use engine_render::glam::{Vec3, Vec4};
use engine_render::Material;
use engine_scene::hecs::EntityBuilder;
use engine_scene::{Entity, Name, Parent, Transform, World};
use engine_script::Script;
use serde::{Deserialize, Serialize};

use crate::app::Context;
use crate::assets::{ModelPart, ShapeKey};
use crate::particles::ParticleEmitter;
use crate::scene::{DirectionalLight, MeshRenderer, PointLight};

/// A material as written in a scene file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialDesc {
    /// Path of the colour texture in the asset directory.
    pub texture: Option<String>,
    /// Linear RGBA.
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    /// Linear RGB.
    pub emissive: [f32; 3],
}

impl Default for MaterialDesc {
    fn default() -> Self {
        let material = Material::default();
        Self {
            texture: None,
            base_color: material.base_color.to_array(),
            metallic: material.metallic,
            roughness: material.roughness,
            emissive: material.emissive.to_array(),
        }
    }
}

impl MaterialDesc {
    /// A plain, non-metallic surface of the given colour (linear RGB).
    pub fn colored(r: f32, g: f32, b: f32) -> Self {
        Self {
            base_color: [r, g, b, 1.0],
            ..Self::default()
        }
    }
}

/// What an entity looks like, in a form that can be saved to a scene file.
///
/// Spawn an entity with a `Transform` and a `Visual` and the engine gives it
/// the matching mesh and material on the next frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Visual {
    Cube {
        size: f32,
        #[serde(default)]
        material: MaterialDesc,
    },
    Sphere {
        radius: f32,
        #[serde(default)]
        material: MaterialDesc,
    },
    /// A flat square facing up; the texture repeats `uv_repeat` times.
    Plane {
        size: f32,
        #[serde(default = "one")]
        uv_repeat: f32,
        #[serde(default)]
        material: MaterialDesc,
    },
    /// A glTF model from the asset directory.
    Model { path: String },
}

fn one() -> f32 {
    1.0
}

/// Marks a [`Visual`] the engine has already turned into meshes.
struct VisualReady;

/// One entity in a scene file. Everything except the transform is optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneEntity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub transform: Transform,
    /// Index of the parent in [`SceneFile::entities`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visual: Option<Visual>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directional_light: Option<DirectionalLight>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point_light: Option<PointLight>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collider: Option<Collider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rigid_body: Option<RigidBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub particles: Option<ParticleEmitter>,
    /// Path of a script file that drives the entity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
}

/// The contents of a scene file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneFile {
    pub entities: Vec<SceneEntity>,
}

impl SceneFile {
    /// Parses a scene from RON text.
    pub fn from_ron(text: &str) -> Result<Self, String> {
        // `implicit_some` lets files write `name: "Sun"` for optional fields
        // instead of `name: Some("Sun")`.
        ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(text)
            .map_err(|e| e.to_string())
    }

    /// Writes the scene as human-readable RON text.
    pub fn to_ron(&self) -> Result<String, String> {
        let pretty =
            ron::ser::PrettyConfig::new().extensions(ron::extensions::Extensions::IMPLICIT_SOME);
        ron::ser::to_string_pretty(self, pretty).map_err(|e| e.to_string())
    }

    /// Describes every entity in `world` that has a `Transform`, except the
    /// parts of loaded models (their root entity's [`Visual`] recreates them).
    pub fn from_world(world: &World) -> Self {
        let mut saved: Vec<Entity> = world
            .query::<Entity>()
            .with::<&Transform>()
            .without::<&ModelPart>()
            .iter()
            .collect();
        // Entity ids give a stable order from one save to the next.
        saved.sort_by_key(|entity| entity.to_bits());
        let index: HashMap<Entity, usize> =
            saved.iter().enumerate().map(|(i, &e)| (e, i)).collect();

        let get = |entity| world.entity(entity).expect("entity came from this world");
        let entities = saved
            .iter()
            .map(|&entity| {
                let entity = get(entity);
                let mut visual = entity.get::<&Visual>().map(|v| (*v).clone());
                // The live material may have been edited since it was created.
                if let (Some(visual), Some(renderer)) =
                    (visual.as_mut(), entity.get::<&MeshRenderer>())
                {
                    visual.take_material_from(&renderer.material);
                }
                SceneEntity {
                    name: entity.get::<&Name>().map(|name| name.0.clone()),
                    transform: *entity.get::<&Transform>().expect("queried with Transform"),
                    parent: entity
                        .get::<&Parent>()
                        .and_then(|parent| index.get(&parent.0).copied()),
                    visual,
                    directional_light: entity.get::<&DirectionalLight>().map(|c| *c),
                    point_light: entity.get::<&PointLight>().map(|c| *c),
                    collider: entity.get::<&Collider>().map(|c| *c),
                    rigid_body: entity.get::<&RigidBody>().map(|c| *c),
                    // One-off effects are transient, not part of the scene.
                    particles: entity
                        .get::<&ParticleEmitter>()
                        .filter(|emitter| !emitter.despawn_when_done)
                        .map(|emitter| (*emitter).clone()),
                    script: entity.get::<&Script>().map(|script| script.path.clone()),
                }
            })
            .collect();
        Self { entities }
    }

    /// Creates the scene's entities in `world` and returns them, in file order.
    pub fn spawn(&self, world: &mut World) -> Vec<Entity> {
        let spawned: Vec<Entity> = self
            .entities
            .iter()
            .map(|desc| {
                let mut builder = EntityBuilder::new();
                builder.add(desc.transform);
                if let Some(name) = &desc.name {
                    builder.add(Name(name.clone()));
                }
                if let Some(visual) = &desc.visual {
                    builder.add(visual.clone());
                }
                if let Some(light) = desc.directional_light {
                    builder.add(light);
                }
                if let Some(light) = desc.point_light {
                    builder.add(light);
                }
                if let Some(collider) = desc.collider {
                    builder.add(collider);
                }
                if let Some(body) = desc.rigid_body {
                    builder.add(body);
                }
                if let Some(emitter) = &desc.particles {
                    builder.add(emitter.clone());
                }
                if let Some(path) = &desc.script {
                    builder.add(Script::new(path.clone()));
                }
                world.spawn(builder.build())
            })
            .collect();
        // Parents are linked afterwards, so a child may precede its parent.
        for (desc, &entity) in self.entities.iter().zip(&spawned) {
            if let Some(&parent) = desc.parent.and_then(|index| spawned.get(index)) {
                let _ = world.insert_one(entity, Parent(parent));
            }
        }
        spawned
    }
}

impl Visual {
    fn material_mut(&mut self) -> Option<&mut MaterialDesc> {
        match self {
            Visual::Cube { material, .. }
            | Visual::Sphere { material, .. }
            | Visual::Plane { material, .. } => Some(material),
            Visual::Model { .. } => None,
        }
    }

    /// Copies the editable material values (not the texture, whose path only
    /// the description knows) from a live material.
    fn take_material_from(&mut self, live: &Material) {
        if let Some(desc) = self.material_mut() {
            desc.base_color = live.base_color.to_array();
            desc.metallic = live.metallic;
            desc.roughness = live.roughness;
            desc.emissive = live.emissive.to_array();
        }
    }
}

impl Context {
    /// Loads a scene file (RON) from the asset directory and spawns its
    /// entities. Returns them in file order.
    pub fn load_scene(&mut self, path: impl AsRef<Path>) -> Result<Vec<Entity>, AssetError> {
        let path = self.assets.root().join(path);
        let text = std::fs::read_to_string(&path).map_err(|e| AssetError::Io {
            path: path.clone(),
            message: e.to_string(),
        })?;
        let scene =
            SceneFile::from_ron(&text).map_err(|message| AssetError::Invalid { path, message })?;
        Ok(scene.spawn(&mut self.world))
    }

    /// Saves the world as a scene file (RON) in the asset directory. See
    /// [`SceneFile::from_world`] for what is included.
    pub fn save_scene(&self, path: impl AsRef<Path>) -> Result<(), AssetError> {
        let path = self.assets.root().join(path);
        let text = SceneFile::from_world(&self.world)
            .to_ron()
            .map_err(|message| AssetError::Invalid {
                path: path.clone(),
                message,
            })?;
        let io_error = |e: std::io::Error| AssetError::Io {
            path: path.clone(),
            message: e.to_string(),
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io_error)?;
        }
        std::fs::write(&path, text).map_err(io_error)
    }

    /// Gives every entity with a new [`Visual`] its mesh and material (or
    /// its model's entities). Run by the engine once per frame.
    pub(crate) fn realize_visuals(&mut self) {
        let new: Vec<(Entity, Visual)> = self
            .world
            .query::<(Entity, &Visual)>()
            .without::<&VisualReady>()
            .iter()
            .map(|(entity, visual)| (entity, visual.clone()))
            .collect();

        for (entity, visual) in new {
            // Marked first, so a visual that fails is reported once rather
            // than retried every frame.
            let _ = self.world.insert_one(entity, VisualReady);
            let (key, desc) = match &visual {
                Visual::Cube { size, material } => (ShapeKey::Cube(size.to_bits()), material),
                Visual::Sphere { radius, material } => {
                    (ShapeKey::Sphere(radius.to_bits()), material)
                }
                Visual::Plane {
                    size,
                    uv_repeat,
                    material,
                } => (
                    ShapeKey::Plane(size.to_bits(), uv_repeat.to_bits()),
                    material,
                ),
                Visual::Model { path } => {
                    match self.load_model(path) {
                        Ok(model) => model.spawn_under(&mut self.world, entity),
                        Err(e) => log::error!("{e}"),
                    }
                    continue;
                }
            };
            let texture = desc
                .texture
                .as_ref()
                .and_then(|path| self.load_texture(path).map_err(|e| log::error!("{e}")).ok());
            let renderer = MeshRenderer {
                mesh: self.assets.shape(&self.renderer, key),
                material: Material {
                    texture,
                    base_color: Vec4::from(desc.base_color),
                    metallic: desc.metallic,
                    roughness: desc.roughness,
                    emissive: Vec3::from(desc.emissive),
                },
            };
            let _ = self.world.insert_one(entity, renderer);
        }
    }
}

#[cfg(test)]
mod tests {
    use engine_physics::Shape;
    use engine_render::glam::Quat;

    use super::*;

    fn sample() -> SceneFile {
        SceneFile {
            entities: vec![
                SceneEntity {
                    name: Some("Sun".to_owned()),
                    transform: Transform::IDENTITY.looking_to(Vec3::new(-0.5, -1.0, -0.4)),
                    directional_light: Some(DirectionalLight::default()),
                    ..SceneEntity::default()
                },
                SceneEntity {
                    name: Some("Crate".to_owned()),
                    transform: Transform::from_xyz(1.0, 2.0, 3.0)
                        .with_rotation(Quat::from_rotation_y(0.5))
                        .with_scale(Vec3::splat(2.0)),
                    visual: Some(Visual::Cube {
                        size: 1.0,
                        material: MaterialDesc {
                            texture: Some("textures/crate.png".to_owned()),
                            metallic: 0.2,
                            ..MaterialDesc::colored(0.8, 0.5, 0.2)
                        },
                    }),
                    collider: Some(Collider::cuboid(Vec3::ONE)),
                    rigid_body: Some(RigidBody::dynamic()),
                    ..SceneEntity::default()
                },
                SceneEntity {
                    transform: Transform::from_xyz(0.0, 1.0, 0.0),
                    parent: Some(1),
                    point_light: Some(PointLight::default()),
                    particles: Some(ParticleEmitter::fire()),
                    script: Some("scripts/spin.rhai".to_owned()),
                    visual: Some(Visual::Model {
                        path: "models/robot.gltf".to_owned(),
                    }),
                    ..SceneEntity::default()
                },
            ],
        }
    }

    #[test]
    fn ron_round_trip_is_lossless() {
        let scene = sample();
        let text = scene.to_ron().unwrap();
        assert_eq!(SceneFile::from_ron(&text).unwrap(), scene);
    }

    #[test]
    fn hand_written_files_may_omit_most_fields() {
        let scene = SceneFile::from_ron(
            r#"(entities: [
                (name: "Floor", visual: Plane(size: 20.0)),
                (transform: (translation: (0.0, 3.0, 0.0)),
                 visual: Sphere(radius: 0.5, material: (base_color: (1.0, 0.0, 0.0, 1.0))),
                 collider: (shape: Sphere(radius: 0.5), friction: 0.5, restitution: 0.8,
                            density: 1.0, sensor: false),
                 rigid_body: ()),
            ])"#,
        )
        .unwrap();

        assert_eq!(scene.entities[0].name.as_deref(), Some("Floor"));
        assert_eq!(scene.entities[0].transform, Transform::IDENTITY);
        assert_eq!(
            scene.entities[0].visual,
            Some(Visual::Plane {
                size: 20.0,
                uv_repeat: 1.0,
                material: MaterialDesc::default(),
            })
        );
        let ball = &scene.entities[1];
        assert_eq!(ball.transform.translation, Vec3::new(0.0, 3.0, 0.0));
        assert_eq!(ball.transform.scale, Vec3::ONE);
        assert_eq!(ball.collider.unwrap().shape, Shape::Sphere { radius: 0.5 });
        assert_eq!(ball.rigid_body, Some(RigidBody::default()));
    }

    #[test]
    fn one_off_particle_effects_are_not_saved() {
        let mut world = World::new();
        world.spawn((Transform::IDENTITY, ParticleEmitter::smoke()));
        world.spawn((Transform::IDENTITY, ParticleEmitter::burst(20)));
        let scene = SceneFile::from_world(&world);
        let saved: Vec<bool> = scene
            .entities
            .iter()
            .map(|e| e.particles.is_some())
            .collect();
        assert_eq!(saved.iter().filter(|&&has| has).count(), 1);

        // Emitters in hand-written files need only the fields that differ.
        let scene = SceneFile::from_ron("(entities: [(particles: (rate: 5.0, additive: true))])");
        let emitter = scene.unwrap().entities[0].particles.clone().unwrap();
        assert_eq!((emitter.rate, emitter.additive), (5.0, true));
        assert_eq!(emitter.lifetime, ParticleEmitter::default().lifetime);
    }

    #[test]
    fn malformed_file_is_an_error() {
        assert!(SceneFile::from_ron("(entities: [ (name: 5) ])").is_err());
    }

    #[test]
    fn world_round_trip_keeps_components_and_hierarchy() {
        let scene = sample();
        let mut world = World::new();
        let spawned = scene.spawn(&mut world);
        assert_eq!(spawned.len(), 3);
        assert_eq!(world.get::<&Parent>(spawned[2]).unwrap().0, spawned[1]);
        assert_eq!(world.get::<&Name>(spawned[0]).unwrap().0, "Sun");

        assert_eq!(SceneFile::from_world(&world), scene);
    }

    #[test]
    fn model_parts_are_left_out_and_edited_materials_are_saved() {
        let mut world = World::new();
        let root = world.spawn((
            Transform::IDENTITY,
            Visual::Sphere {
                radius: 1.0,
                material: MaterialDesc::default(),
            },
        ));
        world.spawn((Transform::IDENTITY, Parent(root), ModelPart));
        let scene = SceneFile::from_world(&world);
        assert_eq!(scene.entities.len(), 1);

        // Without a MeshRenderer the description is saved as written.
        assert_eq!(
            scene.entities[0]
                .visual
                .as_ref()
                .unwrap()
                .clone()
                .material_mut()
                .unwrap()
                .metallic,
            MaterialDesc::default().metallic
        );
        let mut visual = scene.entities[0].visual.clone().unwrap();
        visual.take_material_from(&Material {
            metallic: 1.0,
            roughness: 0.1,
            ..Material::colored(0.0, 1.0, 0.0)
        });
        let material = visual.material_mut().unwrap();
        assert_eq!(material.base_color, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!((material.metallic, material.roughness), (1.0, 0.1));
    }
}
