//! Reading asset files into CPU-side data: images and glTF models.
//!
//! Nothing here touches the GPU; the engine uploads the results and caches
//! them.

mod animation;
mod image_data;
mod model;

use std::path::{Path, PathBuf};

pub use animation::{AnimationData, Channel, Keyframes, Sample, SkinData};
pub use image_data::{decode_image, load_image, ImageData};
pub use model::{load_model, MaterialData, ModelData, NodeData, PrimitiveData};

/// Why an asset could not be loaded.
#[derive(Debug)]
pub enum AssetError {
    /// The file could not be read.
    Io { path: PathBuf, message: String },
    /// The file was read but its contents are invalid or unsupported.
    Invalid { path: PathBuf, message: String },
}

impl AssetError {
    pub(crate) fn io(path: &Path, error: impl std::fmt::Display) -> Self {
        Self::Io {
            path: path.to_owned(),
            message: error.to_string(),
        }
    }

    pub(crate) fn invalid(path: &Path, error: impl std::fmt::Display) -> Self {
        Self::Invalid {
            path: path.to_owned(),
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for AssetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetError::Io { path, message } => {
                write!(f, "cannot read {}: {message}", path.display())
            }
            AssetError::Invalid { path, message } => {
                write!(f, "cannot use {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for AssetError {}
