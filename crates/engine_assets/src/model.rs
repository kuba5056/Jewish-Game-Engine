use std::path::{Path, PathBuf};

use engine_render::{MeshData, Vertex};
use glam::{Mat4, Quat, Vec3};

use crate::animation::{AnimationData, Channel, Keyframes, SkinData};
use crate::{AssetError, ImageData};

/// A model file's contents in CPU memory.
#[derive(Debug, Clone, Default)]
pub struct ModelData {
    pub images: Vec<ImageData>,
    pub materials: Vec<MaterialData>,
    /// Each mesh is a list of primitives, one per material it uses.
    pub meshes: Vec<Vec<PrimitiveData>>,
    /// The node tree, flattened so that a parent always precedes its children.
    pub nodes: Vec<NodeData>,
    pub skins: Vec<SkinData>,
    pub animations: Vec<AnimationData>,
    /// Every file the model was read from, for change detection.
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct MaterialData {
    /// Linear RGBA, multiplied with the texture.
    pub base_color: [f32; 4],
    /// Index into [`ModelData::images`].
    pub base_color_image: Option<usize>,
    /// 0 for non-metals, 1 for bare metal.
    pub metallic: f32,
    /// 0 is mirror-smooth, 1 is fully matte.
    pub roughness: f32,
    /// Linear RGB light the surface gives off.
    pub emissive: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct PrimitiveData {
    pub mesh: MeshData,
    /// Index into [`ModelData::materials`].
    pub material: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct NodeData {
    pub name: Option<String>,
    /// Index into [`ModelData::nodes`]; `None` for top-level nodes.
    pub parent: Option<usize>,
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    /// Index into [`ModelData::meshes`].
    pub mesh: Option<usize>,
    /// Index into [`ModelData::skins`], if the mesh follows a skeleton.
    pub skin: Option<usize>,
}

/// Loads a glTF 2.0 model (`.gltf` with its side files, or a single `.glb`).
///
/// Reads the default scene's node tree, triangle meshes (positions, normals,
/// first UV set, skinning joints and weights), each material's base colour,
/// base colour texture, metallic, roughness and emissive factors, skins, and
/// animations of node translation, rotation and scale. Everything else in
/// the file (morph targets, cameras, lights, other material textures) is
/// ignored.
pub fn load_model(path: &Path) -> Result<ModelData, AssetError> {
    let (document, buffers, images) = gltf::import(path).map_err(|e| match e {
        gltf::Error::Io(io) => AssetError::io(path, io),
        other => AssetError::invalid(path, other),
    })?;

    let mut model = ModelData {
        files: source_files(path, &document),
        ..ModelData::default()
    };

    for (index, image) in images.into_iter().enumerate() {
        let rgba = to_rgba8(&image).ok_or_else(|| {
            AssetError::invalid(
                path,
                format!(
                    "image {index} has an unsupported pixel format {:?}",
                    image.format
                ),
            )
        })?;
        model.images.push(ImageData {
            width: image.width,
            height: image.height,
            rgba,
        });
    }

    for material in document.materials() {
        let pbr = material.pbr_metallic_roughness();
        model.materials.push(MaterialData {
            base_color: pbr.base_color_factor(),
            base_color_image: pbr
                .base_color_texture()
                .map(|info| info.texture().source().index()),
            metallic: pbr.metallic_factor(),
            roughness: pbr.roughness_factor(),
            emissive: material.emissive_factor(),
        });
    }

    for mesh in document.meshes() {
        let mut primitives = Vec::new();
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                log::warn!(
                    "{}: skipping a {:?} primitive, only triangles are supported",
                    path.display(),
                    primitive.mode()
                );
                continue;
            }
            let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|b| &b.0[..]));
            let positions: Vec<[f32; 3]> = reader
                .read_positions()
                .ok_or_else(|| AssetError::invalid(path, "a mesh has no vertex positions"))?
                .collect();
            let indices: Vec<u32> = match reader.read_indices() {
                Some(indices) => indices.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            if indices.iter().any(|&i| i as usize >= positions.len()) {
                return Err(AssetError::invalid(path, "a mesh index is out of range"));
            }
            let normals: Vec<[f32; 3]> = match reader.read_normals() {
                Some(normals) => normals.collect(),
                None => flat_normals(&positions, &indices),
            };
            let uvs: Vec<[f32; 2]> = match reader.read_tex_coords(0) {
                Some(uvs) => uvs.into_f32().collect(),
                None => vec![[0.0, 0.0]; positions.len()],
            };
            let no_skin = || vec![[0; 4]; positions.len()];
            let joints: Vec<[u16; 4]> = reader
                .read_joints(0)
                .map_or_else(no_skin, |joints| joints.into_u16().collect());
            let weights: Vec<[f32; 4]> = match reader.read_weights(0) {
                Some(weights) => weights.into_f32().map(normalized_weights).collect(),
                None => vec![[0.0; 4]; positions.len()],
            };
            let count = positions.len();
            if [normals.len(), uvs.len(), joints.len(), weights.len()] != [count; 4] {
                return Err(AssetError::invalid(
                    path,
                    "a mesh's vertex attributes differ in length",
                ));
            }

            let vertices = (0..count)
                .map(|i| Vertex {
                    position: positions[i],
                    normal: normals[i],
                    uv: uvs[i],
                    joints: joints[i],
                    weights: weights[i],
                })
                .collect();
            primitives.push(PrimitiveData {
                mesh: MeshData { vertices, indices },
                material: primitive.material().index(),
            });
        }
        model.meshes.push(primitives);
    }

    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .ok_or_else(|| AssetError::invalid(path, "the file contains no scene"))?;
    // Depth-first with an explicit stack; a node goes in after its parent.
    // Skins and animations refer to nodes by their index in the file, which
    // differs from the position in our flattened list.
    let mut flat_index: Vec<Option<usize>> = vec![None; document.nodes().len()];
    let mut node_skins: Vec<Option<usize>> = Vec::new();
    let mut pending: Vec<(gltf::Node, Option<usize>)> =
        scene.nodes().map(|node| (node, None)).collect();
    pending.reverse();
    while let Some((node, parent)) = pending.pop() {
        let (translation, rotation, scale) = node.transform().decomposed();
        let index = model.nodes.len();
        flat_index[node.index()] = Some(index);
        node_skins.push(node.skin().map(|skin| skin.index()));
        model.nodes.push(NodeData {
            name: node.name().map(str::to_owned),
            parent,
            translation: Vec3::from(translation),
            rotation: Quat::from_array(rotation),
            scale: Vec3::from(scale),
            mesh: node.mesh().map(|mesh| mesh.index()),
            skin: None,
        });
        let children: Vec<_> = node.children().collect();
        pending.extend(children.into_iter().rev().map(|child| (child, Some(index))));
    }
    let buffer = |buffer: gltf::Buffer| buffers.get(buffer.index()).map(|b| &b.0[..]);

