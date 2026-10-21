struct Camera { view_projection: mat4x4<f32>, light_direction: vec4<f32>, light_color: vec4<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var diffuse_texture: texture_2d<f32>;
@group(1) @binding(1) var diffuse_sampler: sampler;
struct VertexOutput {
 @builtin(position) position: vec4<f32>,
 @location(0) normal: vec3<f32>,
 @location(1) uv: vec2<f32>,
 @location(2) color: vec4<f32>,
}
@vertex fn vs_main(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) uv: vec2<f32>, @location(3) color: vec4<f32>) -> VertexOutput {
 var out: VertexOutput;
 out.position = camera.view_projection * vec4<f32>(p, 1.0);
 out.normal = n; out.uv = uv; out.color = color; return out;
}
@fragment fn fs_main(v: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
 let texel = textureSample(diffuse_texture, diffuse_sampler, v.uv);
 let alpha = v.color.a * texel.a;
 if alpha < 0.01 { discard; }
 let n = normalize(v.normal) * select(-1.0, 1.0, front);
 let light = dot(n, normalize(camera.light_direction.xyz));
 let shade = select(0.64, 1.0, light > -0.05);
 return vec4<f32>(texel.rgb * pow(max(v.color.rgb, vec3<f32>(0.0)), vec3<f32>(2.2)) * camera.light_color.rgb * shade, alpha);
}
