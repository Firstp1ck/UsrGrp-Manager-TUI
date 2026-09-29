//! Durable general settings (`settings.conf`).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Whether moving past the last row of a main table goes to the first
    /// (and past the first to the last). When false the selection stops at
    /// the ends, page keys included.
    pub wrap_selection: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wrap_selection: true,
        }
    }
}

impl Settings {
    pub fn load_or_init(path: &str) -> std::io::Result<Self> {
        match Self::from_file(path) {
            Ok(settings) => Ok(settings),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let settings = Self::default();
                settings.write_file(path)?;
                Ok(settings)
            }
            Err(error) => Err(error),
        }
    }

    pub fn from_file(path: &str) -> std::io::Result<Self> {
        let contents = crate::config::read_bounded(path)?;
        let mut settings = Self::default();
        let mut seen = std::collections::HashSet::new();
        for assignment in crate::config::parse_assignments(&contents)? {
            let key = assignment.key.as_str();
            let value = assignment.value.as_str();
            if !seen.insert(key.to_owned()) {
                return invalid_line(assignment.line, "duplicate settings key");
            }
            match key {
                "wrap_selection" => settings.wrap_selection = parse_bool(assignment.line, value)?,
                _ => return invalid_line(assignment.line, "unknown settings key"),
            }
        }
        Ok(settings)
    }

    pub fn write_file(&self, path: &str) -> std::io::Result<()> {
        let contents = format!(
            "# usrgrp-manager settings\n# Canonical values are lossless and atomically saved.\nwrap_selection = {}\n",
            self.wrap_selection,
        );
        crate::config::atomic_write(path, contents.as_bytes())
    }
}

fn parse_bool(line: usize, value: &str) -> std::io::Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => invalid_line(line, "expected true or false"),
    }
}

fn invalid_line<T>(line: usize, reason: &str) -> std::io::Result<T> {
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("configuration line {line}: {reason}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(contents: &str) -> std::io::Result<Settings> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.conf");
        std::fs::write(&path, contents).unwrap();
        Settings::from_file(path.to_str().unwrap())
    }

    #[test]
    fn wrap_selection_defaults_to_true_and_parses_both_values() {
        assert!(Settings::default().wrap_selection);
        assert!(parse("").unwrap().wrap_selection);
        assert!(
            parse("# comment\nwrap_selection = true\n")
                .unwrap()
                .wrap_selection
        );
        assert!(!parse("wrap_selection = false\n").unwrap().wrap_selection);
    }

    #[test]
    fn bad_lines_name_the_line_and_the_reason() {
        let error = |contents: &str| parse(contents).unwrap_err().to_string();
        assert_eq!(
            error("wrap_selection = true\nwrap_selection = false\n"),
            "configuration line 2: duplicate settings key"
        );
        assert_eq!(
            error("\nwrap = true\n"),
            "configuration line 2: unknown settings key"
        );
        assert_eq!(
            error("wrap_selection = yes\n"),
            "configuration line 1: expected true or false"
        );
    }

    #[test]
    fn load_or_init_writes_defaults_when_missing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.conf");
        let path = path.to_str().unwrap();
        assert_eq!(Settings::load_or_init(path).unwrap(), Settings::default());
        assert_eq!(Settings::from_file(path).unwrap(), Settings::default());
    }
}
