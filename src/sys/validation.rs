//! Validated local-account input types.
//!
//! These types encode D8 at the system boundary.  They are intentionally
//! byte-oriented because the shadow-utils command contracts are byte-bounded.

use crate::error::{CoreError, CoreResult};
use std::{fmt, io::Write, path::Path};
use zeroize::{Zeroize, Zeroizing};

const ACCOUNT_NAME_LIMIT: usize = 32;
const GECOS_LIMIT: usize = 256;
const PATH_LIMIT: usize = 4096;
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

/// The absolute-path rules shared by shell and home paths: 1 to 4096 bytes,
/// absolute, and free of control characters.
fn absolute_path(value: &str, field: &'static str) -> CoreResult<()> {
    if value.is_empty() || value.len() > PATH_LIMIT {
        return Err(CoreError::Validation {
            field,
            reason: "must contain 1 to 4096 bytes",
        });
    }
    if !Path::new(value).is_absolute() {
        return Err(CoreError::Validation {
            field,
            reason: "must be absolute",
        });
    }
    if value.chars().any(char::is_control) {
        return Err(CoreError::Validation {
            field,
            reason: "must not contain control characters",
        });
    }
    Ok(())
}

/// A validated shell path suitable for `usermod -s`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ShellPath(String);

impl ShellPath {
    /// Validate and construct an absolute, bounded, control-free shell path
    /// suitable for a mutation request.
    pub fn new(value: impl AsRef<str>) -> CoreResult<Self> {
        let value = value.as_ref();
        absolute_path(value, "shell path")?;
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

/// A validated home directory path suitable for `usermod -d`: the shell
/// path rules, and no colon, which would split the passwd record.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HomePath(String);

impl HomePath {
    /// Validate and construct an absolute, bounded, control-free and
    /// colon-free home directory path.
    pub fn new(value: impl AsRef<str>) -> CoreResult<Self> {
        let value = value.as_ref();
        absolute_path(value, "home directory")?;
        if value.contains(':') {
            return Err(CoreError::Validation {
                field: "home directory",
                reason: "must not contain colons",
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// The validated path passed to `usermod -d`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for HomePath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for HomePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A validated account expiry date suitable for `chage -E`: `YYYY-MM-DD`,
/// a real proleptic Gregorian date, with a year from 1970 to 9999.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ExpiryDate {
    text: String,
    year: i64,
    month: i64,
    day: i64,
}

impl ExpiryDate {
    const FIELD: &'static str = "expiry date";

    /// Validate and construct an expiry date from its `YYYY-MM-DD` text.
    pub fn new(value: impl AsRef<str>) -> CoreResult<Self> {
        let value = value.as_ref();
        let bytes = value.as_bytes();
        let shaped = bytes.len() == 10
            && bytes.iter().enumerate().all(|(index, byte)| match index {
                4 | 7 => *byte == b'-',
                _ => byte.is_ascii_digit(),
            });
        if !shaped {
            return Err(CoreError::Validation {
                field: Self::FIELD,
                reason: "must be a date as YYYY-MM-DD",
            });
        }
        let number = |range: std::ops::Range<usize>| {
            bytes[range]
                .iter()
                .fold(0_i64, |total, digit| total * 10 + i64::from(digit - b'0'))
        };
        let (year, month, day) = (number(0..4), number(5..7), number(8..10));
        if !(1970..=9999).contains(&year) {
            return Err(CoreError::Validation {
                field: Self::FIELD,
                reason: "year must be between 1970 and 9999",
            });
        }
        if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
            return Err(CoreError::Validation {
                field: Self::FIELD,
                reason: "must be a real calendar date",
            });
        }
        Ok(Self {
            text: value.to_owned(),
            year,
            month,
            day,
        })
    }

    /// The validated `YYYY-MM-DD` text passed to `chage -E`.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Days since 1970-01-01, the unit of the shadow expiry field.
    pub fn days_since_epoch(&self) -> i64 {
        // Howard Hinnant's civil-to-days algorithm: years start on March 1
        // so the leap day is the last day of a year, in 400-year eras.
        let year = if self.month <= 2 {
            self.year - 1
        } else {
            self.year
        };
        let era = year.div_euclid(400);
        let year_of_era = year.rem_euclid(400);
        let shifted_month = (self.month + 9) % 12;
        let day_of_year = (153 * shifted_month + 2) / 5 + self.day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }
}

/// Days in `month` (1 to 12) of `year` in the proleptic Gregorian calendar.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl AsRef<str> for ExpiryDate {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ExpiryDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
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

/// The `chpasswd` protocol rules for a password: at most 1024 bytes and no
/// NUL, carriage return, or newline.
fn check_password(value: &[u8]) -> CoreResult<()> {
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
    Ok(())
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
        check_password(password.as_bytes())?;
        Ok(Self { username, password })
    }

    /// Apply the password rules of [`PasswordRecord::new`] to a borrowed
    /// value without taking or copying it, so a form can report a problem
    /// and keep what was typed.
    pub fn check(password: &str) -> CoreResult<()> {
        check_password(password.as_bytes())
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
    fn home_paths_are_absolute_bounded_and_record_safe() {
        for valid in ["/home/alice", "/srv/alice", "/", "/srv/with space"] {
            assert!(HomePath::new(valid).is_ok(), "{valid}");
        }
        for (invalid, reason) in [
            ("", "must contain 1 to 4096 bytes"),
            ("home/alice", "must be absolute"),
            ("~alice", "must be absolute"),
            ("/home/al\nice", "must not contain control characters"),
            ("/home/a:b", "must not contain colons"),
        ] {
            assert!(
                matches!(
                    HomePath::new(invalid),
                    Err(CoreError::Validation { field: "home directory", reason: got })
                        if got == reason
                ),
                "{invalid:?}"
            );
        }
        assert!(HomePath::new(format!("/{}", "a".repeat(4096))).is_err());
        // The shell path keeps the same shared rules.
        assert!(ShellPath::new("bin/sh").is_err());
    }

    #[test]
    fn expiry_dates_are_real_calendar_dates_from_1970_to_9999() {
        for (valid, days) in [
            ("1970-01-01", 0),
            ("2000-02-29", 11_016),
            ("2024-02-29", 19_782),
            ("2027-01-31", 20_849),
            ("2100-02-28", 47_540),
            ("9999-12-31", 2_932_896),
        ] {
            let date = ExpiryDate::new(valid).unwrap_or_else(|_| panic!("{valid}"));
            assert_eq!(date.as_str(), valid);
            assert_eq!(date.days_since_epoch(), days, "{valid}");
        }
        for (invalid, reason) in [
            ("", "must be a date as YYYY-MM-DD"),
            ("2027-1-31", "must be a date as YYYY-MM-DD"),
            ("2027/01/31", "must be a date as YYYY-MM-DD"),
            ("2027-01-31 ", "must be a date as YYYY-MM-DD"),
            ("-1", "must be a date as YYYY-MM-DD"),
            ("2027-01-3a", "must be a date as YYYY-MM-DD"),
            ("２027-01-31", "must be a date as YYYY-MM-DD"),
            ("1969-12-31", "year must be between 1970 and 9999"),
            ("0000-01-01", "year must be between 1970 and 9999"),
            ("2027-00-10", "must be a real calendar date"),
            ("2027-13-01", "must be a real calendar date"),
            ("2027-01-00", "must be a real calendar date"),
            ("2027-04-31", "must be a real calendar date"),
            ("2027-02-29", "must be a real calendar date"),
            ("2100-02-29", "must be a real calendar date"),
        ] {
            assert!(
                matches!(
                    ExpiryDate::new(invalid),
                    Err(CoreError::Validation { field: "expiry date", reason: got })
                        if got == reason
                ),
                "{invalid:?}"
            );
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

    #[test]
    fn password_check_matches_the_record_rules() {
        let record = |value: &str| {
            PasswordRecord::new(UserName::new("alice").unwrap(), SecretString::new(value))
        };
        for invalid in [
            "a".repeat(1025),
            "é".repeat(513),
            "line\nbreak".to_owned(),
            "carriage\rreturn".to_owned(),
            "nul\0byte".to_owned(),
        ] {
            let checked = PasswordRecord::check(&invalid).unwrap_err();
            let built = record(&invalid).err().expect("record rejects it too");
            assert_eq!(checked, built, "{invalid:?}");
        }
        for valid in [
            "correct horse battery",
            &"a".repeat(1024),
            "é".repeat(512).as_str(),
        ] {
            assert!(PasswordRecord::check(valid).is_ok(), "{valid:?}");
            assert!(record(valid).is_ok(), "{valid:?}");
        }
    }
}
