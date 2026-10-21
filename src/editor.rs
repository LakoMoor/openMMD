use crate::format::{BoneKey, MorphKey, Motion};
use glam::{Quat, Vec3};
#[derive(Default)]
pub struct History {
    undo: Vec<Motion>,
    redo: Vec<Motion>,
}
impl History {
    pub fn checkpoint(&mut self, motion: &Motion) {
        self.undo.push(motion.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    pub fn undo(&mut self, motion: &mut Motion) {
        if let Some(old) = self.undo.pop() {
            self.redo.push(std::mem::replace(motion, old));
        }
    }
    pub fn redo(&mut self, motion: &mut Motion) {
        if let Some(new) = self.redo.pop() {
            self.undo.push(std::mem::replace(motion, new));
        }
    }
}
pub fn linear_interpolation() -> [u8; 64] {
    let mut out = [0; 64];
    for c in 0..4 {
        out[c] = 20;
        out[c + 4] = 20;
        out[c + 8] = 107;
        out[c + 12] = 107;
    }
    out
}
pub fn set_bone(motion: &mut Motion, name: &str, frame: u32, translation: Vec3, rotation: Quat) {
    let keys = motion.bones.entry(name.into()).or_default();
    match keys.binary_search_by_key(&frame, |k| k.frame) {
        Ok(i) => {
            keys[i].translation = translation;
            keys[i].rotation = rotation;
        }
        Err(i) => keys.insert(
            i,
            BoneKey {
                frame,
                translation,
                rotation,
                interpolation: linear_interpolation(),
            },
        ),
    }
    motion.recompute_duration();
}
pub fn set_morph(motion: &mut Motion, name: &str, frame: u32, weight: f32) {
    let keys = motion.morphs.entry(name.into()).or_default();
    match keys.binary_search_by_key(&frame, |k| k.frame) {
        Ok(i) => keys[i].weight = weight,
        Err(i) => keys.insert(i, MorphKey { frame, weight }),
    }
    motion.recompute_duration();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_export_and_undo_roundtrip() {
        let mut m = Motion {
            model_name: "初音ミク".into(),
            ..Default::default()
        };
        let mut h = History::default();
        h.checkpoint(&m);
        set_bone(
            &mut m,
            "センター",
            12,
            Vec3::new(1.0, 2.0, 3.0),
            Quat::IDENTITY,
        );
        set_morph(&mut m, "笑い", 20, 0.5);
        let loaded = Motion::parse(&m.to_vmd().unwrap()).unwrap();
        assert_eq!(
            loaded.bones["センター"][0].translation,
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(loaded.morphs["笑い"][0].weight, 0.5);
        h.undo(&mut m);
        assert!(m.bones.is_empty());
        h.redo(&mut m);
        assert_eq!(m.last_frame, 20);
    }
}
