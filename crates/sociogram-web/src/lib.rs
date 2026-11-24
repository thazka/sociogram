mod lod;
mod scene;

pub use lod::{
    adapt_lod, auto_space_half, LinkFade, LINK_FADE_AUTO_MIN_LINKS, LOD_MIN_SCALE,
    MAX_AUTO_SPACE_HALF,
};
pub use scene::{
    Capture, Scene, SceneError, SceneOptions, SelectHit, SelectionSummary, MAX_CAPTURE_SIDE,
    MAX_INDEX_LIST, MAX_POLYGON_VERTICES,
};

#[cfg(target_arch = "wasm32")]
mod web;
