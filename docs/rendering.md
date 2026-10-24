# Rendering toward MMD 9.32

The reference is the original executable SHA-256 recorded in `parity.md`.
Its RCDATA resources 117 and 118 contain text HLSL effects. They were read
locally to identify the native lighting, specular and sphere-map operations.
The original HLSL and extracted resources are not redistributed in this repo.
The independent WGSL implementation is not a complete translation of those effects.

## Implemented in this material pass

- PMD shininess, specular RGB, ambient RGB, toon index/custom toon names,
  `.sph` multiply and `.spa` add references are retained.
- The optional PMD display/English-name tables are bounded before reading its
  ten custom toon paths. Models without the optional extension still load.
- PMX specular/shininess, sphere mode/texture, custom or shared toon selection,
  edge parameters and double-sided flags are retained.
- Diffuse, sphere and toon textures have independent GPU bindings. Sphere UVs
  come from the view-rotated skinned normal and therefore follow the camera.
- Toon textures affect lighting instead of the previous hard-coded two-band
  shade. Ambient, specular color and shininess affect the surface.
- Shared toon maps can be found beside the model or in neighboring `Data`
  directories. Missing textures generate warnings and fall back safely.
- Four-sample MSAA resolves into the displayed viewport texture. The depth target
  and mesh pipeline use the same sample count.

Textures are sampled as raw color values, surface color is composed in legacy
color space, then converted to linear for the sRGB render target. This replaces
the old approximate `pow(material, 2.2)` multiplication. Original render-state,
color conversion, light preparation and all shader combinations still need
Windows runtime comparison. The current toon-lighting mapping is an approximation,
not a claim that every native MMD shader branch has been reproduced.

## Remaining differences

No self-shadow/ground-shadow map, inverted-hull outline, material morphs or
MME effects are rendered yet. Edge and double-sided parameters are retained;
all surfaces currently use the existing two-sided pipeline. PMX sphere mode 3
requires extra UV channels: it is explicitly warned about and not rendered.
Transparent-material sorting, precise alpha/depth behavior and all material
combinations still require implementation and comparison.

The supplied MONSTER video also uses MME effects and AviUtl. AO, depth of field,
glow, reflections, lens effects and final grading require their own renderer
passes and matching effect parameters. Loading the same model and motion is
not sufficient to reproduce that video. See `examples/catalog.json` for sources.

## Validation

Independent PMD/PMX fixtures assert imported material values and malformed
optional extensions. Original bundled models still import and skin correctly.
The GPU test renders constant material probes and verifies expected pixel colors
for disabled/multiply/add sphere modes and a toon texture. It also checks hidden
models and material offsets in a two-model scene. The Miku/WAVEFILE set was loaded
with its camera and motion and rendered on macOS; no original Windows MMD frame
comparison has been performed.
