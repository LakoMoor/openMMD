//! Shift-JIS VPD pose interchange in MMD coordinates.
use anyhow::{ensure, Context, Result};
use encoding_rs::SHIFT_JIS;
use glam::{Quat, Vec3};
use std::collections::BTreeSet;
use std::fmt::Write;

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_BONES: usize = 65_536;

#[derive(Clone, Debug)]
pub struct PoseFile {
    /// Parent model filename (usually an `.osm` reference).
    pub model_name: String,
    pub bones: Vec<(String, Vec3, Quat)>,
}

fn safe_name(name: &str, allow_empty: bool) -> Result<()> {
    ensure!(allow_empty || !name.is_empty(), "empty VPD bone name");
    ensure!(
        name.len() <= 4096
            && !name.contains("//")
            && !name
                .chars()
                .any(|c| c.is_control() || matches!(c, ';' | '{' | '}')),
        "VPD name contains unsupported characters"
    );
    ensure!(
        name.trim() == name,
        "VPD names cannot start or end with whitespace"
    );
    Ok(())
}
fn statement(line: &str) -> Result<&str> {
    let value = line
        .strip_suffix(';')
        .context("VPD statement missing semicolon")?;
    ensure!(!value.contains(';'), "extra VPD statement data");
    Ok(value.trim())
}
fn values<const N: usize>(line: &str) -> Result<[f32; N]> {
    let mut out = [0.0f32; N];
    let mut fields = statement(line)?.split(',');
    for v in &mut out {
        *v = fields
            .next()
            .context("missing VPD vector component")?
            .trim()
            .parse()
            .context("invalid VPD number")?;
        ensure!(v.is_finite(), "non-finite VPD number");
    }
    ensure!(fields.next().is_none(), "extra VPD vector components");
    Ok(out)
}
fn rotation(q: Quat) -> Result<Quat> {
    let length = q.length_squared();
    ensure!(
        q.is_finite() && length.is_finite(),
        "invalid VPD quaternion"
    );
    // The reference importer treats the zero quaternion as the identity pose.
    Ok(if q.to_array().iter().all(|&v| v == 0.0) {
        Quat::IDENTITY
    } else {
        ensure!(length >= 1e-12, "VPD quaternion is too small");
        q.normalize()
    })
}

pub fn parse(bytes: &[u8]) -> Result<PoseFile> {
    ensure!(bytes.len() <= MAX_BYTES, "VPD exceeds 8 MiB limit");
    let (text, errors) = SHIFT_JIS.decode_without_bom_handling(bytes);
    ensure!(!errors, "invalid Shift-JIS VPD text");
    ensure!(!text.contains('\0'), "NUL in VPD text");
    let mut lines = text
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(v, _)| v).trim())
        .filter(|line| !line.is_empty());
    ensure!(
        lines.next() == Some("Vocaloid Pose Data file"),
        "expected VPD signature"
    );
    let model_name = statement(lines.next().context("missing VPD parent filename")?)?.to_owned();
    safe_name(&model_name, true)?;
    let count: usize = statement(lines.next().context("missing VPD bone count")?)?
        .parse()
        .context("invalid VPD bone count")?;
    ensure!(
        count <= MAX_BONES && count <= bytes.len() / 12,
        "invalid VPD bone count"
    );
    let mut bones = Vec::with_capacity(count);
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for _ in 0..count {
        let header = lines.next().context("missing VPD bone block")?;
        let (id, name) = header
            .strip_prefix("Bone")
            .context("expected VPD Bone block")?
            .split_once('{')
            .context("missing VPD bone opening brace")?;
        let id: usize = id.trim().parse().context("invalid VPD bone index")?;
        ensure!(
            id < MAX_BONES && ids.insert(id),
            "invalid or duplicate VPD bone index"
        );
        let name = name.trim().to_owned();
        safe_name(&name, false)?;
        ensure!(names.insert(name.clone()), "duplicate VPD bone name");
        let translation =
            Vec3::from_array(values(lines.next().context("missing VPD translation")?)?);
        let q = values::<4>(lines.next().context("missing VPD rotation")?)?;
        let q = rotation(Quat::from_xyzw(q[0], q[1], q[2], q[3]))?;
        ensure!(lines.next() == Some("}"), "missing VPD bone closing brace");
        bones.push((name, translation, q));
    }
    ensure!(
        lines.next().is_none(),
        "unexpected trailing VPD blocks or count mismatch"
    );
    Ok(PoseFile { model_name, bones })
}

