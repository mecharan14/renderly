struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct BlurParams {
    /// Sample step in UV space (horizontal: (1/w, 0), vertical: (0, 1/h)).
    texel: vec2<f32>,
    radius: f32,
    _pad: f32,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> params: BlurParams;

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
    let radius = params.radius;
    if (radius < 0.5) {
        return textureSample(src_tex, src_sampler, input.uv);
    }

    let sigma = max(radius * 0.5, 0.01);
    let taps = i32(clamp(ceil(radius), 1.0, 16.0));
    var sum = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var i = -taps; i <= taps; i = i + 1) {
        let fi = f32(i);
        let w = exp(-0.5 * fi * fi / (sigma * sigma));
        let uv = input.uv + params.texel * fi;
        sum = sum + textureSample(src_tex, src_sampler, uv) * w;
        wsum = wsum + w;
    }
    return sum / wsum;
}
