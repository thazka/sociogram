use crate::graph::Graph;
use crate::layout::LayoutConfig;
use core::fmt;

/// Fixed-point scale for the per-cell center-of-mass accumulation (12 bits of
/// sub-cell precision). Integer sums are order independent, which makes the
/// GPU accumulation deterministic and lets the CPU reproduce it exactly.
pub const FIXED_SCALE: f32 = 4096.0;

/// Distances below this are clamped when computing repulsion strength.
const MIN_DIST2: f32 = 1.0;

pub const MIN_GRID_DIM: u32 = 4;
pub const MAX_GRID_DIM: u32 = 256;

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid layout config: {}", self.0)
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLayoutConfig {
    pub base: LayoutConfig,
    /// Cells per axis of the finest repulsion grid. Must be a power of two in
    /// `MIN_GRID_DIM..=MAX_GRID_DIM`. Coarser levels halve this down to 4.
    pub grid_dim: u32,
    /// Half the side of the square layout space; positions are clamped to
    /// `[-space_half, space_half]` on both axes.
    pub space_half: f32,
}

impl Default for GridLayoutConfig {
    fn default() -> Self {
        GridLayoutConfig {
            base: LayoutConfig::default(),
            grid_dim: 128,
            space_half: 1024.0,
        }
    }
}

impl GridLayoutConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let g = self.grid_dim;
        if !g.is_power_of_two() || !(MIN_GRID_DIM..=MAX_GRID_DIM).contains(&g) {
            return Err(ConfigError(format!(
                "grid_dim must be a power of two in {MIN_GRID_DIM}..={MAX_GRID_DIM}, got {g}"
            )));
        }
        if !(self.space_half.is_finite() && self.space_half > 0.0) {
            return Err(ConfigError(
                "space_half must be a positive finite number".into(),
            ));
        }
        Ok(())
    }
}

/// Number of grid levels: dims `grid_dim, grid_dim/2, ..., 4`.
pub fn level_count(grid_dim: u32) -> u32 {
    grid_dim.trailing_zeros() - 1
}

/// Total cells over all levels (finest level first).
pub fn total_cells(grid_dim: u32) -> usize {
    (0..level_count(grid_dim))
        .map(|l| {
            let d = (grid_dim >> l) as usize;
            d * d
        })
        .sum()
}

/// Visit every cell a node in finest cell `c0` interacts with, in a fixed
/// order. Together with the node's own cell (visited once at level 0) the
/// visited cells partition all nodes exactly once:
///
/// * level 0: the 6x6 block of cells whose parents neighbor the node's parent
///   (this contains the 3x3 near field),
/// * levels 1.. below the top: the same block, minus cells that touch the
///   node's own cell at that level (those are covered by finer levels),
/// * top level: every cell not touching the node's own cell.
///
/// This is a fixed-grid Barnes-Hut style approximation: about 150 cells per
/// node instead of one per node. The shader implements the same traversal.
pub fn visit_interactions(grid_dim: u32, c0: [i32; 2], mut f: impl FnMut(u32, i32, i32)) {
    let levels = level_count(grid_dim);
    for level in 0..levels {
        let d = (grid_dim >> level) as i32;
        let c = [c0[0] >> level, c0[1] >> level];
        let top = level == levels - 1;
        let (mut x_lo, mut x_hi, mut y_lo, mut y_hi) = (0, d - 1, 0, d - 1);
        if !top {
            let px = c[0] >> 1;
            let py = c[1] >> 1;
            x_lo = (2 * (px - 1)).max(0);
            x_hi = (2 * (px + 1) + 1).min(d - 1);
            y_lo = (2 * (py - 1)).max(0);
            y_hi = (2 * (py + 1) + 1).min(d - 1);
        }
        for y in y_lo..=y_hi {
            for x in x_lo..=x_hi {
                let near = (x - c[0]).abs() <= 1 && (y - c[1]).abs() <= 1;
                if near && level != 0 {
                    continue;
                }
                f(level, x, y);
            }
        }
    }
}

