//! Cargo's target folder, for tests that keep a sample show or scratch profiles there. Studio's worktrees build into
//! the build bay's target folder (`target-dir` in their `.cargo/config.toml`), so `CARGO_MANIFEST_DIR/../../target`
//! doesn't exist in a worktree. Every test executable lives under the target folder, and Cargo keeps
//! `.rustc_info.json` at its root. vixen-session's Studio tests include this file too.

use std::path::{Path, PathBuf};

pub fn target_dir() -> PathBuf {
    let executable = std::env::current_exe().ok();
    let found = executable.as_deref().and_then(|exe| exe.ancestors().skip(1).find(|d| d.join(".rustc_info.json").is_file()));
    found.map(Path::to_path_buf).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
}
