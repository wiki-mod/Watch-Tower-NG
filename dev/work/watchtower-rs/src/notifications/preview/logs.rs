#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;

use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    pub message: String,
    pub data: HashMap<String, serde_json::Value>,
    pub time: OffsetDateTime,
    pub level: LogLevel,
}

// LogLevel is the analog of logrus.Level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
    Panic,
}

impl LogLevel {
    // Return the compact legacy string form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warning",
            Self::Error => "error",
            Self::Fatal => "fatal",
            Self::Panic => "panic",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// LevelsFromString parses a string of level characters and returns a slice of the corresponding log levels.
pub fn levels_from_string(input: &str) -> Vec<LogLevel> {
    let mut levels = Vec::with_capacity(input.len());

    for c in input.chars() {
        match c {
            'p' => levels.push(LogLevel::Panic),
            'f' => levels.push(LogLevel::Fatal),
            'e' => levels.push(LogLevel::Error),
            'w' => levels.push(LogLevel::Warn),
            'i' => levels.push(LogLevel::Info),
            'd' => levels.push(LogLevel::Debug),
            't' => levels.push(LogLevel::Trace),
            _ => continue,
        }
    }

    levels
}