    // A skin is kept only if all its joints are in the scene; a mesh whose
    // skin is dropped is drawn rigid.
    let mut skin_index: Vec<Option<usize>> = Vec::new();
    for skin in document.skins() {
        let joints: Option<Vec<usize>> = skin.joints().map(|j| flat_index[j.index()]).collect();
        let Some(joints) = joints else {
            log::warn!("{}: a skin uses nodes outside the scene", path.display());
            skin_index.push(None);
            continue;
        };
        let mut inverse_bind: Vec<Mat4> = match skin.reader(buffer).read_inverse_bind_matrices() {
            Some(matrices) => matrices.map(|m| Mat4::from_cols_array_2d(&m)).collect(),
            None => Vec::new(),
        };
        // The file may omit them, meaning identity.
        inverse_bind.resize(joints.len(), Mat4::IDENTITY);
        skin_index.push(Some(model.skins.len()));
        model.skins.push(SkinData {
            joints,
            inverse_bind,
        });
    }
    for (node, skin) in model.nodes.iter_mut().zip(node_skins) {
        node.skin = skin.and_then(|i| skin_index.get(i).copied().flatten());
    }

    for (number, animation) in document.animations().enumerate() {
        let mut channels = Vec::new();
        for channel in animation.channels() {
            let Some(node) = flat_index[channel.target().node().index()] else {
                continue; // Animates a node outside the scene.
            };
            let reader = channel.reader(buffer);
            let (Some(times), Some(outputs)) = (reader.read_inputs(), reader.read_outputs()) else {
                continue;
            };
            let times: Vec<f32> = times.collect();
            use gltf::animation::util::ReadOutputs;
            use gltf::animation::Interpolation;
            let interpolation = channel.sampler().interpolation();
            // Cubic spline samplers store (in-tangent, value, out-tangent)
            // per keyframe. Only the values are used, blended linearly,
            // which is close for densely keyed animation.
            let cubic = interpolation == Interpolation::CubicSpline;
            fn values<T>(all: impl Iterator<Item = T>, cubic: bool) -> Vec<T> {
                if cubic {
                    all.skip(1).step_by(3).collect()
                } else {
                    all.collect()
                }
            }
            let values = match outputs {
                ReadOutputs::Translations(v) => {
                    Keyframes::Translation(values(v.map(Vec3::from), cubic))
                }
                ReadOutputs::Scales(v) => Keyframes::Scale(values(v.map(Vec3::from), cubic)),
                ReadOutputs::Rotations(v) => Keyframes::Rotation(values(
                    v.into_f32().map(|q| Quat::from_array(q).normalize()),
                    cubic,
                )),
                ReadOutputs::MorphTargetWeights(_) => continue,
            };
            channels.push(Channel {
                node,
                times,
                values,
                step: interpolation == Interpolation::Step,
            });
        }
        let duration = channels
            .iter()
            .filter_map(|channel| channel.times.last().copied())
            .fold(0.0, f32::max);
        model.animations.push(AnimationData {
            name: animation
                .name()
                .map_or_else(|| format!("Animation {number}"), str::to_owned),
            duration,
            channels,
        });
    }

