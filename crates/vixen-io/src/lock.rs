//! The profile lock: the file `.lock` in the data folder, holding `{pid}@{machine}`. Port of the lock half of `VixenApplication`.

use crate::error::IoError;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

/// `VixenApplication.LockFile`.
pub const LOCK_FILE: &str = ".lock";

/// Who a lock file names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockOwner {
    /// `None` when the file's first part isn't a number.
    pub pid: Option<i32>,
    pub machine: String,
}

impl LockOwner {
    pub fn is_on_this_machine(&self) -> bool {
        self.machine == machine_name()
    }
}

/// The name a lock is stamped with. `LocalHostName` is stable, where `gethostname` changes with the network and would
/// strand a crashed profile on a laptop that has moved.
pub fn machine_name() -> String {
    local_host_name().unwrap_or_else(host_name)
}

#[cfg(target_os = "macos")]
fn local_host_name() -> Option<String> {
    use std::ffi::{c_char, c_void, CStr};

    const UTF8: u32 = 0x0800_0100;
    #[link(name = "SystemConfiguration", kind = "framework")]
    extern "C" {
        fn SCDynamicStoreCopyLocalHostName(store: *const c_void) -> *const c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringGetCString(string: *const c_void, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
        fn CFRelease(object: *const c_void);
    }
    // SAFETY: a null store is allowed, the returned string is owned here and released once, and the buffer's size is passed.
    unsafe {
        let string = SCDynamicStoreCopyLocalHostName(std::ptr::null());
        if string.is_null() {
            return None;
        }
        let mut buffer = [0 as c_char; 256];
        let converted = CFStringGetCString(string, buffer.as_mut_ptr(), buffer.len() as isize, UTF8);
        CFRelease(string);
        (converted != 0).then(|| CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned())
    }
}

#[cfg(not(target_os = "macos"))]
fn local_host_name() -> Option<String> {
    None
}

fn host_name() -> String {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer's length is passed, and the result is only read up to its terminator.
    let result = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if result != 0 {
        return "localhost".to_string();
    }
    let end = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

fn this_process_id() -> String {
    format!("{}@{}", std::process::id(), machine_name())
}

/// The process's name, or `None` when there is no such process.
fn process_name(pid: i32) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer's length is passed, and only the bytes reported as written are read.
    let written = unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if written > 0 {
        return Some(String::from_utf8_lossy(&buffer[..written as usize]).into_owned());
    }
    // proc_name can fail for a process this user may not inspect. A signal of 0 tells that apart from one that's gone. The
    // name is unknown, so it reads as something other than Vixen and the lock clears, as `Process.GetProcessById` would.
    // SAFETY: signal 0 delivers nothing.
    let alive = unsafe { libc::kill(pid, 0) } == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    alive.then(String::new)
}

/// Who the folder's lock file names, for the alert that offers to open it anyway.
pub fn lock_owner(folder: &Path) -> Option<LockOwner> {
    let text = std::fs::read_to_string(folder.join(LOCK_FILE)).ok()?;
    let parts: Vec<&str> = text.split('@').collect();
    if parts.len() < 2 {
        return None;
    }
    Some(LockOwner { pid: parts[0].trim().parse().ok(), machine: parts[1].to_string() })
}

/// What `IsProfileLocked` found, with the failures upstream logs on the way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LockCheck {
    pub locked: bool,
    /// What upstream's outer `catch` logs as `An error occurred checking the profile lock file.`
    pub check_error: Option<String>,
    /// What `RemoveLockFile` logs when a stale lock couldn't be deleted. The profile still reads as not locked.
    pub remove_error: Option<String>,
}

impl LockCheck {
    fn locked() -> Self {
        Self { locked: true, ..Self::default() }
    }

    fn failed(error: impl ToString) -> Self {
        // If it can't be determined, it can't be assumed unlocked.
        Self { locked: true, check_error: Some(error.to_string()), remove_error: None }
    }

    fn stale(lock: &Path) -> Self {
        Self { locked: false, check_error: None, remove_error: try_remove_lock_file(lock).err().map(|e| e.to_string()) }
    }
}

/// `IsProfileLocked`. Checking has a side effect, as upstream: a stale lock is removed.
pub fn is_profile_locked(folder: &Path) -> bool {
    check_profile_lock(folder).locked
}

