//! The bridge's hold on the profile's `.lock`. A lock left by a process that died is cleared only when it provably has no owner
//! (`recover_dead`), and a held lock is removed on a clean return, on stdin EOF and on SIGTERM, SIGINT or SIGHUP (`hold`).
use std::{
    fs, io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Mutex,
};
use vixen_io::lock::{machine_name, LOCK_FILE};

const LOGGER: &str = "Vixen.Studio.MCP";

/// What a bridge line is.
#[derive(Clone, Copy)]
pub enum Note {
    Info,
    Warn,
    Error,
}

/// A line for both the MCP client's stderr and the Vixen logs, so a headless start or stop is never silent. Logging starts on the
/// first line, so a bridge that has nothing to report creates no log folder. Unit tests write stderr only, never the real logs. Without
/// `HOME` there is no log folder, so the line goes to stderr only rather than into the working directory.
pub fn note(kind: Note, message: &str) {
    use vixen_session::logging;
    eprintln!("Vixen Studio MCP: {message}");
    if cfg!(test) {
        return;
    }
    let Some(logs) = vixen_io::DataPaths::shared_logs_dir() else {
        return;
    };
    logging::init(&logs);
    match kind {
        Note::Info => logging::info(LOGGER, message),
        Note::Warn => logging::warn(LOGGER, message),
        Note::Error => logging::error(LOGGER, "MCP bridge stopped with an error", Some(message)),
    }
    logging::flush();
}

/// Whether `pid` provably names no process: `kill(pid, 0)` fails with `ESRCH`. A live process, or one this user may not signal
/// (`EPERM`), is never dead.
fn is_dead(pid: i32) -> bool {
    // SAFETY: signal 0 delivers nothing.
    pid > 0 && unsafe { libc::kill(pid, 0) } != 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// A lock left by a crash, a force-quit or a killed bridge. Removed, and true returned, only if all of these hold: the lock is a
/// regular same-user file (no symlink), it names this machine and a pid other than this process, that pid is gone (`ESRCH`), and
/// the file is still the same one (device, inode, size) just before unlinking. A lock naming a live process, another machine, or
/// text that isn't `pid@host` returns false and stays. Never retries.
pub fn recover_dead(profile: &Path) -> io::Result<bool> {
    let path = profile.join(LOCK_FILE);
    let seen = match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        other => other?,
    };
    if !seen.is_file() || seen.uid() != unsafe { libc::geteuid() } {
        return Ok(false);
    }
    let text = fs::read_to_string(&path)?;
    let Some((pid, machine)) = text.trim().split_once('@') else { return Ok(false) };
    let Ok(pid) = pid.trim().parse::<i32>() else { return Ok(false) };
    if machine != machine_name() || pid == std::process::id() as i32 || !is_dead(pid) {
        return Ok(false);
    }
    let now = fs::symlink_metadata(&path)?;
    if now.dev() != seen.dev() || now.ino() != seen.ino() || now.len() != seen.len() {
        return Ok(false);
    }
    fs::remove_file(&path)?;
    note(Note::Warn, &format!("Removed stale profile lock {} (pid {pid} on this machine is no longer running)", path.display()));
    Ok(true)
}

/// Who holds the profile's lock, for the refusal: "pid 45080 (running)", "pid 45080 (not running)", "pid 45080 on OTHER-MAC", or
/// what the file says when it isn't `pid@host`.
pub fn describe_owner(profile: &Path) -> String {
    let path = profile.join(LOCK_FILE);
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file()) {
        return "a lock that is not a regular file".to_string();
    }
    let Ok(text) = fs::read_to_string(&path) else { return "an unreadable lock".to_string() };
    let text = text.trim();
    let Some((pid, machine)) = text.split_once('@') else { return format!("{text:?}") };
    let Ok(pid) = pid.trim().parse::<i32>() else { return format!("{text:?}") };
    if machine != machine_name() {
        format!("pid {pid} on {machine}")
    } else if is_dead(pid) {
        format!("pid {pid} (not running)")
    } else {
        format!("pid {pid} (running)")
    }
}

/// `create_lock_file`, clearing a dead process's lock first if that is what stands in the way. A refusal names the owner.
pub fn take(profile: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    match vixen_io::lock::create_lock_file(profile) {
        Ok(path) => Ok(path),
        Err(first) => {
            if recover_dead(profile)? {
                Ok(vixen_io::lock::create_lock_file(profile)?)
            } else if profile.join(LOCK_FILE).symlink_metadata().is_ok() {
                Err(format!("{first}: locked by {}", describe_owner(profile)).into())
            } else {
                Err(first.into())
            }
        }
    }
}

/// The lock this process holds, for the signal thread. Taken under the mutex so a signal and a normal release never both act.
static HELD: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Removes the held lock if it is still this process's own.
fn release_held(held: &mut Option<PathBuf>) {
    if let Some(path) = held.take() {
        let ours = fs::read_to_string(&path)
            .is_ok_and(|t| t.trim() == format!("{}@{}", std::process::id(), machine_name()));
        if ours {
            let _ = vixen_io::lock::try_remove_lock_file(&path);
        }
    }
}