pub fn to_bytes(pose: &PoseFile) -> Result<Vec<u8>> {
    ensure!(pose.bones.len() <= MAX_BONES, "too many VPD bones");
    safe_name(&pose.model_name, true)?;
    let mut text = format!(
        "Vocaloid Pose Data file\r\n\r\n{};\r\n{};\r\n\r\n",
        pose.model_name,
        pose.bones.len()
    );
    let mut names = BTreeSet::new();
    for (i, (name, translation, q)) in pose.bones.iter().enumerate() {
        safe_name(name, false)?;
        ensure!(names.insert(name), "duplicate VPD bone name");
        ensure!(translation.is_finite(), "non-finite VPD translation");
        let q = rotation(*q)?;
        writeln!(
            text,
            "Bone{i}{{{name}\r\n  {},{},{};\r\n  {},{},{},{};\r\n}}\r",
            translation.x, translation.y, translation.z, q.x, q.y, q.z, q.w
        )?;
        text.push_str("\r\n");
        ensure!(text.len() <= MAX_BYTES, "VPD exceeds 8 MiB limit");
    }
    let (bytes, _, errors) = SHIFT_JIS.encode(&text);
    ensure!(!errors, "VPD names cannot be represented in Shift-JIS");
    ensure!(bytes.len() <= MAX_BYTES, "VPD exceeds 8 MiB limit");
    Ok(bytes.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> PoseFile {
        PoseFile {
            model_name: "初音ミク.osm".into(),
            bones: vec![(
                "センター".into(),
                Vec3::new(1.0, -2.0, 0.25),
                Quat::from_rotation_y(0.7),
            )],
        }
    }
    #[test]
    fn round_trip_japanese_and_transform() {
        let pose = sample();
        let bytes = to_bytes(&pose).unwrap();
        assert!(bytes.windows(2).any(|b| b == b"\r\n"));
        let loaded = parse(&bytes).unwrap();
        assert_eq!(loaded.model_name, pose.model_name);
        assert_eq!(loaded.bones[0].0, pose.bones[0].0);
        assert_eq!(loaded.bones[0].1, pose.bones[0].1);
        assert!(loaded.bones[0].2.abs_diff_eq(pose.bones[0].2, 1e-6));
    }
    #[test]
    fn comments_zero_rotation_and_arbitrary_index() {
        let p=parse(b"Vocaloid Pose Data file\n\nx.osm; // parent\n1;\nBone8{center\n0,0,0; // t\n0,0,0,0;\n}\n").unwrap();
        assert_eq!(p.bones[0].2, Quat::IDENTITY);
    }
    #[test]
    fn rejects_malformed_and_injection() {
        for text in [
            "",
            "Vocaloid Pose Data file\nx;\n999999999;",
            "Vocaloid Pose Data file\nx;\n1;\nBone0{x\nNaN,0,0;\n0,0,0,1;\n}",
            "Vocaloid Pose Data file\nx;\n0;\nMorph0{x\n1;\n}",
        ] {
            assert!(parse(text.as_bytes()).is_err());
        }
        let mut pose = sample();
        pose.bones[0].0 = "bad\nBone0{x".into();
        assert!(to_bytes(&pose).is_err());
        pose.bones[0].0 = "😀".into();
        assert!(to_bytes(&pose).is_err());
        assert!(parse(&[0x81]).is_err());
    }
    #[test]
    fn every_nonwhitespace_truncation_rejected() {
        let bytes = to_bytes(&sample()).unwrap();
        let required_end = bytes.iter().rposition(|&b| b == b'}').unwrap() + 1;
        for end in 0..required_end {
            assert!(parse(&bytes[..end]).is_err(), "accepted {end}");
        }
    }
    fn asset(name: &str) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("reference/original/UserFile/Pose")
            .join(name);
        let bytes = std::fs::read(path).expect("run python3 scripts/fetch_reference.py first");
        let pose = parse(&bytes).unwrap();
        assert_eq!(pose.bones.len(), 14);
        let round = parse(&to_bytes(&pose).unwrap()).unwrap();
        assert_eq!(round.model_name, pose.model_name);
        for (a, b) in pose.bones.iter().zip(round.bones.iter()) {
            assert_eq!(a.0, b.0);
            assert_eq!(a.1, b.1);
            assert!(a.2.abs_diff_eq(b.2, 1e-6));
        }
    }
    #[test]
    #[ignore = "requires downloaded MMD reference assets"]
    fn original_left_closed() {
        asset("左手グー.vpd");
    }
    #[test]
    #[ignore = "requires downloaded MMD reference assets"]
    fn original_right_closed() {
        asset("右手グー.vpd");
    }
    #[test]
    #[ignore = "requires downloaded MMD reference assets"]
    fn original_left_open() {
        asset("左手半開き.vpd");
    }
    #[test]
    #[ignore = "requires downloaded MMD reference assets"]
    fn original_right_open() {
        asset("右手半開き.vpd");
    }
}
