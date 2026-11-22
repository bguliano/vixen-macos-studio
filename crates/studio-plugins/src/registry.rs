//! Where plugins live and which are enabled.
//!
//! ```text
//! Plugins/                      (0700)
//!   registry.json               {"version":1,"plugins":{"<id>":{"enabled","source":"installed"|"dev","version"?,"path"?,"sha256"?}}}
//!   <id>/<version>/vixen-plugin.json   installed plugins
//!   data/<id>/                  each plugin's pluginDataDir
//!   logs/<id>.log               each plugin's log
//! ```
//!
//! A plugin folder found on disk without a registry entry is listed but disabled: only an install (which shows the
//! permissions) or the user enables it.
use crate::manifest::{valid_plugin_id, Manifest};
use crate::semver::Version;
use serde_json::{json, Map, Value};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const REGISTRY_FILE: &str = "registry.json";
const MAX_REGISTRY_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Installed,
    Dev,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Installed => "installed",
            Source::Dev => "dev",
        }
    }
}

/// One registry entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub enabled: bool,
    pub source: Source,
    /// The active installed version.
    pub version: Option<String>,
    /// A dev plugin's folder.
    pub path: Option<PathBuf>,
    pub sha256: Option<String>,
}

/// A plugin found in the registry or on disk, with its manifest or the reason it couldn't be read.
#[derive(Clone, Debug)]
pub struct Discovered {
    pub id: String,
    pub dir: PathBuf,
    pub source: Source,
    pub enabled: bool,
    pub manifest: Result<Manifest, String>,
}

pub struct Registry {
    root: PathBuf,
    records: Map<String, Value>,
    /// A note for the host log, e.g. that a damaged registry was set aside.
    pub warning: Option<String>,
}

/// Creates `dir` (and parents) and makes it 0700, refusing a symlink in its place.
pub fn private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let meta = fs::symlink_metadata(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    if meta.permissions().mode() & 0o777 != 0o700 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}

/// Writes `bytes` to `path` through a new temporary file in the same folder, so readers never see half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("no parent folder")?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or("bad file name")?;
    for attempt in 0..16u32 {
        let temp = parent.join(format!(".{name}.{}.{attempt}.tmp", std::process::id()));
        let file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp);
        let mut file = match file {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", temp.display())),
        };
        let written = file.write_all(bytes).and_then(|_| file.sync_all()).and_then(|_| fs::rename(&temp, path));
        if let Err(e) = written {
            let _ = fs::remove_file(&temp);
            return Err(format!("{}: {e}", path.display()));
        }
        return Ok(());
    }
    Err(format!("{}: could not create a temporary file", path.display()))
}

/// Reads a small JSON file, refusing symlinks and anything over `max` bytes. `Ok(None)` when it doesn't exist.
pub fn read_json(path: &Path, max: u64) -> Result<Option<Value>, String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if !meta.is_file() || meta.len() > max {
        return Err(format!("{} is not a regular file under {} KiB", path.display(), max / 1024));
    }
    let text = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&text).map(Some).map_err(|e| format!("{} is not valid JSON: {e}", path.display()))
}

