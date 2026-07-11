struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct LutParams {
    intensity: f32,
    /// 0 = contrast S-curve, 1 = warm color matrix.
    mode: u32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> params: LutParams;

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

/// Embedded contrast grade: per-channel S-curve (no external .cube).
fn lut_contrast(rgb: vec3<f32>) -> vec3<f32> {
    let x = clamp(rgb, vec3(0.0), vec3(1.0));
    // Smoothstep S-curve then mild midtone push.
    let s = x * x * (3.0 - 2.0 * x);
    return clamp((s - vec3(0.5)) * 1.25 + vec3(0.5), vec3(0.0), vec3(1.0));
}

/// Embedded warm grade: simple RGB matrix (lift red, pull blue).
fn lut_warm(rgb: vec3<f32>) -> vec3<f32> {
    let m = mat3x3<f32>(
        vec3<f32>(1.08, 0.04, 0.0),
        vec3<f32>(0.02, 1.0, 0.0),
        vec3<f32>(-0.06, -0.02, 0.90),
    );
    return clamp(m * rgb, vec3(0.0), vec3(1.0));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(src_tex, src_sampler, input.uv);
    let graded = select(lut_warm(c.rgb), lut_contrast(c.rgb), params.mode == 0u);
    let rgb = mix(c.rgb, graded, clamp(params.intensity, 0.0, 1.0));
    return vec4<f32>(rgb, c.a);
}