/// `IsProfileLocked`, keeping what it would have logged.
pub fn check_profile_lock(folder: &Path) -> LockCheck {
    if !folder.is_dir() {
        return LockCheck::default();
    }
    let lock = folder.join(LOCK_FILE);
    if !lock.exists() {
        return LockCheck::default();
    }
    // `File.ReadAllText` fails only when the file can't be read. Bytes that aren't text decode with replacement characters, and
    // then name no machine or some other one.
    let text = match std::fs::read(&lock) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) => return LockCheck::failed(error),
    };
    let lock_parts: Vec<&str> = text.split('@').collect();
    if lock_parts.len() < 2 {
        return LockCheck::locked();
    }
    if lock_parts[1] != machine_name() {
        // Assume it is locked on some other device.
        return LockCheck::locked();
    }
    let pid = match lock_parts[0].trim().parse::<i32>() {
        Ok(pid) => pid,
        Err(error) => return LockCheck::failed(error),
    };
    // GUARD: this process naming itself can only be left from an earlier life of the same pid, or a lock this run already holds
    // and is about to replace. Upstream would call it locked, for good.
    if pid == std::process::id() as i32 {
        return LockCheck::stale(&lock);
    }
    match process_name(pid) {
        // STUDIO-BEGIN mcp-profile-lock: recognize the headless bridge as a profile owner
        Some(name) if name.starts_with("Vixen") || name.starts_with("vixen-studio") => LockCheck::locked(),
        // STUDIO-END mcp-profile-lock
        _ => LockCheck::stale(&lock),
    }
}

/// `CreateLockFile`: creates the folder if missing, and succeeds only if no lock was there. `O_CREAT | O_EXCL`, so two launches at
/// once can't both win.
pub fn create_lock_file(folder: &Path) -> Result<PathBuf, IoError> {
    std::fs::create_dir_all(folder).map_err(|e| IoError::io(folder, e))?;
    let path = folder.join(LOCK_FILE);
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| {
        if e.kind() == ErrorKind::AlreadyExists {
            IoError::io(&path, std::io::Error::new(ErrorKind::AlreadyExists, "the profile is already locked"))
        } else {
            IoError::io(&path, e)
        }
    })?;
    file.write_all(this_process_id().as_bytes()).map_err(|e| IoError::io(&path, e))?;
    Ok(path)
}

/// `RemoveLockFile`: true if there was a file and it went.
pub fn remove_lock_file(path: &Path) -> bool {
    try_remove_lock_file(path).unwrap_or(false)
}

