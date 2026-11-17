// The 3D view: lit meshes with clip planes and hatched cut faces, and a ground grid.

struct U {
    view_proj: mat4x4<f32>,
    // xyz eye; w = 1 for orthographic.
    eye: vec4<f32>,
    // xyz viewing direction; w = pixels per point.
    view_dir: vec4<f32>,
    // xyz direction towards the key light.
    light: vec4<f32>,
    // xyz normal (towards the hidden side), w = offset: hidden if dot(n, p) > w.
    planes: array<vec4<f32>, 6>,
    colors: array<vec4<f32>, 8>,
    selected: vec4<f32>,
    hatch: vec4<f32>,
    // minor step, major step, fade radius, z.
    grid: vec4<f32>,
    grid_minor: vec4<f32>,
    grid_major: vec4<f32>,
    // xy centre of the grid.
    grid_center: vec4<f32>,
    // x plane count, y hatch on, z grid on.
    counts: vec4<u32>,
};

@group(0) @binding(0) var<uniform> u: U;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) material: u32,
    @location(3) flags: u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) @interpolate(flat) material: u32,
    @location(3) @interpolate(flat) flags: u32,
};

@vertex
fn vs_mesh(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.world = v.pos;
    o.normal = v.normal;
    o.material = v.material;
    o.flags = v.flags;
    return o;
}

fn clipped(p: vec3<f32>) -> bool {
    for (var i = 0u; i < u.counts.x; i = i + 1u) {
        let pl = u.planes[i];
        if (dot(pl.xyz, p) - pl.w > 0.5) {
            return true;
        }
    }
    return false;
}

fn to_eye(p: vec3<f32>) -> vec3<f32> {
    if (u.eye.w > 0.5) {
        return -u.view_dir.xyz;
    }
    return normalize(u.eye.xyz - p);
}

@fragment
fn fs_mesh(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (clipped(in.world)) {
        discard;
    }
    let selected = (in.flags & 1u) != 0u;
    if (in.material == 7u) {
        // Cut face: flat colour with screen-space hatching.
        var c = u.colors[7].rgb;
        if (selected) {
            c = mix(c, u.selected.rgb, 0.45);
        }
        if (u.counts.y != 0u) {
            let s = (in.clip.x + in.clip.y) / (7.0 * u.view_dir.w);
            if (fract(s) < 0.16) {
                c = u.hatch.rgb;
            }
        }
        return vec4<f32>(c, 1.0);
    }
    let e = to_eye(in.world);
    var n = normalize(in.normal);
    // Two-sided: inside faces show through a cut, a little darker.
    var inside = 1.0;
    if (!front) {
        n = -n;
        inside = 0.55;
    }
    let base = u.colors[in.material].rgb;
    let key = max(dot(n, u.light.xyz), 0.0);
    let head = max(dot(n, e), 0.0);
    let sky = 0.5 + 0.5 * n.z;
    var c = base * (0.24 + 0.14 * sky + 0.46 * key + 0.22 * head) * inside;
    // A touch of specular on the lens.
    if (in.material == 5u) {
        let h = normalize(e + u.light.xyz);
        c = c + vec3<f32>(pow(max(dot(n, h), 0.0), 40.0) * 0.5);
    }
    if (selected) {
        c = mix(c, u.selected.rgb, 0.55);
    }
    return vec4<f32>(c, 1.0);
}

// Ground grid: one big quad on the layer-0 plane, lines from the fragment position.

struct GridOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs_grid(@builtin(vertex_index) i: u32) -> GridOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[i];
    let p = vec3<f32>(u.grid_center.xy + c * u.grid.z, u.grid.w);
    var o: GridOut;
    o.clip = u.view_proj * vec4<f32>(p, 1.0);
    o.world = p;
    return o;
}

fn grid_line(coord: vec2<f32>, step: f32) -> f32 {
    let q = coord / step;
    let w = max(fwidth(q), vec2<f32>(1e-6));
    let g = abs(fract(q - 0.5) - 0.5) / w;
    return 1.0 - min(min(g.x, g.y), 1.0);
}

@fragment
fn fs_grid(in: GridOut) -> @location(0) vec4<f32> {
    if (u.counts.z == 0u || clipped(in.world)) {
        discard;
    }
    let minor = grid_line(in.world.xy, u.grid.x);
    let major = grid_line(in.world.xy, u.grid.y);
    let r = length(in.world.xy - u.grid_center.xy) / u.grid.z;
    let fade = clamp(1.0 - r, 0.0, 1.0);
    let a_minor = minor * u.grid_minor.a * fade;
    let a_major = major * u.grid_major.a * fade;
    if (a_major >= a_minor) {
        return vec4<f32>(u.grid_major.rgb, a_major);
    }
    return vec4<f32>(u.grid_minor.rgb, a_minor);
}

