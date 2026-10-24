//! PMM model-animation reader, lossless key patches and animation-section writer.
//! Unedited camera, light, accessory and editor records remain byte-identical.
use crate::format::{BoneKey, Model, MorphKey, Motion};
use anyhow::{bail, ensure, Context, Result};
use encoding_rs::SHIFT_JIS;
use glam::{Quat, Vec3};
use std::collections::BTreeMap;
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct PmmModel {
    pub name: String,
    pub path: String,
    path_offset: usize,
    pub motion: Motion,
    bone_names: Vec<String>,
    morph_names: Vec<String>,
    bone_tracks: Vec<Vec<(BoneKey, usize)>>,
    morph_tracks: Vec<Vec<(MorphKey, usize)>>,
    animation_range: Range<usize>,
    last_frame_offset: usize,
    other_last_frame: u32,
    bone_offsets: BTreeMap<(String, u32), Vec<usize>>,
    morph_offsets: BTreeMap<(String, u32), Vec<usize>>,
}
#[derive(Clone, Debug)]
pub struct PmmProject {
    pub version: u8,
    pub models: Vec<PmmModel>,
    bytes: Vec<u8>,
    scene_offset: usize,
}

/// Camera key in MMD coordinates; angles are radians and field of view is degrees.
#[derive(Clone, Debug, PartialEq)]
pub struct PmmCameraKey {
    pub frame: u32,
    pub distance: f32,
    pub look_at: Vec3,
    pub angle: Vec3,
    pub fov: f32,
    pub perspective: bool,
    /// PMM v2 camera attachment indices, -1 when unattached.
    pub parent_model: i32,
    pub parent_bone: i32,
    /// X/Y/Z, rotation, distance, FOV; each is [x1,y1,x2,y2].
    pub interpolation: [[u8; 4]; 6],
}
impl Default for PmmCameraKey {
    fn default() -> Self {
        Self {
            frame: 0,
            distance: -45.,
            look_at: Vec3::new(0., 10., 0.),
            angle: Vec3::ZERO,
            fov: 45.,
            perspective: true,
            parent_model: -1,
            parent_bone: -1,
            interpolation: [[20, 20, 107, 107]; 6],
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct PmmLightKey {
    pub frame: u32,
    pub color: Vec3,
    pub direction: Vec3,
}
impl Default for PmmLightKey {
    fn default() -> Self {
        Self {
            frame: 0,
            color: Vec3::splat(0.6),
            direction: Vec3::new(-0.5, -1., -0.5),
        }
    }
}
#[derive(Clone, Debug)]
pub struct PmmCameraState {
    pub look_at: Vec3,
    pub position: Vec3,
    pub angle: Vec3,
    pub perspective: bool,
    pub fov: f32,
}
#[derive(Clone, Debug)]
pub struct PmmSceneTracks {
    pub camera: Vec<PmmCameraKey>,
    pub light: Vec<PmmLightKey>,
    pub current_camera: PmmCameraState,
    pub current_light: PmmLightKey,
    camera_range: Range<usize>,
    light_range: Range<usize>,
    camera_offsets: BTreeMap<u32, usize>,
    light_offsets: BTreeMap<u32, usize>,
}
impl PmmSceneTracks {
    pub fn camera_at(&self, frame: f32) -> Option<PmmCameraKey> {
        PmmCameraKey::sample(&self.camera, frame)
    }
    pub fn light_at(&self, frame: f32) -> Option<PmmLightKey> {
        PmmLightKey::sample(&self.light, frame)
    }
}
impl PmmCameraKey {
    pub fn sample(keys: &[Self], frame: f32) -> Option<Self> {
        let frame = if frame.is_finite() { frame.max(0.) } else { 0. };
        let next = keys.partition_point(|key| key.frame as f32 <= frame);
        if next == 0 {
            return keys.first().cloned();
        }
        let a = &keys[next - 1];
        if frame == a.frame as f32 {
            return Some(a.clone());
        }
        let Some(b) = keys.get(next) else {
            return Some(a.clone());
        };
        let t = ((frame - a.frame as f32) / (b.frame - a.frame) as f32).clamp(0., 1.);
        let ease = |channel| crate::animation::camera_bezier(t, b.interpolation[channel]);
        let mut out = a.clone();
        out.frame = frame as u32;
        let delta = b.look_at - a.look_at;
        out.look_at = a.look_at + delta * Vec3::new(ease(0), ease(1), ease(2));
        out.angle = a.angle.lerp(b.angle, ease(3));
        out.distance = a.distance + (b.distance - a.distance) * ease(4);
        out.fov = a.fov + (b.fov - a.fov) * ease(5);
        Some(out)
    }
}
impl PmmLightKey {
    pub fn sample(keys: &[Self], frame: f32) -> Option<Self> {
        let frame = if frame.is_finite() { frame.max(0.) } else { 0. };
        let next = keys.partition_point(|key| key.frame as f32 <= frame);
        if next == 0 {
            return keys.first().cloned();
        }
        let a = &keys[next - 1];
        let Some(b) = keys.get(next) else {
            return Some(a.clone());
        };
        let t = ((frame - a.frame as f32) / (b.frame - a.frame) as f32).clamp(0., 1.);
        Some(PmmLightKey {
            frame: frame as u32,
            color: a.color.lerp(b.color, t),
            direction: a.direction.lerp(b.direction, t),
        })
    }
}

fn read_camera(r: &mut Cursor<'_>, indexed: bool, version: u8) -> Result<Record<PmmCameraKey>> {
    let (frame, prev) = base(r, indexed)?;
    let offset = r.pos;
    let distance = r.float()?;
    let look_at = r.vec3()?;
    let angle = r.vec3()?;
    let (parent_model, parent_bone) = if version == 2 {
        (r.u32()? as i32, r.u32()? as i32)
    } else {
        (-1, -1)
    };
    let mut interpolation = [[0; 4]; 6];
    for channel in &mut interpolation {
        channel.copy_from_slice(r.take(4)?);
    }
    let perspective = r.byte()? == 0;
    let fov = r.u32()? as i32 as f32;
    r.byte()?;
    Ok(Record {
        frame,
        prev,
        offset,
        value: PmmCameraKey {
            frame,
            distance,
            look_at,
            angle,
            fov,
            perspective,
            parent_model,
            parent_bone,
            interpolation,
        },
    })
}
fn read_light(r: &mut Cursor<'_>, indexed: bool) -> Result<Record<PmmLightKey>> {
    let (frame, prev) = base(r, indexed)?;
    let offset = r.pos;
    let color = r.vec3()?;
    let direction = r.vec3()?;
    r.byte()?;
    Ok(Record {
        frame,
        prev,
        offset,
        value: PmmLightKey {
            frame,
            color,
            direction,
        },
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).context("PMM offset overflow")?;
        ensure!(
            end <= self.bytes.len(),
            "truncated PMM at {} (need {n})",
            self.pos
        );
        let b = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(b)
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n)?;
        Ok(())
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn count(&mut self, stride: usize) -> Result<usize> {
        let n = self.u32()? as usize;
        ensure!(
            n <= 5_000_000 && n <= (self.bytes.len() - self.pos) / stride.max(1),
            "invalid PMM count {n} at {}",
            self.pos - 4
        );
        Ok(n)
    }
    fn float(&mut self) -> Result<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into()?);
        ensure!(v.is_finite(), "non-finite PMM number at {}", self.pos - 4);
        Ok(v)
    }
    fn vec3(&mut self) -> Result<Vec3> {
        Ok(Vec3::new(self.float()?, self.float()?, self.float()?))
    }
    fn text(&mut self, n: usize) -> Result<String> {
        let b = self.take(n)?;
        let n = b.iter().position(|&v| v == 0).unwrap_or(b.len());
        Ok(SHIFT_JIS
            .decode_without_bom_handling(&b[..n])
            .0
            .into_owned())
    }
    fn variable(&mut self) -> Result<String> {
        let n = self.byte()? as usize;
        self.text(n)
    }
    fn names(&mut self) -> Result<Vec<String>> {
        let n = self.count(1)?;
        (0..n).map(|_| self.variable()).collect()
    }
}
struct Record<T> {
    prev: usize,
    frame: u32,
    offset: usize,
    value: T,
}
fn base(r: &mut Cursor<'_>, indexed: bool) -> Result<(u32, usize)> {
    if indexed {
        r.u32()?;
    }
    let frame = r.u32()?;
    let prev = r.u32()? as usize;
    r.u32()?;
    Ok((frame, prev))
}
fn read_bone(r: &mut Cursor<'_>, indexed: bool, version: u8) -> Result<Record<BoneKey>> {
    let (frame, prev) = base(r, indexed)?;
    let mut interpolation = [0; 64];
    let packed = r.take(16)?;
    // VMD's first 16 bytes are the same control points, transposed by channel.
    for channel in 0..4 {
        for point in 0..4 {
            interpolation[point * 4 + channel] = packed[channel * 4 + point];
        }
    }
    let offset = r.pos;
    let translation = r.vec3()?;
    let q = Quat::from_xyzw(r.float()?, r.float()?, r.float()?, r.float()?);
    let rotation = if q.length_squared() > 1e-12 {
        q.normalize()
    } else {
        Quat::IDENTITY
    };
    r.skip(if version == 2 { 2 } else { 1 })?;
    Ok(Record {
        prev,
        frame,
        offset,
        value: BoneKey {
            frame,
            translation,
            rotation,
            interpolation,
        },
    })
}
fn read_morph(r: &mut Cursor<'_>, indexed: bool) -> Result<Record<MorphKey>> {
    let (frame, prev) = base(r, indexed)?;
    let offset = r.pos;
    let weight = r.float()?;
    r.skip(1)?;
    Ok(Record {
        prev,
        frame,
        offset,
        value: MorphKey { frame, weight },
    })
}
fn owner<T>(records: &[Record<T>], initial: usize, index: usize) -> Result<usize> {
    let mut i = index;
    for _ in 0..=records.len() {
        ensure!(i < records.len(), "PMM keyframe link out of range");
        if i < initial {
            return Ok(i);
        }
        i = records[i].prev;
    }
    bail!("cyclic PMM keyframe links")
}

fn same_bone(a: &BoneKey, b: &BoneKey) -> bool {
    a.frame == b.frame && same_bone_value(a, b)
}
fn same_bone_value(a: &BoneKey, b: &BoneKey) -> bool {
    a.translation == b.translation && a.rotation == b.rotation && a.interpolation == b.interpolation
}
fn same_morph(a: &MorphKey, b: &MorphKey) -> bool {
    a.frame == b.frame && a.weight == b.weight
}
fn same_motion(a: &Motion, b: &Motion) -> bool {
    a.bones.len() == b.bones.len()
        && a.morphs.len() == b.morphs.len()
        && a.bones.iter().all(|(name, keys)| {
            b.bones.get(name).is_some_and(|other| {
                keys.len() == other.len() && keys.iter().zip(other).all(|(a, b)| same_bone(a, b))
            })
        })
        && a.morphs.iter().all(|(name, keys)| {
            b.morphs.get(name).is_some_and(|other| {
                keys.len() == other.len() && keys.iter().zip(other).all(|(a, b)| same_morph(a, b))
            })
        })
}
/// Encode PMM's initial-per-track records followed by indexed additional records.
fn encode_tracks(
    tracks: &[Vec<(u32, Vec<u8>)>],
    value_offset: usize,
) -> Result<(Vec<u8>, Vec<Vec<usize>>)> {
    let extra: usize = tracks.iter().map(|t| t.len() - 1).sum();
    ensure!(extra <= 5_000_000, "too many PMM keyframes");
    let mut first_extra = Vec::with_capacity(tracks.len());
    let mut next_index = tracks.len();
    for track in tracks {
        first_extra.push(next_index);
        next_index += track.len() - 1;
    }
    let mut out = Vec::new();
    let mut offsets = vec![Vec::new(); tracks.len()];
    for (track, entries) in tracks.iter().enumerate() {
        out.extend(entries[0].0.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        let next = if entries.len() > 1 {
            first_extra[track] as u32
        } else {
            0
        };
        out.extend(next.to_le_bytes());
        offsets[track].push(out.len() + value_offset);
        out.extend(&entries[0].1);
    }
    // The count is between the initial and additional records.
    out.extend((extra as u32).to_le_bytes());
    for (track, entries) in tracks.iter().enumerate() {
        for key in 1..entries.len() {
            let index = first_extra[track] + key - 1;
            out.extend((index as u32).to_le_bytes());
            out.extend(entries[key].0.to_le_bytes());
            let prev = if key == 1 { track } else { index - 1 };
            let next = if key + 1 == entries.len() {
                0
            } else {
                index + 1
            };
            out.extend((prev as u32).to_le_bytes());
            out.extend((next as u32).to_le_bytes());
            offsets[track].push(out.len() + value_offset);
            out.extend(&entries[key].1);
        }
    }
    Ok((out, offsets))
}
fn shift_model_offsets(m: &mut PmmModel, boundary: usize, delta: isize) {
    let shift = |n: &mut usize| {
        if *n >= boundary {
            *n = n
                .checked_add_signed(delta)
                .expect("valid PMM splice offset");
        }
    };
    shift(&mut m.animation_range.start);
    shift(&mut m.animation_range.end);
    shift(&mut m.last_frame_offset);
    shift(&mut m.path_offset);
    for offsets in m
        .bone_offsets
        .values_mut()
        .chain(m.morph_offsets.values_mut())
    {
        for offset in offsets {
            shift(offset);
        }
    }
    for track in &mut m.bone_tracks {
        for (_, offset) in track {
            shift(offset);
        }
    }
    for track in &mut m.morph_tracks {
        for (_, offset) in track {
            shift(offset);
        }
    }
}

impl PmmProject {
    /// Read the camera/light sections immediately following the model blocks.
    /// Kept optional so callers can preserve projects with unsupported/truncated tails.
    pub fn scene_tracks(&self) -> Result<PmmSceneTracks> {
        let mut r = Cursor {
            bytes: &self.bytes,
            pos: self.scene_offset,
        };
        let camera_start = r.pos;
        let mut camera_records = vec![read_camera(&mut r, false, self.version)?];
        let count = r.count(if self.version == 2 { 82 } else { 74 })?;
        for _ in 0..count {
            camera_records.push(read_camera(&mut r, true, self.version)?);
        }
        let camera_end = r.pos;
        let current_camera = PmmCameraState {
            look_at: r.vec3()?,
            position: r.vec3()?,
            angle: r.vec3()?,
            perspective: r.byte()? == 0,
            fov: f32::from_le_bytes(self.bytes[42..46].try_into().unwrap()),
        };
        let light_start = r.pos;
        let mut light_records = vec![read_light(&mut r, false)?];
        let count = r.count(41)?;
        for _ in 0..count {
            light_records.push(read_light(&mut r, true)?);
        }
        let light_end = r.pos;
        let current_light = PmmLightKey {
            frame: 0,
            color: r.vec3()?,
            direction: r.vec3()?,
        };
        let mut camera = Vec::new();
        let mut light = Vec::new();
        let mut camera_offsets = BTreeMap::new();
        let mut light_offsets = BTreeMap::new();
        for (i, key) in camera_records.iter().enumerate() {
            owner(&camera_records, 1, i)?;
            ensure!(
                camera_offsets.insert(key.frame, key.offset).is_none(),
                "duplicate PMM camera frame {}",
                key.frame
            );
            camera.push(key.value.clone());
        }
        for (i, key) in light_records.iter().enumerate() {
            owner(&light_records, 1, i)?;
            ensure!(
                light_offsets.insert(key.frame, key.offset).is_none(),
                "duplicate PMM light frame {}",
                key.frame
            );
            light.push(key.value.clone());
        }
        camera.sort_by_key(|key| key.frame);
        light.sort_by_key(|key| key.frame);
        Ok(PmmSceneTracks {
            camera,
            light,
            current_camera,
            current_light,
            camera_range: camera_start..camera_end,
            light_range: light_start..light_end,
            camera_offsets,
            light_offsets,
        })
    }
    pub fn camera_at(&self, frame: f32) -> Result<Option<PmmCameraKey>> {
        Ok(self.scene_tracks()?.camera_at(frame))
    }
    pub fn light_at(&self, frame: f32) -> Result<Option<PmmLightKey>> {
        Ok(self.scene_tracks()?.light_at(frame))
    }
    /// Insert or replace a camera key at its frame, preserving existing selection flags.
    pub fn set_camera_key(&mut self, key: PmmCameraKey) -> Result<()> {
        let mut keys = self.scene_tracks()?.camera;
        if let Some(existing) = keys.iter_mut().find(|existing| existing.frame == key.frame) {
            *existing = key;
        } else {
            keys.push(key);
        }
        self.set_camera_keys(keys)
    }
    /// Replace camera animation; omitted frame zero becomes the default camera.
    pub fn set_camera_keys(&mut self, mut keys: Vec<PmmCameraKey>) -> Result<()> {
        let scene = self.scene_tracks()?;
        keys.sort_by_key(|key| key.frame);
        if keys == scene.camera {
            return Ok(());
        }
        ensure!(keys.len() <= 5_000_000, "too many PMM camera keys");
        ensure!(
            keys.windows(2).all(|p| p[0].frame != p[1].frame),
            "duplicate PMM camera frames"
        );
        if keys.first().is_none_or(|key| key.frame != 0) {
            keys.insert(0, PmmCameraKey::default());
        }
        let mut records = Vec::new();
        let payload_size = if self.version == 2 { 66 } else { 58 };
        for key in keys {
            ensure!(
                key.frame <= i32::MAX as u32
                    && key.distance.is_finite()
                    && key.look_at.is_finite()
                    && key.angle.is_finite()
                    && key.fov.is_finite()
                    && (0.0..=180.0).contains(&key.fov),
                "invalid PMM camera key"
            );
            ensure!(
                self.version == 2 || (key.parent_model == -1 && key.parent_bone == -1),
                "PMM v1 cannot store camera model/bone attachments"
            );
            let existing = scene.camera_offsets.get(&key.frame);
            let selected = existing.map_or(0, |offset| self.bytes[offset + payload_size - 1]);
            let mut payload = Vec::with_capacity(payload_size);
            for value in [
                key.distance,
                key.look_at.x,
                key.look_at.y,
                key.look_at.z,
                key.angle.x,
                key.angle.y,
                key.angle.z,
            ] {
                payload.extend(value.to_le_bytes());
            }
            if self.version == 2 {
                payload.extend(key.parent_model.to_le_bytes());
                payload.extend(key.parent_bone.to_le_bytes());
            }
            for channel in key.interpolation {
                payload.extend(channel);
            }
            payload.push(u8::from(!key.perspective));
            payload.extend((key.fov.round() as i32).to_le_bytes());
            payload.push(selected);
            records.push((key.frame, payload));
        }
        let (encoded, _) = encode_tracks(&[records], 0)?;
        self.bytes.splice(scene.camera_range, encoded);
        Ok(())
    }
    /// Insert or replace a light key at its frame.
    pub fn set_light_key(&mut self, key: PmmLightKey) -> Result<()> {
        let mut keys = self.scene_tracks()?.light;
        if let Some(existing) = keys.iter_mut().find(|existing| existing.frame == key.frame) {
            *existing = key;
        } else {
            keys.push(key);
        }
        self.set_light_keys(keys)
    }
    /// Replace light animation; omitted frame zero becomes the default light.
    pub fn set_light_keys(&mut self, mut keys: Vec<PmmLightKey>) -> Result<()> {
        let scene = self.scene_tracks()?;
        keys.sort_by_key(|key| key.frame);
        if keys == scene.light {
            return Ok(());
        }
        ensure!(keys.len() <= 5_000_000, "too many PMM light keys");
        ensure!(
            keys.windows(2).all(|p| p[0].frame != p[1].frame),
            "duplicate PMM light frames"
        );
        if keys.first().is_none_or(|key| key.frame != 0) {
            keys.insert(0, PmmLightKey::default());
        }
        let mut records = Vec::new();
        for key in keys {
            ensure!(
                key.frame <= i32::MAX as u32 && key.color.is_finite() && key.direction.is_finite(),
                "invalid PMM light key"
            );
            let selected = scene
                .light_offsets
                .get(&key.frame)
                .map_or(0, |offset| self.bytes[offset + 24]);
            let mut payload = Vec::with_capacity(25);
            for value in [
                key.color.x,
                key.color.y,
                key.color.z,
                key.direction.x,
                key.direction.y,
                key.direction.z,
            ] {
                payload.extend(value.to_le_bytes());
            }
            payload.push(selected);
            records.push((key.frame, payload));
        }
        let (encoded, _) = encode_tracks(&[records], 0)?;
        self.bytes.splice(scene.light_range, encoded);
        Ok(())
    }

    /// Version 2 embeds bone/morph names; version 1 requires `parse_with_resolver`.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Self::parse_with_resolver(bytes, |_| {
            bail!("PMM v1 requires its referenced PMD models")
        })
    }
    /// The resolver loads a referenced model; v1 needs its skeleton to locate records.
    pub fn parse_with_resolver<F>(bytes: &[u8], mut resolver: F) -> Result<Self>
    where
        F: FnMut(&str) -> Result<Model>,
    {
        let mut r = Cursor { bytes, pos: 0 };
        let signature = r.take(30)?;
        let version = if signature.starts_with(b"Polygon Movie maker 0002") {
            2
        } else if signature.starts_with(b"Polygon Movie maker 0001") {
            1
        } else {
            bail!("unsupported PMM signature");
        };
        r.skip(16 + if version == 2 { 7 } else { 6 })?;
        r.byte()?;
        let count = r.byte()? as usize;
        if version == 1 {
            r.skip(count * 20)?;
        }
        let mut models = Vec::with_capacity(count);
        for _ in 0..count {
            let model = Self::read_model(&mut r, version, &mut resolver)
                .with_context(|| format!("PMM model {} at byte {}", models.len(), r.pos))?;
            models.push(model);
        }
        // Retain remaining sections verbatim. They are not interpreted or validated here.
        Ok(Self {
            version,
            models,
            bytes: bytes.to_vec(),
            scene_offset: r.pos,
        })
    }
    fn read_model<F>(r: &mut Cursor<'_>, version: u8, resolver: &mut F) -> Result<PmmModel>
    where
        F: FnMut(&str) -> Result<Model>,
    {
        r.byte()?;
        let name = if version == 2 {
            let name = r.variable()?;
            r.variable()?;
            name
        } else {
            r.text(20)?
        };
        let path_offset = r.pos;
        let path = r.text(256)?;
        let (bones, morphs, constraints, outside) = if version == 2 {
            r.byte()?;
            let bones = r.names()?;
            let morphs = r.names()?;
            let constraints = r.count(4)?;
            r.skip(constraints * 4)?;
            let outside = r.count(4)?;
            r.skip(outside * 4)?;
            (bones, morphs, constraints, outside)
        } else {
            let model = resolver(&path).with_context(|| format!("load PMM model {path}"))?;
            (
                model.bones.iter().map(|b| b.name.clone()).collect(),
                model.morphs.iter().map(|m| m.name.clone()).collect(),
                model.ik.len(),
                0,
            )
        };
        r.skip(22)?; // draw order, visibility, selected bone and four selected morph indices
        let expansion = r.byte()? as usize;
        r.skip(expansion + 4)?;
        let last_frame_offset = r.pos;
        r.skip(4)?;
        let animation_start = r.pos;
        let mut bone_records = Vec::with_capacity(bones.len());
        for _ in &bones {
            bone_records.push(read_bone(r, false, version)?);
        }
        let extra = r.count(if version == 2 { 62 } else { 61 })?;
        for _ in 0..extra {
            bone_records.push(read_bone(r, true, version)?);
        }
        let mut morph_records = Vec::with_capacity(morphs.len());
        for _ in &morphs {
            morph_records.push(read_morph(r, false)?);
        }
        let extra = r.count(21)?;
        for _ in 0..extra {
            morph_records.push(read_morph(r, true)?);
        }
        let animation_end = r.pos;
        let model_key_size = 14 + constraints + outside * 8;
        let (mut other_last_frame, _) = base(r, false)?;
        r.skip(model_key_size - 12)?;
        let n = r.count(model_key_size + 4)?;
        for _ in 0..n {
            let (frame, _) = base(r, true)?;
            other_last_frame = other_last_frame.max(frame);
            r.skip(model_key_size - 12)?;
        }
        r.skip(bones.len() * if version == 2 { 31 } else { 34 })?;
        r.skip(morphs.len() * 4 + constraints + outside * 16)?;
        if version == 2 {
            r.skip(7)?;
        }
        let mut motion = Motion {
            model_name: name.clone(),
            ..Motion::default()
        };
        let mut bone_offsets: BTreeMap<_, Vec<usize>> = BTreeMap::new();
        let mut morph_offsets: BTreeMap<_, Vec<usize>> = BTreeMap::new();
        let mut bone_tracks = vec![Vec::new(); bones.len()];
        let mut morph_tracks = vec![Vec::new(); morphs.len()];
        for (i, record) in bone_records.iter().enumerate() {
            let track = owner(&bone_records, bones.len(), i)?;
            bone_tracks[track].push((record.value.clone(), record.offset));
            let name = &bones[track];
            motion.last_frame = motion.last_frame.max(record.frame);
            bone_offsets
                .entry((name.clone(), record.frame))
                .or_default()
                .push(record.offset);
            motion
                .bones
                .entry(name.clone())
                .or_default()
                .push(record.value.clone());
        }
        for (i, record) in morph_records.iter().enumerate() {
            let track = owner(&morph_records, morphs.len(), i)?;
            morph_tracks[track].push((record.value.clone(), record.offset));
            let name = &morphs[track];
            motion.last_frame = motion.last_frame.max(record.frame);
            morph_offsets
                .entry((name.clone(), record.frame))
                .or_default()
                .push(record.offset);
            motion
                .morphs
                .entry(name.clone())
                .or_default()
                .push(record.value.clone());
        }
        for keys in motion.bones.values_mut() {
            keys.sort_by_key(|k| k.frame);
        }
        for keys in motion.morphs.values_mut() {
            keys.sort_by_key(|k| k.frame);
        }
        Ok(PmmModel {
            name,
            path,
            path_offset,
            motion,
            bone_offsets,
            morph_offsets,
            bone_names: bones,
            morph_names: morphs,
            bone_tracks,
            morph_tracks,
            animation_range: animation_start..animation_end,
            last_frame_offset,
            other_last_frame,
        })
    }
    /// Replace the fixed Shift-JIS model path without moving or touching other records.
    /// PMM reserves 256 bytes, including a required NUL terminator.
    pub fn set_model_path(&mut self, model: usize, path: &str) -> Result<()> {
        let m = self
            .models
            .get_mut(model)
            .context("PMM model index out of range")?;
        ensure!(!path.is_empty(), "PMM model path cannot be empty");
        ensure!(
            !path.chars().any(char::is_control),
            "PMM model path contains a NUL or control character"
        );
        let (encoded, _, had_errors) = SHIFT_JIS.encode(path);
        ensure!(
            !had_errors,
            "PMM model path cannot be represented in Shift-JIS"
        );
        ensure!(
            SHIFT_JIS.decode_without_bom_handling(&encoded).0 == path,
            "PMM model path changes when encoded as Shift-JIS"
        );
        ensure!(
            encoded.len() < 256,
            "PMM model path exceeds 255 Shift-JIS bytes"
        );
        if m.path == path {
            return Ok(());
        }
        let field = &mut self.bytes[m.path_offset..m.path_offset + 256];
        field.fill(0);
        field[..encoded.len()].copy_from_slice(&encoded);
        m.path = path.to_owned();
        Ok(())
    }
    /// Atomically replace bone/morph animation, including inserting/deleting/retiming keys.
    /// PMM requires a frame-zero key per track; omitted initial keys become identity/zero.
    /// Tracks with duplicate names can be retained unchanged but cannot be edited by name.
    pub fn set_motion(&mut self, model: usize, mut motion: Motion) -> Result<()> {
        ensure!(motion.properties.is_empty() && motion.trailing_data.is_empty() && !motion.has_scene_tracks(),"PMM model writer accepts bone/morph tracks only; export VMD to retain unsupported tracks and extensions");
        let m = self
            .models
            .get(model)
            .context("PMM model index out of range")?;
        if same_motion(&m.motion, &motion) {
            return Ok(());
        }
        ensure!(
            motion.bones.keys().all(|n| m.bone_names.contains(n)),
            "motion contains a bone absent from the PMM model"
        );
        ensure!(
            motion.morphs.keys().all(|n| m.morph_names.contains(n)),
            "motion contains a morph absent from the PMM model"
        );
        let mut bone_keys = Vec::new();
        let mut morph_keys = Vec::new();
        let mut bones_encoded = Vec::new();
        let mut morphs_encoded = Vec::new();
        for (track, name) in m.bone_names.iter().enumerate() {
            let duplicate = m.bone_names.iter().filter(|n| *n == name).count() > 1;
            let mut keys = if duplicate {
                let old = &m.motion.bones[name];
                let new = motion
                    .bones
                    .get(name)
                    .context("cannot delete an ambiguous duplicate-name bone track")?;
                ensure!(
                    old.len() == new.len() && old.iter().zip(new).all(|(a, b)| same_bone(a, b)),
                    "cannot edit ambiguous duplicate-name bone track {name}"
                );
                m.bone_tracks[track]
                    .iter()
                    .map(|(k, _)| k.clone())
                    .collect::<Vec<_>>()
            } else {
                motion.bones.remove(name).unwrap_or_default()
            };
            keys.sort_by_key(|k| k.frame);
            ensure!(
                keys.windows(2).all(|p| p[0].frame != p[1].frame),
                "duplicate frames in bone track {name}"
            );
            if keys.first().is_none_or(|k| k.frame != 0) {
                let mut interpolation = [0; 64];
                for channel in 0..4 {
                    for (point, value) in [20, 20, 107, 107].into_iter().enumerate() {
                        interpolation[point * 4 + channel] = value;
                    }
                }
                keys.insert(
                    0,
                    BoneKey {
                        frame: 0,
                        translation: Vec3::ZERO,
                        rotation: Quat::IDENTITY,
                        interpolation,
                    },
                );
            }
            let mut encoded = Vec::new();
            for key in &keys {
                ensure!(
                    key.frame <= i32::MAX as u32
                        && key.translation.is_finite()
                        && key.rotation.is_finite()
                        && key.rotation.length_squared().is_finite()
                        && key.rotation.length_squared() > 1e-12,
                    "invalid bone key in {name}"
                );
                let existing = m.bone_tracks[track]
                    .iter()
                    .find(|(old, _)| old.frame == key.frame)
                    .or_else(|| {
                        let mut candidates = m.bone_tracks[track].iter().filter(|(old, _)| {
                            !keys.iter().any(|new| new.frame == old.frame)
                                && same_bone_value(old, key)
                        });
                        let candidate = candidates.next();
                        if candidates.next().is_none() {
                            candidate
                        } else {
                            None
                        }
                    });
                let mut payload = if let Some((_, offset)) = existing {
                    self.bytes[offset - 16..offset + 28 + if self.version == 2 { 2 } else { 1 }]
                        .to_vec()
                } else {
                    vec![0; 44 + if self.version == 2 { 2 } else { 1 }]
                };
                // Keep original float bytes and flags when the key itself did not change.
                if existing.is_none_or(|(old, _)| !same_bone_value(old, key)) {
                    for channel in 0..4 {
                        for point in 0..4 {
                            payload[channel * 4 + point] = key.interpolation[point * 4 + channel];
                        }
                    }
                    let q = key.rotation.normalize();
                    for (i, v) in [
                        key.translation.x,
                        key.translation.y,
                        key.translation.z,
                        q.x,
                        q.y,
                        q.z,
                        q.w,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        payload[16 + i * 4..20 + i * 4].copy_from_slice(&v.to_le_bytes());
                    }
                }
                encoded.push((key.frame, payload));
            }
            bone_keys.push(keys);
            bones_encoded.push(encoded);
        }
        for (track, name) in m.morph_names.iter().enumerate() {
            let duplicate = m.morph_names.iter().filter(|n| *n == name).count() > 1;
            let mut keys = if duplicate {
                let old = &m.motion.morphs[name];
                let new = motion
                    .morphs
                    .get(name)
                    .context("cannot delete an ambiguous duplicate-name morph track")?;
                ensure!(
                    old.len() == new.len() && old.iter().zip(new).all(|(a, b)| same_morph(a, b)),
                    "cannot edit ambiguous duplicate-name morph track {name}"
                );
                m.morph_tracks[track]
                    .iter()
                    .map(|(k, _)| k.clone())
                    .collect::<Vec<_>>()
            } else {
                motion.morphs.remove(name).unwrap_or_default()
            };
            keys.sort_by_key(|k| k.frame);
            ensure!(
                keys.windows(2).all(|p| p[0].frame != p[1].frame),
                "duplicate frames in morph track {name}"
            );
            if keys.first().is_none_or(|k| k.frame != 0) {
                keys.insert(
                    0,
                    MorphKey {
                        frame: 0,
                        weight: 0.,
                    },
                );
            }
            let mut encoded = Vec::new();
            for key in &keys {
                ensure!(
                    key.frame <= i32::MAX as u32 && key.weight.is_finite(),
                    "invalid morph key in {name}"
                );
                let existing = m.morph_tracks[track]
                    .iter()
                    .find(|(old, _)| old.frame == key.frame)
                    .or_else(|| {
                        let mut candidates = m.morph_tracks[track].iter().filter(|(old, _)| {
                            !keys.iter().any(|new| new.frame == old.frame)
                                && old.weight == key.weight
                        });
                        let candidate = candidates.next();
                        if candidates.next().is_none() {
                            candidate
                        } else {
                            None
                        }
                    });
                let mut payload = existing
                    .map(|(_, offset)| self.bytes[*offset..offset + 5].to_vec())
                    .unwrap_or_else(|| vec![0; 5]);
                payload[..4].copy_from_slice(&key.weight.to_le_bytes());
                encoded.push((key.frame, payload));
            }
            morph_keys.push(keys);
            morphs_encoded.push(encoded);
        }
        let (mut encoded, bone_offsets) = encode_tracks(&bones_encoded, 16)?;
        let morph_start = encoded.len();
        let (morph_bytes, morph_offsets) = encode_tracks(&morphs_encoded, 0)?;
        encoded.extend(morph_bytes);
        let range = m.animation_range.clone();
        let delta = encoded.len() as isize - range.len() as isize;
        let mut normalized = Motion {
            model_name: m.name.clone(),
            ..Motion::default()
        };
        for (name, keys) in m.bone_names.iter().zip(&bone_keys) {
            normalized
                .bones
                .entry(name.clone())
                .or_default()
                .extend(keys.iter().cloned());
        }
        for (name, keys) in m.morph_names.iter().zip(&morph_keys) {
            normalized
                .morphs
                .entry(name.clone())
                .or_default()
                .extend(keys.iter().cloned());
        }
        for keys in normalized.bones.values_mut() {
            keys.sort_by_key(|k| k.frame);
        }
        for keys in normalized.morphs.values_mut() {
            keys.sort_by_key(|k| k.frame);
        }
        normalized.last_frame = normalized
            .bones
            .values()
            .flatten()
            .map(|k| k.frame)
            .chain(normalized.morphs.values().flatten().map(|k| k.frame))
            .max()
            .unwrap_or(0);
        let last_frame = normalized.last_frame.max(m.other_last_frame);
        let last_frame_offset = m.last_frame_offset;
        // All validation precedes the first mutation.
        self.bytes.splice(range.clone(), encoded);
        self.scene_offset = self
            .scene_offset
            .checked_add_signed(delta)
            .expect("valid PMM model splice");
        self.bytes[last_frame_offset..last_frame_offset + 4]
            .copy_from_slice(&last_frame.to_le_bytes());
        for (index, other) in self.models.iter_mut().enumerate() {
            if index != model {
                shift_model_offsets(other, range.end, delta);
            }
        }
        let m = &mut self.models[model];
        m.animation_range.end = range.end.checked_add_signed(delta).unwrap();
        m.bone_offsets.clear();
        m.morph_offsets.clear();
        m.bone_tracks.clear();
        m.morph_tracks.clear();
        for (track, keys) in bone_keys.into_iter().enumerate() {
            let entries: Vec<_> = keys
                .into_iter()
                .zip(&bone_offsets[track])
                .map(|(k, &offset)| (k, range.start + offset))
                .collect();
            for (key, offset) in &entries {
                m.bone_offsets
                    .entry((m.bone_names[track].clone(), key.frame))
                    .or_default()
                    .push(*offset);
            }
            m.bone_tracks.push(entries);
        }
        for (track, keys) in morph_keys.into_iter().enumerate() {
            let entries: Vec<_> = keys
                .into_iter()
                .zip(&morph_offsets[track])
                .map(|(k, &offset)| (k, range.start + morph_start + offset))
                .collect();
            for (key, offset) in &entries {
                m.morph_offsets
                    .entry((m.morph_names[track].clone(), key.frame))
                    .or_default()
                    .push(*offset);
            }
            m.morph_tracks.push(entries);
        }
        m.motion = normalized;
        Ok(())
    }

    /// A no-edit save is byte-identical, including all unknown PMM sections.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
    /// Patch an existing key, preserving topology, timing, interpolation and physics flags.
    pub fn set_bone_key(
        &mut self,
        model: usize,
        name: &str,
        frame: u32,
        translation: Vec3,
        rotation: Quat,
    ) -> Result<()> {
        ensure!(
            translation.is_finite()
                && rotation.is_finite()
                && rotation.length_squared().is_finite()
                && rotation.length_squared() > 1e-12,
            "invalid bone transform"
        );
        let m = self
            .models
            .get_mut(model)
            .context("PMM model index out of range")?;
        let offsets = m
            .bone_offsets
            .get(&(name.to_owned(), frame))
            .context("PMM key does not exist; inserting keys is not supported")?;
        ensure!(
            offsets.len() == 1,
            "ambiguous PMM bone name and frame; editing duplicate names is not supported"
        );
        let offset = offsets[0];
        let key = m
            .motion
            .bones
            .get_mut(name)
            .and_then(|keys| keys.iter_mut().find(|k| k.frame == frame))
            .context("PMM motion view was changed outside the key setters")?;
        let rotation = rotation.normalize();
        let values = [
            translation.x,
            translation.y,
            translation.z,
            rotation.x,
            rotation.y,
            rotation.z,
            rotation.w,
        ];
        for (i, value) in values.into_iter().enumerate() {
            self.bytes[offset + i * 4..offset + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        key.translation = translation;
        key.rotation = rotation;
        for track in &mut m.bone_tracks {
            for (key, saved_offset) in track {
                if *saved_offset == offset {
                    key.translation = translation;
                    key.rotation = rotation;
                }
            }
        }
        Ok(())
    }
    pub fn set_morph_key(
        &mut self,
        model: usize,
        name: &str,
        frame: u32,
        weight: f32,
    ) -> Result<()> {
        ensure!(weight.is_finite(), "invalid morph weight");
        let m = self
            .models
            .get_mut(model)
            .context("PMM model index out of range")?;
        let offsets = m
            .morph_offsets
            .get(&(name.to_owned(), frame))
            .context("PMM key does not exist; inserting keys is not supported")?;
        ensure!(
            offsets.len() == 1,
            "ambiguous PMM morph name and frame; editing duplicate names is not supported"
        );
        let offset = offsets[0];
        let key = m
            .motion
            .morphs
            .get_mut(name)
            .and_then(|keys| keys.iter_mut().find(|k| k.frame == frame))
            .context("PMM motion view was changed outside the key setters")?;
        self.bytes[offset..offset + 4].copy_from_slice(&weight.to_le_bytes());
        key.weight = weight;
        for track in &mut m.morph_tracks {
            for (key, saved_offset) in track {
                if *saved_offset == offset {
                    key.weight = weight;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    fn u32b(out: &mut Vec<u8>, n: u32) {
        out.extend(n.to_le_bytes());
    }
    fn float(out: &mut Vec<u8>, n: f32) {
        out.extend(n.to_le_bytes());
    }
    fn text(out: &mut Vec<u8>, s: &str) {
        out.push(s.len() as u8);
        out.extend(s.as_bytes());
    }
    fn fixture() -> Vec<u8> {
        let mut b = b"Polygon Movie maker 0002".to_vec();
        b.resize(30 + 23, 0);
        b.extend([0, 1, 0]); // selection, count, model index
        text(&mut b, "Model");
        text(&mut b, "Model");
        let start = b.len();
        b.extend(b"model.pmd");
        b.resize(start + 256, 0);
        b.push(0);
        u32b(&mut b, 1);
        text(&mut b, "root");
        u32b(&mut b, 1);
        text(&mut b, "smile");
        u32b(&mut b, 0);
        u32b(&mut b, 0); // constraints, outside parents
        b.extend([0; 22]);
        b.push(0);
        b.extend([0; 8]);
        b.extend([0; 12]);
        b.extend([20, 20, 107, 107].repeat(4));
        for value in [0., 0., 0., 0., 0., 0., 1.] {
            float(&mut b, value);
        }
        b.extend([0, 0]);
        u32b(&mut b, 0); // extra bone keys
        b.extend([0; 12]);
        float(&mut b, 0.);
        b.push(0);
        u32b(&mut b, 0);
        b.extend([0; 14]);
        u32b(&mut b, 0); // initial and extra model keys
        b.extend([0; 31 + 4 + 7]);
        b.extend(b"opaque camera and accessory records");
        b
    }
    fn fixture_with_scene() -> Vec<u8> {
        let mut bytes = fixture();
        bytes.truncate(bytes.len() - b"opaque camera and accessory records".len());
        let mut cameras = Vec::new();
        for frame in [0, 30] {
            let mut payload = Vec::new();
            for value in [
                -45. - frame as f32,
                frame as f32,
                10.,
                0.,
                0.,
                frame as f32 / 30.,
                0.,
            ] {
                float(&mut payload, value);
            }
            payload.extend((-1i32).to_le_bytes());
            payload.extend((-1i32).to_le_bytes());
            payload.extend([20, 20, 107, 107].repeat(6));
            payload.push(0);
            u32b(&mut payload, 45 + frame);
            payload.push(1);
            cameras.push((frame, payload));
        }
        bytes.extend(encode_tracks(&[cameras], 0).unwrap().0);
        for value in [0., 10., 0., 0., 10., 45., 0., 0., 0.] {
            float(&mut bytes, value);
        }
        bytes.push(0);
        let mut lights = Vec::new();
        for frame in [0, 30] {
            let mut payload = Vec::new();
            for value in [frame as f32 / 30., 0.6, 0.6, -0.5, -1., -0.5] {
                float(&mut payload, value);
            }
            payload.push(1);
            lights.push((frame, payload));
        }
        bytes.extend(encode_tracks(&[lights], 0).unwrap().0);
        for value in [0.6, 0.6, 0.6, -0.5, -1., -0.5] {
            float(&mut bytes, value);
        }
        bytes.extend(b"opaque accessory and editor data");
        bytes
    }
    #[test]
    fn scene_parse_sample_rebuild_and_model_offset_changes() {
        let source = fixture_with_scene();
        let mut project = PmmProject::parse(&source).unwrap();
        let scene = project.scene_tracks().unwrap();
        assert_eq!(scene.camera.len(), 2);
        assert_eq!(scene.light.len(), 2);
        let camera = scene.camera_at(15.).unwrap();
        assert!((camera.look_at.x - 15.).abs() < 0.001);
        assert!((camera.fov - 60.).abs() < 0.001);
        assert!((camera.distance + 60.).abs() < 0.001);
        assert_eq!(scene.light_at(15.).unwrap().color.x, 0.5);
        assert_eq!(scene.camera_at(-1.).unwrap().frame, 0);
        assert_eq!(scene.camera_at(100.).unwrap().frame, 30);
        assert_eq!(project.to_bytes(), source);
        let suffix = source[scene.light_range.end..].to_vec();
        let mut camera = scene.camera[1].clone();
        camera.frame = 60;
        camera.perspective = false;
        project.set_camera_key(camera).unwrap();
        let mut light = scene.light[1].clone();
        light.frame = 70;
        light.direction = Vec3::NEG_Y;
        project.set_light_key(light).unwrap();
        let scene = project.scene_tracks().unwrap();
        assert_eq!(scene.camera.len(), 3);
        assert_eq!(scene.light.len(), 3);
        assert_eq!(
            &project.to_bytes()[scene.light_range.end..],
            suffix.as_slice()
        );
        assert!(!scene.camera_at(60.).unwrap().perspective);
        assert_eq!(scene.light_at(70.).unwrap().direction, Vec3::NEG_Y);
        let mut motion = project.models[0].motion.clone();
        let mut key = motion.bones["root"][0].clone();
        key.frame = 100;
        motion.bones.get_mut("root").unwrap().push(key);
        project.set_motion(0, motion).unwrap();
        let shifted = project.scene_tracks().unwrap();
        assert_eq!(shifted.camera, scene.camera);
        assert_eq!(shifted.light, scene.light);
        let mut cameras = shifted.camera;
        cameras.retain(|key| key.frame != 30);
        project.set_camera_keys(cameras).unwrap();
        let mut lights = shifted.light;
        lights.retain(|key| key.frame != 30);
        project.set_light_keys(lights).unwrap();
        let loaded = PmmProject::parse(&project.to_bytes()).unwrap();
        let scene = loaded.scene_tracks().unwrap();
        assert_eq!(scene.camera.len(), 2);
        assert_eq!(scene.light.len(), 2);
        assert_eq!(loaded.models[0].motion.bones["root"][1].frame, 100);
        assert_eq!(
            &project.to_bytes()[scene.light_range.end..],
            suffix.as_slice()
        );
        let before = project.to_bytes();
        let invalid = PmmCameraKey {
            distance: f32::NAN,
            ..Default::default()
        };
        assert!(project.set_camera_key(invalid).is_err());
        assert_eq!(project.to_bytes(), before);
    }

    #[test]
    fn model_path_patch_is_bounded_shift_jis_and_atomic() {
        let source = fixture();
        let mut project = PmmProject::parse(&source).unwrap();
        let path = "assets/モデル/ミク.pmd";
        project.set_model_path(0, path).unwrap();
        let changed = project.to_bytes();
        let loaded = PmmProject::parse(&changed).unwrap();
        assert_eq!(loaded.models[0].path, path);
        let offset = project.models[0].path_offset;
        for i in 0..source.len() {
            if !(offset..offset + 256).contains(&i) {
                assert_eq!(source[i], changed[i]);
            }
        }
        assert_eq!(changed[offset + 255], 0);
        let before = project.to_bytes();
        project.set_model_path(0, path).unwrap();
        assert_eq!(project.to_bytes(), before);
        for invalid in [
            String::new(),
            "file\0.pmd".to_owned(),
            "file\n.pmd".to_owned(),
            "model😀.pmd".to_owned(),
            "a".repeat(256),
            "ミ".repeat(128),
            "assets/¥.pmd".to_owned(),
        ] {
            assert!(
                project.set_model_path(0, &invalid).is_err(),
                "accepted invalid path {invalid:?}"
            );
            assert_eq!(project.to_bytes(), before);
            assert_eq!(project.models[0].path, path);
        }
        assert!(project.set_model_path(1, "valid.pmd").is_err());
        assert_eq!(project.to_bytes(), before);
        let longest = "a".repeat(255);
        project.set_model_path(0, &longest).unwrap();
        assert_eq!(
            PmmProject::parse(&project.to_bytes()).unwrap().models[0].path,
            longest
        );
        assert_eq!(project.to_bytes()[offset + 255], 0);
    }
    #[test]
    #[ignore = "requires downloaded MMD sample assets"]
    fn bundled_pmm_model_path_rebase_and_reopen() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("reference/original/UserFile");
        for file in [
            "sample.pmm",
            "sample(AllStar).pmm",
            "sample(follow_camera).pmm",
        ] {
            let source = std::fs::read(root.join(file)).unwrap();
            let mut project = PmmProject::parse_with_resolver(&source, |path| {
                let file = path.rsplit(['\\', '/']).next().unwrap();
                Model::parse(&std::fs::read(root.join("Model").join(file))?)
            })
            .unwrap();
            let scene = project.scene_tracks().unwrap();
            for i in 0..project.models.len() {
                let file = project.models[i].path.rsplit(['\\', '/']).next().unwrap();
                let path = root.join("Model").join(file).canonicalize().unwrap();
                project.set_model_path(i, path.to_str().unwrap()).unwrap();
            }
            let changed = project.to_bytes();
            for i in 0..source.len() {
                if !project
                    .models
                    .iter()
                    .any(|m| (m.path_offset..m.path_offset + 256).contains(&i))
                {
                    assert_eq!(source[i], changed[i]);
                }
            }
            let loaded = PmmProject::parse_with_resolver(&changed, |path| {
                ensure!(
                    Path::new(path).is_absolute(),
                    "expected rebased absolute asset path"
                );
                Model::parse(&std::fs::read(path)?)
            })
            .unwrap();
            for (a, b) in project.models.iter().zip(&loaded.models) {
                assert_eq!(a.path, b.path);
                assert!(same_motion(&a.motion, &b.motion));
            }
            let loaded_scene = loaded.scene_tracks().unwrap();
            assert_eq!(scene.camera, loaded_scene.camera);
            assert_eq!(scene.light, loaded_scene.light);
        }
    }

    #[test]
    fn lossless_no_edit_save_and_bounded_patch() {
        let bytes = fixture();
        let mut project = PmmProject::parse(&bytes).unwrap();
        assert_eq!(project.to_bytes(), bytes);
        assert_eq!(project.models[0].motion.bones["root"].len(), 1);
        project
            .set_bone_key(0, "root", 0, Vec3::new(1., 2., 3.), Quat::IDENTITY)
            .unwrap();
        project.set_morph_key(0, "smile", 0, 0.75).unwrap();
        let changed = project.to_bytes();
        let loaded = PmmProject::parse(&changed).unwrap();
        assert_eq!(
            loaded.models[0].motion.bones["root"][0].translation,
            Vec3::new(1., 2., 3.)
        );
        assert_eq!(loaded.models[0].motion.morphs["smile"][0].weight, 0.75);
        let bone = project.models[0].bone_offsets[&("root".to_owned(), 0)][0];
        let morph = project.models[0].morph_offsets[&("smile".to_owned(), 0)][0];
        for i in 0..bytes.len() {
            if !(bone..bone + 28).contains(&i) && !(morph..morph + 4).contains(&i) {
                assert_eq!(bytes[i], changed[i]);
            }
        }
        let before = project.to_bytes();
        assert!(project
            .set_bone_key(0, "root", 5, Vec3::ZERO, Quat::IDENTITY)
            .is_err());
        assert!(project.set_morph_key(0, "smile", 0, f32::NAN).is_err());
        assert_eq!(project.to_bytes(), before);
    }
    #[test]
    fn insert_delete_retime_and_preserve_other_sections() {
        let source = fixture();
        let original = PmmProject::parse(&source).unwrap();
        let original_range = original.models[0].animation_range.clone();
        let mut project = original.clone();
        let mut motion = project.models[0].motion.clone();
        let mut key = motion.bones["root"][0].clone();
        key.frame = 12;
        key.translation = Vec3::new(1., 2., 3.);
        motion.bones.get_mut("root").unwrap().push(key);
        motion.morphs.get_mut("smile").unwrap().push(MorphKey {
            frame: 7,
            weight: 0.6,
        });
        project.set_motion(0, motion).unwrap();
        let bytes = project.to_bytes();
        let reloaded = PmmProject::parse(&bytes).unwrap();
        assert_eq!(reloaded.models[0].motion.bones["root"][1].frame, 12);
        assert_eq!(reloaded.models[0].motion.morphs["smile"][1].weight, 0.6);
        assert_eq!(
            &source[original_range.end..],
            &bytes[project.models[0].animation_range.end..]
        );
        for i in 0..original_range.start {
            if !(project.models[0].last_frame_offset..project.models[0].last_frame_offset + 4)
                .contains(&i)
            {
                assert_eq!(source[i], bytes[i]);
            }
        }
        // Repeated splices and the old patch API use updated offsets.
        project
            .set_bone_key(0, "root", 12, Vec3::Y, Quat::IDENTITY)
            .unwrap();
        let mut motion = project.models[0].motion.clone();
        motion.bones.get_mut("root").unwrap()[1].frame = 20;
        motion
            .morphs
            .get_mut("smile")
            .unwrap()
            .retain(|k| k.frame != 7);
        project.set_motion(0, motion).unwrap();
        let reloaded = PmmProject::parse(&project.to_bytes()).unwrap();
        assert_eq!(reloaded.models[0].motion.bones["root"][1].frame, 20);
        assert_eq!(
            reloaded.models[0].motion.bones["root"][1].translation,
            Vec3::Y
        );
        assert_eq!(reloaded.models[0].motion.morphs["smile"].len(), 1);
        project.set_motion(0, Motion::default()).unwrap();
        let cleared = PmmProject::parse(&project.to_bytes()).unwrap();
        assert_eq!(cleared.models[0].motion.bones["root"].len(), 1);
        assert_eq!(cleared.models[0].motion.bones["root"][0].frame, 0);
        let before = project.to_bytes();
        let mut bad = project.models[0].motion.clone();
        bad.bones.get_mut("root").unwrap()[0].translation = Vec3::NAN;
        assert!(project.set_motion(0, bad).is_err());
        assert_eq!(before, project.to_bytes());
    }
    #[test]
    fn rebuilding_and_retiming_preserve_existing_record_flags() {
        let mut project = PmmProject::parse(&fixture()).unwrap();
        let mut motion = project.models[0].motion.clone();
        let mut key = motion.bones["root"][0].clone();
        key.frame = 10;
        key.translation = Vec3::X;
        motion.bones.get_mut("root").unwrap().push(key);
        motion.morphs.get_mut("smile").unwrap().push(MorphKey {
            frame: 10,
            weight: 0.5,
        });
        project.set_motion(0, motion).unwrap();
        let bone = project.models[0].bone_offsets[&("root".to_owned(), 10)][0];
        let morph = project.models[0].morph_offsets[&("smile".to_owned(), 10)][0];
        let mut bytes = project.to_bytes();
        bytes[bone + 28] = 1;
        bytes[bone + 29] = 1;
        bytes[morph + 4] = 1;
        let mut project = PmmProject::parse(&bytes).unwrap();
        let mut motion = project.models[0].motion.clone();
        motion.bones.get_mut("root").unwrap()[1].frame = 20;
        motion.morphs.get_mut("smile").unwrap()[1].frame = 20;
        project.set_motion(0, motion).unwrap();
        let bone = project.models[0].bone_offsets[&("root".to_owned(), 20)][0];
        let morph = project.models[0].morph_offsets[&("smile".to_owned(), 20)][0];
        let bytes = project.to_bytes();
        assert_eq!(&bytes[bone + 28..bone + 30], &[1, 1]);
        assert_eq!(bytes[morph + 4], 1);
        let unchanged = project.to_bytes();
        project
            .set_motion(0, project.models[0].motion.clone())
            .unwrap();
        assert_eq!(project.to_bytes(), unchanged);
    }

    #[test]
    fn resizing_one_model_keeps_other_model_editable() {
        let mut bytes = fixture();
        let tail_len = b"opaque camera and accessory records".len();
        let model_end = bytes.len() - tail_len;
        let model_bytes = bytes[55..model_end].to_vec();
        bytes.splice(model_end..model_end, model_bytes);
        bytes[54] = 2;
        let mut project = PmmProject::parse(&bytes).unwrap();
        let mut motion = project.models[0].motion.clone();
        let mut key = motion.bones["root"][0].clone();
        key.frame = 100;
        motion.bones.get_mut("root").unwrap().push(key);
        project.set_motion(0, motion).unwrap();
        project.set_model_path(1, "nested/second.pmd").unwrap();
        project.set_morph_key(1, "smile", 0, 0.8).unwrap();
        let mut motion = project.models[1].motion.clone();
        let mut key = motion.bones["root"][0].clone();
        key.frame = 50;
        motion.bones.get_mut("root").unwrap().push(key);
        project.set_motion(1, motion).unwrap();
        project.set_morph_key(0, "smile", 0, 0.4).unwrap();
        let loaded = PmmProject::parse(&project.to_bytes()).unwrap();
        assert_eq!(loaded.models[0].motion.bones["root"][1].frame, 100);
        assert_eq!(loaded.models[1].motion.bones["root"][1].frame, 50);
        assert_eq!(loaded.models[0].motion.morphs["smile"][0].weight, 0.4);
        assert_eq!(loaded.models[1].motion.morphs["smile"][0].weight, 0.8);
        assert_eq!(loaded.models[1].path, "nested/second.pmd");
    }

    #[test]
    fn truncated_input_and_bad_counts_are_errors() {
        let b = fixture();
        // The opaque tail is intentionally not parsed, so only test truncation in the model section.
        for length in 0..b.len() - b"opaque camera and accessory records".len() {
            assert!(PmmProject::parse(&b[..length]).is_err(), "length={length}");
        }
        let mut bad = b.clone();
        bad[30 + 23 + 3 + 6 + 6 + 256 + 1..][..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(PmmProject::parse(&bad).is_err());
    }
    #[test]
    fn linked_track_ownership_and_cycle_detection() {
        let rec = |prev| Record {
            prev,
            frame: 0,
            offset: 0,
            value: (),
        };
        let records = [rec(0), rec(0), rec(1), rec(2), rec(0)];
        assert_eq!(owner(&records, 2, 3).unwrap(), 1);
        assert_eq!(owner(&records, 2, 4).unwrap(), 0);
        assert!(owner(&[rec(0), rec(2), rec(1)], 1, 1).is_err());
        assert!(owner(&[rec(0), rec(10)], 1, 1).is_err());
    }
    /// Run with `cargo test bundled_pmm -- --ignored` after obtaining the original distribution.
    #[test]
    #[ignore = "requires downloaded MMD sample assets"]
    fn bundled_pmm_roundtrip_and_edit() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("reference/original/UserFile");
        for file in [
            "sample.pmm",
            "sample(AllStar).pmm",
            "sample(follow_camera).pmm",
        ] {
            let bytes = std::fs::read(root.join(file)).unwrap();
            let load = |path: &str| {
                let file = path.rsplit(['\\', '/']).next().unwrap();
                Model::parse(&std::fs::read(root.join("Model").join(file))?)
            };
            let mut project = PmmProject::parse_with_resolver(&bytes, load).unwrap();
            assert!(!project.models.is_empty());
            assert_eq!(project.to_bytes(), bytes);
            let scene = project.scene_tracks().unwrap();
            assert!(!scene.camera.is_empty());
            assert!(!scene.light.is_empty());
            assert!(scene.camera_at(15.).unwrap().look_at.is_finite());
            assert!(scene.light_at(15.).unwrap().direction.is_finite());
            let camera_snapshot = scene.camera.clone();
            let light_snapshot = scene.light.clone();

            let key = project.models[0].motion.bones.iter().next().unwrap();
            let name = key.0.clone();
            let frame = key.1[0].frame;
            project
                .set_bone_key(0, &name, frame, Vec3::new(1., 2., 3.), Quat::IDENTITY)
                .unwrap();
            let edited = PmmProject::parse_with_resolver(&project.to_bytes(), load).unwrap();
            assert_eq!(
                edited.models[0].motion.bones[&name][0].translation,
                Vec3::new(1., 2., 3.)
            );
            // Resize every model, including models with unrelated duplicate-name tracks.
            for i in 0..project.models.len() {
                let mut motion = project.models[i].motion.clone();
                let track_name = project.models[i]
                    .bone_names
                    .iter()
                    .find(|name| {
                        project.models[i]
                            .bone_names
                            .iter()
                            .filter(|other| *other == *name)
                            .count()
                            == 1
                    })
                    .unwrap()
                    .clone();
                let mut key = motion.bones[&track_name][0].clone();
                key.frame = motion.last_frame + 10;
                key.translation = Vec3::X;
                motion.bones.get_mut(&track_name).unwrap().push(key);
                if let Some((_, keys)) = motion.morphs.iter_mut().find(|(name, _)| {
                    project.models[i]
                        .morph_names
                        .iter()
                        .filter(|other| *other == *name)
                        .count()
                        == 1
                }) {
                    keys.push(MorphKey {
                        frame: motion.last_frame + 11,
                        weight: 0.9,
                    });
                }
                project.set_motion(i, motion).unwrap();
            }
            let grown = PmmProject::parse_with_resolver(&project.to_bytes(), load).unwrap();
            assert_eq!(grown.models.len(), project.models.len());
            for i in 0..project.models.len() {
                assert!(same_motion(
                    &grown.models[i].motion,
                    &project.models[i].motion
                ));
                let mut motion = project.models[i].motion.clone();
                for keys in motion.bones.values_mut() {
                    keys.retain(|key| key.frame == 0);
                }
                for keys in motion.morphs.values_mut() {
                    keys.retain(|key| key.frame == 0);
                }
                // Duplicate-name tracks are retained as the API cannot distinguish them by name.
                for name in &project.models[i].bone_names {
                    if project.models[i]
                        .bone_names
                        .iter()
                        .filter(|other| *other == name)
                        .count()
                        > 1
                    {
                        motion
                            .bones
                            .insert(name.clone(), project.models[i].motion.bones[name].clone());
                    }
                }
                for name in &project.models[i].morph_names {
                    if project.models[i]
                        .morph_names
                        .iter()
                        .filter(|other| *other == name)
                        .count()
                        > 1
                    {
                        motion
                            .morphs
                            .insert(name.clone(), project.models[i].motion.morphs[name].clone());
                    }
                }
                project.set_motion(i, motion).unwrap();
            }
            let shrunk = PmmProject::parse_with_resolver(&project.to_bytes(), load).unwrap();
            let scene = shrunk.scene_tracks().unwrap();
            assert_eq!(scene.camera, camera_snapshot);
            assert_eq!(scene.light, light_snapshot);
            let original_tail = project.to_bytes()[scene.light_range.end..].to_vec();
            let mut camera = scene.camera[0].clone();
            camera.frame = scene.camera.iter().map(|k| k.frame).max().unwrap() + 10;
            camera.look_at.x += 1.;
            project.set_camera_key(camera.clone()).unwrap();
            let mut light = scene.light[0].clone();
            light.frame = scene.light.iter().map(|k| k.frame).max().unwrap() + 10;
            light.color = Vec3::ONE;
            project.set_light_key(light.clone()).unwrap();
            let edited = PmmProject::parse_with_resolver(&project.to_bytes(), load).unwrap();
            let edited_scene = edited.scene_tracks().unwrap();
            assert_eq!(edited_scene.camera.last().unwrap(), &camera);
            assert_eq!(edited_scene.light.last().unwrap(), &light);
            assert_eq!(
                &project.to_bytes()[edited_scene.light_range.end..],
                original_tail.as_slice()
            );
            project.set_camera_keys(camera_snapshot.clone()).unwrap();
            project.set_light_keys(light_snapshot.clone()).unwrap();
            let restored = PmmProject::parse_with_resolver(&project.to_bytes(), load)
                .unwrap()
                .scene_tracks()
                .unwrap();
            assert_eq!(restored.camera, camera_snapshot);
            assert_eq!(restored.light, light_snapshot);

            for (a, b) in shrunk.models.iter().zip(&project.models) {
                assert!(same_motion(&a.motion, &b.motion));
            }
        }
    }
    #[test]
    fn unsupported_motion_sections_are_rejected_without_mutating_project() {
        let mut project = PmmProject::parse(&fixture()).unwrap();
        let original = project.to_bytes();
        for mode in 0..3 {
            let mut motion = project.models[0].motion.clone();
            match mode {
                0 => motion.cameras.push(PmmCameraKey::default()),
                1 => motion.properties.push(crate::format::PropertyKey {
                    frame: 0,
                    visible: false,
                    ik: BTreeMap::new(),
                }),
                _ => motion.trailing_data = b"extension".to_vec(),
            }
            assert!(project.set_motion(0, motion).is_err());
            assert_eq!(project.to_bytes(), original);
        }
    }
}
