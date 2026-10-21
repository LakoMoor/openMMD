//! Camera matrices at the MMD left-handed / wgpu right-handed boundary.
use crate::pmm::PmmCameraKey;
use glam::{Mat4, Quat, Vec3};
pub fn projection(key: &PmmCameraKey, size: [u32; 2], parent: Option<Mat4>) -> Mat4 {
    let rotation = Quat::from_euler(
        glam::EulerRot::XYZ,
        -key.angle.x,
        -key.angle.y,
        -key.angle.z,
    );
    let distance = if key.distance.abs() < 0.01 {
        -0.01
    } else {
        key.distance
    };
    let mut eye = key.look_at + rotation * Vec3::new(0.0, 0.0, distance);
    let mut target = key.look_at;
    let mut up = rotation * Vec3::Y;
    if let Some(parent) = parent {
        eye = parent.transform_point3(eye);
        target = parent.transform_point3(target);
        up = parent.transform_vector3(up).normalize_or_zero();
    }
    let mirror = Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0));
    let aspect = size[0].max(1) as f32 / size[1].max(1) as f32;
    let projection = if key.perspective {
        Mat4::perspective_rh(key.fov.clamp(1.0, 179.0).to_radians(), aspect, 0.1, 10000.0)
    } else {
        let half_height = (25.0 * distance.abs() / 45.0).max(0.1) * 0.5;
        Mat4::orthographic_rh(
            -half_height * aspect,
            half_height * aspect,
            -half_height,
            half_height,
            0.1,
            10000.0,
        )
    };
    projection
        * Mat4::look_at_rh(
            mirror.transform_point3(eye),
            mirror.transform_point3(target),
            mirror.transform_vector3(up),
        )
        * mirror
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
        for (a, b) in vp.to_cols_array().into_iter().zip(orbit.to_cols_array()) {
            assert!((a - b).abs() < 1e-5);
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
}
