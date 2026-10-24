# Synthetic SDEF bend

A small original test asset created for openMMD, distributed under the repository's MIT license. No third-party models or textures are included.

Open `strip.pmx` and `bend.vmd` together, then play or seek to frame 60. The strip has 18 SDEF vertices, 20 triangles and two bones. The elbow rotates 60 degrees around Z between frames 0 and 60. It is a deformation smoke test, not a production character or an original-MMD reference capture.

The SDEF center is `(0, 10, 0)`, both radius vectors are zero, and weights vary from one bone to the other along the strip. Shared toon texture `toon01.bmp` is optional; the renderer uses its fallback when absent.

The independent numeric tests in `tests/sdef.rs` use nonzero radius vectors and cover rest pose, a known bend, normals, end weights, common rigid transforms and vertex morph ordering. See [PMX support](../../docs/pmx.md) for remaining compatibility limits.
