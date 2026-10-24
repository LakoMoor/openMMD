//! MMD scene camera in left-handed coordinates with wgpu 0-to-1 depth.
use crate::pmm::PmmCameraKey;
use glam::{Mat4, Quat, Vec3};
pub fn view_eye(key: &PmmCameraKey, parent: Option<Mat4>) -> (Mat4, Vec3) {
    let rotation = Quat::from_euler(
        glam::EulerRot::YXZ,
        -key.angle.y,
        -key.angle.x,
        -key.angle.z,
    );
    let distance = key.distance;
    let mut eye = key.look_at + rotation * Vec3::new(0.0, 0.0, distance);
    let mut target = eye + rotation * (Vec3::Z * 80.);
    let mut up = rotation * Vec3::Y;
    if let Some(parent) = parent {
        eye = parent.transform_point3(eye);
        target = parent.transform_point3(target);
        up = parent.transform_vector3(up).normalize_or_zero();
    }
    (Mat4::look_at_lh(eye, target, up), eye)
}
pub fn projection(key: &PmmCameraKey, size: [u32; 2], parent: Option<Mat4>) -> Mat4 {
    let distance = key.distance;
    let aspect = size[0].max(1) as f32 / size[1].max(1) as f32;
    let projection = if key.perspective {
        Mat4::perspective_lh(
            key.fov.clamp(1.0, 179.0).to_radians(),
            aspect,
            1.0,
            100000.0,
        )
    } else {
        // MMD 9.32 writes these projection coefficients directly (not a FOV-derived box).
        // See docs/parity.md for the EXE identity, addresses and static-analysis boundary.
        let scale = -2.0
            / if distance.abs() < 0.01 {
                if distance > 0.0 {
                    0.01
                } else {
                    -0.01
                }
            } else {
                distance
            };
        Mat4::from_cols_array(&[
            scale,
            0.,
            0.,
            0.,
            0.,
            scale * aspect,
            0.,
            0.,
            0.,
            0.,
            f32::from_bits(0x3aaa64c3),
            0.,
            0.,
            0.,
            0.,
            1.,
        ])
    };
    projection * view_eye(key, parent).0
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn front_camera_matches_orbit_and_targets_center() {
        let key = PmmCameraKey::default();
        let vp = projection(&key, [800, 600], None);
        let orbit = crate::renderer::orbit_projection(
            [800, 600],
            0.0,
            0.0,
            45.0,
            Vec3::new(0.0, 10.0, 0.0),
        );
        // Orbit remains a separate free camera; only screen coordinates coincide.
        for point in [Vec3::ZERO, Vec3::new(5., 12., 3.)] {
            let a = vp.project_point3(point);
            let b = orbit.project_point3(point);
            assert!((a.x - b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5);
        }
        let center = vp.project_point3(key.look_at);
        assert!(center.x.abs() < 1e-6 && center.y.abs() < 1e-6);
    }
    #[test]
    fn orbit_roll_orthographic_and_attachment_are_finite() {
        let key = PmmCameraKey {
            perspective: false,
            angle: Vec3::new(0.4, 0.8, 1.0),
            ..Default::default()
        };
        let vp = projection(
            &key,
            [1920, 1080],
            Some(Mat4::from_translation(Vec3::X * 20.0)),
        );
        assert!(vp.is_finite());
        let center = vp.project_point3(key.look_at + Vec3::X * 20.0);
        assert!(center.x.abs() < 1e-5 && center.y.abs() < 1e-5);
    }
    #[test]
    fn mmd_932_unattached_projection_coefficients_and_rotation_order() {
        let mut key = PmmCameraKey {
            look_at: Vec3::new(2., 8., -3.),
            angle: Vec3::new(0.3, 0.7, -0.2),
            distance: -30.,
            fov: 45.,
            ..Default::default()
        };
        // Independently evaluated row-vector Y -> X -> Z rotations, then camera Z translation.
        let point = Vec3::new(5., 12., 4.);
        let projected = projection(&key, [1600, 900], None).project_point3(point);
        assert!(projected.distance(Vec3::new(0.2848689, 0.09828573, 0.97098255)) < 2e-6);
        key.perspective = false;
        let projected = projection(&key, [1600, 900], None).project_point3(point);
        assert!(projected.distance(Vec3::new(0.48178285, 0.16622515, 0.044785675)) < 2e-6);
        key.fov = 90.;
        assert!(
            projection(&key, [1600, 900], None)
                .project_point3(point)
                .distance(projected)
                < 1e-6
        );
    }
    #[test]
    fn positive_distance_keeps_original_forward_axis_and_perspective_clip_planes() {
        let mut key = PmmCameraKey {
            look_at: Vec3::ZERO,
            distance: 10.,
            ..Default::default()
        };
        let matrix = projection(&key, [800, 600], None);
        assert!(matrix.project_point3(Vec3::new(0., 0., 11.)).z.abs() < 1e-6);
        assert!((matrix.project_point3(Vec3::new(0., 0., 100010.)).z - 1.).abs() < 1e-6);
        key.distance = 100000000.;
        assert!(projection(&key, [800, 600], None).is_finite());
        key.distance = 0.;
        assert!(projection(&key, [800, 600], None).is_finite());
        key.perspective = false;
        assert!(projection(&key, [800, 600], None).is_finite());
    }
}