    Ok(model)
}

/// The model file plus the external buffers and images it refers to.
fn source_files(path: &Path, document: &gltf::Document) -> Vec<PathBuf> {
    let dir = path.parent().unwrap_or(Path::new(""));
    let buffers = document
        .buffers()
        .filter_map(|buffer| match buffer.source() {
            gltf::buffer::Source::Uri(uri) => Some(uri),
            gltf::buffer::Source::Bin => None,
        });
    let images = document.images().filter_map(|image| match image.source() {
        gltf::image::Source::Uri { uri, .. } => Some(uri),
        gltf::image::Source::View { .. } => None,
    });
    let mut files = vec![path.to_owned()];
    files.extend(
        buffers
            .chain(images)
            .filter(|uri| !uri.starts_with("data:"))
            .map(|uri| dir.join(uri)),
    );
    files
}

fn to_rgba8(image: &gltf::image::Data) -> Option<Vec<u8>> {
    use gltf::image::Format;
    let px = &image.pixels;
    Some(match image.format {
        Format::R8G8B8A8 => px.clone(),
        Format::R8G8B8 => px
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        Format::R8 => px.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        // Two channels are grey + alpha.
        Format::R8G8 => px
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[grey, alpha]| [grey, grey, grey, alpha])
            .collect(),
        _ => return None,
    })
}

/// Scales skin weights to sum to 1, so a vertex is not shrunk towards the
/// origin or blown up by an exporter's rounding.
fn normalized_weights(weights: [f32; 4]) -> [f32; 4] {
    let sum: f32 = weights.iter().sum();
    if sum > 1e-6 {
        weights.map(|w| w / sum)
    } else {
        weights
    }
}

