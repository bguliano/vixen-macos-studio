//! Stamps the build's commit into studio-mcp, for `studio_status`, `initialize` and the About window (#71).
//!
//! Reads the checkout with local `git` only. `scripts/build_app.sh` passes `VIXEN_BUILD_DIRTY` fresh on every build, since
//! editing a tracked file changes nothing this script can watch; plain `cargo` builds ask git here instead.
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    // GIT_OPTIONAL_LOCKS=0: `git status` must not rewrite the index this script watches.
    let out = Command::new("git").current_dir(dir).env("GIT_OPTIONAL_LOCKS", "0").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-env-changed=VIXEN_BUILD_DIRTY");
    println!("cargo:rerun-if-changed=build.rs");
    // A new commit or checkout moves HEAD or the branch it names; staging changes the index.
    if let Some(git_dir) = git(&dir, &["rev-parse", "--absolute-git-dir"]) {
        let git_dir = PathBuf::from(git_dir);
        for file in ["HEAD", "index", "packed-refs"] {
            println!("cargo:rerun-if-changed={}", git_dir.join(file).display());
        }
        if let Some(branch) = git(&dir, &["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={}", git_dir.join(branch).display());
        }
    }
    let commit = git(&dir, &["rev-parse", "HEAD"]).unwrap_or_default();
    let date = git(&dir, &["log", "-1", "--format=%cI"]).unwrap_or_default();
    let dirty = match std::env::var("VIXEN_BUILD_DIRTY").ok().as_deref() {
        Some("1") => "true",
        Some("0") => "false",
        _ => match git(&dir, &["status", "--porcelain", "--untracked-files=no", "--ignore-submodules=dirty"]) {
            Some(changes) if changes.is_empty() => "false",
            Some(_) => "true",
            None => "",
        },
    };
    println!("cargo:rustc-env=STUDIO_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=STUDIO_BUILD_DATE={date}");
    println!("cargo:rustc-env=STUDIO_BUILD_DIRTY={dirty}");
}
