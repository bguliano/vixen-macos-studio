//! Conservative write boundary for profile outputs and their automatic backups.
use std::path::Path;
pub fn reject_links(directory: &Path, depth: usize) -> Result<(), String> {
    if depth > 8 {
        return Err("Output backup directory nesting exceeds safety limit".into());
    }
    for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err(
                "Refusing output in a directory containing symlinks (including backups/temp files)"
                    .into(),
            );
        }
        if kind.is_dir() {
            reject_links(&entry.path(), depth + 1)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_symlinked_export_part_and_backup_tree() {
        let root = std::env::temp_dir().join(format!("studio-paths-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let outside = root.with_extension("sentinel");
        std::fs::write(&outside, b"unchanged").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("show.fseq.part")).unwrap();
        assert!(reject_links(&root, 0).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"unchanged");
        std::fs::remove_file(root.join("show.fseq.part")).unwrap();
        std::os::unix::fs::symlink(root.parent().unwrap(), root.join("auto_backup")).unwrap();
        assert!(reject_links(&root, 0).is_err());
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_file(outside).unwrap();
    }
}
