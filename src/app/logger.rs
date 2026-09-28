//! Optional append-only operation log enabled with `--log-file`.
//!
//! Callers only pass redacted previews, report summaries, and classified
//! error text. Password material never reaches this module.

use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::Path,
};

/// Longest message kept per line; longer messages are truncated.
const MAX_LINE_BYTES: usize = 4096;

/// Writes one sanitized line per event and flushes it immediately so a crash
/// or a killed terminal still leaves the preceding lines on disk.
pub struct Logger {
    sink: Box<dyn Write + Send>,
}

impl Logger {
    /// Open `path` for appending, creating it with mode 0600. An existing
    /// symlink at `path` is rejected rather than followed.
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.append(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        Ok(Self::from_writer(Box::new(options.open(path)?)))
    }

    /// Log into any writer. Tests use an in-memory buffer.
    pub fn from_writer(sink: Box<dyn Write + Send>) -> Self {
        Self { sink }
    }

    /// Append `<unix seconds> <kind>: <message>` as a single line.
    pub fn write_event(&mut self, unix_seconds: u64, kind: &str, message: &str) -> io::Result<()> {
        let line = format!("{unix_seconds} {kind}: {}\n", sanitize(message));
        self.sink.write_all(line.as_bytes())?;
        self.sink.flush()
    }
}

/// Keep each event on one line: newlines become ` | `, other control
/// characters become `?`, and the result is capped at `MAX_LINE_BYTES`.
fn sanitize(message: &str) -> String {
    let mut line = String::with_capacity(message.len().min(MAX_LINE_BYTES));
    for character in message.chars() {
        let mut buffer = [0; 4];
        let piece = match character {
            '\n' => " | ",
            '\r' => "",
            character if character.is_control() => "?",
            character => character.encode_utf8(&mut buffer),
        };
        if line.len() + piece.len() > MAX_LINE_BYTES {
            break;
        }
        line.push_str(piece);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_one_line_and_bounds_length() {
        assert_eq!(sanitize("a\nb\r\n\x1b[2Jc"), "a | b | ?[2Jc");
        assert_eq!(sanitize(&"é".repeat(5000)).len(), MAX_LINE_BYTES);
    }
}
