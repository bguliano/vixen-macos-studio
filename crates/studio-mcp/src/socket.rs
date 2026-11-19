//! Same-user local transport. No TCP or network output. A leftover endpoint is removed only when it provably has no owner
//! (`recover_stale`); everything else fails closed.
use crate::{Server, MAX_MESSAGE};
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use vixen_session::app::AppCore;

pub fn socket_path(profile: &Path) -> io::Result<PathBuf> {
    let root = profile.canonicalize()?;
    let mut hash = 0xcbf29ce484222325u64;
    for b in root.as_os_str().as_encoded_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let home = std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is not set"))?;
    Ok(PathBuf::from(home)
        .join("Library/Application Support/Vixen Studio")
        .join(format!("mcp-{hash:016x}.sock")))
}
fn secure(path: &Path, socket: bool) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
        || (socket && !metadata.file_type().is_socket())
        || (!socket && !metadata.is_dir())
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "MCP endpoint must be same-user, private and not a symlink",
        ));
    }
    Ok(())
}
pub struct SocketServer {
    stop: Arc<AtomicBool>,
    path: PathBuf,
    inode: u64,
}
impl SocketServer {
    pub fn start(core: &Arc<AppCore>) -> io::Result<Self> {
        let path = socket_path(core.paths.root())?;
        let dir = path.parent().unwrap();
        if !dir.exists() {
            fs::create_dir_all(dir)?;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        secure(dir, false)?;
        let listener = match UnixListener::bind(&path) {
            Err(e) if e.kind() == io::ErrorKind::AddrInUse
                && recover_stale(&path, lock_held_here(core.paths.root()))? =>
            {
                UnixListener::bind(&path)?
            }
            other => other?,
        };
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let inode = fs::metadata(&path)?.ino();
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let weak = Arc::downgrade(core);
        let clients = Arc::new(AtomicUsize::new(0));
        thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if clients.load(Ordering::Acquire) >= 8 {
                            drop(stream);
                            continue;
                        }
                        let Some(core) = weak.upgrade() else { break };
                        clients.fetch_add(1, Ordering::AcqRel);
                        let clients = clients.clone();
                        thread::spawn(move || {
                            let _ = handle(stream, core);
                            clients.fetch_sub(1, Ordering::AcqRel);
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self { stop, path, inode })
    }
}
impl Drop for SocketServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if fs::symlink_metadata(&self.path).is_ok_and(|m| m.ino() == self.inode) {
            let _ = fs::remove_file(&self.path);
        }
    }
}
fn handle(mut stream: UnixStream, core: Arc<AppCore>) -> io::Result<()> {
    // Darwin may inherit the listener nonblocking flag on accepted sockets.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut input = BufReader::new(stream.try_clone()?);
    let mut identity = Vec::new();
    std::io::Read::take(&mut input, (MAX_MESSAGE + 1) as u64).read_until(b'\n', &mut identity)?;
    if identity.len() > MAX_MESSAGE {
        return Err(io::Error::other("Handshake too large"));
    }
    let handshake: serde_json::Value = serde_json::from_slice(&identity)?;
    let expected = core
        .paths
        .root()
        .canonicalize()?
        .to_string_lossy()
        .into_owned();
    if handshake["profile"] != expected {
        return Err(io::Error::other("Wrong profile"));
    }
    writeln!(stream, "{{\"ready\":true}}")?;
    crate::serve(&mut Server::from_core(core), input, stream)
}
/// None only when no endpoint exists. Any existing-but-broken endpoint fails closed.
/// Whether this process holds the profile's exclusive Vixen lock: the lock file is a regular file naming this pid on this machine.
pub fn lock_held_here(profile: &Path) -> bool {
    let lock = profile.join(vixen_io::lock::LOCK_FILE);
    let Ok(metadata) = fs::symlink_metadata(&lock) else { return false };
    if !metadata.is_file() || metadata.uid() != unsafe { libc::geteuid() } {
        return false;
    }
    let Ok(text) = fs::read_to_string(&lock) else { return false };
    text.trim() == format!("{}@{}", std::process::id(), vixen_io::lock::machine_name())
}

