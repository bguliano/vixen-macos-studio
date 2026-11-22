//! Each plugin's log: its stderr lines, its `log` messages and the host's own notes about it, kept in a ring buffer for
//! `plugin_logs` and the UI and appended to `Plugins/logs/<id>.log` (rotated at 1 MiB, one previous file kept).
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const CAPACITY: usize = 2000;
pub const MAX_MESSAGE: usize = 4096;
pub const MAX_FILE: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn parse(text: &str) -> Option<Level> {
        match text {
            "debug" | "trace" => Some(Level::Debug),
            "info" => Some(Level::Info),
            "warn" | "warning" => Some(Level::Warn),
            "error" => Some(Level::Error),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

/// Where a line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Stderr,
    Plugin,
    Host,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::Stderr => "stderr",
            Source::Plugin => "plugin",
            Source::Host => "host",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub seq: u64,
    pub time_ms: u64,
    pub level: Level,
    pub source: Source,
    pub message: String,
}

impl Entry {
    pub fn to_json(&self) -> Value {
        json!({"seq": self.seq, "timeMs": self.time_ms, "level": self.level.as_str(), "source": self.source.as_str(), "message": self.message})
    }
}

struct State {
    entries: VecDeque<Entry>,
    next_seq: u64,
    file: Option<File>,
    file_len: u64,
}

pub struct LogBuffer {
    path: Option<PathBuf>,
    state: Mutex<State>,
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl LogBuffer {
    /// A buffer that also writes to `path` when given. A file that can't be opened only loses the file copy.
    pub fn new(path: Option<PathBuf>) -> LogBuffer {
        LogBuffer { path, state: Mutex::new(State { entries: VecDeque::new(), next_seq: 1, file: None, file_len: 0 }) }
    }

    pub fn push(&self, level: Level, source: Source, message: &str) {
        let mut message: String = message.chars().filter(|c| !c.is_control() || *c == '\t').collect();
        if message.len() > MAX_MESSAGE {
            let mut cut = MAX_MESSAGE;
            while !message.is_char_boundary(cut) {
                cut -= 1;
            }
            message.truncate(cut);
            message.push('…');
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let entry = Entry { seq: state.next_seq, time_ms: now_ms(), level, source, message };
        state.next_seq += 1;
        self.write_file(&mut state, &entry);
        if state.entries.len() == CAPACITY {
            state.entries.pop_front();
        }
        state.entries.push_back(entry);
    }

    fn write_file(&self, state: &mut State, entry: &Entry) {
        let Some(path) = &self.path else { return };
        let line = format!("{} {} {} {}\n", entry.time_ms, entry.level.as_str(), entry.source.as_str(), entry.message);
        if state.file.is_some() && state.file_len + line.len() as u64 > MAX_FILE {
            state.file = None;
            let _ = std::fs::rename(path, path.with_extension("log.1"));
        }
        if state.file.is_none() {
            if let Some(parent) = path.parent() {
                let _ = crate::registry::private_dir(parent);
            }
            state.file = open_log(path).ok();
            state.file_len = state.file.as_ref().and_then(|f| f.metadata().ok()).map(|m| m.len()).unwrap_or(0);
        }
        if let Some(file) = &mut state.file {
            if file.write_all(line.as_bytes()).is_ok() {
                state.file_len += line.len() as u64;
            }
        }
    }

    /// The newest `limit` entries at or above `min`, oldest first.
    pub fn tail(&self, limit: usize, min: Level) -> Vec<Entry> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut out: Vec<Entry> = state.entries.iter().rev().filter(|e| e.level >= min).take(limit).cloned().collect();
        out.reverse();
        out
    }
}

fn open_log(path: &std::path::Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    // Never follow a symlink planted at the log path.
    OpenOptions::new().create(true).append(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_the_newest_entries_and_filters_by_level() {
        let logs = LogBuffer::new(None);
        for i in 0..CAPACITY + 5 {
            logs.push(if i % 2 == 0 { Level::Info } else { Level::Error }, Source::Stderr, &format!("line {i}"));
        }
        let all = logs.tail(usize::MAX, Level::Debug);
        assert_eq!(all.len(), CAPACITY);
        assert_eq!(all[0].message, "line 5");
        let errors = logs.tail(2, Level::Error);
        assert_eq!(errors.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), ["line 2001", "line 2003"]);
    }

    #[test]
    fn long_and_control_characters_are_tamed_and_the_file_rotates() {
        let dir = std::env::temp_dir().join(format!("studio-plugins-logs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("p.log");
        let logs = LogBuffer::new(Some(path.clone()));
        logs.push(Level::Info, Source::Plugin, &format!("a\u{1b}[31mb{}", "é".repeat(MAX_MESSAGE)));
        let entry = &logs.tail(1, Level::Debug)[0];
        assert!(entry.message.starts_with("a[31mb") && entry.message.ends_with('…') && entry.message.len() <= MAX_MESSAGE + 3);
        for _ in 0..(MAX_FILE as usize / MAX_MESSAGE) + 10 {
            logs.push(Level::Info, Source::Plugin, &"x".repeat(MAX_MESSAGE));
        }
        assert!(path.with_extension("log.1").exists());
        assert!(std::fs::metadata(&path).unwrap().len() <= MAX_FILE);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
