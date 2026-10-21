use openmmd::{
    animation::Pose,
    format::{Model, Motion},
};
#[test]
#[ignore = "requires downloaded original PMD assets"]
fn bundled_models_and_skinning() {
    let root = std::path::Path::new("reference/original/UserFile/Model");
    let mut checked = 0;
    for file in std::fs::read_dir(root).unwrap() {
        let path = file.unwrap().path();
        if path.extension().is_none_or(|s| s != "pmd") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let model = Model::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        let pose = Pose::sample(&model, &Motion::default(), 0.0);
        let skinned = pose.skin(&model);
        for (v, (p, n)) in model.vertices.iter().zip(skinned) {
            assert!(
                v.position.distance(p) < 0.001,
                "rest pose changed in {}",
                path.display()
            );
            assert!(n.is_finite());
        }
        let mut pose = pose;
        pose.solve_ik(&model);
        assert!(pose
            .skin(&model)
            .iter()
            .all(|(p, n)| p.is_finite() && n.is_finite()));
        for length in [0, 3, 7, 100, 280] {
            assert!(
                Model::parse(&bytes[..length]).is_err(),
                "accepted truncated {}",
                path.display()
            );
        }
        checked += 1;
    }
    assert!(checked >= 10);
    println!("Validated {checked} bundled PMD models");
}
#[test]
fn malformed_files_fail_without_panic() {
    for len in 0..512 {
        let bytes = vec![255; len];
        assert!(Model::parse(&bytes).is_err());
        assert!(Motion::parse(&bytes).is_err());
    }
}
