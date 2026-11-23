/// Upper bound on grid cells per axis, so a huge extent with a tiny cell size
/// cannot allocate an enormous grid.
const MAX_CELLS_PER_AXIS: usize = 2048;

/// Uniform-grid spatial index over node positions (CSR layout).
#[derive(Debug, Clone)]
pub struct PickIndex {
    positions: Vec<[f32; 2]>,
    min: [f32; 2],
    cell: f32,
    cols: usize,
    rows: usize,
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl PickIndex {
    pub fn build(positions: &[[f32; 2]], cell: f32) -> Self {
        if positions.is_empty() {
            return PickIndex {
                positions: Vec::new(),
                min: [0.0; 2],
                cell: 1.0,
                cols: 0,
                rows: 0,
                starts: vec![0],
                items: Vec::new(),
            };
        }

        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        for p in positions {
            for k in 0..2 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        let extent = (max[0] - min[0]).max(max[1] - min[1]);
        let cell = cell.max(1e-3).max(extent / MAX_CELLS_PER_AXIS as f32);
        let cols = ((max[0] - min[0]) / cell) as usize + 1;
        let rows = ((max[1] - min[1]) / cell) as usize + 1;

        let cell_of = |p: &[f32; 2]| -> usize {
            let cx = (((p[0] - min[0]) / cell) as usize).min(cols - 1);
            let cy = (((p[1] - min[1]) / cell) as usize).min(rows - 1);
            cy * cols + cx
        };

        let mut starts = vec![0u32; cols * rows + 1];
        for p in positions {
            starts[cell_of(p) + 1] += 1;
        }
        for i in 0..cols * rows {
            starts[i + 1] += starts[i];
        }
        let mut cursor = starts.clone();
        let mut items = vec![0u32; positions.len()];
        for (i, p) in positions.iter().enumerate() {
            let c = cell_of(p);
            items[cursor[c] as usize] = i as u32;
            cursor[c] += 1;
        }

        PickIndex {
            positions: positions.to_vec(),
            min,
            cell,
            cols,
            rows,
            starts,
            items,
        }
    }

    /// Closest node within `max_dist` world units of `p`, ties by lowest index.
    pub fn nearest_within(&self, p: [f32; 2], max_dist: f32) -> Option<usize> {
        if self.cols == 0 || !max_dist.is_finite() || max_dist < 0.0 {
            return None;
        }
        let range = |v: f32, lo: f32, n: usize| {
            let a = (((v - max_dist - lo) / self.cell).floor() as i64).max(0);
            let b = (((v + max_dist - lo) / self.cell).floor() as i64).min(n as i64 - 1);
            (a, b)
        };
        let (cx0, cx1) = range(p[0], self.min[0], self.cols);
        let (cy0, cy1) = range(p[1], self.min[1], self.rows);

        let mut best: Option<(f32, usize)> = None;
        let max2 = max_dist * max_dist;
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                let c = cy as usize * self.cols + cx as usize;
                for &i in &self.items[self.starts[c] as usize..self.starts[c + 1] as usize] {
                    let i = i as usize;
                    let q = self.positions[i];
                    let d2 = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
                    if d2 <= max2 && best.is_none_or(|(bd, bi)| d2 < bd || (d2 == bd && i < bi)) {
                        best = Some((d2, i));
                    }
                }
            }
        }
        best.map(|(_, i)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    fn brute(positions: &[[f32; 2]], p: [f32; 2], max: f32) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for (i, q) in positions.iter().enumerate() {
            let d2 = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
            if d2 <= max * max && best.is_none_or(|(bd, bi)| d2 < bd || (d2 == bd && i < bi)) {
                best = Some((d2, i));
            }
        }
        best.map(|(_, i)| i)
    }

    #[test]
    fn empty_index_returns_none() {
        let idx = PickIndex::build(&[], 10.0);
        assert_eq!(idx.nearest_within([0.0, 0.0], 100.0), None);
    }

    #[test]
    fn finds_nearest_and_respects_max_distance() {
        let pts = [[0.0, 0.0], [10.0, 0.0], [10.5, 0.0]];
        let idx = PickIndex::build(&pts, 5.0);
        assert_eq!(idx.nearest_within([10.4, 0.0], 1.0), Some(2));
        assert_eq!(idx.nearest_within([5.0, 0.0], 1.0), None);
    }

    #[test]
    fn coincident_points_pick_lowest_index() {
        let pts = [[3.0, 3.0], [3.0, 3.0], [3.0, 3.0]];
        let idx = PickIndex::build(&pts, 2.0);
        assert_eq!(idx.nearest_within([3.0, 3.0], 0.5), Some(0));
    }

    #[test]
    fn query_outside_the_data_bounds_is_safe() {
        let pts = [[0.0, 0.0], [1.0, 1.0]];
        let idx = PickIndex::build(&pts, 1.0);
        assert_eq!(idx.nearest_within([1000.0, -1000.0], 5.0), None);
        assert_eq!(idx.nearest_within([-2.0, -2.0], 5.0), Some(0));
    }

    #[test]
    fn matches_brute_force_on_random_points() {
        let mut rng = Rng::new(99);
        let pts: Vec<[f32; 2]> = (0..500)
            .map(|_| {
                [
                    rng.next_f32() * 200.0 - 100.0,
                    rng.next_f32() * 200.0 - 100.0,
                ]
            })
            .collect();
        let idx = PickIndex::build(&pts, 8.0);
        for _ in 0..300 {
            let p = [
                rng.next_f32() * 240.0 - 120.0,
                rng.next_f32() * 240.0 - 120.0,
            ];
            let max = rng.next_f32() * 20.0;
            assert_eq!(idx.nearest_within(p, max), brute(&pts, p, max));
        }
    }

    #[test]
    fn huge_extent_does_not_explode_grid_size() {
        let pts = [[-1.0e7, -1.0e7], [1.0e7, 1.0e7]];
        let idx = PickIndex::build(&pts, 1.0);
        assert_eq!(idx.nearest_within([1.0e7, 1.0e7], 1.0), Some(1));
    }
}
