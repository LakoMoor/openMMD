//! Bounded PMX 2.0/2.1 mesh import. SDEF/QDEF currently use linear skinning.
//! Non-vertex morphs, bone append/axis constraints and physics are consumed but
//! not evaluated by the shared animation model. Original file bytes must be
//! retained for any lossless project workflow.
use crate::format::{Bone, Ik, Material, Model, Morph, Vertex};
use anyhow::{bail, ensure, Context, Result};
use glam::{Vec2, Vec3};

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    utf8: bool,
    widths: [usize; 6],
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).context("PMX offset overflow")?;
        ensure!(end <= self.bytes.len(), "truncated PMX at {}", self.pos);
        let v = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(v)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn f32(&mut self) -> Result<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into()?);
        ensure!(v.is_finite(), "non-finite PMX float");
        Ok(v)
    }
    fn floats(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            self.f32()?;
        }
        Ok(())
    }
    fn v3(&mut self) -> Result<Vec3> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    fn count(&mut self, min: usize) -> Result<usize> {
        let n = self.i32()?;
        ensure!((0..=5_000_000).contains(&n), "invalid PMX count {n}");
        let n = n as usize;
        ensure!(
            n <= (self.bytes.len() - self.pos) / min.max(1),
            "PMX count exceeds input"
        );
        Ok(n)
    }
    fn text(&mut self) -> Result<String> {
        let n = self.count(1)?;
        let utf8 = self.utf8;
        let b = self.take(n)?;
        if utf8 {
            Ok(std::str::from_utf8(b)
                .context("invalid PMX UTF-8")?
                .to_owned())
        } else {
            ensure!(n % 2 == 0, "odd PMX UTF-16 length");
            let u: Vec<u16> = b
                .as_chunks::<2>()
                .0
                .iter()
                .map(|v| u16::from_le_bytes([v[0], v[1]]))
                .collect();
            Ok(String::from_utf16(&u).context("invalid PMX UTF-16")?)
        }
    }
    fn index(&mut self, kind: usize) -> Result<i32> {
        let n = self.widths[kind];
        let b = self.take(n)?;
        Ok(match n {
            1 => {
                if kind == 0 {
                    b[0] as i32
                } else {
                    b[0] as i8 as i32
                }
            }
            2 => {
                if kind == 0 {
                    u16::from_le_bytes(b.try_into()?) as i32
                } else {
                    i16::from_le_bytes(b.try_into()?) as i32
                }
            }
            4 => i32::from_le_bytes(b.try_into()?),
            _ => unreachable!(),
        })
    }
    fn required(&mut self, kind: usize, len: usize) -> Result<usize> {
        let i = self.index(kind)?;
        ensure!(i >= 0 && (i as usize) < len, "invalid PMX index {i}");
        Ok(i as usize)
    }
}

