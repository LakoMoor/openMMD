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

SDEF retains its center and radius vectors and uses spherical interpolation of
the two bone rotations with weighted transformed anchors. QDEF uses normalized dual-quaternion blending for up to four bone transforms.
Additional UV channels are read and discarded.

Material names and comments are not retained. Double-sided parameters control back-face culling in the shared renderer. Sphere mode 3 requires additional
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
comparisons remain outstanding. Synthetic SDEF tests independently verify rest pose,
a numeric two-bone bend, morph ordering, endpoint weights and common rigid transforms.

## SDEF implementation boundary

The shared CPU skinning path applies vertex morphs first, then interpolates the
two global bone rotations. Corrected radius anchors account for each bone's
inverse bind translation. Normals use the interpolated rotation rather than a
linear normal blend. PMD and BDEF paths are unchanged.

The formula was cross-checked against
[saba's independent implementation](https://github.com/benikabocha/saba/blob/master/src/Saba/Model/MMD/PMXModel.cpp),
not recovered from the original Windows MMD executable. The synthetic fixtures
establish mathematical behavior; real-model visual parity remains unverified.
Bone scale/shear, append transforms and full material/bone morph evaluation
are outside this change.

## QDEF implementation boundary

PMX 2.1 mode 4 is retained as QDEF rather than reduced to BDEF4. The shared CPU
path converts inverse-bind-adjusted rigid bone transforms into dual quaternions,
aligns both quaternion components to the hemisphere of the first positive-weight
influence, blends and normalizes them, then transforms morphed positions and
rotates normals. Missing zero-weight influences are ignored. A degenerate real
blend below length 1e-8 falls back to the existing linear path rather than
producing a non-finite pose.

This is an independent implementation of rigid dual-quaternion blending, with
formula references from [Kavan et al.](https://users.cs.utah.edu/~ladislav/dq/index.html)
and a behavior cross-check against [saba](https://github.com/benikabocha/saba/blob/master/src/Saba/Model/MMD/PMXModel.cpp).
It does not support bone scale/shear or claim bit-identical behavior with Windows
MMD. Hemisphere choices at exactly 180 degrees and degenerate blends still need
original-runtime comparison.

Tests cover four influences, rest/end weights, bind translations, common rotations,
rotation/translation coupling, quaternion sign alignment, a zero-weight leading
influence, normals and morph ordering. The original MIT-licensed
[PMX/VMD cylinder example](../examples/qdef/) preserves radius 3 at 120 degrees
of twist; the same fixture evaluated with linear blending shrinks its middle
ring to radius 1.5. Real-character Windows MMD frame comparisons remain outstanding.
