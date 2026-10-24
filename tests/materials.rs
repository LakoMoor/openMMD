use openmmd::format::Model;
fn u32b(b: &mut Vec<u8>, v: u32) {
    b.extend(v.to_le_bytes());
}
fn f(b: &mut Vec<u8>, v: f32) {
    b.extend(v.to_le_bytes());
}
fn fixed(b: &mut Vec<u8>, v: &str, n: usize) {
    b.extend(v.as_bytes());
    b.resize(b.len() + n - v.len(), 0);
}
fn text(b: &mut Vec<u8>, v: &str) {
    u32b(b, v.len() as u32);
    b.extend(v.as_bytes());
}
fn pmd() -> Vec<u8> {
    let mut b = b"Pmd".to_vec();
    f(&mut b, 1.);
    fixed(&mut b, "material test", 20);
    fixed(&mut b, "", 256);
    u32b(&mut b, 3);
    for _ in 0..3 {
        for x in [0., 0., 0., 0., 0., -1., 0., 0.] {
            f(&mut b, x);
        }
        b.extend([0; 4]);
        b.extend([100, 0]);
    }
    u32b(&mut b, 3);
    b.extend([0, 0, 1, 0, 2, 0]);
    u32b(&mut b, 1);
    for x in [0.8, 0.6, 0.4, 0.5, 12., 0.2, 0.3, 0.4, 0.1, 0.2, 0.3] {
        f(&mut b, x);
    }
    b.extend([0, 1]);
    u32b(&mut b, 3);
    fixed(&mut b, "d.png*s.spa", 20);
    b.extend(1u16.to_le_bytes());
    fixed(&mut b, "bone", 20);
    b.extend([255, 255, 255, 255, 0, 0, 0]);
    for _ in 0..3 {
        f(&mut b, 0.);
    }
    b.extend([0; 4]); // IK and morph counts
    b.extend([0, 0]);
    u32b(&mut b, 0);
    b.push(0); // display tables and English flag
    fixed(&mut b, "custom.bmp", 100);
    for _ in 1..10 {
        fixed(&mut b, "", 100);
    }
    b
}
fn pmx() -> Vec<u8> {
    let mut b = b"PMX ".to_vec();
    f(&mut b, 2.);
    b.extend([8, 1, 0, 1, 1, 1, 1, 1, 1]);
    for s in ["test", "", "", ""] {
        text(&mut b, s);
    }
    u32b(&mut b, 3);
    for _ in 0..3 {
        for x in [0., 0., 0., 0., 0., -1., 0., 0.] {
            f(&mut b, x);
        }
        b.extend([0, 0]);
        f(&mut b, 1.);
    }
    u32b(&mut b, 3);
    b.extend([0, 1, 2]);
    u32b(&mut b, 3);
    for s in ["d.png", "s.png", "toon.png"] {
        text(&mut b, s);
    }
    u32b(&mut b, 1);
    text(&mut b, "material");
    text(&mut b, "");
    for x in [0.8, 0.6, 0.4, 0.5, 0.2, 0.3, 0.4, 12., 0.1, 0.2, 0.3] {
        f(&mut b, x);
    }
    b.push(17);
    for x in [0.01, 0.02, 0.03, 1., 1.5] {
        f(&mut b, x);
    }
    b.extend([0, 1, 2, 0, 2]);
    text(&mut b, "");
    u32b(&mut b, 3);
    u32b(&mut b, 1);
    text(&mut b, "bone");
    text(&mut b, "");
    for _ in 0..3 {
        f(&mut b, 0.);
    }
    b.push(255);
    u32b(&mut b, 0);
    b.extend([0, 0]);
    for _ in 0..3 {
        f(&mut b, 0.);
    }
    for _ in 0..4 {
        u32b(&mut b, 0);
    }
    b
}
#[test]
fn pmd_and_pmx_retain_render_material_fields() {
    for (bytes, toon, edge_size) in [(pmd(), "custom.bmp", 1.), (pmx(), "toon.png", 1.5)] {
        let model = Model::parse(&bytes).unwrap();
        let m = &model.materials[0];
        assert_eq!(m.texture, "d.png");
        assert!(m.sphere_texture.starts_with("s."));
        assert_eq!(m.sphere_mode, 2);
        assert_eq!(m.shininess, 12.);
        assert_eq!(m.specular, glam::Vec3::new(0.2, 0.3, 0.4));
        assert_eq!(m.ambient, glam::Vec3::new(0.1, 0.2, 0.3));
        assert_eq!(m.toon_texture, toon);
        assert!(m.double_sided);
        assert!(m.edge_enabled);
        assert_eq!(m.edge_size, edge_size);
    }
}
#[test]
fn malformed_pmd_optional_material_extension_is_rejected() {
    let bytes = pmd();
    assert!(Model::parse(&bytes[..bytes.len() - 1]).is_err());
}

#[test]
fn vertex_edge_controls_are_retained() {
    let mut bytes = pmd();
    // PMD header is 283 bytes followed by the vertex count; each vertex is 38 bytes.
    bytes[287 + 37] = 1;
    let model = Model::parse(&bytes).unwrap();
    assert_eq!(model.vertices[0].edge_scale, 0.);
    assert_eq!(model.vertices[1].edge_scale, 1.);
    let mut bytes = pmx();
    // Fixture has a 37-byte header; first BDEF1 vertex is 38 bytes.
    // Locate the vertex section via its four text lengths instead of assuming a model name.
    let mut offset = 17;
    for _ in 0..4 {
        let length = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4 + length;
    }
    offset += 4;
    bytes[offset + 34..offset + 38].copy_from_slice(&0.25f32.to_le_bytes());
    let model = Model::parse(&bytes).unwrap();
    assert_eq!(model.vertices[0].edge_scale, 0.25);
    assert_eq!(model.vertices[1].edge_scale, 1.);
}
