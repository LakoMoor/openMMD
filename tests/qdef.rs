use glam::{Quat, Vec2, Vec3};
use openmmd::{
    animation::Pose,
    format::{Bone, Model, Morph, Vertex},
};
fn model() -> Model {
    Model {
        name: "QDEF numeric fixture".into(),
        comment: String::new(),
        vertices: vec![Vertex {
            position: Vec3::X,
            normal: Vec3::X,
            uv: Vec2::ZERO,
            bones: [0, 1, 2, 3],
            weights: [0.5, 0.5, 0., 0.],
            edge_scale: 1.,
            sdef: None,
            qdef: true,
        }],
        indices: vec![],
        materials: vec![],
        bones: (0..4)
            .map(|i| Bone {
                name: format!("bone{i}"),
                parent: None,
                position: Vec3::ZERO,
            })
            .collect(),
        ik: vec![],
        morphs: vec![],
        outline_scale: 1.,
    }
}
fn close(actual: Vec3, expected: Vec3) {
    assert!(
        (actual - expected).length() < 0.00001,
        "{actual:?} != {expected:?}"
    );
}
#[test]
fn twist_keeps_radius_and_uses_shortest_quaternion_hemisphere() {
    let mut model = model();
    let mut pose = Pose::sample(&model, &Default::default(), 0.);
    for angle in [60f32, 100.] {
        pose.rotations[0] = Quat::from_rotation_z(angle.to_radians());
        pose.rotations[1] = Quat::from_rotation_z(-angle.to_radians());
        let (p, n) = pose.skin(&model)[0];
        let expected = if angle < 90. { Vec3::X } else { -Vec3::X };
        close(p, expected);
        close(n, expected);
    }
    pose.rotations[0] = Quat::from_rotation_z(60f32.to_radians());
    pose.rotations[1] = Quat::from_rotation_z(-60f32.to_radians());
    model.vertices[0].qdef = false;
    close(pose.skin(&model)[0].0, Vec3::X * 0.5);
}
#[test]
fn numeric_rotation_translation_coupling_and_morph_order() {
    let mut model = model();
    let mut pose = Pose::sample(&model, &Default::default(), 0.);
    pose.rotations[1] = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    pose.translations[1] = Vec3::X * 2.;
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let offset = Vec3::new(1., 1. - std::f32::consts::SQRT_2, 0.);
    let (p, n) = pose.skin(&model)[0];
    close(p, Vec3::new(h, h, 0.) + offset);
    close(n, Vec3::new(h, h, 0.));
    model.morphs = vec![
        Morph {
            name: "base".into(),
            base: true,
            vertices: vec![(0, Vec3::ZERO)],
        },
        Morph {
            name: "move".into(),
            base: false,
            vertices: vec![(0, Vec3::Y)],
        },
    ];
    pose.morphs = vec![0., 1.];
    close(
        pose.skin(&model)[0].0,
        Vec3::new(0., std::f32::consts::SQRT_2, 0.) + offset,
    );
}
#[test]
fn four_bones_rest_pose_endpoints_and_common_rotation() {
    let mut model = model();
    model.vertices[0].weights = [0.1, 0.2, 0.3, 0.4];
    let mut pose = Pose::sample(&model, &Default::default(), 0.);
    close(pose.skin(&model)[0].0, Vec3::X);
    let q = Quat::from_rotation_y(0.8);
    pose.rotations = vec![q; 4];
    pose.translations = vec![Vec3::X, Vec3::Y, Vec3::Z, Vec3::ONE];
    close(
        pose.skin(&model)[0].0,
        q * Vec3::X + Vec3::new(0.5, 0.6, 0.7),
    );
    for i in 0..4 {
        model.bones[i].position = Vec3::new(i as f32, 2., -1.);
        model.vertices[0].weights = [0.; 4];
        model.vertices[0].weights[i] = 1.;
        let dual = pose.skin(&model)[0];
        model.vertices[0].qdef = false;
        let linear = pose.skin(&model)[0];
        model.vertices[0].qdef = true;
        close(dual.0, linear.0);
        close(dual.1, linear.1);
    }
}
#[test]
fn zero_weight_first_bone_does_not_choose_blend_hemisphere() {
    let mut model = model();
    model.vertices[0].weights = [0., 0.5, 0.5, 0.];
    let mut pose = Pose::sample(&model, &Default::default(), 0.);
    pose.rotations[1] = Quat::from_rotation_z(100f32.to_radians());
    pose.rotations[2] = Quat::from_rotation_z(-100f32.to_radians());
    close(pose.skin(&model)[0].0, -Vec3::X);
}

#[test]
fn pmx21_twist_scene_preserves_cylinder_radius() {
    let mut model = Model::parse(include_bytes!("../examples/qdef/cylinder.pmx")).unwrap();
    let motion =
        openmmd::format::Motion::parse(include_bytes!("../examples/qdef/twist.vmd")).unwrap();
    assert_eq!(model.vertices.len(), 84);
    for indices in model.indices.chunks_exact(3) {
        let a = &model.vertices[indices[0] as usize];
        let b = &model.vertices[indices[1] as usize];
        let c = &model.vertices[indices[2] as usize];
        assert!(
            (b.position - a.position)
                .cross(c.position - a.position)
                .dot(a.normal)
                > 0.,
            "fixture winding disagrees with normals"
        );
    }

    assert!(model.vertices.iter().all(|v| v.qdef));
    let pose = Pose::sample(&model, &motion, 60.);
    let deformed = pose.skin(&model);
    for (p, n) in &deformed {
        assert!((p.x.hypot(p.z) - 3.).abs() < 0.00001);
        assert!((n.length() - 1.).abs() < 0.00001);
    }
    model = Model::parse(include_bytes!("../examples/qdef/cylinder_bdef.pmx")).unwrap();
    assert!(model.vertices.iter().all(|v| !v.qdef));
    let linear = pose.skin(&model);
    assert!((linear[36].0.x.hypot(linear[36].0.z) - 1.5).abs() < 0.00001);
}
