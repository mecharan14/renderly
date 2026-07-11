struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct PackLutParams {
    intensity: f32,
    lut_size: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> params: PackLutParams;
@group(0) @binding(3) var lut_tex: texture_3d<f32>;
@group(0) @binding(4) var lut_sampler: sampler;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, 0.0),
    );
    var out: VertexOutput;
    out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.uv = uvs[vertex_index];
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(src_tex, src_sampler, input.uv);
    let n = max(params.lut_size, 2.0);
    // Texel-centered UVW so linear filtering matches CPU trilinear at cube corners.
    let scale = (n - 1.0) / n;
    let offset = 0.5 / n;
    let uvw = clamp(c.rgb, vec3(0.0), vec3(1.0)) * scale + offset;
    let graded = textureSample(lut_tex, lut_sampler, uvw).rgb;
    let rgb = mix(c.rgb, graded, clamp(params.intensity, 0.0, 1.0));
    return vec4<f32>(rgb, c.a);
}
