//! Installing a plugin from a folder or a `.vixenplugin` package (a zip of the plugin folder, manifest at its root) into
//! `Plugins/<id>/<version>/`, and removing it again. An install unpacks into a temporary folder next to the plugins,
//! validates the manifest there, then moves the folder into place in one rename, so a failed install leaves nothing.
use crate::manifest::Manifest;
use crate::registry::private_dir;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

/// Limits on what a plugin may bring.
pub const MAX_FILES: usize = 20_000;
pub const MAX_BYTES: u64 = 512 * 1024 * 1024;
pub const PACKAGE_EXTENSION: &str = "vixenplugin";

/// An unpacked, validated plugin waiting to be moved into place.
pub struct Staged {
    pub dir: PathBuf,
    /// Always set once `stage` returns.
    pub manifest: Option<Manifest>,
    /// The package's SHA-256 (hex); `None` for a folder.
    pub sha256: Option<String>,
    pub files: usize,
    pub bytes: u64,
}

impl Drop for Staged {
    fn drop(&mut self) {
        // Still here means it was never moved into place.
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Copies or unpacks `source` into a new temporary folder under `root` and validates it.
pub fn stage(root: &Path, source: &Path) -> Result<Staged, String> {
    if !source.is_absolute() {
        return Err("the plugin path must be absolute".into());
    }
    let meta = fs::symlink_metadata(source).map_err(|e| format!("{}: {e}", source.display()))?;
    private_dir(root)?;
    let temp = root.join(format!(".install-{}-{}", std::process::id(), uuid::Uuid::new_v4().simple()));
    private_dir(&temp)?;
    // Removed on any failure below; on success the Staged owns it.
    let mut staged = Staged { dir: temp, manifest: None, sha256: None, files: 0, bytes: 0 };
    let mut counter = Counter::default();
    if meta.is_dir() {
        copy_tree(source, &staged.dir, &mut counter)?;
    } else if meta.is_file() && source.extension().and_then(|e| e.to_str()) == Some(PACKAGE_EXTENSION) {
        let bytes = fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
        staged.sha256 = Some(sha256_hex(&bytes));
        unzip(&bytes, &staged.dir, &mut counter)?;
    } else {
        return Err(format!("{} is neither a plugin folder nor a .{PACKAGE_EXTENSION} package (symlinks are not followed)", source.display()));
    }
    staged.manifest = Some(Manifest::load(&staged.dir)?);
    staged.files = counter.files;
    staged.bytes = counter.bytes;
    Ok(staged)
}

/// Moves a staged plugin to `<root>/<id>/<version>`, replacing an earlier copy of the same version. Returns the folder.
pub fn commit(root: &Path, staged: &Staged) -> Result<PathBuf, String> {
    let manifest = staged.manifest.as_ref().ok_or("nothing staged")?;
    let parent = root.join(&manifest.id);
    private_dir(&parent)?;
    let target = parent.join(manifest.version.to_string());
    if fs::symlink_metadata(&target).is_ok() {
        fs::remove_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    fs::rename(&staged.dir, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    strip_quarantine(&target);
    Ok(target)
}

/// Removes every installed version of `id`. Its data and logs stay.
pub fn remove(root: &Path, id: &str) -> Result<(), String> {
    let dir = root.join(id);
    match fs::symlink_metadata(&dir) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display())),
        Ok(_) => Err(format!("{} is not a folder", dir.display())),
        Err(_) => Ok(()),
    }
}

#[derive(Default)]
struct Counter {
    files: usize,
    bytes: u64,
}

impl Counter {
    fn add(&mut self, bytes: u64) -> Result<(), String> {
        self.files += 1;
        self.bytes += bytes;
        if self.files > MAX_FILES {
            return Err(format!("the plugin has more than {MAX_FILES} files"));
        }
        if self.bytes > MAX_BYTES {
            return Err(format!("the plugin is larger than {} MiB", MAX_BYTES / 1024 / 1024));
        }
        Ok(())
    }
}

fn copy_tree(from: &Path, to: &Path, counter: &mut Counter) -> Result<(), String> {
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if kind.is_symlink() {
            return Err(format!("{} is a symlink; a plugin must be plain files", source.display()));
        } else if kind.is_dir() {
            fs::create_dir(&target).map_err(|e| format!("{}: {e}", target.display()))?;
            copy_tree(&source, &target, counter)?;
        } else if kind.is_file() {
            let size = entry.metadata().map_err(|e| e.to_string())?.len();
            counter.add(size)?;
            fs::copy(&source, &target).map_err(|e| format!("{}: {e}", source.display()))?;
        }
    }
    Ok(())
}

fn unzip(bytes: &[u8], to: &Path, counter: &mut Counter) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("not a valid .{PACKAGE_EXTENSION} (zip): {e}"))?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = file.name().map_err(|e| e.to_string())?.to_string();
        let relative = safe_path(&name).ok_or_else(|| format!("the package has an unsafe path: {name}"))?;
        let mode = file.unix_mode();
        if mode.is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(format!("the package has a symlink ({name}); a plugin must be plain files"));
        }
        let target = to.join(&relative);
        if file.is_dir() {
            fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
            continue;
        }
        counter.add(file.size())?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let mut contents = Vec::with_capacity(file.size().min(MAX_BYTES) as usize);
        (&mut file).take(MAX_BYTES + 1).read_to_end(&mut contents).map_err(|e| format!("{name}: {e}"))?;
        if contents.len() as u64 > file.size() {
            return Err(format!("{name} is larger than the package says"));
        }
        fs::write(&target, &contents).map_err(|e| format!("{}: {e}", target.display()))?;
        let executable = mode.is_some_and(|m| m & 0o111 != 0);
        fs::set_permissions(&target, fs::Permissions::from_mode(if executable { 0o700 } else { 0o600 })).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// A zip entry's path as plain relative components, or `None` for an absolute path or one with `..`.
fn safe_path(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Clears macOS's download quarantine flag, which the user accepted by confirming the install.
fn strip_quarantine(dir: &Path) {
    #[cfg(target_os = "macos")]
    {
        fn walk(path: &Path) {
            if let Ok(c) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) {
                // SAFETY: two valid NUL-terminated strings; XATTR_NOFOLLOW so a symlink is never followed.
                unsafe {
                    libc::removexattr(c.as_ptr(), c"com.apple.quarantine".as_ptr(), libc::XATTR_NOFOLLOW);
                }
            }
            if path.is_dir() {
                for entry in fs::read_dir(path).into_iter().flatten().flatten() {
                    walk(&entry.path());
                }
            }
        }
        walk(dir);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = dir;
}

/// SHA-256 of `bytes` as lowercase hex (FIPS 180-4), for the registry's record of an installed package.
pub fn sha256_hex(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut message = bytes.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((bytes.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_standard_vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(sha256_hex(long), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
    }

    #[test]
    fn package_paths_stay_inside_the_folder() {
        assert_eq!(safe_path("main.py"), Some(PathBuf::from("main.py")));
        assert_eq!(safe_path("./lib/a.py"), Some(PathBuf::from("lib/a.py")));
        assert_eq!(safe_path("../evil"), None);
        assert_eq!(safe_path("/etc/passwd"), None);
        assert_eq!(safe_path("lib/../../x"), None);
    }
}
