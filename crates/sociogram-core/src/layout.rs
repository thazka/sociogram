use crate::graph::Graph;

/// Distances below this are clamped when computing repulsion strength.
const MIN_DIST2: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutConfig {
    pub gravity: f32,
    pub repulsion: f32,
    pub link_spring: f32,
    pub link_distance: f32,
    pub friction: f32,
    pub alpha_decay: f32,
    pub alpha_min: f32,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        LayoutConfig {
            gravity: 0.02,
            repulsion: 20.0,
            link_spring: 0.1,
            link_distance: 30.0,
            friction: 0.2,
            alpha_decay: 0.01,
            alpha_min: 0.001,
        }
    }
}

/// O(n^2) CPU reference layout. Used for tests and as ground truth for the
/// GPU layout; not meant for large graphs.
#[derive(Debug, Clone)]
pub struct Layout {
    positions: Vec<[f32; 2]>,
    velocities: Vec<[f32; 2]>,
    alpha: f32,
    config: LayoutConfig,
}

impl Layout {
    pub fn new(graph: &Graph, config: LayoutConfig) -> Self {
        Layout {
            positions: graph.positions().to_vec(),
            velocities: vec![[0.0; 2]; graph.node_count()],
            alpha: 1.0,
            config,
        }
    }

    pub fn positions(&self) -> &[[f32; 2]] {
        &self.positions
    }

    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    pub fn is_settled(&self) -> bool {
        self.alpha < self.config.alpha_min
    }

    #[allow(clippy::needless_range_loop)]
    pub fn step(&mut self, graph: &Graph) {
        let n = self.positions.len();
        let a = self.alpha;
        let c = self.config;

        if c.repulsion != 0.0 {
            for i in 0..n {
                for j in (i + 1)..n {
                    let mut dx = self.positions[i][0] - self.positions[j][0];
                    let mut dy = self.positions[i][1] - self.positions[j][1];
                    let mut d2 = dx * dx + dy * dy;
                    if d2 < 1e-6 {
                        // Deterministic nudge so coincident nodes can separate.
                        dx = (i as f32 - j as f32) * 1e-3;
                        dy = 1e-3;
                        d2 = dx * dx + dy * dy;
                    }
                    let k = c.repulsion * a / d2.max(MIN_DIST2);
                    let (fx, fy) = (dx * k, dy * k);
                    self.velocities[i][0] += fx;
                    self.velocities[i][1] += fy;
                    self.velocities[j][0] -= fx;
                    self.velocities[j][1] -= fy;
                }
            }
        }

        for &[s, t] in graph.edges() {
            let (s, t) = (s as usize, t as usize);
            if s == t {
                continue;
            }
            let dx = self.positions[t][0] - self.positions[s][0];
            let dy = self.positions[t][1] - self.positions[s][1];
            let dist = (dx * dx + dy * dy).sqrt().max(1e-3);
            let k = (dist - c.link_distance) / dist * c.link_spring * a * 0.5;
            self.velocities[s][0] += dx * k;
            self.velocities[s][1] += dy * k;
            self.velocities[t][0] -= dx * k;
            self.velocities[t][1] -= dy * k;
        }

        for i in 0..n {
            self.velocities[i][0] -= self.positions[i][0] * c.gravity * a;
            self.velocities[i][1] -= self.positions[i][1] * c.gravity * a;
            self.velocities[i][0] *= 1.0 - c.friction;
            self.velocities[i][1] *= 1.0 - c.friction;
            self.positions[i][0] += self.velocities[i][0];
            self.positions[i][1] += self.velocities[i][1];
        }

        self.alpha *= 1.0 - c.alpha_decay;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphData;

    fn pure(spring: f32, gravity: f32, repulsion: f32) -> LayoutConfig {
        LayoutConfig {
            gravity,
            repulsion,
            link_spring: spring,
            link_distance: 30.0,
            friction: 0.2,
            alpha_decay: 0.0,
            alpha_min: 0.001,
        }
    }

    fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
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
        let mut l = Layout::new(&g, pure(0.1, 0.0, 0.0));
        for _ in 0..500 {
            l.step(&g);
        }
        let p = l.positions();
        assert!((dist(p[0], p[1]) - 30.0).abs() < 0.01);
    }

    #[test]
    fn gravity_pulls_node_to_origin() {
        let g = Graph::new(GraphData {
            node_count: 1,
            positions: Some(vec![[100.0, 0.0]]),
            ..Default::default()
        })
        .unwrap();
        let mut l = Layout::new(&g, pure(0.0, 0.1, 0.0));
        for _ in 0..500 {
            l.step(&g);
        }
        assert!(l.positions()[0][0].abs() < 0.01);
    }

    #[test]
    fn repulsion_pushes_unconnected_nodes_apart() {
        let g = Graph::new(GraphData {
            node_count: 2,
            positions: Some(vec![[0.0, 0.0], [1.0, 0.0]]),
            ..Default::default()
        })
        .unwrap();
        let mut l = Layout::new(&g, pure(0.0, 0.0, 1.0));
        for _ in 0..10 {
            l.step(&g);
        }
        let p = l.positions();
        assert!(dist(p[0], p[1]) > 1.0);
    }

    #[test]
    fn coincident_nodes_separate_without_nan() {
        let g = Graph::new(GraphData {
            node_count: 2,
            positions: Some(vec![[5.0, 5.0], [5.0, 5.0]]),
            ..Default::default()
        })
        .unwrap();
        let mut l = Layout::new(&g, pure(0.0, 0.0, 1.0));
        for _ in 0..20 {
            l.step(&g);
        }
        let p = l.positions();
        assert!(p.iter().all(|v| v[0].is_finite() && v[1].is_finite()));
        assert!(dist(p[0], p[1]) > 0.0);
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
        let mut l = Layout::new(&g, LayoutConfig::default());
        assert!(!l.is_settled());
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
            let mut l = Layout::new(&g, LayoutConfig::default());
            for _ in 0..100 {
                l.step(&g);
            }
            l.positions().to_vec()
        };
        assert_eq!(run(), run());
    }
}