pub fn parse(bytes: &[u8]) -> Result<Model> {
    let mut r = Reader {
        bytes,
        pos: 0,
        utf8: false,
        widths: [1; 6],
    };
    ensure!(r.take(4)? == b"PMX ", "expected PMX signature");
    let version = r.f32()?;
    ensure!(
        (version - 2.0).abs() < 0.0001 || (version - 2.1).abs() < 0.0001,
        "unsupported PMX version {version}"
    );
    let header_len = r.u8()? as usize;
    ensure!(header_len >= 8, "short PMX globals");
    let h = r.take(header_len)?;
    ensure!(
        h[0] <= 1 && h[1] <= 4,
        "invalid PMX encoding/additional UV count"
    );
    r.utf8 = h[0] == 1;
    let uv_count = h[1] as usize;
    for (i, w) in h[2..8].iter().enumerate() {
        ensure!(matches!(w, 1 | 2 | 4), "invalid PMX index width");
        r.widths[i] = *w as usize;
    }
    let name = r.text()?;
    r.text()?;
    let comment = r.text()?;
    r.text()?;
    let n = r.count(38)?;
    let mut vertices = Vec::with_capacity(n);
    for _ in 0..n {
        let position = r.v3()?;
        let normal = r.v3()?;
        let uv = Vec2::new(r.f32()?, r.f32()?);
        r.floats(uv_count * 4)?;
        let mut raw = [-1; 4];
        let mut weights = [0.0; 4];
        let mode = r.u8()?;
        match mode {
            0 => {
                raw[0] = r.index(3)?;
                weights[0] = 1.0;
            }
            1 | 3 => {
                raw[0] = r.index(3)?;
                raw[1] = r.index(3)?;
                weights[0] = r.f32()?;
                weights[1] = 1.0 - weights[0];
                if mode == 3 {
                    r.floats(9)?;
                }
            }
            2 | 4 => {
                ensure!(mode != 4 || version > 2.05, "QDEF requires PMX 2.1");
                for b in &mut raw {
                    *b = r.index(3)?;
                }
                for w in &mut weights {
                    *w = r.f32()?;
                }
            }
            _ => bail!("unsupported PMX skinning mode {mode}"),
        }
        let mut bones = [0; 4];
        for i in 0..4 {
            ensure!(
                weights[i] >= 0.0 && weights[i] <= 1.0,
                "invalid PMX skin weight"
            );
            ensure!(
                raw[i] >= -1 && raw[i] <= u16::MAX as i32,
                "PMX bone index exceeds renderer capacity"
            );
            if raw[i] < 0 {
                ensure!(weights[i] == 0.0, "weighted missing PMX bone");
            } else {
                bones[i] = raw[i] as u16;
            }
        }
        let sum: f32 = weights.iter().sum();
        ensure!(
            (sum - 1.0).abs() < 0.01,
            "PMX skin weights do not sum to one"
        );
        for w in &mut weights {
            *w /= sum;
        }
        r.f32()?;
        vertices.push(Vertex {
            position,
            normal,
            uv,
            bones,
            weights,
        });
    }
    let n = r.count(r.widths[0])?;
    ensure!(n % 3 == 0, "PMX mesh is not triangular");
    let mut indices = Vec::with_capacity(n);
    for _ in 0..n {
        indices.push(r.required(0, vertices.len())? as u32);
    }
    let n = r.count(4)?;
    let mut textures = Vec::with_capacity(n);
    for _ in 0..n {
        textures.push(r.text()?);
    }
    let n = r.count(70)?;
    let mut materials = Vec::with_capacity(n);
    let mut start = 0u32;
    for _ in 0..n {
        r.text()?;
        r.text()?;
        let diffuse = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
        let specular = r.v3()?;
        let shininess = r.f32()?;
        let ambient = r.v3()?;
        let flags = r.u8()?;
        let edge_color = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
        let edge_size = r.f32()?;
        let ti = r.index(1)?;
        ensure!(
            ti >= -1 && (ti < 0 || (ti as usize) < textures.len()),
            "invalid PMX texture index"
        );
        let texture = if ti < 0 {
            String::new()
        } else {
            textures[ti as usize].clone()
        };
        let sphere = r.index(1)?;
        ensure!(
            sphere >= -1 && (sphere < 0 || (sphere as usize) < textures.len()),
            "invalid PMX sphere texture index"
        );
        let sphere_mode = r.u8()?;
        ensure!(sphere_mode <= 3, "invalid PMX sphere mode");
        let toon_texture = match r.u8()? {
            0 => {
                let toon = r.index(1)?;
                ensure!(
                    toon >= -1 && (toon < 0 || (toon as usize) < textures.len()),
                    "invalid PMX toon index"
                );
                if toon < 0 {
                    String::new()
                } else {
                    textures[toon as usize].clone()
                }
            }
            1 => {
                let toon = r.u8()?;
                ensure!(toon < 10, "invalid shared toon");
                format!("toon{:02}.bmp", toon + 1)
            }
            _ => bail!("invalid PMX toon mode"),
        };
        r.text()?;
        let count = r.i32()?;
        ensure!(count >= 0 && count % 3 == 0, "invalid PMX material faces");
        let count = count as u32;
        materials.push(Material {
            diffuse,
            ambient,
            specular,
            shininess,
            sphere_texture: if sphere < 0 {
                String::new()
            } else {
                textures[sphere as usize].clone()
            },
            sphere_mode,
            toon_texture,
            double_sided: flags & 1 != 0,
            edge_color,
            edge_size,
            edge_enabled: flags & 16 != 0,
            texture,
            start,
            count,
        });
        start = start
            .checked_add(count)
            .context("PMX material range overflow")?;
    }
    ensure!(
        start as usize == indices.len(),
        "PMX material ranges do not cover mesh"
    );
    let n = r.count(22)?;
    ensure!(n <= 65536, "PMX skeleton exceeds renderer capacity");
    let mut bones = Vec::with_capacity(n);
    let mut ik = Vec::new();
    for controller in 0..n {
        let name = r.text()?;
        r.text()?;
        let position = r.v3()?;
        let p = r.index(3)?;
        ensure!(p >= -1 && (p < 0 || (p as usize) < n), "invalid PMX parent");
        r.i32()?;
        let flags = r.u16()?;
        if flags & 1 != 0 {
            let tail = r.index(3)?;
            ensure!(
                tail >= -1 && (tail < 0 || (tail as usize) < n),
                "invalid PMX tail index"
            );
        } else {
            r.floats(3)?;
        }
        if flags & 0x300 != 0 {
            r.required(3, n)?;
            r.f32()?;
        }
        if flags & 0x400 != 0 {
            r.floats(3)?;
        }
        if flags & 0x800 != 0 {
            r.floats(6)?;
        }
        if flags & 0x2000 != 0 {
            r.i32()?;
        }
        if flags & 0x20 != 0 {
            let target = r.required(3, n)?;
            let iterations = r.i32()?;
            ensure!(
                (0..=65535).contains(&iterations),
                "invalid PMX IK iterations"
            );
            let angle = r.f32()?;
            ensure!(angle >= 0.0, "negative PMX IK angle");
            let k = r.count(r.widths[3] + 1)?;
            let mut links = Vec::with_capacity(k);
            for _ in 0..k {
                links.push(r.required(3, n)?);
                match r.u8()? {
                    0 => {}
                    1 => r.floats(6)?,
                    _ => bail!("invalid PMX IK limit flag"),
                }
            }
            ik.push(Ik {
                controller,
                target,
                iterations: iterations as u16,
                angle,
                links,
            });
        }
        bones.push(Bone {
            name,
            parent: if p < 0 { None } else { Some(p as usize) },
            position,
        });
    }
    let mut state = vec![0u8; n];
    for i in 0..n {
        if state[i] != 0 {
            continue;
        }
        let mut chain = Vec::new();
        let mut p = Some(i);
        while let Some(j) = p {
            ensure!(state[j] != 1, "cyclic PMX skeleton");
            if state[j] == 2 {
                break;
            }
            state[j] = 1;
            chain.push(j);
            p = bones[j].parent;
        }
        for j in chain {
            state[j] = 2;
        }
    }
    for v in &vertices {
        for i in 0..4 {
            ensure!(
                v.weights[i] == 0.0 || (v.bones[i] as usize) < n,
                "invalid PMX skin bone"
            );
        }
    }
    let n = r.count(14)?;
    let morph_count = n;
    let mut morphs = vec![Morph {
        name: "__pmx_base".into(),
        base: true,
        vertices: vertices
            .iter()
            .enumerate()
            .map(|(i, v)| (i, v.position))
            .collect(),
    }];
    let mut impulse_bodies = Vec::new();
    for _ in 0..n {
        let name = r.text()?;
        r.text()?;
        ensure!(r.u8()? <= 4, "invalid PMX morph panel");
        let kind = r.u8()?;
        ensure!(
            kind <= 10 && (kind < 9 || version > 2.05),
            "invalid PMX morph kind for version"
        );
        let count = r.count(1)?;
        let mut entries = Vec::new();
        for _ in 0..count {
            match kind {
                0 | 9 => {
                    r.required(4, morph_count)?;
                    r.f32()?;
                }
                1 => {
                    let i = r.required(0, vertices.len())?;
                    entries.push((i, r.v3()?));
                }
                2 => {
                    r.required(3, bones.len())?;
                    r.floats(7)?;
                }
                3..=7 => {
                    r.required(0, vertices.len())?;
                    r.floats(4)?;
                }
                8 => {
                    let i = r.index(2)?;
                    ensure!(
                        i >= -1 && (i < 0 || (i as usize) < materials.len()),
                        "invalid PMX material morph index"
                    );
                    ensure!(r.u8()? <= 1, "invalid PMX material operation");
                    r.floats(28)?;
                }
                10 => {
                    ensure!(version > 2.05, "impulse morph requires PMX 2.1");
                    impulse_bodies.push(r.index(5)?);
                    ensure!(r.u8()? <= 1, "invalid impulse local flag");
                    r.floats(6)?;
                }
                _ => bail!("unsupported PMX morph kind {kind}"),
            }
        }
        // Preserve names of unsupported morphs in UI; evaluation is intentionally empty.
        morphs.push(Morph {
            name,
            base: false,
            vertices: entries,
        });
    }
    // Display frames, rigid bodies and joints are structurally validated, not simulated.
    let n = r.count(10)?;
    for _ in 0..n {
        r.text()?;
        r.text()?;
        ensure!(r.u8()? <= 1, "invalid PMX display flag");
        let k = r.count(2)?;
        for _ in 0..k {
            match r.u8()? {
                0 => {
                    r.required(3, bones.len())?;
                }
                1 => {
                    r.required(4, morph_count)?;
                }
                _ => bail!("invalid PMX display element"),
            }
        }
    }
    let n = r.count(70)?;
    let rigid_count = n;
    ensure!(
        impulse_bodies
            .iter()
            .all(|&b| b >= 0 && (b as usize) < rigid_count),
        "invalid PMX impulse rigid index"
    );
    for _ in 0..n {
        r.text()?;
        r.text()?;
        let b = r.index(3)?;
        ensure!(
            b >= -1 && (b < 0 || (b as usize) < bones.len()),
            "invalid rigid body bone"
        );
        r.u8()?;
        r.u16()?;
        ensure!(r.u8()? <= 2, "invalid PMX rigid shape");
        r.floats(14)?;
        ensure!(r.u8()? <= 2, "invalid PMX rigid mode");
    }
    let n = r.count(100)?;
    for _ in 0..n {
        r.text()?;
        r.text()?;
        let kind = r.u8()?;
        ensure!(
            kind <= 5 && (kind == 0 || version > 2.05),
            "invalid PMX joint kind"
        );
        for _ in 0..2 {
            let b = r.index(5)?;
            ensure!(
                b >= -1 && (b < 0 || (b as usize) < rigid_count),
                "invalid PMX joint rigid index"
            );
        }
        r.floats(24)?;
    }
    if version > 2.05 {
        let n = r.count(1)?;
        ensure!(n == 0, "PMX 2.1 soft bodies are not supported");
    }
    ensure!(r.pos == bytes.len(), "unrecognized trailing PMX data");
    Ok(Model {
        name,
        comment,
        vertices,
        indices,
        materials,
        bones,
        ik,
        morphs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn i(out: &mut Vec<u8>, v: i32) {
        out.extend(v.to_le_bytes());
    }
    fn f(out: &mut Vec<u8>, v: f32) {
        out.extend(v.to_le_bytes());
    }
    fn text(out: &mut Vec<u8>, s: &str, utf8: bool) {
        let b: Vec<u8> = if utf8 {
            s.as_bytes().to_vec()
        } else {
            s.encode_utf16().flat_map(u16::to_le_bytes).collect()
        };
        i(out, b.len() as i32);
        out.extend(b);
    }
    fn fixture(utf8: bool, mode: u8, width: u8) -> Vec<u8> {
        let mut out = b"PMX ".to_vec();
        f(&mut out, if mode == 4 { 2.1 } else { 2.0 });
        out.push(8);
        out.extend([utf8 as u8, 0, width, width, width, width, width, width]);
        text(&mut out, "ミク", utf8);
        for _ in 0..3 {
            text(&mut out, "", utf8);
        }
        i(&mut out, 1);
        for v in [0., 1., 0., 0., 1., 0., 0., 0.] {
            f(&mut out, v);
        }
        out.push(mode);
        let index =
            |out: &mut Vec<u8>, v: i32| out.extend_from_slice(&v.to_le_bytes()[..width as usize]);
        match mode {
            0 => index(&mut out, 0),
            1 | 3 => {
                index(&mut out, 0);
                index(&mut out, 0);
                f(&mut out, 0.75);
                if mode == 3 {
                    for _ in 0..9 {
                        f(&mut out, 0.);
                    }
                }
            }
            2 | 4 => {
                for _ in 0..4 {
                    index(&mut out, 0);
                }
                for w in [0.4, 0.3, 0.2, 0.1] {
                    f(&mut out, w);
                }
            }
            _ => unreachable!(),
        };
        f(&mut out, 1.);
        // No triangle, texture or material records; one root bone.
        for _ in 0..3 {
            i(&mut out, 0);
        }
        i(&mut out, 1);
        text(&mut out, "センター", utf8);
        text(&mut out, "", utf8);
        for _ in 0..3 {
            f(&mut out, 0.);
        }
        index(&mut out, -1);
        i(&mut out, 0);
        out.extend(0u16.to_le_bytes());
        for _ in 0..3 {
            f(&mut out, 0.);
        }
        i(&mut out, 1);
        text(&mut out, "笑い", utf8);
        text(&mut out, "", utf8);
        out.extend([1, 1]);
        i(&mut out, 1);
        index(&mut out, 0);
        for v in [1., 2., 3.] {
            f(&mut out, v);
        }
        for _ in 0..3 {
            i(&mut out, 0);
        }
        if mode == 4 {
            i(&mut out, 0);
        }
        out
    }
    #[test]
    fn decodes_all_widths_encodings_and_skin_modes() {
        for utf8 in [false, true] {
            for width in [1, 2, 4] {
                for mode in 0..=4 {
                    let m = parse(&fixture(utf8, mode, width)).unwrap();
                    assert_eq!(m.name, "ミク");
                    assert_eq!(m.bones[0].parent, None);
                    assert_eq!(m.morphs[1].vertices[0], (0, Vec3::new(1., 2., 3.)));
                    assert!((m.vertices[0].weights.iter().sum::<f32>() - 1.).abs() < 0.0001);
                }
            }
        }
    }
    #[test]
    fn every_truncated_prefix_is_rejected() {
        let bytes = fixture(true, 2, 4);
        for end in 0..bytes.len() {
            assert!(parse(&bytes[..end]).is_err(), "accepted prefix {end}");
        }
    }
    #[test]
    fn rejects_invalid_header_and_nonfinite_vertex() {
        let mut bytes = fixture(true, 0, 1);
        bytes[11] = 3;
        assert!(parse(&bytes).is_err());
        let mut bytes = fixture(true, 0, 1);
        let vertex_start = 17 + 4 + "ミク".len() + 12 + 4;
        bytes[vertex_start..vertex_start + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(parse(&bytes).is_err());
    }
    #[test]
    fn rejects_cycles_counts_and_invalid_skin_reference() {
        let mut bytes = fixture(true, 0, 1);
        let needle = "センター".as_bytes();
        let name = bytes
            .windows(needle.len())
            .position(|v| v == needle)
            .unwrap();
        let parent = name + needle.len() + 4 + 12;
        bytes[parent] = 0;
        assert!(parse(&bytes).unwrap_err().to_string().contains("cyclic"));
        let mut bytes = fixture(true, 0, 1);
        let vertex_count = 17 + 4 + "ミク".len() + 12;
        bytes[vertex_count..vertex_count + 4].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(parse(&bytes).is_err());
        let mut bytes = fixture(true, 0, 1);
        bytes[vertex_count + 4 + 33] = 127;
        assert!(parse(&bytes).unwrap_err().to_string().contains("skin bone"));
    }
    #[test]
    fn imported_rest_pose_morph_and_four_weight_skinning() {
        use crate::animation::Pose;
        use crate::format::Motion;
        let mut model = parse(&fixture(true, 2, 1)).unwrap();
        let pose = Pose::sample(&model, &Motion::default(), 0.0);
        assert!(pose.skin(&model)[0]
            .0
            .abs_diff_eq(model.vertices[0].position, 1e-6));
        let mut pose = pose;
        pose.morphs[1] = 0.5;
        assert!(pose.skin(&model)[0]
            .0
            .abs_diff_eq(Vec3::new(0.5, 2.0, 1.5), 1e-6));
        for i in 1..4 {
            model.bones.push(Bone {
                name: i.to_string(),
                parent: None,
                position: Vec3::ZERO,
            });
        }
        model.vertices[0].bones = [0, 1, 2, 3];
        let mut pose = Pose::sample(&model, &Motion::default(), 0.0);
        for i in 0..4 {
            pose.translations[i] = Vec3::X * (i as f32 * 10.0);
        }
        assert!(pose.skin(&model)[0]
            .0
            .abs_diff_eq(Vec3::new(10.0, 1.0, 0.0), 1e-5));
    }
}
