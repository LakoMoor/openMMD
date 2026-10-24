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

No self-shadow/ground-shadow map, material morphs or MME effects are rendered yet.
Double-sided parameters are retained;
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

## Outline pass

PMD per-vertex edge-disable flags and PMX per-vertex edge multipliers are now
retained. The separate inverted-hull pass uses skinned normals, front-face
culling, the scene depth buffer and four-sample MSAA. Material edge color,
alpha, enable flag and size control the pass; texture and material alpha also
suppress transparent outlines. Zero vertex multipliers suppress the outline.

Width currently means viewport pixels: material size 1 with vertex multiplier 1
expands one pixel along the projected normal. This is an explicit approximation,
not the recovered MMD 9.32 edge-width formula. Perspective/orthographic projection,
viewport scaling and the original edge render state still need comparison with
Windows MMD. Ordinary mesh culling/normal handling remains unchanged in this pass.

The parser field layout was cross-checked against the independent
[mmd-parser implementation](https://github.com/takahirox/mmd-parser/blob/master/src/Parser.ts).
GPU regression probes verify colored silhouette coverage, zero vertex scale and
material disabling, in addition to the existing material and hidden-model checks.

### Per-model thickness control

Desktop and web inspectors expose an independent model multiplier, initially 1,
with a slider from 0 to 2 in steps of 0.01. It multiplies material width and
vertex scale; zero removes the outline. The setting is loaded from and saved to PMM v2. PMM v1 has no matching field;
export rejects a changed multiplier instead of silently dropping it. Standalone
model files still start at 1.

Static Ghidra evidence from the reference binary establishes that its English
edge-thickness dialog (resource 662) uses controls 646/647. Callback
`0x140056e50` sets a trackbar range 0..200 and stores its position divided by 100
in the selected model at offset `0x355c`. Evidence:
`ev_83ae7eb8c8245ce130647a079e330c52bf2d582d78827156ed7ec90a464524bd`.
The edit-field callback `0x140057190` commits parsed text on Enter without an
explicit clamp (`ev_c2d0ee0fb3d919564235aa54038a46343c53e788607a1bde18965f86f585fa6d`).
openMMD currently provides the bounded slider; the original text-entry behavior
and native rendering formula remain unimplemented. These are static findings,
not runtime observations of Windows MMD.

### PMM v2 storage

The model tail stores a blend byte, the float32 outline multiplier, a self-shadow
byte and a transform-order byte. This layout was checked against nanoem's
[independent parser and writer](https://github.com/hkrn/nanoem/blob/main/nanoem/ext/document.c).
Only the multiplier's four bytes are patched. Model and scene offsets remain
valid when animation sections grow or shrink. Finite imported values outside
the UI slider range are preserved; non-finite values are rejected. Regression
tests cover byte preservation, invalid inputs and independent multipliers after
rebuilding animation for two models. Original bundled PMM projects are also
parsed and reopened. Windows MMD acceptance of edited copies remains unverified.
