use std::path::{Path, PathBuf};

use engine_render::{MeshData, Vertex};
use glam::{Quat, Vec3};

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
    /// Every file the model was read from, for change detection.
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct MaterialData {
    /// Linear RGBA, multiplied with the texture.
    pub base_color: [f32; 4],
    /// Index into [`ModelData::images`].
    pub base_color_image: Option<usize>,
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
}

/// Loads a glTF 2.0 model (`.gltf` with its side files, or a single `.glb`).
///
/// Reads the default scene's node tree, triangle meshes (positions, normals,
/// first UV set) and each material's base colour and base colour texture.
/// Everything else in the file (animations, skins, cameras, lights, other
/// material properties) is ignored.
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
            if normals.len() != positions.len() || uvs.len() != positions.len() {
                return Err(AssetError::invalid(
                    path,
                    "a mesh's vertex attributes differ in length",
                ));
            }

            let vertices = positions
                .iter()
                .zip(&normals)
                .zip(&uvs)
                .map(|((&position, &normal), &uv)| Vertex {
                    position,
                    normal,
                    uv,
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
    let mut pending: Vec<(gltf::Node, Option<usize>)> =
        scene.nodes().map(|node| (node, None)).collect();
    pending.reverse();
    while let Some((node, parent)) = pending.pop() {
        let (translation, rotation, scale) = node.transform().decomposed();
        let index = model.nodes.len();
        model.nodes.push(NodeData {
            name: node.name().map(str::to_owned),
            parent,
            translation: Vec3::from(translation),
            rotation: Quat::from_array(rotation),
            scale: Vec3::from(scale),
            mesh: node.mesh().map(|mesh| mesh.index()),
        });
        let children: Vec<_> = node.children().collect();
        pending.extend(children.into_iter().rev().map(|child| (child, Some(index))));
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
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        Format::R8 => px.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        // Two channels are grey + alpha.
        Format::R8G8 => px
            .chunks_exact(2)
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        _ => return None,
    })
}

/// Normals for a mesh that has none: each vertex gets the normal of the last
/// triangle that uses it, which is exact for unshared (flat-shaded) vertices.
fn flat_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    for tri in indices.chunks_exact(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| Vec3::from(positions[i as usize]));
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
