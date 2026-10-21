# VPD pose interchange

`src/vpd.rs` imports and exports the original MMD VPD bone pose format.
The API is `parse(&[u8]) -> Result<PoseFile>` and
`to_bytes(&PoseFile) -> Result<Vec<u8>>`. `PoseFile` contains the parent model
reference (usually `miku.osm`) and an ordered list of bone names, translations
and `x,y,z,w` quaternions. Coordinate values are kept in MMD coordinates;
the application applies its own display coordinate conversion afterward.

The reader checks the `Vocaloid Pose Data file` signature, semicolon-terminated
model/count/vector records, `BoneN{name` blocks and closing braces. Blank lines,
CRLF/LF line endings and `//` comments are accepted. Bone indices must be unique
but need not be contiguous. Bone names must also be unique, since applying two
conflicting poses by name is ambiguous. The declared bone count must match
exactly. Additional `Morph` blocks used by MikuMikuMoving are rejected.

Text uses the `encoding_rs::SHIFT_JIS` implementation of the Windows-compatible
Japanese encoding. Invalid byte sequences and characters that cannot be
represented on export return an error. Export produces CRLF lines, sequential
bone indices and decimal values preserving `f32` precision. It does not preserve
input comments, spacing, bone indices or the exact quaternion magnitude.
Quaternions are normalized; the zero quaternion becomes identity, matching the
primary reference importer. Nonfinite vectors/quaternions and extremely small
nonzero quaternions are rejected.

Files are limited to 8 MiB, at most 65,536 bones and at most 4,096 UTF-8 bytes per
name. Names cannot contain control characters, braces, semicolons, comment
markers or surrounding whitespace, because those characters would be ambiguous
in the format. Parent model references are metadata; importing a VPD does not
load or execute a referenced file. A pose has no timeline, camera, lighting,
IK-switch state, physics or project settings.

The primary layout reference is the
[MMD Tools maintainers' VPD importer/exporter](https://github.com/MMD-Blender/blender_mmd_tools/blob/main/mmd_tools/core/vpd/__init__.py).
The Rust implementation is independently written. The four pose files supplied
in the original MMD 9.32 archive are used as optional reference fixtures.

Run synthetic validation using `cargo test vpd::tests`. After fetching the
original archive using `python3 scripts/fetch_reference.py`, run
`cargo test vpd::tests::original_ -- --ignored`. The four asset tests check
import, 14 bone records per pose, Japanese name preservation and transform
round trips. Synthetic tests cover Japanese text, comments, arbitrary indices,
zero quaternions, invalid syntax/encoding, export injection and every required
prefix truncation. Original MMD runtime pose comparison remains outstanding.