/// Owns a held lock: dropping it removes the lock (a normal return, stdin EOF, or an error the bridge reports).
pub struct Held(());
impl Drop for Held {
    fn drop(&mut self) {
        release_held(&mut HELD.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

/// Records `path` as this process's lock until the returned guard drops, or a stop signal arrives.
pub fn hold(path: PathBuf) -> Held {
    *HELD.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    Held(())
}

/// Turns SIGTERM, SIGINT and SIGHUP into an orderly stop: the held lock is removed, the reason logged, and the process exits with
/// 128 + the signal. Call before any other thread starts: the signals are blocked here and every later thread inherits that, so
/// only the waiting thread receives them. Child processes start with a clear mask (std resets it).
pub fn handle_stop_signals() -> io::Result<()> {
    // SAFETY: the set is initialised by sigemptyset before use, and only valid signal numbers are added.
    let set = unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::sigaddset(&mut set, signal);
        }
        let result = libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result));
        }
        set
    };
    std::thread::Builder::new().name("stop signals".into()).spawn(move || {
        let mut signal = 0;
        // SAFETY: the set lives in this closure, and sigwait only writes the signal number.
        if unsafe { libc::sigwait(&set, &mut signal) } != 0 {
            return;
        }
        let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
        let had_lock = held.is_some();
        // The endpoint goes first, so no client attaches to a session whose lock is already gone.
        crate::socket::remove_served_endpoint();
        release_held(&mut held);
        let name = match signal {
            libc::SIGTERM => "SIGTERM",
            libc::SIGINT => "SIGINT",
            _ => "SIGHUP",
        };
        let removed = if had_lock { "; profile lock removed" } else { "" };
        note(Note::Info, &format!("Stopped by {name}{removed}"));
        std::process::exit(128 + signal);
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = PathBuf::from(format!("/tmp/vxl-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }
    /// A pid no process has. Found by probing instead of spawning and reaping a child: a child forked while another test binds a
    /// socket can inherit that socket before close-on-exec is set, and keep it looking live.
    fn dead_pid() -> u32 {
        (90_000..99_000).rev().find(|&pid| is_dead(pid)).unwrap() as u32
    }

    #[test]
    fn a_dead_pid_on_this_machine_is_removed_and_the_lock_can_be_taken() {
        let d = dir("dead");
        let lock = d.join(LOCK_FILE);
        fs::write(&lock, format!("{}@{}", dead_pid(), machine_name())).unwrap();
        assert!(vixen_io::lock::create_lock_file(&d).is_err(), "the plain create still refuses it");
        let taken = take(&d).unwrap();
        assert_eq!(fs::read_to_string(&taken).unwrap(), format!("{}@{}", std::process::id(), machine_name()));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_live_process_another_machine_or_bad_text_stays() {
        let d = dir("live");
        let lock = d.join(LOCK_FILE);
        // The test runner that started this process is alive, as is launchd (pid 1), which this user may not signal.
        for text in [
            format!("{}@{}", std::os::unix::process::parent_id(), machine_name()),
            format!("1@{}", machine_name()),
            format!("{}@SOME-OTHER-MAC", dead_pid()),
            format!("{}@{}", std::process::id(), machine_name()),
            format!("xyz@{}", machine_name()),
            "abc".to_string(),
            format!("0@{}", machine_name()),
        ] {
            fs::write(&lock, &text).unwrap();
            assert!(!recover_dead(&d).unwrap(), "{text}");
            assert!(take(&d).is_err(), "{text}");
            assert_eq!(fs::read_to_string(&lock).unwrap(), text);
        }
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_refusal_names_the_owner() {
        let d = dir("owner");
        let lock = d.join(LOCK_FILE);
        let (parent, dead) = (std::os::unix::process::parent_id(), dead_pid());
        for (text, expected) in [
            (format!("{parent}@{}", machine_name()), format!("locked by pid {parent} (running)")),
            (format!("{dead}@SOME-OTHER-MAC"), format!("locked by pid {dead} on SOME-OTHER-MAC")),
            ("abc".to_string(), "locked by \"abc\"".to_string()),
        ] {
            fs::write(&lock, &text).unwrap();
            let error = take(&d).unwrap_err().to_string();
            assert!(error.contains("the profile is already locked") && error.ends_with(&expected), "{error}");
        }
        let real = d.join("real");
        fs::write(&real, format!("{dead}@{}", machine_name())).unwrap();
        fs::remove_file(&lock).unwrap();
        std::os::unix::fs::symlink(&real, &lock).unwrap();
        assert!(take(&d).unwrap_err().to_string().ends_with("locked by a lock that is not a regular file"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_linked_lock_is_never_followed_or_removed() {
        let d = dir("link");
        let real = d.join("real");
        fs::write(&real, format!("{}@{}", dead_pid(), machine_name())).unwrap();
        std::os::unix::fs::symlink(&real, d.join(LOCK_FILE)).unwrap();
        assert!(!recover_dead(&d).unwrap());
        assert!(real.exists() && fs::symlink_metadata(d.join(LOCK_FILE)).is_ok());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn no_lock_is_nothing_to_recover() {
        let d = dir("none");
        assert!(!recover_dead(&d).unwrap());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_held_lock_goes_when_its_guard_drops_but_only_if_still_ours() {
        let d = dir("held");
        let path = take(&d).unwrap();
        drop(hold(path.clone()));
        assert!(!path.exists());
        let path = take(&d).unwrap();
        let guard = hold(path.clone());
        fs::write(&path, format!("{}@{}", dead_pid(), machine_name())).unwrap();
        drop(guard);
        assert!(path.exists(), "someone else's lock is never removed");
        let _ = fs::remove_dir_all(d);
    }
}
