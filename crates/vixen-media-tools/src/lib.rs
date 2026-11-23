//! Private, bundled FFmpeg tools for media the primary native decoders cannot
//! read. No PATH lookup or shell is used. A single poll loop drains both pipes,
//! bounds diagnostics and always reaps the owned child on cancellation/failure.

use std::ffi::OsString;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

mod studio_developer_tool; // STUDIO: find the staged tools from the executable, for worktrees sharing the bay's target (B88)

const STDERR_LIMIT: usize = 64 * 1024;
const CHUNK: usize = 64 * 1024;

#[derive(Clone, Copy, Debug)]
pub enum Tool {
    Ffmpeg,
    Ffprobe,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::Ffprobe => "ffprobe",
        }
    }

    /// A broken app bundle must never silently use a developer installation.
    pub fn executable(self) -> Result<PathBuf, Error> {
        locate(self.name(), &std::env::current_exe()?)
    }
}

fn locate(name: &str, executable: &Path) -> Result<PathBuf, Error> {
    let directory = executable.parent().and_then(Path::parent);
    let path = if let Some(contents) =
        directory.filter(|p| p.file_name().is_some_and(|n| n == "Contents"))
    {
        contents.join("Helpers").join(name)
    } else {
        let staged = studio_developer_tool::find(name, executable); // STUDIO: worktrees (B88)
        staged.unwrap_or_else(|| developer_tool(name)) // STUDIO: worktrees (B88)
    };
    if path.is_file() {
        Ok(path)
    } else {
        Err(Error::MissingTool(path))
    }
}

/// Only standalone developer tools/tests take this path. The reproducible media build stages its output here; no
/// machine-wide install is needed. App-bundle builds leave it out, so the build machine's path isn't shipped.
#[cfg(not(vixen_app_bundle))]
fn developer_tool(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/media-tools/bin").join(name)
}

#[cfg(vixen_app_bundle)]
fn developer_tool(name: &str) -> PathBuf {
    PathBuf::from("Contents/Helpers").join(name)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("The bundled media decoder is missing: {0}")]
    MissingTool(PathBuf),
    #[error("Media decoding was cancelled")]
    Cancelled,
    #[error("Media decoding timed out")]
    TimedOut,
    #[error("Media decoder failed ({status}): {stderr}")]
    Process { status: ExitStatus, stderr: String },
    #[error("{0}")]
    Consumer(String),
    #[error("Media decoder I/O: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug)]
pub struct Report {
    pub stdout_bytes: u64,
}

