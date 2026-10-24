struct Camera {
 view_projection: mat4x4<f32>, light_direction: vec4<f32>, light_color: vec4<f32>,
 view: mat4x4<f32>, eye: vec4<f32>, viewport: vec4<f32>
}
struct Material { ambient: vec4<f32>, specular: vec4<f32>, settings: vec4<f32>, edge_color: vec4<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var diffuse_texture: texture_2d<f32>;
@group(1) @binding(1) var diffuse_sampler: sampler;
@group(1) @binding(2) var sphere_texture: texture_2d<f32>;
@group(1) @binding(3) var toon_texture: texture_2d<f32>;
@group(1) @binding(4) var clamp_sampler: sampler;
@group(1) @binding(5) var<uniform> material: Material;
struct VertexOutput {
 @builtin(position) position: vec4<f32>,
 @location(0) normal: vec3<f32>, @location(1) uv: vec2<f32>,
 @location(2) color: vec4<f32>, @location(3) world_position: vec3<f32>,
 @location(4) sphere_uv: vec2<f32>
}
@vertex fn vs_main(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) uv: vec2<f32>, @location(3) color: vec4<f32>) -> VertexOutput {
 var out: VertexOutput;
 out.position = camera.view_projection * vec4<f32>(p, 1.0);
 out.normal = n; out.uv = uv; out.color = color; out.world_position = p;
 let view_normal = (camera.view * vec4<f32>(n, 0.0)).xyz;
 out.sphere_uv = view_normal.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
 return out;
}
fn safe_normalize(v: vec3<f32>) -> vec3<f32> { return v / max(length(v), 0.000001); }
fn decode_srgb(v: vec3<f32>) -> vec3<f32> {
 return select(pow((max(v, vec3<f32>(0.0)) + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), v / 12.92, v <= vec3<f32>(0.04045));
}
@fragment fn fs_main(v: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
 let texel = textureSample(diffuse_texture, diffuse_sampler, v.uv);
 let sphere = textureSample(sphere_texture, clamp_sampler, v.sphere_uv);
 let n = safe_normalize(v.normal) * select(-1.0, 1.0, front);
 let light = dot(n, safe_normalize(camera.light_direction.xyz));
 let toon = textureSample(toon_texture, clamp_sampler, vec2<f32>(0.5, 0.5 - light * 0.5));
 let alpha = v.color.a * texel.a;
 if alpha < 0.01 { discard; }
 var lighting: vec3<f32>;
 if material.settings.y > 0.5 {
   lighting = clamp(material.ambient.rgb + v.color.rgb * camera.light_color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * toon.rgb;
 } else {
   lighting = clamp(material.ambient.rgb + v.color.rgb * camera.light_color.rgb * max(light, 0.0), vec3<f32>(0.0), vec3<f32>(1.0));
 }
 var color = lighting * texel.rgb;
 if material.settings.x > 0.5 && material.settings.x < 1.5 { color *= sphere.rgb; }
 if material.settings.x > 1.5 && material.settings.x < 2.5 { color += sphere.rgb; }
 let half_vector = safe_normalize(safe_normalize(camera.light_direction.xyz) + safe_normalize(camera.eye.xyz - v.world_position));
 let specular = pow(max(dot(n, half_vector), 0.0), max(material.specular.w, 0.0001)) * material.specular.rgb * camera.light_color.rgb;
 color += clamp(specular, vec3<f32>(0.0), vec3<f32>(1.0));
 return vec4<f32>(decode_srgb(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0))), alpha);
}

struct OutlineOutput {
 @builtin(position) position: vec4<f32>,
 @location(0) uv: vec2<f32>,
 @location(1) alpha: f32,
 @location(2) width: f32,
}
@vertex fn vs_outline(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) uv: vec2<f32>, @location(3) color: vec4<f32>, @location(4) edge_scale: f32) -> OutlineOutput {
 var out: OutlineOutput;
 var clip = camera.view_projection * vec4<f32>(p, 1.0);
 let normal_clip = camera.view_projection * vec4<f32>(n, 0.0);
 // Project the normal derivative before measuring its direction in viewport pixels.
 let direction = (normal_clip.xy * clip.w - clip.xy * normal_clip.w) * camera.viewport.xy;
 let pixel_direction = direction / max(length(direction), 0.000001);
 let width = max(edge_scale, 0.0) * material.settings.z;
 clip = vec4<f32>(clip.xy + pixel_direction * (2.0 / camera.viewport.xy) * width * clip.w, clip.zw);
 out.position = clip; out.uv = uv; out.alpha = color.a; out.width = width;
 return out;
}
@fragment fn fs_outline(v: OutlineOutput) -> @location(0) vec4<f32> {
 let alpha = material.edge_color.a * v.alpha * textureSample(diffuse_texture, diffuse_sampler, v.uv).a;
 if alpha < 0.01 || v.width <= 0.0 { discard; }
 return vec4<f32>(decode_srgb(clamp(material.edge_color.rgb, vec3<f32>(0.0), vec3<f32>(1.0))), alpha);
}
