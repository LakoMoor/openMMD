use glam::Vec3;
use openmmd::format::Motion;
fn u32b(b: &mut Vec<u8>, v: u32) {
    b.extend(v.to_le_bytes());
}
fn floats(b: &mut Vec<u8>, v: &[f32]) {
    for x in v {
        b.extend(x.to_le_bytes());
    }
}
fn fixture() -> (Vec<u8>, Vec<usize>) {
    let mut b = b"Vocaloid Motion Data 0002\0\0\0\0\0".to_vec();
    b.extend(b"camera");
    b.resize(50, 0);
    u32b(&mut b, 0);
    u32b(&mut b, 0);
    let mut ends = vec![b.len()];
    u32b(&mut b, 2);
    for frame in [0, 60] {
        u32b(&mut b, frame);
        floats(
            &mut b,
            &[-45.0 - frame as f32, frame as f32, 10.0, 2.0, 0.1, 0.2, 0.3],
        );
        for _ in 0..6 {
            b.extend([16, 96, 32, 120]);
        }
        u32b(&mut b, 45 + frame / 2);
        b.push(u8::from(frame == 60));
    }
    ends.push(b.len());
    u32b(&mut b, 1);
    u32b(&mut b, 90);
    floats(&mut b, &[0.4, 0.6, 0.8, -0.5, -1.0, -0.25]);
    ends.push(b.len());
    u32b(&mut b, 1);
    u32b(&mut b, 120);
    b.push(2);
    floats(&mut b, &[0.003]);
    ends.push(b.len());
    u32b(&mut b, 2);
    for (frame, visible, enabled) in [(0, 1, 1), (150, 0, 0)] {
        u32b(&mut b, frame);
        b.push(visible);
        u32b(&mut b, 1);
        b.extend(b"leg IK");
        b.resize(b.len() + 14, 0);
        b.push(enabled);
    }
    (b, ends)
}
#[test]
fn independent_fixture_preserves_all_sections_and_unknown_extensions() {
    let (mut bytes, _) = fixture();
    bytes.extend(b"vendor-extension\0\xff");
    let m = Motion::parse(&bytes).unwrap();
    assert!(m.has_model_tracks());
    assert!(m.has_scene_tracks());
    assert_eq!(m.cameras.len(), 2);
    assert_eq!(m.cameras[1].interpolation[0], [16, 32, 96, 120]);
    assert!(!m.cameras[1].perspective);
    assert_eq!(m.last_frame, 150);
    assert_eq!(m.lights[0].direction, Vec3::new(-0.5, -1.0, -0.25));
    assert_eq!(m.shadows[0].mode, 2);
    assert!(!m.properties[1].visible);
    assert!(!m.properties[1].ik["leg IK"]);
    assert_eq!(m.to_vmd().unwrap(), bytes);
    let scene = m.scene_only();
    assert!(scene.bones.is_empty() && scene.properties.is_empty());
    assert_eq!(scene.last_frame, 120);
    let saved = Motion::parse(&scene.to_vmd().unwrap()).unwrap();
    assert_eq!(saved.cameras, m.cameras);
    assert_eq!(saved.lights, m.lights);
    assert_eq!(saved.shadows, m.shadows);
    assert_eq!(saved.trailing_data, m.trailing_data);
}
#[test]
fn old_optional_boundaries_are_valid_but_partial_records_are_not() {
    let (bytes, ends) = fixture();
    for length in 0..bytes.len() {
        let result = Motion::parse(&bytes[..length]);
        assert_eq!(
            result.is_ok(),
            ends.contains(&length),
            "unexpected truncation result at {length}: {result:?}"
        );
    }
}
#[test]
fn invalid_counts_flags_curves_and_nonfinite_values_fail() {
    let (bytes, _) = fixture();
    for (index, value) in [(94, 128), (122, 2), (224, 3), (237, 2), (262, 2)] {
        let mut bad = bytes.clone();
        bad[index] = value;
        assert!(Motion::parse(&bad).is_err(), "accepted bad byte at {index}");
    }
    for (index, value) in [
        (58, u32::MAX),
        (118, 0),
        (238, u32::MAX),
        (192, f32::NAN.to_bits()),
    ] {
        let mut bad = bytes.clone();
        bad[index..index + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            Motion::parse(&bad).is_err(),
            "accepted bad field at {index}"
        );
    }
}
#[test]
fn camera_interpolation_and_perspective_are_sampled_correctly() {
    let (bytes, _) = fixture();
    let mut motion = Motion::parse(&bytes).unwrap();
    for key in &mut motion.cameras {
        key.interpolation = [[20, 20, 107, 107]; 6];
    }
    let key = motion.camera_at(30.0).unwrap();
    assert!((key.look_at.x - 30.0).abs() < 0.001);
    assert!((key.distance + 75.0).abs() < 0.001);
    assert!((key.fov - 60.0).abs() < 0.001);
    assert!(key.perspective);
    assert!(!motion.camera_at(60.0).unwrap().perspective);
    assert_eq!(motion.camera_at(1000.0).unwrap().look_at.x, 60.0);
}
#[test]
fn writer_rejects_unrepresentable_or_invalid_scene_values() {
    let (bytes, _) = fixture();
    let original = Motion::parse(&bytes).unwrap();
    let mut m = original.clone();
    m.cameras[0].parent_model = 0;
    assert!(m.to_vmd().is_err());
    let mut m = original.clone();
    m.lights[0].direction.x = f32::NAN;
    assert!(m.to_vmd().is_err());
    let mut m = original.clone();
    m.cameras[0].fov = 180.0;
    assert!(m.to_vmd().is_err());
    let mut m = original.clone();
    m.model_name = "bad\0name".into();
    assert!(m.to_vmd().is_err());
    let mut m = original;
    m.properties[0].ik.insert("unencodable 😀".into(), true);
    assert!(m.to_vmd().is_err());
}
#[test]
fn separate_light_import_retains_camera_and_conflicting_extensions_fail_atomically() {
    let (bytes, _) = fixture();
    let motion = Motion::parse(&bytes).unwrap();
    let mut cameras = motion.scene_only();
    cameras.lights.clear();
    cameras.shadows.clear();
    let mut lights = Motion {
        lights: motion.lights.clone(),
        ..Default::default()
    };
    lights.lights[0].color = Vec3::X;
    let merged = cameras.merged_scene(&lights).unwrap();
    assert_eq!(merged.cameras, cameras.cameras);
    assert_eq!(merged.lights, lights.lights);
    cameras.trailing_data = b"vendor-A".to_vec();
    lights.trailing_data = b"vendor-B".to_vec();
    assert!(cameras.merged_scene(&lights).is_err());
    assert_eq!(cameras.trailing_data, b"vendor-A");
}