/// Finest-level cell coordinates and the continuous coordinates `u0` in
/// finest-cell units.
fn locate0(p: [f32; 2], cfg: &GridLayoutConfig) -> ([i32; 2], [f32; 2]) {
    let g = cfg.grid_dim as f32;
    let cell_size = 2.0 * cfg.space_half / g;
    let mut c0 = [0i32; 2];
    let mut u0 = [0f32; 2];
    for k in 0..2 {
        u0[k] = (p[k] + cfg.space_half) / cell_size;
        c0[k] = u0[k].floor().clamp(0.0, g - 1.0) as i32;
    }
    (c0, u0)
}

/// Fixed-point in-cell offset of a node at `level`. Coarse cell coordinates
/// are derived from the finest ones by shifting, never recomputed from floats,
/// so a node is always in exactly the cell the traversal thinks it is in.
fn level_fixed(c0: [i32; 2], u0: [f32; 2], level: u32) -> [i32; 2] {
    let scale = (1u32 << level) as f32;
    let mut fixed = [0i32; 2];
    for k in 0..2 {
        let c = (c0[k] >> level) as f32;
        let f = (u0[k] / scale - c).clamp(0.0, 1.0);
        fixed[k] = (f * FIXED_SCALE) as i32;
    }
    fixed
}

fn level_start(grid_dim: u32, level: u32) -> usize {
    (0..level)
        .map(|l| {
            let d = (grid_dim >> l) as usize;
            d * d
        })
        .sum()
}

/// CPU oracle for the GPU layout: same algorithm, same traversal order and
/// the same fixed-point accumulation as the GPU shader.
#[derive(Debug, Clone)]
pub struct GridLayout {
    positions: Vec<[f32; 2]>,
    velocities: Vec<[f32; 2]>,
    alpha: f32,
    config: GridLayoutConfig,
}

impl GridLayout {
    pub fn new(graph: &Graph, config: GridLayoutConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        Ok(GridLayout {
            positions: graph.positions().to_vec(),
            velocities: vec![[0.0; 2]; graph.node_count()],
            alpha: 1.0,
            config,
        })
    }

    pub fn positions(&self) -> &[[f32; 2]] {
        &self.positions
    }

    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    pub fn is_settled(&self) -> bool {
        self.alpha < self.config.base.alpha_min
    }

    /// `[count, sum_x, sum_y]` per cell over all levels.
    fn accumulate(&self, old: &[[f32; 2]]) -> Vec<[i32; 3]> {
        let cfg = &self.config;
        let levels = level_count(cfg.grid_dim);
        let mut cells = vec![[0i32; 3]; total_cells(cfg.grid_dim)];
        for p in old {
            let (c0, u0) = locate0(*p, cfg);
            for level in 0..levels {
                let d = (cfg.grid_dim >> level) as usize;
                let (cx, cy) = ((c0[0] >> level) as usize, (c0[1] >> level) as usize);
                let fixed = level_fixed(c0, u0, level);
                let cell = &mut cells[level_start(cfg.grid_dim, level) + cy * d + cx];
                cell[0] += 1;
                cell[1] += fixed[0];
                cell[2] += fixed[1];
            }
        }
        cells
    }

