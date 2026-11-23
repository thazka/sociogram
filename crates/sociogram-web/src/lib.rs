mod lod;
mod scene;

pub use lod::{
    adapt_lod, auto_space_half, LinkFade, LINK_FADE_AUTO_MIN_LINKS, LOD_MIN_SCALE,
    MAX_AUTO_SPACE_HALF,
};
pub use scene::{Capture, Scene, SceneError, SceneOptions, MAX_CAPTURE_SIDE};

#[cfg(target_arch = "wasm32")]
mod web;
