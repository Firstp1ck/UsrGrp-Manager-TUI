//! Durable, bounded line-oriented configuration helpers.
//!
//! Settings writers use a same-directory restricted temporary file, `sync_all`,
//! atomic rename, and directory sync. Existing symlinks are rejected rather
//! than followed so an interactive configuration save cannot overwrite an
//! unrelated file.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

/// The maximum accepted configuration source size. Configuration is local UI
/// state, not an unbounded input channel.
pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;
/// A single assignment line must remain small enough for a bounded diagnostic.
pub const MAX_CONFIG_LINE_BYTES: usize = 16 * 1024;

/// A bounded, source-line-aware configuration assignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Assignment {
    pub line: usize,
    pub key: String,
    pub value: String,
}

/// Points at which a deterministic atomic-write test may inject a failure.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AtomicWriteStage {
    BeforeWrite,
    BeforeFlush,
    BeforeFileSync,
    BeforeRename,
    BeforeDirectorySync,
}

/// Read a configuration file with a byte bound enforced while reading.
pub fn read_bounded(path: impl AsRef<Path>) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::with_capacity(MAX_CONFIG_BYTES.min(8192));
    Read::by_ref(&mut file)
        .take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "configuration exceeds the 1048576-byte limit",
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "configuration is not valid UTF-8",
        )
    })
}

/// Parse supported `key = value` source lines while retaining bounded line
/// diagnostics. Blank lines and comment lines are intentionally ignored.
pub fn parse_assignments(contents: &str) -> io::Result<Vec<Assignment>> {
    let mut assignments = Vec::new();
    for (index, raw) in contents.lines().enumerate() {
        let line = index + 1;
        if raw.len() > MAX_CONFIG_LINE_BYTES {
            return invalid_line(line, "line exceeds the 16384-byte limit");
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = trimmed.split_once('=') else {
            return invalid_line(line, "expected `key = value`");
        };
        let key = key.trim();
        let raw_value = raw_value.trim();
        if key.is_empty() || raw_value.is_empty() {
            return invalid_line(line, "key and value are required");
        }
        let value = raw_value
            .split_once(" #")
            .map_or(raw_value, |(value, _)| value)
            .trim();
        if value.is_empty() {
            return invalid_line(line, "value is required");
        }
        assignments.push(Assignment {
            line,
            key: key.to_owned(),
            value: value.to_owned(),
        });
    }
    Ok(assignments)
}

/// Return a `key = value` pair for callers that only need a single line.
pub fn split_assignment(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (key, raw_value) = line.split_once('=')?;
    let key = key.trim();
    let raw_value = raw_value.trim();
    if key.is_empty() || raw_value.is_empty() {
        return None;
    }
    let value = raw_value
        .split_once(" #")
        .map_or(raw_value, |(value, _)| value)
        .trim();
    (!value.is_empty()).then_some((key, value))
}

/// Atomically replace `path` with `contents`.
pub fn atomic_write(path: impl AsRef<Path>, contents: &[u8]) -> io::Result<()> {
    atomic_write_with_fault(path, contents, |_| Ok(()))
}

/// Atomically replace `path`, allowing deterministic fault injection at every
/// durable-write boundary. Production callers should use [`atomic_write`].
pub fn atomic_write_with_fault<F>(
    path: impl AsRef<Path>,
    contents: &[u8],
    mut fault: F,
) -> io::Result<()>
where
    F: FnMut(AtomicWriteStage) -> io::Result<()>,
{
    let path = path.as_ref();
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to write configuration through a symlink",
        ));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "configuration path has no parent directory",
            )
        })?;
    fs::create_dir_all(parent)?;
    let temporary = temporary_path(path, parent)?;
    let result = (|| {
        let mut file = open_restricted(&temporary)?;
        fault(AtomicWriteStage::BeforeWrite)?;
        file.write_all(contents)?;
        fault(AtomicWriteStage::BeforeFlush)?;
        file.flush()?;
        fault(AtomicWriteStage::BeforeFileSync)?;
        file.sync_all()?;
        drop(file);
        fault(AtomicWriteStage::BeforeRename)?;
        fs::rename(&temporary, path)?;
        fault(AtomicWriteStage::BeforeDirectorySync)?;
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        // Before rename this removes the incomplete replacement; after rename
        // it is a harmless no-op and the destination is complete old or new.
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn invalid_line<T>(line: usize, reason: &str) -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("configuration line {line}: {reason}"),
    ))
}

fn temporary_path(path: &Path, parent: &Path) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "configuration filename is not valid UTF-8",
            )
        })?;
    for attempt in 0..128_u32 {
        let candidate = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), attempt));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a configuration temporary filename",
    ))
}

fn open_restricted(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_preserves_hex_and_reports_source_line() {
        let parsed = parse_assignments("title = #CBA6F7 # mauve\n").unwrap();
        assert_eq!(parsed[0].key, "title");
        assert_eq!(parsed[0].value, "#CBA6F7");
        assert!(
            parse_assignments("bad line\n")
                .unwrap_err()
                .to_string()
                .contains("line 1")
        );
    }
}