/// `RemoveLockFile`, keeping the failure upstream logs as `An error occurred removing the profile lock file.`
pub fn try_remove_lock_file(path: &Path) -> std::io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(path).map(|()| true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        crate::test_dir(name)
    }

    #[test]
    fn no_folder_or_no_file_is_not_locked() {
        let dir = folder("lock-none");
        assert!(!is_profile_locked(&dir.join("missing")));
        assert!(!is_profile_locked(&dir));
    }

    #[test]
    fn text_with_no_at_sign_is_locked_and_stays() {
        let dir = folder("lock-abc");
        std::fs::write(dir.join(LOCK_FILE), "abc").unwrap();
        assert!(is_profile_locked(&dir));
        assert!(dir.join(LOCK_FILE).exists());
        assert_eq!(lock_owner(&dir), None);
    }

    #[test]
    fn another_machine_is_locked_and_stays() {
        let dir = folder("lock-other");
        std::fs::write(dir.join(LOCK_FILE), "1234@BRIANS-PC").unwrap();
        assert!(is_profile_locked(&dir));
        assert!(dir.join(LOCK_FILE).exists());
        let owner = lock_owner(&dir).unwrap();
        assert_eq!(owner, LockOwner { pid: Some(1234), machine: "BRIANS-PC".to_string() });
        assert!(!owner.is_on_this_machine());
    }

    #[test]
    fn this_machine_with_a_dead_pid_is_stale_and_removed() {
        let dir = folder("lock-dead");
        // Spawn and reap a process, so its pid is known to be free.
        let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        std::fs::write(dir.join(LOCK_FILE), format!("{pid}@{}", machine_name())).unwrap();
        assert!(!is_profile_locked(&dir));
        assert!(!dir.join(LOCK_FILE).exists());
    }

    #[test]
    fn this_machine_with_a_live_process_of_another_name_is_stale_and_removed() {
        let dir = folder("lock-other-process");
        let mut child = std::process::Command::new("/bin/sleep").arg("30").spawn().unwrap();
        std::fs::write(dir.join(LOCK_FILE), format!("{}@{}", child.id(), machine_name())).unwrap();
        let locked = is_profile_locked(&dir);
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!locked);
        assert!(!dir.join(LOCK_FILE).exists());
    }

    #[test]
    fn this_machine_with_a_live_vixen_process_is_locked() {
        let dir = folder("lock-vixen");
        // A copy of a real program, so the process's name starts with Vixen as the app's does.
        let program = dir.join("VixenLockTest");
        std::fs::copy("/bin/sleep", &program).unwrap();
        // The copy of a system binary needs a signature of its own, or the system kills it and files a crash report.
        let signed = std::process::Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(&program)
            .output()
            .is_ok_and(|o| o.status.success());
        assert!(signed, "the test program couldn't be signed");
        let mut child = std::process::Command::new(&program).arg("30").spawn().unwrap();
        std::fs::write(dir.join(LOCK_FILE), format!("{}@{}", child.id(), machine_name())).unwrap();
        let locked = is_profile_locked(&dir);
        let name = process_name(child.id() as i32);
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(name.as_deref(), Some("VixenLockTest"));
        assert!(locked);
        assert!(dir.join(LOCK_FILE).exists());
    }

    #[test]
    fn a_pid_that_is_not_a_number_is_locked() {
        let dir = folder("lock-xyz");
        std::fs::write(dir.join(LOCK_FILE), format!("xyz@{}", machine_name())).unwrap();
        assert!(is_profile_locked(&dir));
        assert!(dir.join(LOCK_FILE).exists());
    }

    #[test]
    fn a_check_that_fails_says_why_and_a_plain_answer_does_not() {
        let dir = folder("lock-check-errors");
        std::fs::write(dir.join(LOCK_FILE), format!("xyz@{}", machine_name())).unwrap();
        let check = check_profile_lock(&dir);
        assert!(check.locked);
        assert!(check.check_error.is_some());
        assert_eq!(check.remove_error, None);

        // Not text: `File.ReadAllText` reads it anyway, and nothing in it names this machine.
        std::fs::write(dir.join(LOCK_FILE), [0xff, 0xfe, 0xff]).unwrap();
        assert_eq!(check_profile_lock(&dir), LockCheck { locked: true, check_error: None, remove_error: None });
        std::fs::write(dir.join(LOCK_FILE), [0xff, b'@', 0xfe]).unwrap();
        assert_eq!(check_profile_lock(&dir), LockCheck { locked: true, check_error: None, remove_error: None });

        // A file that can't be read at all is the outer catch.
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::write(dir.join(LOCK_FILE), format!("1@{}", machine_name())).unwrap();
        std::fs::set_permissions(dir.join(LOCK_FILE), std::fs::Permissions::from_mode(0o000)).unwrap();
        let check = check_profile_lock(&dir);
        std::fs::set_permissions(dir.join(LOCK_FILE), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(check.locked);
        assert!(check.check_error.is_some());
        assert!(dir.join(LOCK_FILE).exists());

        std::fs::write(dir.join(LOCK_FILE), "1234@SOME-OTHER-PC").unwrap();
        assert_eq!(check_profile_lock(&dir), LockCheck { locked: true, check_error: None, remove_error: None });
        std::fs::write(dir.join(LOCK_FILE), "abc").unwrap();
        assert_eq!(check_profile_lock(&dir), LockCheck { locked: true, check_error: None, remove_error: None });
        assert_eq!(check_profile_lock(&dir.join("missing")), LockCheck::default());
    }

    #[test]
    fn a_stale_lock_that_cannot_be_deleted_is_still_not_locked_and_says_why() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = folder("lock-stale-readonly");
        let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        std::fs::write(dir.join(LOCK_FILE), format!("{pid}@{}", machine_name())).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let check = check_profile_lock(&dir);
        let removal = try_remove_lock_file(&dir.join(LOCK_FILE));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!check.locked);
        assert_eq!(check.check_error, None);
        assert!(check.remove_error.is_some());
        assert!(removal.is_err());
        assert!(dir.join(LOCK_FILE).exists());
        assert_eq!(try_remove_lock_file(&dir.join(LOCK_FILE)).unwrap(), true);
        assert_eq!(try_remove_lock_file(&dir.join(LOCK_FILE)).unwrap(), false);
    }

    #[test]
    fn this_very_process_naming_itself_is_stale() {
        let dir = folder("lock-self");
        create_lock_file(&dir).unwrap();
        assert!(!is_profile_locked(&dir));
        assert!(!dir.join(LOCK_FILE).exists());
    }

    #[test]
    fn creating_twice_fails_the_second_time_and_holds_the_right_text() {
        let dir = folder("lock-create");
        let path = create_lock_file(&dir).unwrap();
        assert!(create_lock_file(&dir).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("{}@{}", std::process::id(), machine_name()));
        assert!(remove_lock_file(&path));
        assert!(!remove_lock_file(&path));
    }

    #[test]
    fn creating_makes_a_missing_folder() {
        let dir = folder("lock-mkdir").join("new").join("profile");
        let path = create_lock_file(&dir).unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn the_machine_name_is_not_empty_and_has_no_at_sign() {
        let name = machine_name();
        assert!(!name.is_empty());
        assert!(!name.contains('@'));
    }
}
