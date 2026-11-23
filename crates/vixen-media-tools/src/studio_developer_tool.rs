//! Developer builds find the staged tools next to the running test or tool. Studio's worktrees share the build bay's
//! target folder (`target-dir` in their `.cargo/config.toml`), so a worktree has no `target/media-tools` of its own and
//! `CARGO_MANIFEST_DIR` can name a checkout that no longer exists. Cargo always puts executables under the target
//! folder, which is where `scripts/build_media_tools.sh` stages `media-tools/bin`.

use std::path::{Path, PathBuf};

#[cfg(not(vixen_app_bundle))]
pub(crate) fn find(name: &str, executable: &Path) -> Option<PathBuf> {
    executable
        .ancestors()
        .skip(1)
        .map(|directory| directory.join("media-tools/bin").join(name))
        .find(|path| path.is_file())
}

/// App-bundle builds only ever use the bundle's own Helpers.
#[cfg(vixen_app_bundle)]
pub(crate) fn find(_name: &str, _executable: &Path) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(vixen_app_bundle))]
    fn tools_are_found_in_the_target_folder_holding_the_executable() {
        let root =
            std::env::temp_dir().join(format!("vixen-media-tools-find-{}", std::process::id()));
        let bin = root.join("target/media-tools/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("ffprobe"), b"").unwrap();
        let executable = root.join("target/debug/deps/vixen_session-0123456789abcdef");
        assert_eq!(
            crate::locate("ffprobe", &executable).unwrap(),
            bin.join("ffprobe")
        );
        assert!(find("ffmpeg", &executable).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
