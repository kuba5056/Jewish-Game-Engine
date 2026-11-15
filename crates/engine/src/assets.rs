use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use engine_assets::{load_image, load_model, AssetError, ModelData};
use engine_audio::Sound;
use engine_render::glam::{Vec3, Vec4};
use engine_render::{Material, Mesh, Renderer, Texture, WeakTexture};
use engine_scene::hecs::EntityBuilder;
use engine_scene::{Entity, Name, Parent, Transform, World};

use crate::scene::MeshRenderer;

/// How often unused assets are dropped and files checked for changes, in seconds.
const MAINTAIN_INTERVAL: f32 = 0.5;

/// A loaded 3D model: a tree of nodes with meshes, ready to be placed in the
/// world any number of times with [`Model::spawn`]. Cheap to clone.
#[derive(Clone)]
pub struct Model(Arc<ModelInner>);

struct ModelInner {
    textures: Vec<Texture>,
    /// Per mesh, its primitives.
    meshes: Vec<Vec<Primitive>>,
    /// Parents precede children.
    nodes: Vec<Node>,
}

struct Primitive {
    mesh: Mesh,
    material: Material,
}

struct Node {
    name: Option<String>,
    parent: Option<usize>,
    transform: Transform,
    mesh: Option<usize>,
}

impl Model {
    fn upload(renderer: &Renderer, data: &ModelData) -> Self {
        let textures: Vec<Texture> = data
            .images
            .iter()
            .map(|image| renderer.create_texture(image.width, image.height, &image.rgba))
            .collect();
        let meshes = data
            .meshes
            .iter()
            .map(|primitives| {
                primitives
                    .iter()
                    .map(|primitive| {
                        let source = primitive.material.and_then(|i| data.materials.get(i));
                        let material = source.map_or_else(Material::default, |m| Material {
                            texture: m.base_color_image.and_then(|i| textures.get(i).cloned()),
                            base_color: Vec4::from(m.base_color),
                            metallic: m.metallic,
                            roughness: m.roughness,
                            emissive: Vec3::from(m.emissive),
                        });
                        Primitive {
                            mesh: renderer.create_mesh(&primitive.mesh),
                            material,
                        }
                    })
                    .collect()
            })
            .collect();
        let nodes = data
            .nodes
            .iter()
            .map(|node| Node {
                name: node.name.clone(),
                parent: node.parent,
                transform: Transform {
                    translation: node.translation,
                    rotation: node.rotation,
                    scale: node.scale,
                },
                mesh: node.mesh,
            })
            .collect();
        Self(Arc::new(ModelInner {
            textures,
            meshes,
            nodes,
        }))
    }

    /// Replaces this model's geometry and textures with a re-read file, so
    /// entities already spawned from it change on screen.
    fn refresh(&self, renderer: &Renderer, data: &ModelData, path: &Path) {
        for (texture, image) in self.0.textures.iter().zip(&data.images) {
            renderer.update_texture(texture, image.width, image.height, &image.rgba);
        }
        let old = self.0.meshes.iter().flatten();
        let new = data.meshes.iter().flatten();
        for (primitive, data) in old.zip(new) {
            renderer.update_mesh(&primitive.mesh, &data.mesh);
        }

        let shape = |meshes: Vec<usize>, images: usize, nodes: usize| (meshes, images, nodes);
        let before = shape(
            self.0.meshes.iter().map(Vec::len).collect(),
            self.0.textures.len(),
            self.0.nodes.len(),
        );
        let after = shape(
            data.meshes.iter().map(Vec::len).collect(),
            data.images.len(),
            data.nodes.len(),
        );
        if before != after {
            log::warn!(
                "{}: the model's structure changed; restart to see added or removed parts",
                path.display()
            );
        }
    }

    /// Whether nothing outside the asset cache uses the model any more:
    /// neither a `Model` handle nor an entity drawing one of its meshes.
    fn is_unused(&self) -> bool {
        Arc::strong_count(&self.0) == 1
            && self.0.meshes.iter().flatten().all(|p| p.mesh.is_unique())
    }

    /// Creates entities for the model's nodes under a new root entity placed
    /// at `transform`, and returns the root.
    ///
    /// Each node becomes an entity with a `Transform`, a `Parent`, a `Name`
    /// (if the file names it) and a `MeshRenderer` (if it has a mesh). Remove
    /// the whole thing with `despawn_recursive(world, root)`.
    pub fn spawn(&self, world: &mut World, transform: Transform) -> Entity {
        let root = world.spawn((transform,));
        let mut entities: Vec<Entity> = Vec::with_capacity(self.0.nodes.len());
        for node in &self.0.nodes {
            let parent = node.parent.map_or(root, |index| entities[index]);
            let primitives = node
                .mesh
                .and_then(|index| self.0.meshes.get(index))
                .map_or(&[][..], Vec::as_slice);
            let renderer = |primitive: &Primitive| MeshRenderer {
                mesh: primitive.mesh.clone(),
                material: primitive.material.clone(),
            };

            let mut builder = EntityBuilder::new();
            builder.add(node.transform).add(Parent(parent));
            if let Some(name) = &node.name {
                builder.add(Name(name.clone()));
            }
            // An entity holds one MeshRenderer, so a mesh with several
            // materials puts the first on the node and the rest on children.
            if let Some(first) = primitives.first() {
                builder.add(renderer(first));
            }
            let entity = world.spawn(builder.build());
            for extra in primitives.iter().skip(1) {
                world.spawn((Transform::IDENTITY, Parent(entity), renderer(extra)));
            }
            entities.push(entity);
        }
        root
    }
}

