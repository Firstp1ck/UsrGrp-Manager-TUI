//! Validated local-account input types.
//!
//! These types encode D8 at the system boundary.  They are intentionally
//! byte-oriented because the shadow-utils command contracts are byte-bounded.

use crate::error::{CoreError, CoreResult};
use std::{fmt, io::Write, path::Path};
use zeroize::{Zeroize, Zeroizing};

const ACCOUNT_NAME_LIMIT: usize = 32;
const GECOS_LIMIT: usize = 256;
const SHELL_LIMIT: usize = 4096;
const PASSWORD_LIMIT: usize = 1024;

fn account_name(value: &str, field: &'static str) -> CoreResult<()> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > ACCOUNT_NAME_LIMIT {
        return Err(CoreError::Validation {
            field,
            reason: "must contain 1 to 32 bytes",
        });
    }
    if value.starts_with('-') {
        return Err(CoreError::Validation {
            field,
            reason: "must not start with a hyphen",
        });
    }
    let mut chars = bytes.iter().copied();
    let first = chars.next().expect("non-empty name checked above");
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return Err(CoreError::Validation {
            field,
            reason: "must start with an ASCII letter or underscore",
        });
    }
    let mut trailing_dollar = false;
    for (index, byte) in bytes.iter().copied().enumerate().skip(1) {
        if byte == b'$' && index + 1 == bytes.len() {
            trailing_dollar = true;
            continue;
        }
        if !(byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
            return Err(CoreError::Validation {
                field,
                reason: "contains a disallowed character",
            });
        }
    }
    if value.contains('$') && !trailing_dollar {
        return Err(CoreError::Validation {
            field,
            reason: "may contain a dollar only as its final byte",
        });
    }
    Ok(())
}

/// A validated local user name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UserName(String);

impl UserName {
    /// Validate and construct a user name.
    pub fn new(value: impl AsRef<str>) -> CoreResult<Self> {
        let value = value.as_ref();
        account_name(value, "user name")?;
        Ok(Self(value.to_owned()))
    }

    /// The validated name passed to account tools.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for UserName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for UserName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<&str> for UserName {
    type Error = CoreError;

    fn try_from(value: &str) -> CoreResult<Self> {
        Self::new(value)
    }
}

/// A validated local group name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GroupName(String);

impl GroupName {
    /// Validate and construct a group name.
    pub fn new(value: impl AsRef<str>) -> CoreResult<Self> {
        let value = value.as_ref();
        account_name(value, "group name")?;
        Ok(Self(value.to_owned()))
    }

    /// The validated name passed to account tools.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for GroupName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for GroupName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<&str> for GroupName {
    type Error = CoreError;

    fn try_from(value: &str) -> CoreResult<Self> {
        Self::new(value)
    }
}

/// A validated shell path suitable for `usermod -s`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ShellPath(String);

impl ShellPath {
    /// Validate and construct an absolute, bounded, control-free shell path
    /// suitable for a mutation request.
    pub fn new(value: impl AsRef<str>) -> CoreResult<Self> {
        let value = value.as_ref();
        if value.is_empty() || value.len() > SHELL_LIMIT {
            return Err(CoreError::Validation {
                field: "shell path",
                reason: "must contain 1 to 4096 bytes",
            });
        }
        if !Path::new(value).is_absolute() {
            return Err(CoreError::Validation {
                field: "shell path",
                reason: "must be absolute",
            });
        }
        if value.chars().any(char::is_control) {
            return Err(CoreError::Validation {
                field: "shell path",
                reason: "must not contain control characters",
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// Parse the observed passwd shell field without applying mutation-input
    /// rules. An empty passwd shell is valid Linux-local data and means the
    /// documented default shell; it must not be offered back to `usermod -s`.
    pub(crate) fn from_observed_passwd(value: &str) -> CoreResult<Self> {
        if value.is_empty() {
            return Ok(Self(String::new()));
        }
        Self::new(value)
    }

    /// Whether this is the valid empty observed passwd shell rather than a
    /// mutation-safe shell path.
    pub fn is_observed_default(&self) -> bool {
        self.0.is_empty()
    }

    /// A display label which makes an observed empty passwd shell explicit.
    pub fn display_label(&self) -> &str {
        if self.is_observed_default() {
            "(default /bin/sh)"
        } else {
            self.as_str()
        }
    }

    /// The raw parsed shell field. Empty is possible only for observed passwd
    /// records created by the local data source.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ShellPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ShellPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A validated GECOS field suitable for `usermod -c`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Gecos(String);

impl Gecos {
    /// Validate and construct a bounded, delimiter-free GECOS field.
    pub fn new(value: impl AsRef<str>) -> CoreResult<Self> {
        let value = value.as_ref();
        if value.len() > GECOS_LIMIT {
            return Err(CoreError::Validation {
                field: "GECOS",
                reason: "must not exceed 256 bytes",
            });
        }
        if value.contains(':') || value.chars().any(char::is_control) {
            return Err(CoreError::Validation {
                field: "GECOS",
                reason: "must not contain colons or control characters",
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// The validated GECOS text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Gecos {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// An owned secret which is zeroized on drop.
///
/// It deliberately implements neither `Debug`, `Display`, `Clone`, nor
/// comparison traits.  Callers should pass it directly to authentication or a
/// password record instead of retaining it in application state.
pub struct SecretString {
    value: Zeroizing<String>,
}

impl SecretString {
    /// Wrap a secret for one-shot trusted-boundary use.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: Zeroizing::new(value.into()),
        }
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.value.as_bytes()
    }

    pub(crate) fn write_to(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        writer.write_all(self.as_bytes())
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

/// A bounded password record for `chpasswd` stdin.
///
/// The password is never rendered, placed in argv, or exposed by `Debug`.
pub struct PasswordRecord {
    username: UserName,
    password: SecretString,
}

impl PasswordRecord {
    /// Construct a record after enforcing the unambiguous `chpasswd` protocol.
    pub fn new(username: UserName, password: SecretString) -> CoreResult<Self> {
        let value = password.as_bytes();
        if value.len() > PASSWORD_LIMIT {
            return Err(CoreError::Validation {
                field: "password",
                reason: "must not exceed 1024 bytes",
            });
        }
        if value
            .iter()
            .any(|byte| matches!(byte, b'\0' | b'\r' | b'\n'))
        {
            return Err(CoreError::Validation {
                field: "password",
                reason: "must not contain NUL, carriage return, or newline",
            });
        }
        Ok(Self { username, password })
    }

    /// The account whose password is being changed.
    pub fn username(&self) -> &UserName {
        &self.username
    }

    pub(crate) fn write_to(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        writer.write_all(self.username.as_str().as_bytes())?;
        writer.write_all(b":")?;
        self.password.write_to(writer)?;
        writer.write_all(b"\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_names_follow_d8_grammar() {
        for valid in ["alice", "_daemon", "build-user", "machine$"] {
            assert!(UserName::new(valid).is_ok(), "{valid}");
            assert!(GroupName::new(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "",
            "-alice",
            "1alice",
            "alice$more",
            "alice:name",
            "alice name",
        ] {
            assert!(UserName::new(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn password_record_writes_only_the_stdin_protocol() {
        let user = UserName::new("alice").unwrap();
        let record = PasswordRecord::new(user, SecretString::new("not-in-argv")).unwrap();
        let mut stdin = Vec::new();
        record.write_to(&mut stdin).unwrap();
        assert_eq!(stdin, b"alice:not-in-argv\n");
    }
}
