//! Same-user local transport. No TCP, automatic stale-socket removal, or network output.
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
        let listener = UnixListener::bind(&path)?;
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
