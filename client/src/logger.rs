use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use chrono::Local;

/// Log level matching the C# MySEQ log level enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug = 0,
    Info  = 1,
    Warn  = 2,
    Error = 3,
}

impl LogLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info  => "INFO",
            Self::Warn  => "WARN",
            Self::Error => "ERROR",
        }
    }

    /// Config/UI string representation (lowercase).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info  => "info",
            Self::Warn  => "warn",
            Self::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "debug"           => Self::Debug,
            "warn" | "warning" => Self::Warn,
            "error"           => Self::Error,
            _                 => Self::Info,
        }
    }

    pub fn all() -> &'static [LogLevel] {
        &[Self::Debug, Self::Info, Self::Warn, Self::Error]
    }
}

/// Writes timestamped log entries to dated files in `log_dir`.
/// File names follow the MySEQ convention: `MM-dd-yyyy.txt`.
///
/// All clones share the same `enabled` and `min_level` atomics, so calling
/// `set_enabled` / `set_level` on any clone (including the one held by the UI)
/// immediately affects the network thread's copy without a restart.
#[derive(Debug, Clone)]
pub struct Logger {
    log_dir:   PathBuf,
    enabled:   Arc<AtomicBool>,
    min_level: Arc<AtomicU8>,
}

impl Logger {
    pub fn new(log_dir: impl Into<PathBuf>) -> Self {
        Self {
            log_dir:   log_dir.into(),
            enabled:   Arc::new(AtomicBool::new(true)),
            min_level: Arc::new(AtomicU8::new(LogLevel::Info as u8)),
        }
    }

    /// Builder-style level setter (used in tests and startup).
    pub fn with_level(self, level: LogLevel) -> Self {
        self.min_level.store(level as u8, Ordering::Relaxed);
        self
    }

    /// Enable or disable all file logging at runtime.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Change the minimum log level at runtime.
    pub fn set_level(&self, level: LogLevel) {
        self.min_level.store(level as u8, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn level(&self) -> LogLevel {
        match self.min_level.load(Ordering::Relaxed) {
            0 => LogLevel::Debug,
            2 => LogLevel::Warn,
            3 => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }

    pub fn debug(&self, msg: &str) { self.write(LogLevel::Debug, msg); }
    pub fn info(&self,  msg: &str) { self.write(LogLevel::Info,  msg); }
    pub fn warn(&self,  msg: &str) { self.write(LogLevel::Warn,  msg); }
    pub fn error(&self, msg: &str) { self.write(LogLevel::Error, msg); }

    fn write(&self, level: LogLevel, msg: &str) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        if (level as u8) < self.min_level.load(Ordering::Relaxed) {
            return;
        }
        let now = Local::now();
        let filename = now.format("%m-%d-%Y").to_string() + ".txt";
        let path = self.log_dir.join(filename);
        let line = format!(
            "[{}] [{}] {}\n",
            now.format("%H:%M:%S"),
            level.label(),
            msg
        );
        if let Err(e) = self.append(&path, &line) {
            eprintln!("Logger: failed to write to {}: {e}", path.display());
        }
    }

    fn append(&self, path: &Path, line: &str) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(line.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_log_file_to_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let logger = Logger::new(tmp.path());
        logger.info("hello from test");

        let entries: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1);

        let contents = fs::read_to_string(entries[0].path()).unwrap();
        assert!(contents.contains("[INFO]"));
        assert!(contents.contains("hello from test"));
    }

    #[test]
    fn respects_min_level() {
        let tmp = tempfile::tempdir().unwrap();
        let logger = Logger::new(tmp.path()).with_level(LogLevel::Warn);
        logger.info("should be suppressed");
        logger.warn("should appear");

        let entries: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        let contents = fs::read_to_string(entries[0].path()).unwrap();
        assert!(!contents.contains("should be suppressed"));
        assert!(contents.contains("should appear"));
    }

    #[test]
    fn set_enabled_suppresses_all_output() {
        let tmp = tempfile::tempdir().unwrap();
        let logger = Logger::new(tmp.path());
        logger.set_enabled(false);
        logger.info("should not appear");
        logger.warn("also suppressed");

        assert!(fs::read_dir(tmp.path()).unwrap().next().is_none());
    }

    #[test]
    fn set_level_affects_clones() {
        let tmp = tempfile::tempdir().unwrap();
        let logger = Logger::new(tmp.path());
        let thread_clone = logger.clone();

        // Raise level via original — clone should respect it
        logger.set_level(LogLevel::Error);
        thread_clone.warn("suppressed by clone");
        thread_clone.error("appears");

        let entries: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        let contents = fs::read_to_string(entries[0].path()).unwrap();
        assert!(!contents.contains("suppressed by clone"));
        assert!(contents.contains("appears"));
    }

    #[test]
    fn creates_log_dir_if_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b").join("logs");
        let logger = Logger::new(&nested);
        logger.info("nested dir test");
        assert!(nested.exists());
    }
}