/// A leftover endpoint from a crash or force-quit. Removed, and true returned, only if all of these hold:
/// this process holds the profile lock (`lock_held`), the directory and socket pass `secure`, `connect` is refused (no listener),
/// and the path is still the same file (device and inode) just before unlinking. Anything else returns false or the error, and
/// the caller fails closed as before. Never retries; never touches the profile lock.
pub fn recover_stale(path: &Path, lock_held: bool) -> io::Result<bool> {
    if !lock_held {
        return Ok(false);
    }
    secure(path.parent().ok_or_else(|| io::Error::other("socket has no directory"))?, false)?;
    secure(path, true)?;
    let seen = fs::symlink_metadata(path)?;
    match UnixStream::connect(path) {
        Ok(_) => return Ok(false), // a live server owns it
        Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {}
        Err(e) => return Err(e),
    }
    let now = fs::symlink_metadata(path)?;
    if now.dev() != seen.dev() || now.ino() != seen.ino() {
        return Ok(false);
    }
    fs::remove_file(path)?;
    vixen_session::logging::warn(
        "Vixen.Studio.MCP",
        &format!("Removed stale MCP socket {} (no listener; profile lock held)", path.display()),
    );
    Ok(true)
}

pub fn connect(profile: &Path) -> io::Result<Option<UnixStream>> {
    let path = socket_path(profile)?;
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
        Ok(_) => {}
    }
    secure(path.parent().unwrap(), false)?;
    secure(&path, true)?;
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    writeln!(
        stream,
        "{}",
        serde_json::json!({"profile":profile.canonicalize()?.to_string_lossy()})
    )?;
    let mut input = BufReader::new(stream.try_clone()?);
    let mut reply = Vec::new();
    std::io::Read::take(&mut input, 1024).read_until(b'\n', &mut reply)?;
    if serde_json::from_slice::<serde_json::Value>(&reply)?["ready"] != true {
        return Err(io::Error::other("MCP server rejected profile"));
    }
    stream.set_read_timeout(None)?;
    Ok(Some(stream))
}

#[cfg(test)]
mod stale_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn dir(tag: &str) -> PathBuf {
        let d = PathBuf::from(format!("/tmp/vxs-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
        d
    }
    /// A socket with no listener: binding then dropping leaves the file behind, as a crash does.
    fn leftover(path: &Path, mode: u32) {
        drop(UnixListener::bind(path).unwrap());
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn removes_a_refused_socket_only_with_the_lock() {
        let d = dir("refused");
        let s = d.join("mcp.sock");
        leftover(&s, 0o600);
        assert!(!recover_stale(&s, false).unwrap(), "no lock, no recovery");
        assert!(s.exists());
        assert!(recover_stale(&s, true).unwrap());
        assert!(!s.exists());
        UnixListener::bind(&s).unwrap();
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn never_removes_a_live_listener() {
        let d = dir("live");
        let s = d.join("mcp.sock");
        let _listener = UnixListener::bind(&s).unwrap();
        fs::set_permissions(&s, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!recover_stale(&s, true).unwrap());
        assert!(s.exists());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn insecure_or_linked_endpoints_stay_fail_closed() {
        let d = dir("insecure");
        let open = d.join("open.sock");
        leftover(&open, 0o644);
        assert!(recover_stale(&open, true).is_err());
        assert!(open.exists());
        let target = d.join("target.sock");
        leftover(&target, 0o600);
        let link = d.join("link.sock");
        symlink(&target, &link).unwrap();
        assert!(recover_stale(&link, true).is_err());
        assert!(target.exists() && fs::symlink_metadata(&link).is_ok());
        let file = d.join("file.sock");
        fs::write(&file, b"").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(recover_stale(&file, true).is_err());
        assert!(file.exists());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn lock_is_held_only_when_it_names_this_process() {
        let d = dir("lock");
        let lock = d.join(vixen_io::lock::LOCK_FILE);
        assert!(!lock_held_here(&d));
        fs::write(&lock, format!("{}@{}", std::process::id(), vixen_io::lock::machine_name())).unwrap();
        assert!(lock_held_here(&d));
        fs::write(&lock, format!("1@{}", vixen_io::lock::machine_name())).unwrap();
        assert!(!lock_held_here(&d));
        fs::remove_file(&lock).unwrap();
        let real = d.join("real");
        fs::write(&real, format!("{}@{}", std::process::id(), vixen_io::lock::machine_name())).unwrap();
        symlink(&real, &lock).unwrap();
        assert!(!lock_held_here(&d), "a linked lock never counts");
        let _ = fs::remove_dir_all(d);
    }
}