/// Stream raw output without retaining the complete clip. `consume` runs on the
/// caller's worker thread; returning an error cancels and reaps the decoder.
/// Callers may set a whole-operation deadline in addition to their cancel flag.
pub fn stream(
    tool: Tool,
    args: &[OsString],
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    consume: impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<Report, Error> {
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    let mut command = Command::new(tool.executable()?);
    command.args(["-hide_banner", "-loglevel", "error"]);
    if matches!(tool, Tool::Ffmpeg) {
        command.arg("-nostdin");
    }
    // Local media only, including when a container points to another resource.
    // The packaged FFmpeg also has networking disabled at build time.
    command
        .args(["-protocol_whitelist", "file,pipe"])
        .args(args);
    command.env_remove("FFREPORT");
    run_command(command, cancel, timeout, consume)
}

/// Small metadata responses only. Audio/video frames should use `stream`.
pub fn collect(
    tool: Tool,
    args: &[OsString],
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    max_bytes: usize,
) -> Result<Vec<u8>, Error> {
    let mut output = Vec::new();
    stream(tool, args, cancel, timeout, |bytes| {
        if bytes.len() > max_bytes.saturating_sub(output.len()) {
            return Err("Media metadata exceeded its size limit".into());
        }
        output
            .try_reserve(bytes.len())
            .map_err(|_| "Not enough memory for media metadata".to_string())?;
        output.extend_from_slice(bytes);
        Ok(())
    })?;
    Ok(output)
}

struct OwnedChild {
    child: Child,
    reaped: bool,
}
impl OwnedChild {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        self.reaped = status.is_some();
        Ok(status)
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.reaped {
            // process_group(0) gives this child a private group. It remains
            // unreaped until here, pinning its PID/group against reuse.
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fd belongs to a live ChildStdout/ChildStderr for this whole call.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn check_cancel(
    cancel: &AtomicBool,
    start: Instant,
    timeout: Option<Duration>,
) -> Result<(), Error> {
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    if timeout.is_some_and(|limit| start.elapsed() >= limit) {
        return Err(Error::TimedOut);
    }
    Ok(())
}

fn run_command(
    mut command: Command,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    mut consume: impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<Report, Error> {
    let start = Instant::now();
    check_cancel(cancel, start, timeout)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut owned = OwnedChild {
        child: command.spawn()?,
        reaped: false,
    };
    let mut stdout = owned
        .child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("No decoder output pipe"))?;
    let mut stderr = owned
        .child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("No decoder diagnostic pipe"))?;
    nonblocking(stdout.as_raw_fd())?;
    nonblocking(stderr.as_raw_fd())?;
    let mut descriptors = [
        libc::pollfd {
            fd: stdout.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: stderr.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let mut buffer = vec![0; CHUNK];
    let mut diagnostics = Vec::new();
    let mut stdout_bytes = 0_u64;
    while descriptors.iter().any(|p| p.fd >= 0) {
        check_cancel(cancel, start, timeout)?;
        // SAFETY: exactly two initialized pollfd values are live for the call.
        let ready = unsafe { libc::poll(descriptors.as_mut_ptr(), 2, 50) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        for (index, descriptor) in descriptors.iter_mut().enumerate() {
            if descriptor.fd < 0 || descriptor.revents == 0 {
                continue;
            }
            if descriptor.revents & libc::POLLNVAL != 0 {
                return Err(io::Error::other("Media decoder pipe became invalid").into());
            }
            // Bound each drain pass so a busy stdout cannot starve stderr,
            // cancellation or deadlines, even when the child writes continuously.
            for _ in 0..16 {
                check_cancel(cancel, start, timeout)?;
                let read = if index == 0 {
                    stdout.read(&mut buffer)
                } else {
                    stderr.read(&mut buffer)
                };
                match read {
                    Ok(0) => {
                        descriptor.fd = -1;
                        break;
                    }
                    Ok(count) if index == 0 => {
                        stdout_bytes = stdout_bytes
                            .checked_add(count as u64)
                            .ok_or_else(|| io::Error::other("Media output size overflow"))?;
                        consume(&buffer[..count]).map_err(Error::Consumer)?;
                    }
                    Ok(count) => {
                        let keep = count.min(STDERR_LIMIT);
                        let excess = diagnostics
                            .len()
                            .saturating_add(keep)
                            .saturating_sub(STDERR_LIMIT);
                        diagnostics.drain(..excess);
                        diagnostics.extend_from_slice(&buffer[count - keep..count]);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
    // A program can close its pipes before it exits. Never turn that into an
    // uninterruptible wait; retain ownership until waitpid actually reaps it.
    let status = loop {
        check_cancel(cancel, start, timeout)?;
        if let Some(status) = owned.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        return Err(Error::Process {
            status,
            stderr: String::from_utf8_lossy(&diagnostics).trim().to_owned(),
        });
    }
    Ok(Report { stdout_bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(source: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", source]);
        command
    }

    #[test]
    fn both_pipes_are_drained_and_error_diagnostics_are_bounded() {
        let mut output = Vec::new();
        let error = run_command(shell("/usr/bin/head -c 262144 /dev/zero >&2; printf 'output'; printf 'final diagnostic' >&2; exit 7"),
            &AtomicBool::new(false), Some(Duration::from_secs(5)), |bytes| { output.extend_from_slice(bytes); Ok(()) }).unwrap_err();
        assert_eq!(output, b"output");
        let Error::Process { status, stderr } = error else {
            panic!("{error:?}")
        };
        assert_eq!(status.code(), Some(7));
        assert!(stderr.len() <= STDERR_LIMIT);
        assert!(stderr.ends_with("final diagnostic"));
    }

    #[test]
    fn arguments_are_passed_literally_without_shell_expansion() {
        let mut command = Command::new("/usr/bin/printf");
        command.args([
            "%s|%s|%s",
            "two words",
            "$(touch must-not-exist)",
            "'quoted'",
        ]);
        let mut output = Vec::new();
        let report = run_command(
            command,
            &AtomicBool::new(false),
            Some(Duration::from_secs(5)),
            |bytes| {
                output.extend_from_slice(bytes);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(output, b"two words|$(touch must-not-exist)|'quoted'");
        assert_eq!(report.stdout_bytes, output.len() as u64);
    }

    #[test]
    fn cancellation_stops_and_reaps_the_owned_child() {
        let cancel = AtomicBool::new(false);
        let mut pid = Vec::new();
        let start = Instant::now();
        let error = run_command(
            shell("echo $$; exec /bin/sleep 60"),
            &cancel,
            None,
            |bytes| {
                pid.extend_from_slice(bytes);
                cancel.store(true, Ordering::Release);
                Ok(())
            },
        )
        .unwrap_err();
        assert!(matches!(error, Error::Cancelled));
        assert!(start.elapsed() < Duration::from_secs(2));
        let pid: i32 = String::from_utf8(pid).unwrap().trim().parse().unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn consumer_failure_cleans_up_instead_of_waiting_for_more_output() {
        let mut pid = 0;
        let error = run_command(
            shell("echo $$; exec /bin/sleep 60"),
            &AtomicBool::new(false),
            None,
            |bytes| {
                pid = std::str::from_utf8(bytes).unwrap().trim().parse().unwrap();
                Err("frame writer failed".into())
            },
        )
        .unwrap_err();
        assert!(matches!(error, Error::Consumer(ref message) if message == "frame writer failed"));
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    }

    #[test]
    fn deadline_still_applies_after_the_child_closes_its_pipes() {
        let start = Instant::now();
        let error = run_command(
            shell("exec 1>&- 2>&-; exec /bin/sleep 60"),
            &AtomicBool::new(false),
            Some(Duration::from_millis(100)),
            |_| Ok(()),
        )
        .unwrap_err();
        assert!(matches!(error, Error::TimedOut));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn precancelled_operation_does_not_spawn_even_a_missing_command() {
        let error = run_command(
            Command::new("/missing/media-tool"),
            &AtomicBool::new(true),
            None,
            |_| Ok(()),
        )
        .unwrap_err();
        assert!(matches!(error, Error::Cancelled));
    }

    #[test]
    fn a_missing_bundled_tool_does_not_fall_back_to_the_checkout_or_path() {
        let path = Path::new("/missing/Relocated Vixen ü.app/Contents/MacOS/Vixen");
        let Error::MissingTool(missing) = locate("ffmpeg", path).unwrap_err() else {
            panic!("wrong error")
        };
        assert_eq!(
            missing,
            Path::new("/missing/Relocated Vixen ü.app/Contents/Helpers/ffmpeg")
        );
    }
}
