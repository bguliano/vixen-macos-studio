//! `vixen-studio-mcp plugin pack`: a plugin folder as a `.vixenplugin` package (a zip of the folder, manifest at its
//! root, the form `plugin_install` and Settings ▸ Plugins take), with its SHA-256 written next to it. The same folder
//! always packs to the same bytes: files in name order, fixed timestamps, only the executable bit of each mode.
use crate::install::{sha256_hex, MAX_BYTES, MAX_FILES, PACKAGE_EXTENSION};
use crate::manifest::Manifest;
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Left out of packages: version control, editor and OS clutter, caches, Rust's build folder (a Rust plugin ships its
/// program in `bin/`), and earlier packages.
fn skipped(name: &str) -> bool {
    matches!(name, ".git" | ".hg" | ".svn" | ".DS_Store" | ".gitignore" | ".vscode" | ".idea" | "__pycache__" | "target")
        || name.ends_with(".pyc")
        || name.ends_with(&format!(".{PACKAGE_EXTENSION}"))
        || name.ends_with(&format!(".{PACKAGE_EXTENSION}.sha256"))
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf, bool)>, bytes: &mut u64) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if skipped(&name) {
            continue;
        }
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let relative = path.strip_prefix(root).expect("inside the folder").to_string_lossy().into_owned();
        if meta.file_type().is_symlink() {
            return Err(format!("{relative} is a symlink; packages can't contain symlinks (copy the file in instead)"));
        }
        if meta.is_dir() {
            collect(root, &path, out, bytes)?;
        } else if meta.is_file() {
            *bytes += meta.len();
            if out.len() >= MAX_FILES || *bytes > MAX_BYTES {
                return Err(format!("the plugin is larger than a package may be ({MAX_FILES} files, {} MiB)", MAX_BYTES / 1024 / 1024));
            }
            out.push((relative, path, meta.permissions().mode() & 0o111 != 0));
        }
    }
    Ok(())
}

/// Packs `dir` into `<out_dir>/<id>-<version>.vixenplugin` (default: the folder's parent) and writes
/// `<that>.sha256`. Refuses an invalid manifest, a missing program (`entry.command` inside the folder, such as a Rust
/// plugin's `bin/` before it is built) and an existing package unless `overwrite`. Returns `{path, sha256, files,
/// bytes, id, version}`.
pub fn pack(dir: &Path, out_dir: Option<&Path>, overwrite: bool) -> Result<Value, String> {
    let dir = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
    let manifest = Manifest::load(&dir)?;
    if manifest.entry.command.contains('/') {
        crate::process::resolve_command(&dir, &manifest.entry.command).map_err(|why| format!("{why}; build the plugin before packing it"))?;
    }
    let mut files = Vec::new();
    let mut bytes = 0;
    collect(&dir, &dir, &mut files, &mut bytes)?;
    let out_dir = match out_dir {
        Some(path) => path.to_path_buf(),
        None => dir.parent().ok_or("the folder has no parent to put the package in")?.to_path_buf(),
    };
    let target = out_dir.join(format!("{}-{}.{PACKAGE_EXTENSION}", manifest.id, manifest.version));
    if !overwrite && target.exists() {
        return Err(format!("{} already exists (pass --force to replace it)", target.display()));
    }
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, path, executable) in &files {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(if *executable { 0o755 } else { 0o644 });
        let contents = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        zip.start_file(name.as_str(), options).map_err(|e| e.to_string())?;
        zip.write_all(&contents).map_err(|e| e.to_string())?;
    }
    let package = zip.finish().map_err(|e| e.to_string())?.into_inner();
    let sha256 = sha256_hex(&package);
    let temp = out_dir.join(format!(".{}.{}.tmp", target.file_name().and_then(|n| n.to_str()).unwrap_or("package"), std::process::id()));
    std::fs::write(&temp, &package).and_then(|_| std::fs::rename(&temp, &target)).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("{}: {e}", target.display())
    })?;
    let file_name = target.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let checksum = PathBuf::from(format!("{}.sha256", target.display()));
    std::fs::write(&checksum, format!("{sha256}  {file_name}\n")).map_err(|e| format!("{}: {e}", checksum.display()))?;
    Ok(json!({
        "path": target.to_string_lossy(),
        "sha256": sha256,
        "sha256File": checksum.to_string_lossy(),
        "files": files.len(),
        "bytes": package.len(),
        "id": manifest.id,
        "version": manifest.version.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vixen-pack-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn packs_what_install_accepts_and_the_same_folder_packs_the_same_bytes() {
        let parent = temp("src");
        let made = crate::scaffold::create(&parent, "com.example.packed", "Packed", "python", "controller", None).unwrap();
        let dir = PathBuf::from(made["folder"].as_str().unwrap());
        std::fs::create_dir_all(dir.join("__pycache__")).unwrap();
        std::fs::write(dir.join("__pycache__/x.pyc"), b"junk").unwrap();
        std::fs::write(dir.join("run.sh"), b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(dir.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = temp("out");
        let first = pack(&dir, Some(&out), false).unwrap();
        assert!(first["path"].as_str().unwrap().ends_with("com.example.packed-0.1.0.vixenplugin"));
        assert!(pack(&dir, Some(&out), false).unwrap_err().contains("already exists"));
        let second = pack(&dir, Some(&out), true).unwrap();
        assert_eq!(first["sha256"], second["sha256"], "deterministic");
        let checksum = std::fs::read_to_string(second["sha256File"].as_str().unwrap()).unwrap();
        assert!(checksum.starts_with(second["sha256"].as_str().unwrap()));

        let root = temp("root");
        let staged = crate::install::stage(&root, Path::new(first["path"].as_str().unwrap())).unwrap();
        assert_eq!(staged.manifest.as_ref().unwrap().id, "com.example.packed");
        assert_eq!(staged.sha256.as_deref(), first["sha256"].as_str());
        assert!(!staged.dir.join("__pycache__").exists());
        assert!(staged.dir.join("vixen_plugin.py").is_file());
        assert_eq!(std::fs::metadata(staged.dir.join("run.sh")).unwrap().permissions().mode() & 0o777, 0o700);
        drop(staged);
        for d in [parent, out, root] {
            std::fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn refuses_an_unbuilt_program_and_symlinks() {
        let parent = temp("refuse");
        let made = crate::scaffold::create(&parent, "com.example.unbuilt", "Unbuilt", "rust", "tools", None).unwrap();
        let dir = PathBuf::from(made["folder"].as_str().unwrap());
        assert!(pack(&dir, Some(&parent), false).unwrap_err().contains("build the plugin"));
        let made = crate::scaffold::create(&parent, "com.example.linked", "Linked", "python", "tools", None).unwrap();
        let dir = PathBuf::from(made["folder"].as_str().unwrap());
        std::os::unix::fs::symlink("/etc/hosts", dir.join("hosts")).unwrap();
        assert!(pack(&dir, Some(&parent), false).unwrap_err().contains("symlink"));
        std::fs::remove_dir_all(parent).unwrap();
    }
}
