//! Level-of-detail rules that keep large graphs fast and readable: link fading by
//! on-screen length, its frame-time controller and the layout space size.

/// How links are faded out by on-screen length. Long links dominate the cost of
/// drawing large graphs (fill rate) while adding little to the picture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LinkFade {
    /// Draw every link at full strength.
    Off,
    /// Off for small graphs; for graphs with at least
    /// [`LINK_FADE_AUTO_MIN_LINKS`] links, links fade out between 1/8 and 3/8
    /// of the larger viewport side.
    Auto,
    /// Fade between `start` and `end` pixels of on-screen length (drawing-buffer
    /// pixels). Invalid ranges behave like `Off`.
    Px { start: f32, end: f32 },
}

/// Graphs with fewer links than this are never faded by [`LinkFade::Auto`].
pub const LINK_FADE_AUTO_MIN_LINKS: usize = 50_000;

/// Frame time (ms, smoothed) above which `Auto` link fading gets stricter.
const LOD_SLOW_MS: f32 = 40.0;
/// Frame time (ms, smoothed) below which it relaxes again.
const LOD_FAST_MS: f32 = 22.0;
/// Strictest fade scale the adaptive controller will use (1.0 = unscaled range).
pub const LOD_MIN_SCALE: f32 = 0.05;
/// A single frame slower than this is treated as a hitch (hidden tab, GC), not load.
const LOD_OUTLIER_MS: f32 = 250.0;

/// Next smoothed frame time and fade scale after one more frame of `ms`.
///
/// Slow frames shrink the scale by 15% (fast response, the page is already
/// janky); fast frames grow it by 3% (slow recovery, avoids flip-flopping at
/// the edge). Between the two thresholds nothing changes. Invalid samples are
/// ignored.
pub fn adapt_lod(ema: Option<f32>, scale: f32, ms: f32) -> (Option<f32>, f32) {
    if !ms.is_finite() || ms <= 0.0 {
        return (ema, scale);
    }
    let ms = ms.min(LOD_OUTLIER_MS);
    let ema = ema.map_or(ms, |e| e * 0.8 + ms * 0.2);
    let scale = if ema > LOD_SLOW_MS {
        (scale * 0.85).max(LOD_MIN_SCALE)
    } else if ema < LOD_FAST_MS {
        (scale * 1.03).min(1.0)
    } else {
        scale
    };
    (Some(ema), scale)
}

/// Largest layout half-size chosen automatically (float precision stays fine).
pub const MAX_AUTO_SPACE_HALF: f32 = 32_768.0;

/// Half-size of the layout space for a graph of `node_count` nodes.
///
/// Repulsion falls off as 1/distance and gravity grows linearly, so a roughly
/// uniform graph settles at radius `sqrt(repulsion * n / gravity)`. The space
/// must be larger than that, or nodes are squeezed against its walls. `minimum`
/// is returned for small graphs and whenever the forces give no estimate
/// (repulsion or gravity off).
pub fn auto_space_half(
    node_count: usize,
    base: &sociogram_core::LayoutConfig,
    minimum: f32,
) -> f32 {
    if !(base.repulsion > 0.0 && base.gravity > 0.0) || node_count == 0 {
        return minimum;
    }
    let radius = (base.repulsion * node_count as f32 / base.gravity).sqrt();
    (1.6 * radius).clamp(minimum, MAX_AUTO_SPACE_HALF.max(minimum))
}
