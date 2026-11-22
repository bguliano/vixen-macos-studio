//! `<profile>/SystemData/StudioPlugins.json`: the plugins a show's controllers need, so a show moved to another Mac can
//! say "needs Govee Output ≥1.2" instead of silently losing lights. Vanilla and Windows Vixen ignore the file.
use crate::registry::{read_json, write_atomic};
use crate::semver::{Range, Version};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const FILE: &str = "StudioPlugins.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Requirement {
    pub id: String,
    pub version_range: String,
}

pub fn path(profile: &Path) -> PathBuf {
    profile.join("SystemData").join(FILE)
}

/// The show's requirements; none when the file is missing.
pub fn read(profile: &Path) -> Result<Vec<Requirement>, String> {
    let Some(value) = read_json(&path(profile), 1024 * 1024)? else { return Ok(Vec::new()) };
    let items = value.get("plugins").and_then(Value::as_array).ok_or_else(|| format!("{FILE} has no \"plugins\" list"))?;
    let mut out = Vec::new();
    for item in items {
        let id = item.get("id").and_then(Value::as_str).filter(|id| crate::manifest::valid_plugin_id(id));
        let range = item.get("versionRange").and_then(Value::as_str).filter(|r| Range::parse(r).is_ok());
        match (id, range) {
            (Some(id), Some(range)) => out.push(Requirement { id: id.into(), version_range: range.into() }),
            _ => return Err(format!("{FILE} has an entry without a valid id and versionRange: {item}")),
        }
    }
    Ok(out)
}

fn write(profile: &Path, requirements: &[Requirement]) -> Result<(), String> {
    let mut sorted = requirements.to_vec();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let plugins: Vec<Value> = sorted.iter().map(|r| json!({"id": r.id, "versionRange": r.version_range})).collect();
    let dir = path(profile).parent().expect("has a parent").to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_atomic(&path(profile), &serde_json::to_vec_pretty(&json!({"version": 1, "plugins": plugins})).expect("serializes"))
}

/// Records that the show needs `id` in `version_range` (replacing an older range for it). Returns whether it changed.
pub fn require(profile: &Path, id: &str, version_range: &str) -> Result<bool, String> {
    Range::parse(version_range)?;
    let mut requirements = read(profile)?;
    if requirements.iter().any(|r| r.id == id && r.version_range == version_range) {
        return Ok(false);
    }
    requirements.retain(|r| r.id != id);
    requirements.push(Requirement { id: id.into(), version_range: version_range.into() });
    write(profile, &requirements).map(|_| true)
}

/// Makes the file list exactly `requirements` (no file is created for none). Returns whether it changed.
pub fn replace(profile: &Path, requirements: &[Requirement]) -> Result<bool, String> {
    let mut current = read(profile)?;
    let mut wanted = requirements.to_vec();
    current.sort_by(|a, b| a.id.cmp(&b.id));
    wanted.sort_by(|a, b| a.id.cmp(&b.id));
    if current == wanted {
        return Ok(false);
    }
    for r in &wanted {
        Range::parse(&r.version_range)?;
    }
    write(profile, &wanted).map(|_| true)
}

/// Drops `id` once no controller in the show uses it. Returns whether it changed.
pub fn release(profile: &Path, id: &str) -> Result<bool, String> {
    let mut requirements = read(profile)?;
    let before = requirements.len();
    requirements.retain(|r| r.id != id);
    if requirements.len() == before {
        return Ok(false);
    }
    write(profile, &requirements).map(|_| true)
}

/// The range a new controller of a plugin at `version` records: compatible releases from that one on (`^1.2.0`).
pub fn default_range(version: &Version) -> String {
    format!("^{}.{}.{}", version.major, version.minor, version.patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirements_round_trip_sorted_and_deduplicated() {
        let profile = std::env::temp_dir().join(format!("studio-plugins-profile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&profile);
        assert!(read(&profile).unwrap().is_empty());
        assert!(require(&profile, "com.example.zed", "^1.0.0").unwrap());
        assert!(require(&profile, "com.example.alpha", "^2.1.0").unwrap());
        assert!(!require(&profile, "com.example.alpha", "^2.1.0").unwrap());
        assert!(require(&profile, "com.example.alpha", "^2.3.0").unwrap());
        assert!(require(&profile, "com.example.alpha", "not a range").is_err());
        let ids: Vec<(String, String)> = read(&profile).unwrap().into_iter().map(|r| (r.id, r.version_range)).collect();
        assert_eq!(ids, [("com.example.alpha".to_string(), "^2.3.0".to_string()), ("com.example.zed".to_string(), "^1.0.0".to_string())]);
        assert!(release(&profile, "com.example.zed").unwrap());
        assert!(!release(&profile, "com.example.zed").unwrap());
        assert_eq!(read(&profile).unwrap().len(), 1);
        assert_eq!(default_range(&Version::new(1, 2, 0)), "^1.2.0");
        std::fs::write(path(&profile), r#"{"plugins":[{"id":"BAD"}]}"#).unwrap();
        assert!(read(&profile).is_err());
        std::fs::remove_dir_all(&profile).unwrap();
    }
}
