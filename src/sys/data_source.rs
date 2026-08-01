//! Linux-local account data sources and fail-closed parsers.

use super::validation::{Gecos, GroupName, ShellPath, UserName};
use crate::error::{CoreError, CoreResult};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

/// Maximum size accepted for an individual Linux-local account database.
/// The bound is enforced while reading rather than after an unbounded allocation.
pub const MAX_ACCOUNT_FILE_BYTES: usize = 1024 * 1024;
const MAX_RECORD_BYTES: usize = 8192;
const MAX_RECORDS: usize = 100_000;

/// A Unix user ID parsed without fallback coercion.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Uid(pub u32);

/// A Unix group ID parsed without fallback coercion.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Gid(pub u32);

/// A typed record from the local passwd file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountUser {
    pub uid: Uid,
    pub name: UserName,
    pub primary_gid: Gid,
    pub full_name: Option<Gecos>,
    pub home_dir: PathBuf,
    pub shell: ShellPath,
}

/// A typed record from the local group file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountGroup {
    pub gid: Gid,
    pub name: GroupName,
    pub members: Vec<UserName>,
}

/// A bounded diagnostic for one malformed local-file record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseDiagnostic {
    pub source: PathBuf,
    pub line: usize,
    pub reason: &'static str,
}

/// A parsed record collection that preserves source/line diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedRecords<T> {
    pub records: Vec<T>,
    pub diagnostics: Vec<ParseDiagnostic>,
}

/// The account data observed during one explicit refresh.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountSnapshot {
    pub users: Vec<AccountUser>,
    pub groups: Vec<AccountGroup>,
    pub shells: Vec<ShellPath>,
    pub diagnostics: Vec<ParseDiagnostic>,
}

impl AccountSnapshot {
    /// An explicit empty snapshot is distinct from a failed refresh.
    pub fn empty() -> Self {
        Self {
            users: Vec::new(),
            groups: Vec::new(),
            shells: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

/// Snapshot state retained by callers across refresh failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SnapshotState {
    Fresh(AccountSnapshot),
    Stale {
        prior: AccountSnapshot,
        error: CoreError,
    },
    Unavailable {
        error: CoreError,
    },
}

/// Refresh a source without converting a failure into an empty success.
pub fn refresh_retaining(
    source: &dyn AccountDataSource,
    prior: Option<AccountSnapshot>,
) -> SnapshotState {
    match source.refresh() {
        Ok(snapshot) => SnapshotState::Fresh(snapshot),
        Err(error) => match prior {
            Some(prior) => SnapshotState::Stale { prior, error },
            None => SnapshotState::Unavailable { error },
        },
    }
}

/// Injectable source for a complete account refresh.
pub trait AccountDataSource: Send + Sync {
    /// Read one coherent account snapshot.  Errors must not be converted to an
    /// empty snapshot by implementations or callers.
    fn refresh(&self) -> CoreResult<AccountSnapshot>;
}

/// Linux-local paths used by [`LocalFileAccountDataSource`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountPaths {
    pub passwd: PathBuf,
    pub group: PathBuf,
    pub shells: PathBuf,
}

impl Default for AccountPaths {
    fn default() -> Self {
        Self {
            passwd: PathBuf::from("/etc/passwd"),
            group: PathBuf::from("/etc/group"),
            shells: PathBuf::from("/etc/shells"),
        }
    }
}

/// The supported Linux-local account-file source.
#[derive(Clone, Debug)]
pub struct LocalFileAccountDataSource {
    paths: AccountPaths,
}

impl LocalFileAccountDataSource {
    /// Create the default `/etc`-backed Linux-local source.
    pub fn new() -> Self {
        Self::with_paths(AccountPaths::default())
    }

    /// Create a source rooted in injected fixture paths.
    pub fn with_paths(paths: AccountPaths) -> Self {
        Self { paths }
    }

    /// Paths used by this source.
    pub fn paths(&self) -> &AccountPaths {
        &self.paths
    }
}

impl Default for LocalFileAccountDataSource {
    fn default() -> Self {
        Self::new()
    }
}

impl AccountDataSource for LocalFileAccountDataSource {
    fn refresh(&self) -> CoreResult<AccountSnapshot> {
        let passwd = read_account_file(&self.paths.passwd, "passwd data")?;
        let group = read_account_file(&self.paths.group, "group data")?;
        let shells = read_account_file(&self.paths.shells, "shell data")?;

        let users = parse_passwd_records(&passwd, &self.paths.passwd);
        let groups = parse_group_records(&group, &self.paths.group);
        let shells = parse_shell_records(&shells, &self.paths.shells);
        let mut diagnostics = users.diagnostics;
        diagnostics.extend(groups.diagnostics);
        diagnostics.extend(shells.diagnostics);
        Ok(AccountSnapshot {
            users: users.records,
            groups: groups.records,
            shells: shells.records,
            diagnostics,
        })
    }
}

/// Parse local passwd data while retaining malformed-line diagnostics.
pub fn parse_passwd_records(
    contents: &str,
    source: impl AsRef<Path>,
) -> ParsedRecords<AccountUser> {
    let source = source.as_ref();
    let mut records = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, line) in contents.lines().take(MAX_RECORDS).enumerate() {
        let line_number = index + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.len() > MAX_RECORD_BYTES {
            diagnostic(
                &mut diagnostics,
                source,
                line_number,
                "passwd record exceeds byte limit",
            );
            continue;
        }
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() != 7 {
            diagnostic(
                &mut diagnostics,
                source,
                line_number,
                "expected seven colon-delimited fields",
            );
            continue;
        }
        let (name, uid, gid, full_name, home_dir, shell) = match (
            UserName::new(fields[0]),
            parse_id(fields[2], "UID"),
            parse_id(fields[3], "GID"),
            if fields[4].is_empty() {
                Ok(None)
            } else {
                Gecos::new(fields[4]).map(Some)
            },
            shell_home(fields[5]),
            ShellPath::from_observed_passwd(fields[6]),
        ) {
            (Ok(name), Ok(uid), Ok(gid), Ok(full_name), Ok(home_dir), Ok(shell)) => {
                (name, uid, gid, full_name, home_dir, shell)
            }
            _ => {
                diagnostic(
                    &mut diagnostics,
                    source,
                    line_number,
                    "invalid passwd field",
                );
                continue;
            }
        };
        records.push(AccountUser {
            uid: Uid(uid),
            name,
            primary_gid: Gid(gid),
            full_name,
            home_dir,
            shell,
        });
    }
    ParsedRecords {
        records,
        diagnostics,
    }
}

