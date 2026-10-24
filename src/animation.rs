use crate::format::{BoneKey, Model, Motion};
use glam::{Mat4, Quat, Vec3};

/// Solve x(t) of the VMD cubic Bezier and return y(t).
pub fn bezier(x: f32, control: [u8; 4]) -> f32 {
    let [x1, y1, x2, y2] = control.map(|v| v.min(127) as f32 / 127.0);
    let curve = |t: f32, a: f32, b: f32| {
        3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..20 {
        let t = (lo + hi) * 0.5;
        if curve(t, x1, x2) < x {
            lo = t;
        } else {
            hi = t;
        }
    }
    curve((lo + hi) * 0.5, y1, y2)
}
/// MMD 9.32 camera evaluator: linear shortcut and at most 12 binary-search steps.
/// Arithmetic order follows the scalar operations observed at 0x14005a8b0.
/// Exact keyframes bypass this evaluator in PmmCameraKey::sample.
pub fn camera_bezier(x: f32, control: [u8; 4]) -> f32 {
    let [x1, y1, x2, y2] = control.map(|v| v.min(127) as f32);
    if x1 == y1 && x2 == y2 {
        return x;
    }
    let (x1, x2) = (x1 / 127., x2 / 127.);
    let (mut t, mut step) = (0.5_f32, 0.5_f32);
    for _ in 0..12 {
        step *= 0.5;
        let inverse = 1. - t;
        let base = inverse * 3.;
        let value = base * inverse * t * x1 + base * t * t * x2 + t * t * t;
        if x == value {
            break;
        }
        t += if x <= value { -step } else { step };
    }
    let inverse = 1. - t;
    let base = inverse * 3.;
    base * inverse * t * y1 / 127. + base * t * t * y2 / 127. + t * t * t
}
pub fn sample_bone(keys: &[BoneKey], frame: f32) -> (Vec3, Quat) {
    if keys.is_empty() {
        return (Vec3::ZERO, Quat::IDENTITY);
    }
    let next = keys.partition_point(|k| (k.frame as f32) <= frame);
    if next == 0 {
        return (keys[0].translation, keys[0].rotation);
    }
    let a = &keys[next - 1];
    let Some(b) = keys.get(next) else {
        return (a.translation, a.rotation);
    };
    let t = ((frame - a.frame as f32) / (b.frame - a.frame) as f32).clamp(0.0, 1.0);
    let eased = |channel: usize| {
        bezier(
            t,
            [
                b.interpolation[channel],
                b.interpolation[channel + 4],
                b.interpolation[channel + 8],
                b.interpolation[channel + 12],
            ],
        )
    };
    let delta = b.translation - a.translation;
    (
        a.translation + delta * Vec3::new(eased(0), eased(1), eased(2)),
        a.rotation.slerp(b.rotation, eased(3)),
    )
}
pub fn sample_morph(keys: &[crate::format::MorphKey], frame: f32) -> f32 {
    let next = keys.partition_point(|k| k.frame as f32 <= frame);
    if next == 0 {
        return keys.first().map_or(0.0, |k| k.weight);
    }
    let a = &keys[next - 1];
    let Some(b) = keys.get(next) else {
        return a.weight;
    };
    a.weight + (b.weight - a.weight) * (frame - a.frame as f32) / (b.frame - a.frame) as f32
}
#[derive(Clone)]
pub struct Pose {
    pub translations: Vec<Vec3>,
    pub rotations: Vec<Quat>,
    pub morphs: Vec<f32>,
    pub visible: bool,
    pub ik_enabled: Vec<bool>,
}
impl Pose {
    pub fn sample(model: &Model, motion: &Motion, frame: f32) -> Self {
        let mut translations = vec![Vec3::ZERO; model.bones.len()];
        let mut rotations = vec![Quat::IDENTITY; model.bones.len()];
        for (i, bone) in model.bones.iter().enumerate() {
            if let Some(keys) = motion.bones.get(&bone.name) {
                (translations[i], rotations[i]) = sample_bone(keys, frame);
            }
        }
        let morphs = model
            .morphs
            .iter()
            .map(|m| {
                motion
                    .morphs
                    .get(&m.name)
                    .map_or(0.0, |k| sample_morph(k, frame))
            })
            .collect();
        let frame = if frame.is_finite() {
            frame.max(0.0)
        } else {
            0.0
        };
        let end = motion
            .properties
            .partition_point(|key| key.frame as f32 <= frame);
        let visible = motion.properties[..end]
            .last()
            .is_none_or(|key| key.visible);
        let ik_enabled = model
            .ik
            .iter()
            .map(|ik| {
                let name = &model.bones[ik.controller].name;
                motion.properties[..end]
                    .iter()
                    .rev()
                    .find_map(|key| key.ik.get(name).copied())
                    .unwrap_or(true)
            })
            .collect();
        Self {
            translations,
            rotations,
            morphs,
            visible,
            ik_enabled,
        }
    }
    pub fn globals(&self, model: &Model) -> Vec<Mat4> {
        let mut out = vec![Mat4::IDENTITY; model.bones.len()];
        let mut done = vec![false; model.bones.len()];
        let mut chain = Vec::new();
        for i in 0..out.len() {
            let mut cursor = Some(i);
            while let Some(j) = cursor {
                if done[j] {
                    break;
                }
                chain.push(j);
                cursor = model.bones[j].parent;
            }
            while let Some(j) = chain.pop() {
                let b = &model.bones[j];
                let (parent, offset) = if let Some(p) = b.parent {
                    (out[p], b.position - model.bones[p].position)
                } else {
                    (Mat4::IDENTITY, b.position)
                };
                out[j] = parent
                    * Mat4::from_rotation_translation(
                        self.rotations[j],
                        offset + self.translations[j],
                    );
                done[j] = true;
            }
        }
        out
    }

    /// PMD CCD IK. Knee constraints are approximated as a local X hinge.
    pub fn solve_ik(&mut self, model: &Model) {
        for (index, ik) in model.ik.iter().enumerate() {
            if !self.ik_enabled[index] {
                continue;
            }
            let mut globals = self.globals(model);
            let goal = globals[ik.controller].transform_point3(Vec3::ZERO);
            for _ in 0..ik.iterations.min(256) {
                if globals[ik.target]
                    .transform_point3(Vec3::ZERO)
                    .distance_squared(goal)
                    < 1e-8
                {
                    break;
                }
                for &link in &ik.links {
                    let inv = globals[link].inverse();
                    let a = inv
                        .transform_point3(globals[ik.target].transform_point3(Vec3::ZERO))
                        .normalize_or_zero();
                    let b = inv.transform_point3(goal).normalize_or_zero();
                    let angle = a.dot(b).clamp(-1.0, 1.0).acos().min(ik.angle.abs() * 4.0);
                    let axis = a.cross(b).normalize_or_zero();
                    if axis.length_squared() < 0.5 || angle < 1e-6 {
                        continue;
                    }
                    let mut q =
                        (self.rotations[link] * Quat::from_axis_angle(axis, angle)).normalize();
                    if model.bones[link].name.contains("ひざ") {
                        let (x, _, _) = q.to_euler(glam::EulerRot::XYZ);
                        q = Quat::from_rotation_x(x.clamp(0.002, std::f32::consts::PI));
                    }
                    self.rotations[link] = q;
                    globals = self.globals(model);
                }
            }
        }
    }
    pub fn skin(&self, model: &Model) -> Vec<(Vec3, Vec3)> {
        let globals = self.globals(model);
        let matrices: Vec<_> = globals
            .iter()
            .zip(&model.bones)
            .map(|(m, b)| *m * Mat4::from_translation(-b.position))
            .collect();
        let mut positions: Vec<_> = model.vertices.iter().map(|v| v.position).collect();
        if let Some(base) = model.morphs.iter().find(|m| m.base) {
            for (m, &weight) in model.morphs.iter().zip(&self.morphs) {
                if m.base || weight == 0.0 {
                    continue;
                }
                for &(i, delta) in &m.vertices {
                    positions[base.vertices[i].0] += delta * weight;
                }
            }
        }
        model
            .vertices
            .iter()
            .zip(positions)
            .map(|(v, p)| {
                let mut position = Vec3::ZERO;
                let mut normal = Vec3::ZERO;
                for (&bone, &weight) in v.bones.iter().zip(&v.weights) {
                    if weight == 0.0 {
                        continue;
                    }
                    let matrix = matrices[bone as usize];
                    position += matrix.transform_point3(p) * weight;
                    normal += matrix.transform_vector3(v.normal) * weight;
                }
                (position, normal.normalize_or_zero())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_curve_matches_932_search_precision_and_linear_shortcut() {
        for (x, expected) in [
            (0.1, 0x3e205e71),
            (0.25, 0x3eb122cf),
            (0.5, 0x3f1e7ddb),
            (0.75, 0x3f595966),
            (0.9, 0x3f74cf74),
        ] {
            assert_eq!(camera_bezier(x, [16, 32, 96, 120]).to_bits(), expected);
            assert_eq!(camera_bezier(x, [20, 20, 107, 107]), x);
        }
    }
    #[test]
    fn bezier_endpoints_and_linear() {
        for x in [0.0, 0.1, 0.5, 0.9, 1.0] {
            assert!((bezier(x, [20, 20, 107, 107]) - x).abs() < 1e-5);
        }
        assert!(bezier(0.25, [10, 100, 20, 120]) > 0.5);
    }
    #[test]
    fn motion_interpolates_and_holds_endpoints() {
        let mut interpolation = [0; 64];
        for c in 0..4 {
            interpolation[c] = 20;
            interpolation[c + 4] = 20;
            interpolation[c + 8] = 107;
            interpolation[c + 12] = 107;
        }
        let keys = vec![
            BoneKey {
                frame: 0,
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                interpolation,
            },
            BoneKey {
                frame: 30,
                translation: Vec3::X * 10.0,
                rotation: Quat::from_rotation_y(1.0),
                interpolation,
            },
        ];
        assert!((sample_bone(&keys, 15.0).0.x - 5.0).abs() < 1e-4);
        assert_eq!(sample_bone(&keys, 90.0).0.x, 10.0);
        assert!(
            (sample_bone(&keys, 15.0)
                .1
                .angle_between(Quat::from_rotation_y(0.5)))
            .abs()
                < 1e-4
        );
    }
}
