struct Params {
    counts: vec4<u32>, // node_count, grid_dim, 0, 0
    a: vec4<f32>,      // gravity, repulsion, link_spring, link_distance
    b: vec4<f32>,      // friction, alpha, space_half, 0
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> pos_in: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read_write> pos_out: array<vec2<f32>>;
@group(0) @binding(3) var<storage, read_write> vel: array<vec2<f32>>;
@group(0) @binding(4) var<storage, read> offsets: array<u32>;
@group(0) @binding(5) var<storage, read> neighbors: array<u32>;

// Hierarchical grid, finest level first, one vec4<i32> per cell:
// (count, sum_x, sum_y, unused), the sums in fixed point (FIXED_SCALE)
// relative to the cell origin. The same buffer is bound twice: as atomics for
// the accumulate pass (group 1) and as plain read-only data for the step pass
// (group 2). The passes run separately, so the usages never overlap.
@group(1) @binding(0) var<storage, read_write> cells_acc: array<atomic<i32>>;
@group(2) @binding(0) var<storage, read> cells: array<vec4<i32>>;

const FIXED_SCALE: f32 = 4096.0;
const MIN_DIST2: f32 = 1.0;

struct Loc0 {
    c0: vec2<i32>, // finest cell coordinates
    u0: vec2<f32>, // continuous coordinates in finest-cell units
};

fn locate0(p: vec2<f32>) -> Loc0 {
    let g = f32(params.counts.y);
    let half = params.b.z;
    let cell_size = 2.0 * half / g;
    var l: Loc0;
    l.u0 = (p + vec2<f32>(half)) / cell_size;
    l.c0 = vec2<i32>(clamp(floor(l.u0), vec2<f32>(0.0), vec2<f32>(g - 1.0)));
    return l;
}

// Coarse cell coordinates come from shifting the finest ones, never from
// floats, so a node is always in exactly the cell the traversal expects.
fn level_fixed(l: Loc0, level: u32) -> vec2<i32> {
    let scale = f32(1u << level);
    let c = vec2<f32>(f32(l.c0.x >> level), f32(l.c0.y >> level));
    let f = clamp(l.u0 / scale - c, vec2<f32>(0.0), vec2<f32>(1.0));
    return vec2<i32>(f * FIXED_SCALE);
}

@compute @workgroup_size(64)
fn accumulate(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.counts.x) {
        return;
    }
    let g = params.counts.y;
    let levels = countTrailingZeros(g) - 1u;
    let l = locate0(pos_in[i]);
    var start = 0u;
    for (var level = 0u; level < levels; level = level + 1u) {
        let d = g >> level;
        let cx = u32(l.c0.x >> level);
        let cy = u32(l.c0.y >> level);
        let fixed = level_fixed(l, level);
        let base = (start + cy * d + cx) * 4u;
        atomicAdd(&cells_acc[base], 1);
        atomicAdd(&cells_acc[base + 1u], fixed.x);
        atomicAdd(&cells_acc[base + 2u], fixed.y);
        start = start + d * d;
    }
}

@compute @workgroup_size(64)
fn step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.counts.x) {
        return;
    }
    let g = params.counts.y;
    let half = params.b.z;
    let alpha = params.b.y;
    let gravity = params.a.x;
    let repulsion = params.a.y;
    let spring = params.a.z;
    let link_distance = params.a.w;
    let friction = params.b.x;

    let p = pos_in[i];
    let own = locate0(p);
    let own_fixed = level_fixed(own, 0u);
    var v = vel[i];

    if (repulsion != 0.0) {
        let levels = countTrailingZeros(g) - 1u;
        var start = 0u;
        for (var level = 0u; level < levels; level = level + 1u) {
            let d = i32(g >> level);
            let cx = own.c0.x >> level;
            let cy = own.c0.y >> level;
            let top = level == levels - 1u;
            var x_lo = 0;
            var x_hi = d - 1;
            var y_lo = 0;
            var y_hi = d - 1;
            if (!top) {
                let px = cx >> 1u;
                let py = cy >> 1u;
                x_lo = max(2 * (px - 1), 0);
                x_hi = min(2 * (px + 1) + 1, d - 1);
                y_lo = max(2 * (py - 1), 0);
                y_hi = min(2 * (py + 1) + 1, d - 1);
            }
            let cell_size = 2.0 * half / f32(d);
            for (var y = y_lo; y <= y_hi; y = y + 1) {
                for (var x = x_lo; x <= x_hi; x = x + 1) {
                    let near = abs(x - cx) <= 1 && abs(y - cy) <= 1;
                    if (near && level != 0u) {
                        continue;
                    }
                    let cell = cells[start + u32(y * d + x)];
                    var cnt = cell.x;
                    if (cnt == 0) {
                        continue;
                    }
                    var sx = cell.y;
                    var sy = cell.z;
                    if (level == 0u && x == own.c0.x && y == own.c0.y) {
                        cnt = cnt - 1;
                        sx = sx - own_fixed.x;
                        sy = sy - own_fixed.y;
                        if (cnt == 0) {
                            continue;
                        }
                    }
                    let fc = f32(cnt) * FIXED_SCALE;
                    let com = vec2<f32>(f32(x) + f32(sx) / fc, f32(y) + f32(sy) / fc) * cell_size
                        - vec2<f32>(half);
                    let dv = p - com;
                    let d2 = dot(dv, dv);
                    if (d2 < 1e-6) {
                        continue;
                    }
                    v = v + dv * (repulsion * alpha * f32(cnt) / max(d2, MIN_DIST2));
                }
            }
            start = start + u32(d * d);
        }
    }

    for (var k = offsets[i]; k < offsets[i + 1u]; k = k + 1u) {
        let q = pos_in[neighbors[k]];
        let dq = q - p;
        let dist = max(sqrt(dot(dq, dq)), 1e-3);
        let kk = (dist - link_distance) / dist * spring * alpha * 0.5;
        v = v + dq * kk;
    }

    v = v - p * gravity * alpha;
    v = v * (1.0 - friction);
    vel[i] = v;
    pos_out[i] = clamp(p + v, vec2<f32>(-half), vec2<f32>(half));
}
