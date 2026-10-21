//! Bounded little-endian PMD 1.0 and VMD 0002 readers.
use anyhow::{bail, ensure, Context, Result};
use encoding_rs::SHIFT_JIS;
use glam::{Quat, Vec2, Vec3};
use std::collections::BTreeMap;

pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).context("offset overflow")?;
        ensure!(
            end <= self.bytes.len(),
            "truncated file at byte {} (need {n})",
            self.pos
        );
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn f32(&mut self) -> Result<f32> {
        let f = f32::from_le_bytes(self.take(4)?.try_into()?);
        ensure!(f.is_finite(), "non-finite float");
        Ok(f)
    }
    fn vec3(&mut self) -> Result<Vec3> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    fn text(&mut self, n: usize) -> Result<String> {
        let raw = self.take(n)?;
        let raw = &raw[..raw.iter().position(|&v| v == 0).unwrap_or(raw.len())];
        Ok(SHIFT_JIS.decode_without_bom_handling(raw).0.into_owned())
    }
    fn count(&mut self, stride: usize) -> Result<usize> {
        let n = self.u32()? as usize;
        ensure!(
            n <= 5_000_000 && n <= (self.bytes.len() - self.pos) / stride.max(1),
            "invalid record count {n}"
        );
        Ok(n)
    }
}
#[derive(Clone, Debug)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: Vec2,
    pub bones: [u16; 4],
    pub weights: [f32; 4],
}
#[derive(Clone, Debug)]
pub struct Material {
    pub diffuse: [f32; 4],
    pub ambient: Vec3,
    pub texture: String,
    pub start: u32,
    pub count: u32,
}
#[derive(Clone, Debug)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub position: Vec3,
}
#[derive(Clone, Debug)]
pub struct Ik {
    pub controller: usize,
    pub target: usize,
    pub iterations: u16,
    pub angle: f32,
    pub links: Vec<usize>,
}
#[derive(Clone, Debug)]
pub struct Morph {
    pub name: String,
    pub base: bool,
    pub vertices: Vec<(usize, Vec3)>,
}
#[derive(Clone, Debug)]
pub struct Model {
    pub name: String,
    pub comment: String,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub materials: Vec<Material>,
    pub bones: Vec<Bone>,
    pub ik: Vec<Ik>,
    pub morphs: Vec<Morph>,
}
impl Model {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.starts_with(b"PMX ") {
            return crate::pmx::parse(bytes);
        }
        let mut r = Reader::new(bytes);
        ensure!(r.take(3)? == b"Pmd", "expected PMD or PMX signature");
        ensure!((r.f32()? - 1.0).abs() < 0.0001, "unsupported PMD version");
        let name = r.text(20)?;
        let comment = r.text(256)?;
        let n = r.count(38)?;
        let mut vertices = Vec::with_capacity(n);
        for _ in 0..n {
            let position = r.vec3()?;
            let normal = r.vec3()?;
            let uv = Vec2::new(r.f32()?, r.f32()?);
            let bones = [r.u16()?, r.u16()?, 0, 0];
            let weight = r.u8()?;
            ensure!(weight <= 100, "invalid vertex weight");
            r.u8()?;
            vertices.push(Vertex {
                position,
                normal,
                uv,
                bones,
                weights: [weight as f32 / 100.0, 1.0 - weight as f32 / 100.0, 0.0, 0.0],
            });
        }
        let n = r.count(2)?;
        ensure!(n % 3 == 0, "non-triangular indices");
        let mut indices = Vec::with_capacity(n);
        for _ in 0..n {
            let i = r.u16()? as u32;
            ensure!((i as usize) < vertices.len(), "invalid vertex index");
            indices.push(i);
        }
        let n = r.count(70)?;
        let mut materials = Vec::with_capacity(n);
        let mut start = 0u32;
        for _ in 0..n {
            let color = r.vec3()?;
            let alpha = r.f32()?;
            r.f32()?;
            r.vec3()?;
            let ambient = r.vec3()?;
            r.u8()?;
            r.u8()?;
            let count = r.u32()?;
            let texture = r.text(20)?;
            ensure!(count % 3 == 0, "non-triangular material");
            materials.push(Material {
                diffuse: [color.x, color.y, color.z, alpha],
                ambient,
                texture,
                start,
                count,
            });
            start = start
                .checked_add(count)
                .context("material range overflow")?;
        }
        ensure!(
            start as usize == indices.len(),
            "material index ranges do not cover mesh"
        );
        let n = r.u16()? as usize;
        let mut bones = Vec::with_capacity(n);
        for _ in 0..n {
            let name = r.text(20)?;
            let p = r.u16()?;
            r.u16()?;
            r.u8()?;
            r.u16()?;
            let position = r.vec3()?;
            let parent = if p == u16::MAX {
                None
            } else {
                Some(p as usize)
            };
            ensure!(parent.is_none_or(|p| p < n), "invalid bone parent");
            bones.push(Bone {
                name,
                parent,
                position,
            });
        }
        // Three-color iterative traversal validates unsorted hierarchies in linear time.
        let mut states = vec![0u8; n];
        let mut chain = Vec::new();
        for i in 0..n {
            let mut cursor = Some(i);
            while let Some(j) = cursor {
                if states[j] == 2 {
                    break;
                }
                ensure!(states[j] != 1, "cyclic skeleton");
                states[j] = 1;
                chain.push(j);
                cursor = bones[j].parent;
            }
            while let Some(j) = chain.pop() {
                states[j] = 2;
            }
        }
        for v in &vertices {
            ensure!(
                v.bones.iter().all(|&b| (b as usize) < n),
                "invalid skin bone"
            );
        }
        let n = r.u16()? as usize;
        let mut ik = Vec::with_capacity(n);
        for _ in 0..n {
            let controller = r.u16()? as usize;
            let target = r.u16()? as usize;
            let links_count = r.u8()?;
            let iterations = r.u16()?;
            let angle = r.f32()?;
            ensure!(
                controller < bones.len() && target < bones.len(),
                "invalid IK bone"
            );
            let mut links = Vec::new();
            for _ in 0..links_count {
                let b = r.u16()? as usize;
                ensure!(b < bones.len(), "invalid IK link");
                links.push(b);
            }
            ik.push(Ik {
                controller,
                target,
                iterations,
                angle,
                links,
            });
        }
        let n = r.u16()? as usize;
        let mut morphs = Vec::with_capacity(n);
        for _ in 0..n {
            let name = r.text(20)?;
            let count = r.u32()? as usize;
            let base = r.u8()? == 0;
            ensure!(count <= (r.bytes.len() - r.pos) / 16, "invalid morph count");
            let mut entries = Vec::with_capacity(count);
            for _ in 0..count {
                entries.push((r.u32()? as usize, r.vec3()?));
            }
            morphs.push(Morph {
                name,
                base,
                vertices: entries,
            });
        }
        let base = morphs.iter().find(|m| m.base);
        for m in &morphs {
            for &(i, _) in &m.vertices {
                ensure!(
                    if m.base {
                        i < vertices.len()
                    } else {
                        base.is_some_and(|b| i < b.vertices.len())
                    },
                    "invalid morph vertex"
                );
            }
        }
        Ok(Self {
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
}
#[derive(Clone, Debug)]
pub struct BoneKey {
    pub frame: u32,
    pub translation: Vec3,
    pub rotation: Quat,
    pub interpolation: [u8; 64],
}
#[derive(Clone, Debug)]
pub struct MorphKey {
    pub frame: u32,
    pub weight: f32,
}
#[derive(Clone, Debug, Default)]
pub struct Motion {
    pub model_name: String,
    pub bones: BTreeMap<String, Vec<BoneKey>>,
    pub morphs: BTreeMap<String, Vec<MorphKey>>,
    pub last_frame: u32,
}
impl Motion {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let magic = r.take(30)?;
        if !magic.starts_with(b"Vocaloid Motion Data 0002\0") {
            bail!("expected VMD 0002 signature");
        }
        let model_name = r.text(20)?;
        let mut out = Self {
            model_name,
            ..Self::default()
        };
        let n = r.count(111)?;
        for _ in 0..n {
            let name = r.text(15)?;
            let frame = r.u32()?;
            let translation = r.vec3()?;
            let q = Quat::from_xyzw(r.f32()?, r.f32()?, r.f32()?, r.f32()?);
            ensure!(q.length_squared() > 1e-12, "zero quaternion");
            let interpolation = r.take(64)?.try_into()?;
            out.last_frame = out.last_frame.max(frame);
            out.bones.entry(name).or_default().push(BoneKey {
                frame,
                translation,
                rotation: q.normalize(),
                interpolation,
            });
        }
        let n = r.count(23)?;
        for _ in 0..n {
            let name = r.text(15)?;
            let frame = r.u32()?;
            let weight = r.f32()?;
            out.last_frame = out.last_frame.max(frame);
            out.morphs
                .entry(name)
                .or_default()
                .push(MorphKey { frame, weight });
        }
        for keys in out.bones.values_mut() {
            keys.sort_by_key(|k| k.frame);
            keys.dedup_by_key(|k| k.frame);
        }
        for keys in out.morphs.values_mut() {
            keys.sort_by_key(|k| k.frame);
            keys.dedup_by_key(|k| k.frame);
        }
        // Camera/light/shadow/IK-switch tracks are deliberately not interpreted in this milestone.
        Ok(out)
    }
}

impl Motion {
    /// Export bone and morph tracks as VMD 0002 (other track types are empty).
    pub fn to_vmd(&self) -> Result<Vec<u8>> {
        fn text(out: &mut Vec<u8>, value: &str, width: usize) -> Result<()> {
            let (bytes, _, errors) = SHIFT_JIS.encode(value);
            ensure!(
                !errors && bytes.len() <= width,
                "name cannot be represented in VMD: {value}"
            );
            out.extend_from_slice(&bytes);
            out.resize(out.len() + width - bytes.len(), 0);
            Ok(())
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"Vocaloid Motion Data 0002\0\0\0\0\0");
        text(&mut out, &self.model_name, 20)?;
        let count: usize = self.bones.values().map(Vec::len).sum();
        out.extend_from_slice(&(count as u32).to_le_bytes());
        for (name, keys) in &self.bones {
            for key in keys {
                text(&mut out, name, 15)?;
                out.extend_from_slice(&key.frame.to_le_bytes());
                for value in key
                    .translation
                    .to_array()
                    .into_iter()
                    .chain(key.rotation.to_array())
                {
                    out.extend_from_slice(&value.to_le_bytes());
                }
                out.extend_from_slice(&key.interpolation);
            }
        }
        let count: usize = self.morphs.values().map(Vec::len).sum();
        out.extend_from_slice(&(count as u32).to_le_bytes());
        for (name, keys) in &self.morphs {
            for key in keys {
                text(&mut out, name, 15)?;
                out.extend_from_slice(&key.frame.to_le_bytes());
                out.extend_from_slice(&key.weight.to_le_bytes());
            }
        }
        for _ in 0..4 {
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        Ok(out)
    }
    pub fn recompute_duration(&mut self) {
        self.last_frame = self
            .bones
            .values()
            .flatten()
            .map(|k| k.frame)
            .chain(self.morphs.values().flatten().map(|k| k.frame))
            .max()
            .unwrap_or(0);
    }
}
