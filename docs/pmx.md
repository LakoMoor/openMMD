# PMX import scope

`src/pmx.rs` implements an independent bounded reader and converts the imported
model into the shared PMD/PMX runtime representation. Loading a PMX file does
not establish visual or behavioral equivalence with MikuMikuDance.

## Implemented

- PMX 2.0 and 2.1 headers, strict UTF-8 and little-endian UTF-16 decoding.
- All six independently sized index categories (1, 2 or 4 bytes), unsigned
  vertex indices and signed nullable object references.
- Positions, normals, base UVs, triangle indices and material draw ranges.
- Diffuse RGBA, ambient/specular colors, shininess, diffuse/sphere/toon texture references.
- Drawing and edge parameters are retained. See [rendering](rendering.md) for runtime limits.
- Four-influence BDEF1/BDEF2/BDEF4 linear skinning. Weights must be finite,
  nonnegative, and sum to one within 0.01; accepted weights are normalized.
- Rest positions and parent relationships, including parents declared later
  in the file. Cycles are rejected with an iterative linear-time traversal.
- IK controller, target, iteration count, angle and link indices. Per-link
  PMX angle limits are read but are not stored in the common runtime.
- Vertex displacement morphs. A synthetic base morph maps the direct PMX
  vertex indices to the common PMD morph representation.

## Approximations and missing evaluation

SDEF parameters and QDEF weights are read; both are currently evaluated using
ordinary linear blend skinning. SDEF and QDEF deformation will differ from MMD.
Additional UV channels are read and discarded.

Material names and comments are not retained. Double-sided parameters
are retained but do not yet control rasterization. Sphere mode 3 requires additional
UVs and is retained with a runtime warning rather than rendered.
Bone flags, transform ordering, append rotation/translation, fixed/local axes,
external parent transforms and post-physics transforms are read but not retained
or evaluated. This can change animation substantially even for a valid mesh.

Group, flip, bone, UV, material and impulse morph records are consumed and their
names remain visible, but their runtime displacement lists are empty. Changing
their weights therefore has no effect. Group morphs do not drive their children.
Only vertex morphs are evaluated.

Display panels, rigid bodies and joints are structurally consumed without
retaining or simulating them. Nonempty PMX 2.1 soft-body sections are rejected
explicitly. This importer has no PMX exporter and cannot provide a lossless
PMX round trip. Any workflow that needs the original data must retain the
original file separately. PMM compatibility is a separate implementation.

## Input limits and validation

Each length/count must fit the remaining input, is nonnegative and is capped
at 5,000,000. The shared bone identifiers are `u16`, limiting skeletons to
65,536 bones. Every read checks its bounds and all parsed floats must be finite.
Index widths, skinning modes, morph kinds/version combinations, parent/skin/IK
references, material face coverage and trailing bytes are checked. Unsupported
soft bodies return an error rather than silently succeeding.

These limits protect the parser from malformed lengths; they are not a promise
that the application can interactively render a model at the maximum sizes.

## Evidence and tests

The primary implementation reference for PMX 2.0 record layout is the
[MMD Tools maintainers' PMX reader/writer](https://github.com/MMD-Blender/blender_mmd_tools/blob/main/mmd_tools/core/pmx/__init__.py),
particularly `FileReadStream`, `BoneWeight.load`, `Material.load`, `Bone.load`
and morph record classes. The Rust code is independently written. The linked
reader covers PMX 2.0; its source does not establish PMX 2.1 compatibility.
The limited PMX 2.1 QDEF/flip/impulse handling is not yet verified against a
real PMX 2.1 model or original MMD runtime.

Synthetic tests cover both encodings, all three index widths, all five skinning
modes, root sentinels, direct vertex morph indices, every truncated prefix,
invalid counts/references, parent cycles and nonfinite vertices. A runtime test
checks rest pose, vertex morph displacement and blending across four bones. Run
`cargo test pmx::tests`. Real PMX asset comparison and SDEF/QDEF reference pose
comparisons remain outstanding; these tests establish synthetic parsing and linear skinning behavior only.
