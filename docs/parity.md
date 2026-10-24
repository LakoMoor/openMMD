# MMD 9.32 parity work

The target is the behavior and editor workflow of the supplied MMD 9.32 English
x64 release, implemented in Rust and wgpu. The current application is **not** a
1:1 port. Website localization is separate from this compatibility work.

## Reference identity and evidence

- Archive: `MikuMikuDanceE_v932x64.zip` from LearnMMD.
- Executable SHA-256: `07516fd3bf1e6b1339836b6773a156f61bdd6f848eeb621fdda012375df313a1`.
- Native static analysis: REA with Ghidra 12.1.4, original bytes unchanged.
- Overview: 1,442 analyzed procedures and 1,102 analyzed strings;
  `ev_a2d46df3f3a9c8749804c54fe4f64ae4a6aac2320157a8022810c9022d07bf64`.
- Camera key application at `0x140059c30`:
  `ev_09c30323e378a03dcaf5e1e2b81c87cda5ea10ae4a3d809d9a98bcfce153c088`.
- Playback preparation at `0x140069210`:
  `ev_a81b690db9f168f067e1a863d6bf49a3726ba4301264485d759409091ec0782a`.
- Render function containing the call at `0x140027fe6`:
  `ev_2ac3aeec62068bb5e2862755943208cbb483dfeed8039ac1f4763d7c0a95a106`.

The complete local evidence export is `reference/mmd-evidence.json`; it is
ignored by Git, as are the original executable and resources. Ghidra pseudocode
is analysis output, not the original source. Calling conventions that omit
floating-point arguments in pseudocode were checked against the x64 instructions.
Static evidence does not establish observed runtime behavior.

## Unattached camera: first corrected differences

The original render path composes row-vector world rotations **Y, then X, then
Z** at `0x1400279aa`–`0x140027a1f`. The inverse camera orientation in our
column-vector representation is Y(-y) X(-x) Z(-z). The old implementation used
X(-x) Y(-y) Z(-z), which differs when pitch and yaw are both nonzero.

The original `D3DXMatrixPerspectiveFovLH` calls use a near plane of **1** and a
far plane of **100000**. The old camera used 0.1 and 10000. The new camera uses
the equivalent left-handed, 0-to-1 depth projection for wgpu.

At `0x140027ed2`–`0x140027f2f`, the non-perspective projection is written directly:

| Matrix coefficient | Original expression |
| --- | --- |
| m11 | -2 / signed camera distance |
| m22 | m11 × viewport width / viewport height |
| m33 | float bits `0x3aaa64c3` (approximately 0.0013) |
| m44 | 1 |
| Other coefficients | 0 |

This replaces our arbitrary box size derived from a nominal height of 25.
The original view looks along positive Z even for positive camera distance
(eye Z = distance, target Z = distance + 80 at `0x140027f75`–`0x140027fe6`).
The previous look-at-target implementation flipped the forward axis for positive
distance. The corrected camera retains the original forward direction.

A zero or near-zero distance produces a singular original orthographic matrix.
openMMD deliberately clamps the denominator to a magnitude of 0.01 to keep the
GPU input finite. This is an explicit compatibility exception, not a claim of
matching that degenerate case. Parent-attached cameras remain approximate.

Tests use independent scalar calculations for combined nonzero rotations and
projected coordinates, signed distance, clipping planes, FOV-independent
orthographic projection, and finite degenerate behavior. They do **not** compare
against screenshots or matrices captured from a running Windows MMD.

## Camera interpolation

`0x14005a8b0` uses a linear-curve shortcut (both control points lie on the
diagonal) and a binary search with **12** steps, starting at 0.5. The search
terminates early on an exact float comparison. The previous shared evaluator
used 20 steps and evaluated even diagonal curves. The scene camera now follows
the 9.32 operation order and search precision. Bone interpolation is unchanged
pending its own reference analysis.

Evidence: `ev_5dea2cf5a789af812ea7185a243c90b6e9b593529680712d45b11b66e00746fe`.
The decompiler inferred a void return, so the actual computed float in XMM3 and
subsequent caller use were checked in the x64 instructions. Exact keyframes
are assigned directly in `0x140059c30`, bypassing curve evaluation; our sampler
now does the same. Golden tests use separately evaluated float32 scalar
operations. They establish the implemented arithmetic, not end-to-end runtime
parity with Windows MMD.

## Original interface inventory

Run:

```sh
python3 scripts/compat/inspect_pe_ui.py \
  reference/original/MikuMikuDance.exe --output reference/mmd-ui.json
python3 -m unittest discover -s scripts/compat -p 'test_*.py'
```

The supplied executable contains 76 resource leaves, including **48 dialog
templates with 822 controls**. The report retains resource IDs, control IDs,
classes, text, font declarations, styles and rectangles in **dialog units**.
Both standard and extended Win32 dialog layouts are decoded. These templates
are not runtime screenshots: controls can be created, moved, translated or
hidden by code. In particular this inventory is not a reconstruction of the
main editor window.

## Remaining acceptance work

| Area | Required evidence before claiming parity |
| --- | --- |
| Interface | Original main-window layout, command routing, dialogs, selection, gizmos, timeline ranges and keyboard behavior |
| Camera/light | Windows runtime comparison including attachments, interpolation and clipping |
| Rendering | Toon, sphere maps, edges, ground/self shadows and material comparison |
| Animation | Exact IK, all PMX skinning/morph/bone modes and model-property tracks |
| Physics | Matching rigid bodies, constraints, stepping and physics keys |
| Scene formats | Complete PMM editing/creation, accessories, audio, background and settings |
| Output | Frame/image/video export and original file reopen validation |
| Platforms | Real application/GPU tests on Windows and Linux, beyond CI builds |

The current macOS host has no Wine or Windows VM runtime available. Runtime
comparison must be performed on a suitable Windows environment; static analysis
and format round trips cannot substitute for it. Work can continue on verified
static behavior and the remaining implementation without claiming final parity.