/// A file being watched, with the modification time it was last loaded at.
struct Watched {
    path: PathBuf,
    modified: Option<SystemTime>,
}

impl Watched {
    fn new(path: PathBuf) -> Self {
        let modified = modified(&path);
        Self { path, modified }
    }

    fn changed(&self) -> bool {
        modified(&self.path) != self.modified
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

struct TextureEntry {
    texture: WeakTexture,
    file: Watched,
    reload_failed: bool,
}

struct ModelEntry {
    /// Held strongly: spawned entities keep only the meshes and textures, and
    /// reloading needs the whole model. Dropped once `Model::is_unused`.
    model: Model,
    files: Vec<Watched>,
    reload_failed: bool,
}

/// Loads asset files, shares already-loaded ones, and reloads them when they
/// change on disk.
pub(crate) struct Assets {
    root: PathBuf,
    hot_reload: bool,
    since_check: f32,
    textures: HashMap<PathBuf, TextureEntry>,
    models: HashMap<PathBuf, ModelEntry>,
    /// Kept for the whole run: decoded sounds are small and there is no way
    /// to tell whether one will be played again.
    sounds: HashMap<PathBuf, Sound>,
}

impl Assets {
    pub(crate) fn new(asset_dir: &Path, hot_reload: bool) -> Self {
        let root = resolve_asset_dir(asset_dir);
        log::info!("assets: {}", root.display());
        Self {
            root,
            hot_reload,
            since_check: 0.0,
            textures: HashMap::new(),
            models: HashMap::new(),
            sounds: HashMap::new(),
        }
    }

    pub(crate) fn texture(
        &mut self,
        renderer: &Renderer,
        path: &Path,
    ) -> Result<Texture, AssetError> {
        let path = self.root.join(path);
        if let Some(texture) = self.textures.get(&path).and_then(|e| e.texture.upgrade()) {
            return Ok(texture);
        }
        let image = load_image(&path)?;
        let texture = renderer.create_texture(image.width, image.height, &image.rgba);
        let entry = TextureEntry {
            texture: texture.downgrade(),
            file: Watched::new(path.clone()),
            reload_failed: false,
        };
        self.textures.insert(path, entry);
        Ok(texture)
    }

    pub(crate) fn model(&mut self, renderer: &Renderer, path: &Path) -> Result<Model, AssetError> {
        let path = self.root.join(path);
        if let Some(entry) = self.models.get(&path) {
            return Ok(entry.model.clone());
        }
        let data = load_model(&path)?;
        let model = Model::upload(renderer, &data);
        let entry = ModelEntry {
            model: model.clone(),
            files: data.files.into_iter().map(Watched::new).collect(),
            reload_failed: false,
        };
        self.models.insert(path, entry);
        Ok(model)
    }

    pub(crate) fn sound(&mut self, path: &Path) -> Result<Sound, AssetError> {
        let path = self.root.join(path);
        if let Some(sound) = self.sounds.get(&path) {
            return Ok(sound.clone());
        }
        let sound = Sound::from_file(&path).map_err(|message| AssetError::Invalid {
            path: path.clone(),
            message,
        })?;
        self.sounds.insert(path, sound.clone());
        Ok(sound)
    }

    /// Forgets assets nothing uses any more and, with hot reload on, reloads
    /// those whose files changed. Call at the start of every frame; the work
    /// is only done twice a second.
    pub(crate) fn maintain(&mut self, renderer: &Renderer, dt: f32) {
        self.since_check += dt;
        if self.since_check < MAINTAIN_INTERVAL {
            return;
        }
        self.since_check = 0.0;

        self.textures
            .retain(|_, entry| entry.texture.upgrade().is_some());
        self.models.retain(|_, entry| !entry.model.is_unused());
        if !self.hot_reload {
            return;
        }

        // A failed reload (typically a file caught half-written by an editor)
        // keeps the old timestamp, so it is retried on the next check; the
        // flag limits the warning to once per failure streak.
        for (path, entry) in &mut self.textures {
            let Some(texture) = entry.texture.upgrade() else {
                continue;
            };
            if entry.file.changed() {
                match load_image(path) {
                    Ok(image) => {
                        renderer.update_texture(&texture, image.width, image.height, &image.rgba);
                        entry.file = Watched::new(path.clone());
                        entry.reload_failed = false;
                        log::info!("reloaded {}", path.display());
                    }
                    Err(e) => {
                        if !entry.reload_failed {
                            log::warn!("reload failed, keeping the old version: {e}");
                        }
                        entry.reload_failed = true;
                    }
                }
            }
        }

        for (path, entry) in &mut self.models {
            if entry.files.iter().any(Watched::changed) {
                match load_model(path) {
                    Ok(data) => {
                        entry.model.refresh(renderer, &data, path);
                        entry.files = data.files.into_iter().map(Watched::new).collect();
                        entry.reload_failed = false;
                        log::info!("reloaded {}", path.display());
                    }
                    Err(e) => {
                        if !entry.reload_failed {
                            log::warn!("reload failed, keeping the old version: {e}");
                        }
                        entry.reload_failed = true;
                    }
                }
            }
        }
    }
}

/// Finds the asset directory: a relative path is looked for under the
/// working directory first (running from the project during development),
/// then next to the executable (a shipped game).
fn resolve_asset_dir(asset_dir: &Path) -> PathBuf {
    if asset_dir.is_absolute() || asset_dir.is_dir() {
        return asset_dir.to_owned();
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(asset_dir)));
    match beside_exe {
        Some(dir) if dir.is_dir() => dir,
        _ => asset_dir.to_owned(),
    }
}
