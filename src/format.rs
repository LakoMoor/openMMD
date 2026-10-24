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
    /// PMD edge-disable flag converted to 0/1; PMX per-vertex edge multiplier.
    pub edge_scale: f32,
}
#[derive(Clone, Debug, Default)]
pub struct Material {
    pub diffuse: [f32; 4],
    pub ambient: Vec3,
    pub specular: Vec3,
    pub shininess: f32,
    pub sphere_texture: String,
    /// 0=disabled, 1=multiply, 2=add, 3=PMX extra-UV subtexture (not rendered yet).
    pub sphere_mode: u8,
    pub toon_texture: String,
    pub double_sided: bool,
    pub edge_color: [f32; 4],
    pub edge_size: f32,
    pub edge_enabled: bool,
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
            let edge_scale = if r.u8()? == 0 { 1. } else { 0. };
            vertices.push(Vertex {
                position,
                normal,
                uv,
                bones,
                weights: [weight as f32 / 100.0, 1.0 - weight as f32 / 100.0, 0.0, 0.0],
                edge_scale,
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
            let shininess = r.f32()?;
            let specular = r.vec3()?;
            let ambient = r.vec3()?;
            let toon = r.u8()?;
            ensure!(toon < 10 || toon == 255, "invalid PMD toon index");
            let edge = r.u8()?;
            let count = r.u32()?;
            let names = r.text(20)?;
            let sphere_texture = names
                .split('*')
                .find(|name| {
                    let ext = name.to_ascii_lowercase();
                    ext.ends_with(".sph") || ext.ends_with(".spa")
                })
                .unwrap_or("")
                .to_owned();
            let texture = names
                .split('*')
                .find(|name| !name.is_empty() && *name != sphere_texture)
                .unwrap_or("")
                .to_owned();
            let sphere_mode = if sphere_texture.is_empty() {
                0
            } else if sphere_texture.to_ascii_lowercase().ends_with(".spa") {
                2
            } else {
                1
            };
            ensure!(count % 3 == 0, "non-triangular material");
            materials.push(Material {
                diffuse: [color.x, color.y, color.z, alpha],
                ambient,
                specular,
                shininess,
                sphere_texture,
                sphere_mode,
                toon_texture: if toon == 255 {
                    String::new()
                } else {
                    format!("toon{:02}.bmp", toon + 1)
                },
                double_sided: alpha < 1.,
                edge_color: [0., 0., 0., 1.],
                edge_size: 1.,
                edge_enabled: edge != 0,
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
        // Optional PMD display/English-name extension followed by the ten toon filenames.
        if r.pos < r.bytes.len() {
            let display_morphs = r.u8()? as usize;
            r.take(display_morphs * 2)?;
            let groups = r.u8()? as usize;
            r.take(groups * 50)?;
            let displayed_bones = r.count(3)?;
            r.take(displayed_bones * 3)?;
            let english = r.u8()?;
            ensure!(english <= 1, "invalid PMD English flag");
            if english == 1 {
                r.take(
                    20 + 256 + bones.len() * 20 + morphs.len().saturating_sub(1) * 20 + groups * 50,
                )?;
            }
            if r.pos < r.bytes.len() {
                let mut toon_names = Vec::with_capacity(10);
                for _ in 0..10 {
                    toon_names.push(r.text(100)?);
                }
                for material in &mut materials {
                    if let Some(index) = material
                        .toon_texture
                        .strip_prefix("toon")
                        .and_then(|s| s.strip_suffix(".bmp"))
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        if (1..=10).contains(&index) {
                            material.toon_texture = toon_names[index - 1].clone();
                        }
                    }
                }
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
#[derive(Clone, Debug, PartialEq)]
pub struct ShadowKey {
    pub frame: u32,
    pub mode: u8,
    /// Raw VMD distance parameter; retained without unit conversion.
    pub distance: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyKey {
    pub frame: u32,
    pub visible: bool,
    pub ik: BTreeMap<String, bool>,
}
#[derive(Clone, Debug, Default)]
pub struct Motion {
    pub model_name: String,
    pub bones: BTreeMap<String, Vec<BoneKey>>,
    pub morphs: BTreeMap<String, Vec<MorphKey>>,
    pub last_frame: u32,
    pub cameras: Vec<crate::pmm::PmmCameraKey>,
    pub lights: Vec<crate::pmm::PmmLightKey>,
    pub shadows: Vec<ShadowKey>,
    pub properties: Vec<PropertyKey>,
    /// Unknown bytes after all standard VMD sections. Never silently discarded on export.
    pub trailing_data: Vec<u8>,
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
        // Older VMD files can end at a complete optional-section boundary.
        if r.pos < r.bytes.len() {
            let n = r.count(61)?;
            for _ in 0..n {
                let frame = r.u32()?;
                let distance = r.f32()?;
                let look_at = r.vec3()?;
                let angle = r.vec3()?;
                let mut interpolation = [[0; 4]; 6];
                for curve in &mut interpolation {
                    let raw = r.take(4)?;
                    ensure!(
                        raw.iter().all(|&v| v <= 127),
                        "invalid camera Bezier coordinate"
                    );
                    // VMD camera stores x1,x2,y1,y2; shared camera uses x1,y1,x2,y2.
                    *curve = [raw[0], raw[2], raw[1], raw[3]];
                }
                let fov = r.u32()?;
                ensure!((1..=179).contains(&fov), "invalid camera field of view");
                let perspective = match r.u8()? {
                    0 => true,
                    1 => false,
                    _ => bail!("invalid camera perspective flag"),
                };
                out.cameras.push(crate::pmm::PmmCameraKey {
                    frame,
                    distance,
                    look_at,
                    angle,
                    fov: fov as f32,
                    perspective,
                    interpolation,
                    ..Default::default()
                });
            }
        }
        if r.pos < r.bytes.len() {
            let n = r.count(28)?;
            for _ in 0..n {
                out.lights.push(crate::pmm::PmmLightKey {
                    frame: r.u32()?,
                    color: r.vec3()?,
                    direction: r.vec3()?,
                });
            }
        }
        if r.pos < r.bytes.len() {
            let n = r.count(9)?;
            for _ in 0..n {
                let frame = r.u32()?;
                let mode = r.u8()?;
                ensure!(mode <= 2, "invalid shadow mode");
                let distance = r.f32()?;
                out.shadows.push(ShadowKey {
                    frame,
                    mode,
                    distance,
                });
            }
        }
        if r.pos < r.bytes.len() {
            let n = r.count(9)?;
            for _ in 0..n {
                let frame = r.u32()?;
                let visible = read_flag(&mut r)?;
                let count = r.count(21)?;
                let mut ik = BTreeMap::new();
                for _ in 0..count {
                    let name = r.text(20)?;
                    let enabled = read_flag(&mut r)?;
                    ensure!(
                        ik.insert(name, enabled).is_none(),
                        "duplicate IK switch name"
                    );
                }
                out.properties.push(PropertyKey { frame, visible, ik });
            }
            out.trailing_data = r.take(r.bytes.len() - r.pos)?.to_vec();
        }
        out.cameras.sort_by_key(|k| k.frame);
        out.lights.sort_by_key(|k| k.frame);
        out.shadows.sort_by_key(|k| k.frame);
        out.properties.sort_by_key(|k| k.frame);
        out.recompute_duration();
        Ok(out)
    }
}

impl Motion {
    /// Export all standard VMD tracks and retain any unknown extension bytes.
    pub fn to_vmd(&self) -> Result<Vec<u8>> {
        fn text(out: &mut Vec<u8>, value: &str, width: usize) -> Result<()> {
            let (bytes, _, errors) = SHIFT_JIS.encode(value);
            ensure!(
                !errors
                    && bytes.len() <= width
                    && !value.contains('\0')
                    && SHIFT_JIS.decode_without_bom_handling(&bytes).0 == value,
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
        write_count(&mut out, count)?;
        for (name, keys) in &self.bones {
            for key in keys {
                text(&mut out, name, 15)?;
                ensure!(
                    key.rotation.is_finite() && key.rotation.length_squared() > 1e-12,
                    "invalid VMD quaternion"
                );
                out.extend_from_slice(&key.frame.to_le_bytes());
                for value in key
                    .translation
                    .to_array()
                    .into_iter()
                    .chain(key.rotation.to_array())
                {
                    ensure!(value.is_finite(), "non-finite VMD bone value");
                    out.extend_from_slice(&value.to_le_bytes());
                }
                out.extend_from_slice(&key.interpolation);
            }
        }
        let count: usize = self.morphs.values().map(Vec::len).sum();
        write_count(&mut out, count)?;
        for (name, keys) in &self.morphs {
            for key in keys {
                text(&mut out, name, 15)?;
                out.extend_from_slice(&key.frame.to_le_bytes());
                write_floats(&mut out, [key.weight])?;
            }
        }
        write_count(&mut out, self.cameras.len())?;
        for key in &self.cameras {
            ensure!(
                key.parent_model == -1 && key.parent_bone == -1,
                "VMD cannot represent attached cameras; bake the attachment first"
            );
            ensure!(
                key.fov.is_finite() && (1.0..=179.0).contains(&key.fov),
                "invalid camera field of view"
            );
            out.extend_from_slice(&key.frame.to_le_bytes());
            write_floats(
                &mut out,
                [key.distance]
                    .into_iter()
                    .chain(key.look_at.to_array())
                    .chain(key.angle.to_array()),
            )?;
            for c in key.interpolation {
                ensure!(
                    c.iter().all(|&v| v <= 127),
                    "invalid camera Bezier coordinate"
                );
                out.extend([c[0], c[2], c[1], c[3]]);
            }
            out.extend_from_slice(&(key.fov.round() as u32).to_le_bytes());
            out.push(u8::from(!key.perspective));
        }
        write_count(&mut out, self.lights.len())?;
        for key in &self.lights {
            out.extend_from_slice(&key.frame.to_le_bytes());
            write_floats(
                &mut out,
                key.color
                    .to_array()
                    .into_iter()
                    .chain(key.direction.to_array()),
            )?;
        }
        write_count(&mut out, self.shadows.len())?;
        for key in &self.shadows {
            ensure!(key.mode <= 2, "invalid shadow mode");
            out.extend_from_slice(&key.frame.to_le_bytes());
            out.push(key.mode);
            write_floats(&mut out, [key.distance])?;
        }
        write_count(&mut out, self.properties.len())?;
        for key in &self.properties {
            out.extend_from_slice(&key.frame.to_le_bytes());
            out.push(u8::from(key.visible));
            write_count(&mut out, key.ik.len())?;
            for (name, &enabled) in &key.ik {
                text(&mut out, name, 20)?;
                out.push(u8::from(enabled));
            }
        }
        out.extend_from_slice(&self.trailing_data);
        Ok(out)
    }
    pub fn recompute_duration(&mut self) {
        self.last_frame = self
            .bones
            .values()
            .flatten()
            .map(|k| k.frame)
            .chain(self.morphs.values().flatten().map(|k| k.frame))
            .chain(self.cameras.iter().map(|k| k.frame))
            .chain(self.lights.iter().map(|k| k.frame))
            .chain(self.shadows.iter().map(|k| k.frame))
            .chain(self.properties.iter().map(|k| k.frame))
            .max()
            .unwrap_or(0);
    }
}

fn read_flag(r: &mut Reader<'_>) -> Result<bool> {
    match r.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => bail!("invalid VMD boolean flag"),
    }
}
fn write_count(out: &mut Vec<u8>, count: usize) -> Result<()> {
    ensure!(count <= 5_000_000, "too many VMD records");
    out.extend_from_slice(&(count as u32).to_le_bytes());
    Ok(())
}
fn write_floats(out: &mut Vec<u8>, values: impl IntoIterator<Item = f32>) -> Result<()> {
    for v in values {
        ensure!(v.is_finite(), "non-finite VMD value");
        out.extend_from_slice(&v.to_le_bytes());
    }
    Ok(())
}
impl Motion {
    pub fn has_scene_tracks(&self) -> bool {
        !self.cameras.is_empty() || !self.lights.is_empty() || !self.shadows.is_empty()
    }
    pub fn has_model_tracks(&self) -> bool {
        !self.bones.is_empty() || !self.morphs.is_empty() || !self.properties.is_empty()
    }
    pub fn model_only(&self) -> Self {
        let mut model = self.clone();
        model.cameras.clear();
        model.lights.clear();
        model.shadows.clear();
        model.recompute_duration();
        model
    }
    pub fn scene_only(&self) -> Self {
        let mut motion = Self {
            model_name: "カメラ・照明".into(),
            cameras: self.cameras.clone(),
            lights: self.lights.clone(),
            shadows: self.shadows.clone(),
            trailing_data: self.trailing_data.clone(),
            ..Default::default()
        };
        motion.recompute_duration();
        motion
    }
    /// Replace only scene channels present in an imported file (e.g. load light after camera).
    pub fn merged_scene(&self, source: &Self) -> Result<Self> {
        if !source.has_scene_tracks() {
            return Ok(self.scene_only());
        }
        ensure!(
            self.trailing_data.is_empty()
                || source.trailing_data.is_empty()
                || self.trailing_data == source.trailing_data,
            "Cannot combine different unknown VMD extensions; keep these files separate"
        );
        let mut result = self.scene_only();
        if !source.cameras.is_empty() {
            result.cameras = source.cameras.clone();
        }
        if !source.lights.is_empty() {
            result.lights = source.lights.clone();
        }
        if !source.shadows.is_empty() {
            result.shadows = source.shadows.clone();
        }
        if !source.trailing_data.is_empty() {
            result.trailing_data = source.trailing_data.clone();
        }
        result.recompute_duration();
        Ok(result)
    }
    pub fn camera_at(&self, frame: f32) -> Option<crate::pmm::PmmCameraKey> {
        crate::pmm::PmmCameraKey::sample(&self.cameras, frame)
    }
    pub fn light_at(&self, frame: f32) -> Option<crate::pmm::PmmLightKey> {
        crate::pmm::PmmLightKey::sample(&self.lights, frame)
    }
}
