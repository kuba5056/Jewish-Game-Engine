use glam::{Vec3, Vec4};

use crate::texture::Texture;

/// How a surface looks: its colour and how it reacts to light.
///
/// ```
/// # use engine_render::Material;
/// # use engine_render::glam::Vec4;
/// let gold = Material {
///     base_color: Vec4::new(1.0, 0.77, 0.34, 1.0),
///     metallic: 1.0,
///     roughness: 0.3,
///     ..Material::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct Material {
    /// Colour texture; `None` for a plain surface.
    pub texture: Option<Texture>,
    /// Multiplied with the texture (linear RGBA).
    pub base_color: Vec4,
    /// 0 for non-metals (plastic, wood, stone), 1 for bare metal.
    pub metallic: f32,
    /// 0 is mirror-smooth, 1 is fully matte.
    pub roughness: f32,
    /// Light the surface gives off by itself (linear RGB); values above 1 glow.
    pub emissive: Vec3,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            texture: None,
            base_color: Vec4::ONE,
            metallic: 0.0,
            roughness: 0.6,
            emissive: Vec3::ZERO,
        }
    }
}

impl Material {
    /// A plain, non-metallic surface of the given colour (linear RGB).
    pub fn colored(r: f32, g: f32, b: f32) -> Self {
        Self {
            base_color: Vec4::new(r, g, b, 1.0),
            ..Self::default()
        }
    }

    /// A non-metallic surface showing `texture`.
    pub fn textured(texture: Texture) -> Self {
        Self {
            texture: Some(texture),
            ..Self::default()
        }
    }
}
