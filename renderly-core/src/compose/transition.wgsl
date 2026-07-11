// Dual-input transition pass. Samples outgoing (A) and incoming (B) full-frame textures.
// Uniform: progress 0..1, kind id (see TransitionKind::shader_id).

struct TransitionParams {
    progress: f32,
    kind: u32,
    _pad0: f32,
    _pad1: f32,
}

@group(0) @binding(0) var tex_a: texture_2d<f32>;
@group(0) @binding(1) var tex_b: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<uniform> params: TransitionParams;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    var positions = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0),
        vec2(-1.0, 1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2(0.0, 1.0), vec2(1.0, 1.0), vec2(0.0, 0.0),
        vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(1.0, 0.0),
    );
    var out: VsOut;
    out.pos = vec4(positions[idx], 0.0, 1.0);
    out.uv = uvs[idx];
    return out;
}

fn sample_or_black(tex: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }
    return textureSample(tex, samp, uv);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let u = clamp(params.progress, 0.0, 1.0);
    let uv = in.uv;
    let a = textureSample(tex_a, samp, uv);
    let b = textureSample(tex_b, samp, uv);

    // 0 crossfade
    if (params.kind == 0u) {
        return mix(a, b, u);
    }
    // 1 fade_black
    if (params.kind == 1u) {
        if (u < 0.5) {
            return mix(a, vec4(0.0, 0.0, 0.0, 1.0), u * 2.0);
        }
        return mix(vec4(0.0, 0.0, 0.0, 1.0), b, (u - 0.5) * 2.0);
    }
    // 2 wipe_left (B reveals from left)
    if (params.kind == 2u) {
        return select(a, b, uv.x < u);
    }
    // 3 wipe_right
    if (params.kind == 3u) {
        return select(a, b, uv.x > (1.0 - u));
    }
    // 4 wipe_up
    if (params.kind == 4u) {
        return select(a, b, uv.y < u);
    }
    // 5 wipe_down
    if (params.kind == 5u) {
        return select(a, b, uv.y > (1.0 - u));
    }
    // 6 slide_left
    if (params.kind == 6u) {
        let a_uv = uv + vec2(u, 0.0);
        let b_uv = uv + vec2(u - 1.0, 0.0);
        if (uv.x < (1.0 - u)) {
            return sample_or_black(tex_a, a_uv);
        }
        return sample_or_black(tex_b, b_uv);
    }
    // 7 slide_right
    if (params.kind == 7u) {
        let a_uv = uv - vec2(u, 0.0);
        let b_uv = uv - vec2(u - 1.0, 0.0);
        if (uv.x > u) {
            return sample_or_black(tex_a, a_uv);
        }
        return sample_or_black(tex_b, b_uv);
    }
    // 8 iris
    if (params.kind == 8u) {
        let d = distance(uv, vec2(0.5, 0.5));
        let radius = u * 0.75;
        return select(a, b, d < radius);
    }
    // 9 blur_dissolve (soft edge mix using luma noise-ish from uv)
    let noise = fract(sin(dot(uv, vec2(12.9898, 78.233))) * 43758.5453);
    let edge = smoothstep(u - 0.15, u + 0.15, noise);
    return mix(b, a, edge);
}