/// Normals for a mesh that has none: each vertex gets the normal of the last
/// triangle that uses it, which is exact for unshared (flat-shaded) vertices.
fn flat_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    for tri in indices.as_chunks::<3>().0 {
        let [a, b, c] = tri.map(|i| Vec3::from(positions[i as usize]));
        let normal = (b - a).cross(c - a).normalize_or_zero();
        if normal != Vec3::ZERO {
            for &i in tri {
                normals[i as usize] = normal.to_array();
            }
        }
    }
    normals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn robot_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models/robot.gltf")
    }

    #[test]
    fn loads_the_robot_model() {
        let model = load_model(&robot_path()).unwrap();

        // Node tree: parents precede children, names and hierarchy survive.
        let names: Vec<_> = model
            .nodes
            .iter()
            .map(|n| n.name.as_deref().unwrap())
            .collect();
        assert_eq!(names[0], "Robot");
        assert_eq!(model.nodes.len(), 8);
        for (i, node) in model.nodes.iter().enumerate() {
            assert!(node.parent.is_none_or(|p| p < i));
        }
        let find = |name: &str| names.iter().position(|n| *n == name).unwrap();
        assert_eq!(model.nodes[find("Robot")].parent, None);
        assert_eq!(model.nodes[find("Head")].parent, Some(find("Robot")));
        assert_eq!(model.nodes[find("Antenna")].parent, Some(find("Head")));
        assert_eq!(model.nodes[find("Body")].scale, Vec3::new(0.9, 1.0, 0.5));
        assert!(model.nodes[find("Robot")].mesh.is_none());

        // Geometry: two box meshes, one primitive each.
        assert_eq!(model.meshes.len(), 2);
        for mesh in &model.meshes {
            assert_eq!(mesh.len(), 1);
            assert_eq!(mesh[0].mesh.vertices.len(), 24);
            assert_eq!(mesh[0].mesh.indices.len(), 36);
        }

        // Materials: one textured, one flat orange.
        assert_eq!(model.materials.len(), 2);
        assert_eq!(model.materials[0].base_color_image, Some(0));
        assert_eq!(model.materials[1].base_color_image, None);
        assert_eq!(model.materials[1].base_color, [1.0, 0.45, 0.1, 1.0]);
        assert_eq!(model.materials[0].metallic, 0.0);
        assert_eq!(model.materials[0].roughness, 0.8);
        assert_eq!(model.materials[1].metallic, 1.0);
        assert_eq!(model.materials[1].roughness, 0.35);
        assert_eq!(model.images.len(), 1);
        let image = &model.images[0];
        assert_eq!((image.width, image.height), (64, 64));
        assert_eq!(image.rgba.len(), 64 * 64 * 4);

        // Side files are reported so they can be watched for changes.
        let files: Vec<_> = model
            .files
            .iter()
            .map(|f| f.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(files, ["robot.gltf", "robot.bin", "robot_panel.png"]);
    }

    #[test]
    fn loads_node_animation_from_the_robot() {
        let model = load_model(&robot_path()).unwrap();
        assert!(model.skins.is_empty());
        assert_eq!(model.animations.len(), 1);
        let clip = &model.animations[0];
        assert_eq!(clip.name, "LookAround");
        assert_eq!(clip.duration, 3.0);
        assert_eq!(clip.channels.len(), 1);
        let head = clip.channels[0].node;
        assert_eq!(model.nodes[head].name.as_deref(), Some("Head"));
        assert!(matches!(&clip.channels[0].values, Keyframes::Rotation(v) if v.len() == 4));
    }

    #[test]
    fn loads_the_skinned_stalk() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models/stalk.gltf");
        let model = load_model(&path).unwrap();

        // The skin's joints are the four chained joint nodes, in order.
        assert_eq!(model.skins.len(), 1);
        let skin = &model.skins[0];
        let names: Vec<_> = skin
            .joints
            .iter()
            .map(|&j| model.nodes[j].name.as_deref().unwrap())
            .collect();
        assert_eq!(names, ["Joint0", "Joint1", "Joint2", "Joint3"]);
        for pair in skin.joints.windows(2) {
            assert_eq!(model.nodes[pair[1]].parent, Some(pair[0]));
        }
        // Joint i rests at height i; its inverse bind matrix undoes that.
        assert_eq!(
            skin.inverse_bind[2].w_axis.truncate(),
            Vec3::new(0.0, -2.0, 0.0)
        );

        let mesh_node = model.nodes.iter().find(|n| n.mesh.is_some()).unwrap();
        assert_eq!(mesh_node.skin, Some(0));
        for vertex in &model.meshes[0][0].mesh.vertices {
            let total: f32 = vertex.weights.iter().sum();
            assert!((total - 1.0).abs() < 1e-5);
            assert!(vertex
                .joints
                .iter()
                .all(|&j| (j as usize) < skin.joints.len()));
        }

        let names: Vec<_> = model.animations.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Sway", "Twist"]);
        let sway = &model.animations[0];
        assert_eq!(sway.duration, 2.0);
        assert_eq!(sway.channels.len(), 4);
        assert!(sway.channels.iter().all(|c| skin.joints.contains(&c.node)));
    }

    #[test]
    fn weights_are_normalised() {
        assert_eq!(
            normalized_weights([2.0, 2.0, 0.0, 0.0]),
            [0.5, 0.5, 0.0, 0.0]
        );
        assert_eq!(normalized_weights([0.0; 4]), [0.0; 4]);
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let error = load_model(Path::new("no/such/model.gltf")).unwrap_err();
        assert!(matches!(error, AssetError::Io { .. }), "{error}");
    }

    #[test]
    fn flat_normals_point_out_of_the_triangle() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]];
        let normals = flat_normals(&positions, &[0, 1, 2]);
        assert_eq!(normals, [[0.0, 1.0, 0.0]; 3]);
    }

    #[test]
    fn narrow_pixel_formats_expand_to_rgba() {
        let image = |format, pixels: &[u8]| gltf::image::Data {
            pixels: pixels.to_vec(),
            format,
            width: 1,
            height: 1,
        };
        use gltf::image::Format;
        assert_eq!(
            to_rgba8(&image(Format::R8G8B8, &[1, 2, 3])).unwrap(),
            [1, 2, 3, 255]
        );
        assert_eq!(to_rgba8(&image(Format::R8, &[7])).unwrap(), [7, 7, 7, 255]);
        assert_eq!(
            to_rgba8(&image(Format::R8G8, &[7, 9])).unwrap(),
            [7, 7, 7, 9]
        );
        assert!(to_rgba8(&image(Format::R16, &[0, 0])).is_none());
    }
}