impl Registry {
    pub fn open(root: &Path) -> Result<Registry, String> {
        private_dir(root)?;
        let path = root.join(REGISTRY_FILE);
        let mut warning = None;
        let records = match read_json(&path, MAX_REGISTRY_BYTES) {
            Ok(None) => Map::new(),
            Ok(Some(value)) => match value.get("plugins").and_then(Value::as_object) {
                Some(plugins) => plugins.clone(),
                None => {
                    warning = set_aside(&path, "has no \"plugins\" object");
                    Map::new()
                }
            },
            Err(why) => {
                warning = set_aside(&path, &why);
                Map::new()
            }
        };
        // Ids become folder and file names: an entry that isn't a valid id (say "../x") is ignored.
        let records = records.into_iter().filter(|(id, value)| valid_plugin_id(id) && value.is_object()).collect();
        Ok(Registry { root: root.to_path_buf(), records, warning })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn data_dir(&self, id: &str) -> PathBuf {
        self.root.join("data").join(id)
    }

    pub fn log_path(&self, id: &str) -> PathBuf {
        self.root.join("logs").join(format!("{id}.log"))
    }

    pub fn record(&self, id: &str) -> Option<Record> {
        let value = self.records.get(id)?.as_object()?;
        Some(Record {
            enabled: value.get("enabled").and_then(Value::as_bool).unwrap_or(false),
            source: if value.get("source").and_then(Value::as_str) == Some("dev") { Source::Dev } else { Source::Installed },
            version: value.get("version").and_then(Value::as_str).map(str::to_string),
            path: value.get("path").and_then(Value::as_str).map(PathBuf::from),
            sha256: value.get("sha256").and_then(Value::as_str).map(str::to_string),
        })
    }

    pub fn set_record(&mut self, id: &str, record: &Record) -> Result<(), String> {
        let mut value = json!({"enabled": record.enabled, "source": record.source.as_str()});
        if let Some(version) = &record.version {
            value["version"] = json!(version);
        }
        if let Some(path) = &record.path {
            value["path"] = json!(path.to_string_lossy());
        }
        if let Some(sha256) = &record.sha256 {
            value["sha256"] = json!(sha256);
        }
        self.records.insert(id.to_string(), value);
        self.save()
    }

    pub fn remove_record(&mut self, id: &str) -> Result<(), String> {
        self.records.remove(id);
        self.save()
    }

    fn save(&self) -> Result<(), String> {
        let text = serde_json::to_vec_pretty(&json!({"version": 1, "plugins": self.records})).expect("JSON serializes");
        write_atomic(&self.root.join(REGISTRY_FILE), &text)
    }

    /// Enables or disables a plugin, creating its record when it was only found on disk.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        let mut record = self.record(id).unwrap_or(Record { enabled, source: Source::Installed, version: None, path: None, sha256: None });
        record.enabled = enabled;
        self.set_record(id, &record)
    }

    /// Links a development folder, enabled. Its manifest must be valid; the id comes from it.
    pub fn link_dev(&mut self, folder: &Path) -> Result<Manifest, String> {
        if !folder.is_absolute() {
            return Err("a development folder must be an absolute path".into());
        }
        let meta = fs::symlink_metadata(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        if !meta.is_dir() {
            return Err(format!("{} is not a folder (symlinks are not followed)", folder.display()));
        }
        let folder = folder.canonicalize().map_err(|e| format!("{}: {e}", folder.display()))?;
        let manifest = Manifest::load(&folder)?;
        self.set_record(&manifest.id, &Record { enabled: true, source: Source::Dev, version: None, path: Some(folder), sha256: None })?;
        Ok(manifest)
    }

    /// Every plugin: registry entries first, then installed folders without one. Sorted by id.
    pub fn discover(&self) -> Vec<Discovered> {
        let mut found: Vec<Discovered> = Vec::new();
        for id in self.records.keys() {
            let record = self.record(id).expect("records are objects");
            let found_one = match record.source {
                Source::Dev => {
                    let dir = record.path.clone().unwrap_or_default();
                    let manifest = dev_manifest(&dir).and_then(|m| matching(m, id, None));
                    Discovered { id: id.clone(), dir, source: Source::Dev, enabled: record.enabled, manifest }
                }
                Source::Installed => {
                    let (dir, manifest) = self.installed(id, record.version.as_deref());
                    Discovered { id: id.clone(), dir, source: Source::Installed, enabled: record.enabled, manifest }
                }
            };
            found.push(found_one);
        }
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let Some(id) = entry.file_name().to_str().map(str::to_string) else { continue };
                if !valid_plugin_id(&id) || self.records.contains_key(&id) || !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let (dir, manifest) = self.installed(&id, None);
                found.push(Discovered { id, dir, source: Source::Installed, enabled: false, manifest });
            }
        }
        found.sort_by(|a, b| a.id.cmp(&b.id));
        found
    }

    /// The installed folder for `id`: the registry's version when present, otherwise the newest version on disk.
    fn installed(&self, id: &str, version: Option<&str>) -> (PathBuf, Result<Manifest, String>) {
        let base = self.root.join(id);
        let chosen = match version {
            Some(version) if Version::parse(version).is_err() => {
                return (base.clone(), Err(format!("the registry names an invalid version \"{version}\"")));
            }
            Some(version) => Some(version.to_string()),
            None => fs::read_dir(&base).ok().and_then(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .filter_map(|e| e.file_name().to_str().and_then(|n| Version::parse(n).ok()))
                    .max()
                    .map(|v| v.to_string())
            }),
        };
        let Some(chosen) = chosen else {
            return (base.clone(), Err(format!("no installed version in {}", base.display())));
        };
        let dir = base.join(&chosen);
        let manifest = match fs::symlink_metadata(&dir) {
            Ok(meta) if meta.is_dir() => Manifest::load(&dir).and_then(|m| matching(m, id, Some(&chosen))),
            Ok(_) => Err(format!("{} is not a folder", dir.display())),
            Err(e) => Err(format!("{}: {e}", dir.display())),
        };
        (dir, manifest)
    }
}

