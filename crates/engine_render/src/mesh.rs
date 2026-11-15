use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

/// One mesh vertex as laid out in GPU memory.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

impl Vertex {
    pub(crate) const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
}

/// Mesh geometry in CPU memory: triangles, counter-clockwise front faces.
/// Upload it with `Renderer::create_mesh`.
#[derive(Debug, Clone, Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// A cube centred on the origin with edges of length `size`.
    pub fn cube(size: f32) -> Self {
        // (normal, u axis, v axis) per face, with u x v = normal.
        const FACES: [(Vec3, Vec3, Vec3); 6] = [
            (Vec3::X, Vec3::NEG_Z, Vec3::Y),
            (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            (Vec3::Y, Vec3::X, Vec3::NEG_Z),
            (Vec3::NEG_Y, Vec3::X, Vec3::Z),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        ];
        let mut mesh = Self::default();
        let half = size * 0.5;
        for (normal, u, v) in FACES {
            mesh.push_quad(normal * half, u * half, v * half, normal, 1.0);
        }
        mesh
    }

    /// A flat square in the XZ plane facing +Y, `size` units on a side.
    /// The texture repeats `uv_repeat` times across it.
    pub fn plane(size: f32, uv_repeat: f32) -> Self {
        let mut mesh = Self::default();
        let half = size * 0.5;
        mesh.push_quad(
            Vec3::ZERO,
            Vec3::X * half,
            Vec3::NEG_Z * half,
            Vec3::Y,
            uv_repeat,
        );
        mesh
    }

    /// Adds a quad spanning `centre ± u ± v`; `u x v` must point along `normal`.
    fn push_quad(&mut self, centre: Vec3, u: Vec3, v: Vec3, normal: Vec3, uv_scale: f32) {
        let base = self.vertices.len() as u32;
        let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
        for (su, sv) in corners {
            self.vertices.push(Vertex {
                position: (centre + u * su + v * sv).to_array(),
                normal: normal.to_array(),
                // Image row 0 is the top, so v runs downwards.
                uv: [(su + 1.0) * 0.5 * uv_scale, (1.0 - sv) * 0.5 * uv_scale],
            });
        }
        self.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Mesh geometry on the GPU.
///
/// Cheap to clone; clones share the same buffers, so replacing the geometry
/// with `Renderer::update_mesh` changes every clone at once.
#[derive(Debug, Clone)]
pub struct Mesh(pub(crate) Arc<Mutex<MeshGpu>>);

#[derive(Debug)]
pub(crate) struct MeshGpu {
    pub(crate) vertex_buffer: wgpu::Buffer,
    pub(crate) index_buffer: wgpu::Buffer,
    pub(crate) index_count: u32,
}

impl Mesh {
    pub(crate) fn new(gpu: MeshGpu) -> Self {
        Self(Arc::new(Mutex::new(gpu)))
    }

    /// Whether this is the only handle left to the mesh, i.e. no clone of it
    /// exists anywhere else.
    pub fn is_unique(&self) -> bool {
        Arc::strong_count(&self.0) == 1
    }

    pub(crate) fn gpu(&self) -> MutexGuard<'_, MeshGpu> {
        // Replaced whole, never left half-written, so valid even if poisoned.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triangle's winding must agree with its vertex normals, otherwise
    /// back-face culling hides it.
    fn assert_outward_winding(mesh: &MeshData) {
        for tri in mesh.indices.chunks(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| mesh.vertices[i as usize]);
            let (pa, pb, pc) = (
                Vec3::from(a.position),
                Vec3::from(b.position),
                Vec3::from(c.position),
            );
            let face_normal = (pb - pa).cross(pc - pa).normalize();
            assert!(face_normal.dot(Vec3::from(a.normal)) > 0.99);
        }
    }

    #[test]
    fn cube_is_closed_and_faces_outwards() {
        let cube = MeshData::cube(2.0);
        assert_eq!(cube.vertices.len(), 24);
        assert_eq!(cube.indices.len(), 36);
        assert_outward_winding(&cube);
        for v in &cube.vertices {
            // Normals point away from the centre, and corners sit at ±1.
            assert!(Vec3::from(v.position).dot(Vec3::from(v.normal)) > 0.0);
            assert!(v.position.iter().all(|c| c.abs() == 1.0));
        }
    }

    #[test]
    fn plane_faces_up() {
        let plane = MeshData::plane(10.0, 4.0);
        assert_outward_winding(&plane);
        assert!(plane.vertices.iter().all(|v| v.position[1] == 0.0));
        let max_u = plane.vertices.iter().map(|v| v.uv[0]).fold(0.0, f32::max);
        assert_eq!(max_u, 4.0);
    }
}