fn switched_rig() -> openmmd::format::Model {
    use openmmd::format::{Bone, Ik, Model};
    let mut bones = Vec::new();
    let mut chains = Vec::new();
    for (name, x) in [("left IK", 0.), ("right IK", 3.)] {
        let i = bones.len();
        bones.extend([
            Bone {
                name: format!("link {name}"),
                parent: None,
                position: Vec3::new(x, 0., 0.),
            },
            Bone {
                name: format!("target {name}"),
                parent: Some(i),
                position: Vec3::new(x + 1., 0., 0.),
            },
            Bone {
                name: name.into(),
                parent: None,
                position: Vec3::new(x, 1., 0.),
            },
        ]);
        chains.push(Ik {
            controller: i + 2,
            target: i + 1,
            iterations: 8,
            angle: 1.,
            links: vec![i],
        });
    }
    Model {
        name: "switch rig".into(),
        comment: String::new(),
        vertices: vec![],
        indices: vec![],
        materials: vec![],
        bones,
        ik: chains,
        morphs: vec![],
    }
}

#[test]
fn visibility_and_sparse_ik_switches_hold_and_rewind_independently() {
    use openmmd::{animation::Pose, format::PropertyKey};
    let model = switched_rig();
    let motion = Motion {
        properties: vec![
            PropertyKey {
                frame: 10,
                visible: false,
                ik: [("left IK".into(), false)].into(),
            },
            PropertyKey {
                frame: 20,
                visible: true,
                ik: [("right IK".into(), false)].into(),
            },
            PropertyKey {
                frame: 30,
                visible: false,
                ik: [("left IK".into(), true)].into(),
            },
        ],
        ..Default::default()
    };
    let motion = Motion::parse(&motion.to_vmd().unwrap()).unwrap();
    for (frame, visible, enabled) in [
        (0., true, [true, true]),
        (9.9, true, [true, true]),
        (10., false, [false, true]),
        (19.9, false, [false, true]),
        (20., true, [false, false]),
        (99., false, [true, false]),
        (5., true, [true, true]),
        (15., false, [false, true]),
    ] {
        let pose = Pose::sample(&model, &motion, frame);
        assert_eq!(pose.visible, visible, "frame {frame}");
        assert_eq!(pose.ik_enabled, enabled, "frame {frame}");
    }
    let mut pose = Pose::sample(&model, &motion, 15.);
    pose.solve_ik(&model);
    assert_eq!(pose.rotations[0], glam::Quat::IDENTITY);
    assert!(pose.rotations[3].angle_between(glam::Quat::IDENTITY) > 1.);
    // Hidden models still evaluate their bones for attached cameras and later visibility keys.
    assert!(!pose.visible);
    let mut pose = Pose::sample(&model, &motion, 20.);
    let original = pose.rotations.clone();
    pose.solve_ik(&model);
    assert_eq!(pose.rotations, original);
}
