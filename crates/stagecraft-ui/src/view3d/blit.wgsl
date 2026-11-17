// Copies the resolved offscreen 3D image into egui's render pass.


@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

struct BlitOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_blit(@builtin(vertex_index) i: u32) -> BlitOut {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var o: BlitOut;
    o.clip = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

// egui's usual framebuffer is not sRGB: write gamma-encoded values.
@fragment
fn fs_blit_gamma(in: BlitOut) -> @location(0) vec4<f32> {
    let c = textureSample(image, image_sampler, in.uv);
    return vec4<f32>(to_srgb(c.rgb), 1.0);
}

@fragment
fn fs_blit_linear(in: BlitOut) -> @location(0) vec4<f32> {
    let c = textureSample(image, image_sampler, in.uv);
    return vec4<f32>(c.rgb, 1.0);
}
