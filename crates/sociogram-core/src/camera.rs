pub const MIN_ZOOM: f32 = 1e-4;
pub const MAX_ZOOM: f32 = 1e4;

/// 2D camera. Screen space is pixels with the origin at the top-left and y
/// down; world space has y up. `zoom` is pixels per world unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub center: [f32; 2],
    pub zoom: f32,
    pub viewport: [f32; 2],
}

impl Camera {
    pub fn new(viewport: [f32; 2]) -> Self {
        Camera {
            center: [0.0, 0.0],
            zoom: 1.0,
            viewport,
        }
    }

    pub fn world_to_screen(&self, w: [f32; 2]) -> [f32; 2] {
        [
            (w[0] - self.center[0]) * self.zoom + self.viewport[0] * 0.5,
            self.viewport[1] * 0.5 - (w[1] - self.center[1]) * self.zoom,
        ]
    }

    pub fn screen_to_world(&self, s: [f32; 2]) -> [f32; 2] {
        [
            (s[0] - self.viewport[0] * 0.5) / self.zoom + self.center[0],
            (self.viewport[1] * 0.5 - s[1]) / self.zoom + self.center[1],
        ]
    }

    /// Drag the content by a pixel delta (right/down on screen).
    pub fn pan_by_pixels(&mut self, dx: f32, dy: f32) {
        self.center[0] -= dx / self.zoom;
        self.center[1] += dy / self.zoom;
    }

    /// Multiply zoom by `factor`, keeping the world point under `screen` fixed.
    /// Non-finite or non-positive factors are ignored.
    pub fn zoom_at(&mut self, factor: f32, screen: [f32; 2]) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let before = self.screen_to_world(screen);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.screen_to_world(screen);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
    }

    /// Center on the bounds and zoom so they fit inside the viewport with
    /// `padding_px` on every side.
    pub fn fit_bounds(&mut self, min: [f32; 2], max: [f32; 2], padding_px: f32) {
        let w = (max[0] - min[0]).max(1e-6);
        let h = (max[1] - min[1]).max(1e-6);
        let avail_w = (self.viewport[0] - 2.0 * padding_px).max(1.0);
        let avail_h = (self.viewport[1] - 2.0 * padding_px).max(1.0);
        self.zoom = (avail_w / w).min(avail_h / h).clamp(MIN_ZOOM, MAX_ZOOM);
        self.center = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
    }

    /// `[center.x, center.y, zoom, 0, width, height, 0, 0]`, the layout of the
    /// shader's `Camera` uniform (two `vec4<f32>`).
    pub fn uniform(&self) -> [f32; 8] {
        [
            self.center[0],
            self.center[1],
            self.zoom,
            0.0,
            self.viewport[0],
            self.viewport[1],
            0.0,
            0.0,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    #[test]
    fn center_maps_to_viewport_middle() {
        let c = Camera::new([400.0, 300.0]);
        assert!(close(c.world_to_screen([0.0, 0.0]), [200.0, 150.0]));
    }

    #[test]
    fn y_axis_is_flipped() {
        let c = Camera::new([400.0, 300.0]);
        // +10 world units up is 10 pixels up the screen (smaller y).
        assert!(close(c.world_to_screen([0.0, 10.0]), [200.0, 140.0]));
    }

    #[test]
    fn roundtrip_world_screen() {
        let mut c = Camera::new([800.0, 600.0]);
        c.center = [12.5, -7.0];
        c.zoom = 3.0;
        let w = [40.0, 25.0];
        assert!(close(c.screen_to_world(c.world_to_screen(w)), w));
    }

    #[test]
    fn pan_moves_content_with_the_pointer() {
        let mut c = Camera::new([400.0, 300.0]);
        let before = c.world_to_screen([5.0, 5.0]);
        c.pan_by_pixels(30.0, -20.0);
        let after = c.world_to_screen([5.0, 5.0]);
        assert!(close(after, [before[0] + 30.0, before[1] - 20.0]));
    }

    #[test]
    fn zoom_at_keeps_point_under_cursor_fixed() {
        let mut c = Camera::new([800.0, 600.0]);
        c.center = [10.0, 20.0];
        let cursor = [123.0, 456.0];
        let world_before = c.screen_to_world(cursor);
        c.zoom_at(2.5, cursor);
        assert!((c.zoom - 2.5).abs() < 1e-6);
        assert!(close(c.screen_to_world(cursor), world_before));
    }

    #[test]
    fn zoom_is_clamped_and_bad_factors_are_ignored() {
        let mut c = Camera::new([100.0, 100.0]);
        c.zoom_at(1e9, [50.0, 50.0]);
        assert_eq!(c.zoom, MAX_ZOOM);
        c.zoom_at(1e-12, [50.0, 50.0]);
        assert_eq!(c.zoom, MIN_ZOOM);
        let z = c.zoom;
        c.zoom_at(f32::NAN, [50.0, 50.0]);
        c.zoom_at(-1.0, [50.0, 50.0]);
        c.zoom_at(0.0, [50.0, 50.0]);
        assert_eq!(c.zoom, z);
    }

    #[test]
    fn fit_bounds_maps_corners_to_viewport_edges() {
        let mut c = Camera::new([400.0, 300.0]);
        c.fit_bounds([-100.0, -50.0], [100.0, 50.0], 0.0);
        assert!((c.zoom - 2.0).abs() < 1e-5);
        let lo = c.world_to_screen([-100.0, 50.0]);
        let hi = c.world_to_screen([100.0, -50.0]);
        assert!(close(lo, [0.0, 50.0]));
        assert!(close(hi, [400.0, 250.0]));
    }

    #[test]
    fn fit_bounds_handles_a_single_point() {
        let mut c = Camera::new([400.0, 300.0]);
        c.fit_bounds([5.0, 5.0], [5.0, 5.0], 10.0);
        assert!(c.zoom.is_finite() && c.zoom <= MAX_ZOOM);
        assert!(close(c.center, [5.0, 5.0]));
    }

    #[test]
    fn uniform_matches_shader_layout() {
        let mut c = Camera::new([640.0, 480.0]);
        c.center = [1.0, 2.0];
        c.zoom = 4.0;
        assert_eq!(c.uniform(), [1.0, 2.0, 4.0, 0.0, 640.0, 480.0, 0.0, 0.0]);
    }
}
