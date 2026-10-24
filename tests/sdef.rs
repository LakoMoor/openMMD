use glam::{Quat, Vec2, Vec3};
use openmmd::{
    animation::Pose,
    format::{Bone, Model, Morph, Sdef, Vertex},
};
fn model() -> Model {
    Model {
        name: "SDEF numeric fixture".into(),
        comment: String::new(),
        indices: vec![],
        materials: vec![],
        vertices: vec![Vertex {
            position: Vec3::new(2., 0., 0.),
            normal: Vec3::X,
            uv: Vec2::ZERO,
            bones: [0, 1, 0, 0],
            weights: [0.5, 0.5, 0., 0.],
            edge_scale: 1.,
            qdef: false,
            sdef: Some(Sdef {
                center: Vec3::X,
                r0: Vec3::X * 2.,
                r1: Vec3::ZERO,
            }),
        }],
        bones: (0..2)
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
fn sdef_preserves_rest_pose_and_matches_numeric_bend() {
    let mut model = model();
    let mut pose = Pose::sample(&model, &Default::default(), 0.);
    close(pose.skin(&model)[0].0, Vec3::new(2., 0., 0.));
    pose.rotations[1] = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    pose.translations[1] = Vec3::Y * 2.;
    let (p, n) = pose.skin(&model)[0];
    let h = std::f32::consts::FRAC_1_SQRT_2;
    close(p, Vec3::new(0.75 + h, 1.25 + h, 0.));
    close(n, Vec3::new(h, h, 0.));
    model.vertices[0].sdef = None;
    let linear = pose.skin(&model)[0].0;
    close(linear, Vec3::new(1., 2., 0.));
    assert!((linear - p).length() > 0.4);
}
#[test]
fn sdef_end_weights_and_common_rigid_transform() {
    let mut model = model();
    model.bones[0].position = Vec3::new(1., 2., 0.);
    model.bones[1].position = Vec3::new(-2., 1., 0.);
    let mut pose = Pose::sample(&model, &Default::default(), 0.);
    pose.rotations = vec![Quat::from_rotation_x(0.7), Quat::from_rotation_z(-1.2)];
    pose.translations = vec![Vec3::new(2., 1., -1.), Vec3::new(-1., 2., 3.)];
    for w in [0., 1.] {
        model.vertices[0].weights = [w, 1. - w, 0., 0.];
        let sdef = pose.skin(&model)[0];
        let saved = model.vertices[0].sdef.take();
        let linear = pose.skin(&model)[0];
        model.vertices[0].sdef = saved;
        close(sdef.0, linear.0);
        close(sdef.1, linear.1);
    }
    model = self::model();
    let mut pose = Pose::sample(&model, &Default::default(), 0.);
    let rotation = Quat::from_rotation_y(0.8);
    pose.rotations = vec![rotation; 2];
    pose.translations = vec![Vec3::Y; 2];
    let (p, n) = pose.skin(&model)[0];
    close(p, rotation * model.vertices[0].position + Vec3::Y);
    close(n, rotation * Vec3::X);
}
#[test]
fn vertex_morph_is_applied_before_sdef() {
    let mut model = model();
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
    let mut pose = Pose::sample(&model, &Default::default(), 0.);
    pose.morphs[1] = 1.;
    pose.rotations[1] = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    pose.translations[1] = Vec3::Y * 2.;
    close(
        pose.skin(&model)[0].0,
        Vec3::new(0.75, 1.25 + std::f32::consts::SQRT_2, 0.),
    );
}

#[test]
fn bundled_synthetic_scene_imports_and_bends() {
    let mut model = Model::parse(include_bytes!("../examples/sdef/strip.pmx")).unwrap();
    let motion =
        openmmd::format::Motion::parse(include_bytes!("../examples/sdef/bend.vmd")).unwrap();
    assert_eq!(model.vertices.len(), 18);
    assert!(model.vertices.iter().all(|v| v.sdef.is_some()));
    let rest = Pose::sample(&model, &motion, 0.).skin(&model);
    for (v, (p, _)) in model.vertices.iter().zip(rest) {
        close(p, v.position);
    }
    let pose = Pose::sample(&model, &motion, 60.);
    let spherical = pose.skin(&model);
    for vertex in &mut model.vertices {
        vertex.sdef = None;
    }
    let linear = pose.skin(&model);
    assert!(spherical
        .iter()
        .zip(linear)
        .any(|((s, _), (l, _))| (*s - l).length() > 0.1));
}
