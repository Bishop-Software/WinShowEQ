use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::Local;

/// Log level matching the C# MySEQ log level enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

/// Writes timestamped log entries to dated files in `log_dir`.
/// File names follow the MySEQ convention: `MM-dd-yyyy.txt`.
/// The log directory is created on first write if it does not exist.
#[derive(Debug, Clone)]
pub struct Logger {
    log_dir: PathBuf,
    min_level: LogLevel,
}

impl Logger {
    pub fn new(log_dir: impl Into<PathBuf>) -> Self {
        Self {
            log_dir: log_dir.into(),
            min_level: LogLevel::Info,
        }
    }

    pub fn with_level(mut self, level: LogLevel) -> Self {
        self.min_level = level;
        self
    }

    pub fn debug(&self, msg: &str) {
        self.write(LogLevel::Debug, msg);
    }

    pub fn info(&self, msg: &str) {
        self.write(LogLevel::Info, msg);
    }

    pub fn warn(&self, msg: &str) {
        self.write(LogLevel::Warn, msg);
    }

    pub fn error(&self, msg: &str) {
        self.write(LogLevel::Error, msg);
    }

    fn write(&self, level: LogLevel, msg: &str) {
        if level < self.min_level {
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
    fn creates_log_dir_if_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b").join("logs");
        let logger = Logger::new(&nested);
        logger.info("nested dir test");
        assert!(nested.exists());
    }
}