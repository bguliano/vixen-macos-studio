//! A memory heartbeat, so a process the system kills (jetsam, SIGKILL) or that otherwise exits without shutting down still shows up
//! in the logs. Every 30 seconds the app writes its pid and memory footprint to `studio-heartbeat` in the logs folder, and logs the
//! footprint whenever it has grown by 512 MB since the last line. A clean shutdown removes the file; the next launch that finds it
//! naming a process that no longer exists logs that the previous session ended without shutting down, with its last footprint.
use crate::logging;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use vixen_io::LocalDateTime;

const LOGGER: &str = "VixenApplication.Program";
const FILE: &str = "studio-heartbeat";
const INTERVAL: Duration = Duration::from_secs(30);
const STEP: u64 = 512 * 1024 * 1024;

static RUNNING: AtomicBool = AtomicBool::new(false);
static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// This process's physical memory footprint in bytes: what the system's memory-pressure limits count.
pub fn footprint() -> Option<u64> {
    // SAFETY: the buffer is a zeroed rusage_info_v2 passed as the flavor that fills it.
    unsafe {
        let mut info: libc::rusage_info_v2 = std::mem::zeroed();
        let result = libc::proc_pid_rusage(
            std::process::id() as i32,
            libc::RUSAGE_INFO_V2,
            (&mut info as *mut libc::rusage_info_v2).cast(),
        );
        (result == 0).then_some(info.ri_phys_footprint)
    }
}

fn megabytes(bytes: u64) -> u64 {
    bytes / (1024 * 1024)
}

/// What a heartbeat file says: pid, footprint in bytes, seconds since 1970.
#[derive(Debug, PartialEq, Eq)]
struct Beat {
    pid: i32,
    footprint: u64,
    at: u64,
}

impl Beat {
    fn parse(text: &str) -> Option<Beat> {
        let mut parts = text.split_whitespace().map(str::parse::<u64>);
        let (pid, footprint, at) = (parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?);
        Some(Beat { pid: i32::try_from(pid).ok()?, footprint, at })
    }

    fn text(&self) -> String {
        format!("{} {} {}\n", self.pid, self.footprint, self.at)
    }
}

fn is_dead(pid: i32) -> bool {
    // SAFETY: signal 0 delivers nothing.
    pid > 0 && unsafe { libc::kill(pid, 0) } != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// The line for a previous session's leftover heartbeat, or `None` when there is nothing to report (no file, unreadable, this
/// process, or a process that is still running, such as a second copy of the app).
fn previous_session_report(path: &Path) -> Option<String> {
    let beat = Beat::parse(&fs::read_to_string(path).ok()?)?;
    if beat.pid == std::process::id() as i32 || !is_dead(beat.pid) {
        return None;
    }
    let when = LocalDateTime::from_system_time(UNIX_EPOCH + Duration::from_secs(beat.at)).log_text();
    Some(format!(
        "The previous Vixen session (pid {}) ended without shutting down: crashed, killed or stopped by the system. Its last heartbeat was at {when}, using {} MB of memory.",
        beat.pid,
        megabytes(beat.footprint)
    ))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Starts the heartbeat in the folder logging writes to. Does nothing when logging hasn't started (tests, tools) or it is running.
pub fn start() {
    let Some(folder) = logging::logs_folder() else { return };
    if RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    let path = folder.join(FILE);
    if let Some(line) = previous_session_report(&path) {
        logging::warn(LOGGER, &line);
    }
    *PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.clone());
    let _ = std::thread::Builder::new().name("memory heartbeat".into()).spawn(move || {
        let mut logged = 0u64;
        while RUNNING.load(Ordering::Acquire) {
            if let Some(bytes) = footprint() {
                {
                    // Under the lock, so a beat never lands after `stop` removed the file.
                    let guard = PATH.lock().unwrap_or_else(|e| e.into_inner());
                    if guard.is_some() {
                        let _ = fs::write(&path, Beat { pid: std::process::id() as i32, footprint: bytes, at: now() }.text());
                    }
                }
                if logged == 0 || bytes >= logged + STEP {
                    logging::info(LOGGER, &format!("Memory heartbeat: {} MB in use.", megabytes(bytes)));
                    logged = bytes;
                }
            }
            std::thread::sleep(INTERVAL);
        }
    });
}

/// A clean shutdown: no more beats, and no leftover for the next launch to report.
pub fn stop() {
    RUNNING.store(false, Ordering::Release);
    if let Some(path) = PATH.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vixen-heartbeat-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_beat_round_trips_and_bad_text_is_nothing() {
        let beat = Beat { pid: 42, footprint: 3 * 1024 * 1024 * 1024, at: 1_790_000_000 };
        assert_eq!(Beat::parse(&beat.text()), Some(beat));
        assert_eq!(Beat::parse("42 nope 1"), None);
        assert_eq!(Beat::parse("42 1"), None);
        assert_eq!(Beat::parse(""), None);
    }

    #[test]
    fn a_dead_previous_session_is_reported_with_its_memory() {
        let d = folder("dead");
        // Probed rather than a reaped child: a child forked while another test binds a socket can inherit it.
        let pid = (90_000..99_000).rev().find(|&pid| is_dead(pid)).unwrap();
        let path = d.join(FILE);
        fs::write(&path, Beat { pid, footprint: 4300 * 1024 * 1024, at: now() }.text()).unwrap();
        let line = previous_session_report(&path).unwrap();
        assert!(line.contains(&format!("pid {pid}")) && line.contains("4300 MB"), "{line}");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_live_process_this_process_or_no_file_is_not_reported() {
        let d = folder("live");
        let path = d.join(FILE);
        assert_eq!(previous_session_report(&path), None);
        fs::write(&path, Beat { pid: std::os::unix::process::parent_id() as i32, footprint: 1, at: now() }.text()).unwrap();
        assert_eq!(previous_session_report(&path), None, "the test runner is still running");
        fs::write(&path, Beat { pid: std::process::id() as i32, footprint: 1, at: now() }.text()).unwrap();
        assert_eq!(previous_session_report(&path), None);
        fs::write(&path, "garbage").unwrap();
        assert_eq!(previous_session_report(&path), None);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn the_footprint_is_readable() {
        assert!(footprint().is_some_and(|b| b > 0));
    }
}
