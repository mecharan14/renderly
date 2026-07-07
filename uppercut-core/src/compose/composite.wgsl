struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// "Cover" crop: scale/offset remap the visible-viewport UV range (0..1) to the
// sub-rectangle of the source texture that fills the output without distorting aspect
// ratio, cropping any overflow. Identity (scale=1, offset=0) when the layer already
// matches the output's aspect ratio (e.g. burned-in caption layers).
struct LayerTransform {
    scale: vec2<f32>,
    offset: vec2<f32>,
};

@group(0) @binding(2) var<uniform> layer_transform: LayerTransform;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    var out: VertexOutput;
    out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.uv = uvs[vertex_index] * layer_transform.scale + layer_transform.offset;
    return out;
}

@group(0) @binding(0) var layer_tex: texture_2d<f32>;
@group(0) @binding(1) var layer_sampler: sampler;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(layer_tex, layer_sampler, input.uv);
}