/// Parse local group data while retaining malformed-line diagnostics.
pub fn parse_group_records(
    contents: &str,
    source: impl AsRef<Path>,
) -> ParsedRecords<AccountGroup> {
    let source = source.as_ref();
    let mut records = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, line) in contents.lines().take(MAX_RECORDS).enumerate() {
        let line_number = index + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.len() > MAX_RECORD_BYTES {
            diagnostic(
                &mut diagnostics,
                source,
                line_number,
                "group record exceeds byte limit",
            );
            continue;
        }
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() != 4 {
            diagnostic(
                &mut diagnostics,
                source,
                line_number,
                "expected four colon-delimited fields",
            );
            continue;
        }
        let name = match GroupName::new(fields[0]) {
            Ok(name) => name,
            Err(_) => {
                diagnostic(&mut diagnostics, source, line_number, "invalid group name");
                continue;
            }
        };
        let gid = match parse_id(fields[2], "GID") {
            Ok(gid) => Gid(gid),
            Err(_) => {
                diagnostic(&mut diagnostics, source, line_number, "invalid GID");
                continue;
            }
        };
        let mut members = Vec::new();
        let mut invalid_member = false;
        if !fields[3].is_empty() {
            for member in fields[3].split(',') {
                match UserName::new(member) {
                    Ok(member) => members.push(member),
                    Err(_) => {
                        invalid_member = true;
                        break;
                    }
                }
            }
        }
        if invalid_member {
            diagnostic(
                &mut diagnostics,
                source,
                line_number,
                "invalid group member name",
            );
            continue;
        }
        records.push(AccountGroup { gid, name, members });
    }
    ParsedRecords {
        records,
        diagnostics,
    }
}

/// Parse shell entries from an injected local shell-file payload.
pub fn parse_shell_records(contents: &str, source: impl AsRef<Path>) -> ParsedRecords<ShellPath> {
    let source = source.as_ref();
    let mut records = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, line) in contents.lines().take(MAX_RECORDS).enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.len() > MAX_RECORD_BYTES {
            diagnostic(
                &mut diagnostics,
                source,
                index + 1,
                "shell record exceeds byte limit",
            );
            continue;
        }
        match ShellPath::new(line) {
            Ok(shell) => records.push(shell),
            Err(_) => diagnostic(&mut diagnostics, source, index + 1, "invalid shell path"),
        }
    }
    ParsedRecords {
        records,
        diagnostics,
    }
}

fn read_account_file(path: &Path, operation: &'static str) -> CoreResult<String> {
    let mut file = File::open(path).map_err(|error| CoreError::Refresh {
        operation,
        kind: error.kind(),
    })?;
    let mut bytes = Vec::with_capacity(MAX_ACCOUNT_FILE_BYTES.min(8192));
    let mut limited = file.by_ref().take((MAX_ACCOUNT_FILE_BYTES + 1) as u64);
    limited
        .read_to_end(&mut bytes)
        .map_err(|error| CoreError::Refresh {
            operation,
            kind: error.kind(),
        })?;
    if bytes.len() > MAX_ACCOUNT_FILE_BYTES {
        return Err(CoreError::Validation {
            field: "account file",
            reason: "exceeds the configured byte limit",
        });
    }
    String::from_utf8(bytes).map_err(|_| CoreError::Refresh {
        operation,
        kind: std::io::ErrorKind::InvalidData,
    })
}

fn parse_id(value: &str, field: &'static str) -> CoreResult<u32> {
    value.parse::<u32>().map_err(|_| CoreError::Validation {
        field,
        reason: "must be an unsigned 32-bit integer",
    })
}

fn shell_home(value: &str) -> CoreResult<PathBuf> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(CoreError::Validation {
            field: "home directory",
            reason: "must be non-empty and control-free",
        });
    }
    Ok(PathBuf::from(value))
}

fn diagnostic(
    diagnostics: &mut Vec<ParseDiagnostic>,
    source: &Path,
    line: usize,
    reason: &'static str,
) {
    const MAX_DIAGNOSTICS: usize = 64;
    if diagnostics.len() < MAX_DIAGNOSTICS {
        diagnostics.push(ParseDiagnostic {
            source: source.to_path_buf(),
            line,
            reason,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_ids_are_diagnosed_not_coerced_to_root() {
        let parsed = parse_passwd_records(
            "broken:x:not-a-number:0:Broken:/home/broken:/bin/sh\nroot:x:0:0:root:/root:/bin/sh\n",
            "fixture/passwd",
        );
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(parsed.records[0].name.as_str(), "root");
        assert_eq!(parsed.records[0].uid, Uid(0));
        assert_eq!(parsed.diagnostics[0].line, 1);
    }
}