    pub fn step(&mut self, graph: &Graph) {
        let cfg = self.config;
        let c = cfg.base;
        let a = self.alpha;
        let half = cfg.space_half;
        let old = self.positions.clone();
        let cells = self.accumulate(&old);
        let starts: Vec<usize> = (0..level_count(cfg.grid_dim))
            .map(|l| level_start(cfg.grid_dim, l))
            .collect();

        for i in 0..old.len() {
            let p = old[i];
            let (c0, u0) = locate0(p, &cfg);
            let own_fixed = level_fixed(c0, u0, 0);
            let mut v = self.velocities[i];

            if c.repulsion != 0.0 {
                visit_interactions(cfg.grid_dim, c0, |level, x, y| {
                    let d = (cfg.grid_dim >> level) as i32;
                    let cell = cells[starts[level as usize] + (y * d + x) as usize];
                    let (mut cnt, mut sx, mut sy) = (cell[0], cell[1], cell[2]);
                    if cnt == 0 {
                        return;
                    }
                    if level == 0 && x == c0[0] && y == c0[1] {
                        cnt -= 1;
                        sx -= own_fixed[0];
                        sy -= own_fixed[1];
                        if cnt == 0 {
                            return;
                        }
                    }
                    let cell_size = 2.0 * half / d as f32;
                    let fc = cnt as f32 * FIXED_SCALE;
                    let com = [
                        (x as f32 + sx as f32 / fc) * cell_size - half,
                        (y as f32 + sy as f32 / fc) * cell_size - half,
                    ];
                    let dv = [p[0] - com[0], p[1] - com[1]];
                    let d2 = dv[0] * dv[0] + dv[1] * dv[1];
                    if d2 < 1e-6 {
                        return;
                    }
                    let k = c.repulsion * a * cnt as f32 / d2.max(MIN_DIST2);
                    v[0] += dv[0] * k;
                    v[1] += dv[1] * k;
                });
            }

            for &j in graph.neighbors_of(i) {
                let q = old[j as usize];
                let d = [q[0] - p[0], q[1] - p[1]];
                let dist = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-3);
                let k = (dist - c.link_distance) / dist * c.link_spring * a * 0.5;
                v[0] += d[0] * k;
                v[1] += d[1] * k;
            }

            v[0] -= p[0] * c.gravity * a;
            v[1] -= p[1] * c.gravity * a;
            v[0] *= 1.0 - c.friction;
            v[1] *= 1.0 - c.friction;
            self.velocities[i] = v;
            self.positions[i] = [
                (p[0] + v[0]).clamp(-half, half),
                (p[1] + v[1]).clamp(-half, half),
            ];
        }

        self.alpha *= 1.0 - c.alpha_decay;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphData;
    use crate::rng::Rng;

    fn cfg(spring: f32, gravity: f32, repulsion: f32) -> GridLayoutConfig {
        GridLayoutConfig {
            base: LayoutConfig {
                gravity,
                repulsion,
                link_spring: spring,
                link_distance: 30.0,
                friction: 0.2,
                alpha_decay: 0.0,
                alpha_min: 0.001,
            },
            ..Default::default()
        }
    }

    fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
    }

    #[test]
    fn validates_config() {
        assert!(GridLayoutConfig::default().validate().is_ok());
        for bad in [0, 1, 2, 3, 6, 100, 512] {
            let c = GridLayoutConfig {
                grid_dim: bad,
                ..Default::default()
            };
            assert!(c.validate().is_err(), "grid_dim {bad} should be rejected");
        }
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let c = GridLayoutConfig {
                space_half: bad,
                ..Default::default()
            };
            assert!(c.validate().is_err());
        }
    }

    #[test]
    fn level_layout_is_consistent() {
        assert_eq!(level_count(4), 1);
        assert_eq!(level_count(128), 6);
        assert_eq!(total_cells(4), 16);
        assert_eq!(total_cells(8), 64 + 16);
    }

    #[test]
    fn interactions_partition_all_other_nodes_exactly_once() {
        // For every node, the visited cells (the node's own cell is visited
        // once, at level 0, and still contains the node itself) must account
        // for every node exactly once: the sum of visited counts equals n.
        // Checked for several grid sizes and random positions.
        for grid_dim in [4u32, 8, 16, 64, 128] {
            let mut rng = Rng::new(5);
            let n = 300;
            let positions: Vec<[f32; 2]> = (0..n)
                .map(|_| {
                    [
                        rng.next_f32() * 2000.0 - 1000.0,
                        rng.next_f32() * 2000.0 - 1000.0,
                    ]
                })
                .collect();
            let graph = Graph::new(GraphData {
                node_count: n,
                positions: Some(positions.clone()),
                ..Default::default()
            })
            .unwrap();
            let config = GridLayoutConfig {
                grid_dim,
                ..Default::default()
            };
            let layout = GridLayout::new(&graph, config).unwrap();
            let cells = layout.accumulate(&positions);
            for p in &positions {
                let (c0, _) = locate0(*p, &config);
                let mut total = 0;
                visit_interactions(grid_dim, c0, |level, x, y| {
                    let d = (grid_dim >> level) as i32;
                    let idx = level_start(grid_dim, level) + (y * d + x) as usize;
                    total += cells[idx][0];
                });
                assert_eq!(total, n as i32, "grid_dim {grid_dim}");
            }
        }
    }

    #[test]
    fn spring_converges_to_link_distance() {
        let g = Graph::new(GraphData {
            node_count: 2,
            edges: vec![[0, 1]],
            positions: Some(vec![[0.0, 0.0], [1.0, 0.0]]),
            ..Default::default()
        })
        .unwrap();
        let mut l = GridLayout::new(&g, cfg(0.1, 0.0, 0.0)).unwrap();
        for _ in 0..500 {
            l.step(&g);
        }
        let p = l.positions();
        assert!((dist(p[0], p[1]) - 30.0).abs() < 0.01);
    }

    #[test]
    fn nodes_in_the_same_cell_still_repel() {
        // Finest cell size is 16, so both nodes share a cell.
        let g = Graph::new(GraphData {
            node_count: 2,
            positions: Some(vec![[0.5, 0.0], [1.5, 0.0]]),
            ..Default::default()
        })
        .unwrap();
        let mut l = GridLayout::new(&g, cfg(0.0, 0.0, 1.0)).unwrap();
        for _ in 0..10 {
            l.step(&g);
        }
        let p = l.positions();
        assert!(dist(p[0], p[1]) > 1.0);
    }

    #[test]
    fn far_apart_nodes_repel_through_coarse_levels() {
        let g = Graph::new(GraphData {
            node_count: 2,
            positions: Some(vec![[-900.0, 0.0], [900.0, 0.0]]),
            ..Default::default()
        })
        .unwrap();
        let mut l = GridLayout::new(&g, cfg(0.0, 0.0, 1.0e6)).unwrap();
        l.step(&g);
        let p = l.positions();
        assert!(p[0][0] < -900.0 && p[1][0] > 900.0, "{p:?}");
    }

    #[test]
    fn positions_are_clamped_to_the_space() {
        let g = Graph::new(GraphData {
            node_count: 2,
            positions: Some(vec![[1023.0, 0.0], [1023.5, 0.0]]),
            ..Default::default()
        })
        .unwrap();
        let mut l = GridLayout::new(&g, cfg(0.0, 0.0, 1.0e6)).unwrap();
        for _ in 0..20 {
            l.step(&g);
        }
        for p in l.positions() {
            assert!(p[0].abs() <= 1024.0 && p[1].abs() <= 1024.0);
            assert!(p[0].is_finite() && p[1].is_finite());
        }
    }

    #[test]
    fn default_config_stays_finite_and_settles() {
        let n = 200u32;
        let edges: Vec<[u32; 2]> = (0..n).map(|i| [i, (i + 1) % n]).collect();
        let g = Graph::new(GraphData {
            node_count: n as usize,
            edges,
            ..Default::default()
        })
        .unwrap();
        let mut l = GridLayout::new(&g, GridLayoutConfig::default()).unwrap();
        for _ in 0..800 {
            l.step(&g);
        }
        assert!(l
            .positions()
            .iter()
            .all(|p| p[0].is_finite() && p[1].is_finite()));
        assert!(l.is_settled());
    }

    #[test]
    fn layout_is_deterministic() {
        let n = 50u32;
        let edges: Vec<[u32; 2]> = (0..n).map(|i| [i, (i * 7 + 3) % n]).collect();
        let g = Graph::new(GraphData {
            node_count: n as usize,
            edges,
            ..Default::default()
        })
        .unwrap();
        let run = || {
            let mut l = GridLayout::new(&g, GridLayoutConfig::default()).unwrap();
            for _ in 0..100 {
                l.step(&g);
            }
            l.positions().to_vec()
        };
        assert_eq!(run(), run());
    }
}
