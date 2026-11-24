struct Camera { view: vec4<f32>, viewport: vec4<f32> };
@group(0) @binding(0) var<uniform> cam: Camera;

// World positions of all nodes, written by the layout (or uploaded by the CPU).
@group(1) @binding(0) var<storage, read> positions: array<vec2<f32>>;

// Selection state. One word per node and per link instance (0 or 1).
// dim.v.x = 1 while a selection is active; dim.v.y and dim.v.z are the alpha
// multipliers for unselected nodes and links.
@group(2) @binding(0) var<storage, read> node_flags: array<u32>;
@group(2) @binding(1) var<storage, read> link_flags: array<u32>;
struct Dim { v: vec4<f32> };
@group(2) @binding(2) var<uniform> dim: Dim;

fn to_ndc(world: vec2<f32>) -> vec2<f32> {
    let px = (world - cam.view.xy) * cam.view.z;
    return px / (cam.viewport.xy * 0.5);
}

var<private> CORNERS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
    vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0));

struct NodeIn {
    @location(0) size: f32,
    @location(1) color: vec4<f32>,
};
struct NodeOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) radius_px: f32,
};

@vertex
fn vs_node(
    @builtin(vertex_index) vi: u32,
    @builtin(instance_index) ii: u32,
    inst: NodeIn,
) -> NodeOut {
    let c = CORNERS[vi];
    let radius_px = max(inst.size * sqrt(cam.view.z), 1.0);
    let ndc = to_ndc(positions[ii]) + c * radius_px / (cam.viewport.xy * 0.5);
    var o: NodeOut;
    o.clip = vec4<f32>(ndc, 0.0, 1.0);
    o.uv = c;
    var a = inst.color.a;
    if (dim.v.x > 0.5 && node_flags[ii] == 0u) {
        a = a * dim.v.y;
    }
    o.color = vec4<f32>(inst.color.rgb, a);
    o.radius_px = radius_px;
    return o;
}

@fragment
fn fs_node(i: NodeOut) -> @location(0) vec4<f32> {
    let d = length(i.uv);
    let edge = 1.5 / i.radius_px;
    let a = 1.0 - smoothstep(1.0 - edge, 1.0, d);
    return vec4<f32>(i.color.rgb, i.color.a * a);
}

struct LinkIn {
    @location(0) src: u32,
    @location(1) dst: u32,
    @location(2) color: vec4<f32>,
    @location(3) width: f32,
};
struct LinkOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_link(
    @builtin(vertex_index) vi: u32,
    @builtin(instance_index) ii: u32,
    inst: LinkIn,
) -> LinkOut {
    let c = CORNERS[vi];
    let t = c.x * 0.5 + 0.5;
    let side = c.y;
    let pa = (positions[inst.src] - cam.view.xy) * cam.view.z;
    let pb = (positions[inst.dst] - cam.view.xy) * cam.view.z;
    let dir = normalize(pb - pa + vec2<f32>(1e-6, 0.0));
    let nrm = vec2<f32>(-dir.y, dir.x);
    let half_w = max(inst.width * 0.5, 0.5);
    let px = mix(pa, pb, t) + nrm * side * half_w;
    var o: LinkOut;
    // Long links cost the most to draw (fill rate) and add the least to the
    // picture. viewport.z/w hold the fade range in pixels (0/0 = off): links
    // fade out between z and w, and fully faded ones are moved outside the clip
    // volume so they are never rasterized.
    var visibility = 1.0;
    if (cam.viewport.w > 0.0) {
        visibility = 1.0 - smoothstep(cam.viewport.z, cam.viewport.w, length(pb - pa));
    }
    if (visibility <= 0.0) {
        o.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        o.color = vec4<f32>(0.0);
        return o;
    }
    o.clip = vec4<f32>(px / (cam.viewport.xy * 0.5), 0.0, 1.0);
    var shade = 1.0;
    if (dim.v.x > 0.5 && link_flags[ii] == 0u) {
        shade = dim.v.z;
    }
    o.color = vec4<f32>(inst.color.rgb, inst.color.a * visibility * shade);
    return o;
}

@fragment
fn fs_link(i: LinkOut) -> @location(0) vec4<f32> {
    return i.color;
}