fn dev_manifest(dir: &Path) -> Result<Manifest, String> {
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() => Manifest::load(dir),
        Ok(_) => Err(format!("{} is not a folder", dir.display())),
        Err(e) => Err(format!("development folder {}: {e}", dir.display())),
    }
}

fn matching(manifest: Manifest, id: &str, version: Option<&str>) -> Result<Manifest, String> {
    if manifest.id != id {
        return Err(format!("its manifest says id \"{}\", but it is registered as \"{id}\"", manifest.id));
    }
    if let Some(version) = version {
        if manifest.version.to_string() != version {
            return Err(format!("its manifest says version {}, but it is installed as {version}", manifest.version));
        }
    }
    Ok(manifest)
}

fn set_aside(path: &Path, why: &str) -> Option<String> {
    let aside = path.with_extension(format!("json.damaged-{}", crate::logs::now_ms()));
    let _ = fs::rename(path, &aside);
    Some(format!("plugin registry {why}; moved it to {} and started an empty one", aside.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("studio-plugins-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn write_manifest(dir: &Path, id: &str, version: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("vixen-plugin.json"),
            json!({"id": id, "name": "Test", "version": version, "apiVersion": "^1.0", "entry": {"command": "./run"}}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn installed_folders_are_found_disabled_until_enabled() {
        let root = temp("registry-installed");
        write_manifest(&root.join("com.example.a/1.0.0"), "com.example.a", "1.0.0");
        write_manifest(&root.join("com.example.a/1.2.0"), "com.example.a", "1.2.0");
        write_manifest(&root.join("com.example.b/2.0.0"), "com.example.wrong", "2.0.0");
        fs::create_dir_all(root.join("logs")).unwrap();
        let mut registry = Registry::open(&root).unwrap();
        assert_eq!(fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o700);
        let found = registry.discover();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].manifest.as_ref().unwrap().version, Version::new(1, 2, 0));
        assert!(!found[0].enabled);
        assert!(found[1].manifest.as_ref().unwrap_err().contains("registered as"));

        registry.set_enabled("com.example.a", true).unwrap();
        let reopened = Registry::open(&root).unwrap();
        assert!(reopened.record("com.example.a").unwrap().enabled);
        assert!(reopened.discover()[0].enabled);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dev_folders_link_by_absolute_path_and_never_through_symlinks() {
        let root = temp("registry-dev");
        let dev = temp("registry-dev-folder");
        write_manifest(&dev, "com.example.dev", "0.1.0");
        let mut registry = Registry::open(&root).unwrap();
        assert!(registry.link_dev(Path::new("relative/path")).is_err());
        let link = temp("registry-dev-link");
        std::os::unix::fs::symlink(&dev, &link).unwrap();
        assert!(registry.link_dev(&link).unwrap_err().contains("symlinks are not followed"));
        assert_eq!(registry.link_dev(&dev).unwrap().id, "com.example.dev");
        let found = Registry::open(&root).unwrap().discover();
        assert_eq!((found[0].source, found[0].enabled), (Source::Dev, true));
        assert_eq!(found[0].dir, dev.canonicalize().unwrap());
        for dir in [&root, &dev] {
            fs::remove_dir_all(dir).unwrap();
        }
        fs::remove_file(&link).unwrap();
    }

    #[test]
    fn entries_that_are_not_plugin_ids_are_ignored() {
        let root = temp("registry-ids");
        fs::create_dir_all(&root).unwrap();
        let registry = json!({"version": 1, "plugins": {"../escape": {"enabled": true, "source": "dev", "path": "/tmp"}, "com.example.ok": {"enabled": true}, "com.example.bad": 3}});
        fs::write(root.join(REGISTRY_FILE), registry.to_string()).unwrap();
        let found = Registry::open(&root).unwrap().discover();
        assert_eq!(found.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["com.example.ok"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_damaged_registry_is_set_aside() {
        let root = temp("registry-damaged");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(REGISTRY_FILE), "{not json").unwrap();
        let registry = Registry::open(&root).unwrap();
        assert!(registry.warning.as_ref().unwrap().contains("not valid JSON"));
        assert!(fs::read_dir(&root).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("damaged")));
        fs::remove_dir_all(&root).unwrap();
    }
}
