use crate::error::Result;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct SystemUser {
    pub uid: u32,
    pub name: String,
    pub primary_gid: u32,
    pub full_name: Option<String>,
    pub home_dir: String,
    pub shell: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct SystemGroup {
    pub gid: u32,
    pub name: String,
    pub members: Vec<String>,
}

#[allow(dead_code)]
pub struct SystemAdapter {
    pub sudo_password: Option<String>,
}

#[allow(dead_code)]
impl SystemAdapter {
    pub fn new() -> Self { Self { sudo_password: None } }

    pub fn with_sudo_password(password: Option<String>) -> Self {
        Self { sudo_password: password }
    }

    pub fn list_users(&self) -> Result<Vec<SystemUser>> {
        parse_passwd("/etc/passwd")
    }

    pub fn list_groups(&self) -> Result<Vec<SystemGroup>> {
        parse_group("/etc/group")
    }

    pub fn groups_for_user(&self, username: &str, primary_gid: u32) -> Result<Vec<SystemGroup>> {
        let groups = self.list_groups()?;
        let filtered = groups
            .into_iter()
            .filter(|g| g.gid == primary_gid || g.members.iter().any(|m| m == username))
            .collect();
        Ok(filtered)
    }

    pub fn add_user_to_group(&self, username: &str, groupname: &str) -> Result<()> {
        // Prefer gpasswd for membership changes
        let output = self
            .run_privileged("gpasswd", &["-a", username, groupname])
            .map_err(|e| crate::error::simple_error(format!("failed to execute gpasswd -a {} {}: {}", username, groupname, e)))?;
        if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("gpasswd -a", &output))) }
    }

    pub fn remove_user_from_group(&self, username: &str, groupname: &str) -> Result<()> {
        let output = self
            .run_privileged("gpasswd", &["-d", username, groupname])
            .map_err(|e| crate::error::simple_error(format!("failed to execute gpasswd -d {} {}: {}", username, groupname, e)))?;
        if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("gpasswd -d", &output))) }
    }

    pub fn create_group(&self, groupname: &str) -> Result<()> {
        let output = self
            .run_privileged("groupadd", &[groupname])
            .map_err(|e| crate::error::simple_error(format!("failed to execute groupadd {}: {}", groupname, e)))?;
        if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("groupadd", &output))) }
    }

    pub fn create_user(&self, username: &str, create_home: bool) -> Result<()> {
        let mut args: Vec<&str> = Vec::new();
        if create_home { args.push("-m"); }
        args.push(username);
        let output = self
            .run_privileged("useradd", &args)
            .map_err(|e| crate::error::simple_error(format!("failed to execute useradd {}: {}", username, e)))?;
        if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("useradd", &output))) }
    }

    pub fn delete_group(&self, groupname: &str) -> Result<()> {
        let output = self
            .run_privileged("groupdel", &[groupname])
            .map_err(|e| crate::error::simple_error(format!("failed to execute groupdel {}: {}", groupname, e)))?;
        if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("groupdel", &output))) }
    }

    pub fn rename_group(&self, old_name: &str, new_name: &str) -> Result<()> {
        let output = self
            .run_privileged("groupmod", &["-n", new_name, old_name])
            .map_err(|e| crate::error::simple_error(format!("failed to execute groupmod -n {} {}: {}", new_name, old_name, e)))?;
        if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("groupmod -n", &output))) }
    }

    pub fn delete_user(&self, username: &str, delete_home: bool) -> Result<()> {
        let mut args: Vec<&str> = Vec::new();
        if delete_home { args.push("-r"); }
        args.push(username);
        let output = self
            .run_privileged("userdel", &args)
            .map_err(|e| crate::error::simple_error(format!("failed to execute userdel {}: {}", username, e)))?;
        if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("userdel", &output))) }
    }

    pub fn list_shells(&self) -> Result<Vec<String>> {
        let contents = fs::read_to_string("/etc/shells")?;
        let shells = contents
            .lines()
            .filter_map(|line| {
                let t = line.trim();
                if t.is_empty() || t.starts_with('#') { None } else { Some(t.to_string()) }
            })
            .collect::<Vec<_>>();
        Ok(shells)
    }

    pub fn change_user_shell(&self, username: &str, new_shell: &str) -> Result<()> {
        let output = self
            .run_privileged("usermod", &["-s", new_shell, username])
            .map_err(|e| crate::error::simple_error(format!("failed to execute usermod -s {} {}: {}", new_shell, username, e)))?;
        if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("usermod -s", &output))) }
    }

    pub fn change_user_fullname(&self, username: &str, new_fullname: &str) -> Result<()> {
        let output = self
            .run_privileged("usermod", &["-c", new_fullname, username])
            .map_err(|e| crate::error::simple_error(format!("failed to execute usermod -c {} {}: {}", new_fullname, username, e)))?;
        if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("usermod -c", &output))) }
    }

    pub fn change_username(&self, old_username: &str, new_username: &str) -> Result<()> {
        let output = self
            .run_privileged("usermod", &["-l", new_username, old_username])
            .map_err(|e| crate::error::simple_error(format!("failed to execute usermod -l {} {}: {}", new_username, old_username, e)))?;
        if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("usermod -l", &output))) }
    }

    pub fn set_user_password(&self, username: &str, password: &str) -> Result<()> {
        use std::io::Write;
        if current_uid() == 0 {
            // Root: write to chpasswd stdin directly
            let mut child = std::process::Command::new("chpasswd")
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("failed to spawn chpasswd: {}", e))?;
            if let Some(mut stdin) = child.stdin.take() {
                let line = format!("{}:{}\n", username, password);
                let _ = stdin.write_all(line.as_bytes());
            }
            let output = child.wait_with_output()?;
            if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("chpasswd", &output))) }
        } else {
            // Non-root: avoid mixing sudo password and chpasswd input on the same stdin.
            // If we don't yet have a sudo password, surface an explicit authentication error
            // instead of attempting sudo with an empty line (which would count as a failed try).
            if self.sudo_password.is_none() {
                return Err(crate::error::simple_error("Authentication required"));
            }
            // Use a bash -c pipeline so chpasswd reads from echo, while we send only the sudo password to sudo.
            fn escape_for_double_quotes(s: &str) -> String {
                let mut out = String::with_capacity(s.len());
                for ch in s.chars() {
                    match ch {
                        '\\' => out.push_str("\\\\"),
                        '"' => out.push_str("\\\""),
                        '$' => out.push_str("\\$"),
                        '`' => out.push_str("\\`"),
                        _ => out.push(ch),
                    }
                }
                out
            }
            let u = escape_for_double_quotes(username);
            let p = escape_for_double_quotes(password);
            let cmd = format!("echo \"{}:{}\" | chpasswd", u, p);
            let mut child = std::process::Command::new("sudo")
                .arg("-S").arg("-p").arg("")
                .arg("bash").arg("-c").arg(cmd)
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("failed to spawn sudo bash -c ... chpasswd: {}", e))?;
            if let Some(mut stdin) = child.stdin.take() {
                if let Some(pw) = &self.sudo_password { let _ = stdin.write_all(pw.as_bytes()); let _ = stdin.write_all(b"\n"); }
            }
            let output = child.wait_with_output()?;
            if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("chpasswd", &output))) }
        }
    }

    pub fn expire_user_password(&self, username: &str) -> Result<()> {
        let output = self
            .run_privileged("chage", &["-d", "0", username])
            .map_err(|e| crate::error::simple_error(format!("failed to execute chage -d 0 {}: {}", username, e)))?;
        if output.status.success() { Ok(()) } else { Err(crate::error::simple_error(format_cli_error("chage -d 0", &output))) }
    }

    fn run_privileged(&self, cmd: &str, args: &[&str]) -> Result<std::process::Output> {
        if current_uid() == 0 {
            return Command::new(cmd)
                .args(args)
                .stderr(Stdio::piped())
                .output()
                .map_err(Into::into);
        }

        // Without a sudo password, don't attempt sudo with a blank line.
        // Return a clear error so the UI can prompt first.
        if self.sudo_password.is_none() {
            return Err(crate::error::simple_error("Authentication required"));
        }

        // Step 1: validate sudo credentials to populate timestamp without mixing with command IO
        let mut validate = Command::new("sudo")
            .arg("-S").arg("-p").arg("")
            .arg("-v")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to spawn sudo -v: {}", e))?;
        if let Some(mut stdin) = validate.stdin.take() {
            if let Some(pw) = &self.sudo_password {
                use std::io::Write;
                let _ = stdin.write_all(pw.as_bytes());
                let _ = stdin.write_all(b"\n");
            }
        }
        let validate_out = validate.wait_with_output()?;
        if !validate_out.status.success() {
            return Err(crate::error::simple_error(format_cli_error("sudo -v", &validate_out)));
        }

        // Step 2: run the actual command without reading from stdin (use -n to avoid prompting)
        let output = Command::new("sudo")
            .arg("-n")
            .arg(cmd)
            .args(args)
            .stderr(Stdio::piped())
            .output()?;
        Ok(output)
    }
}

fn parse_passwd<P: AsRef<Path>>(path: P) -> Result<Vec<SystemUser>> {
    let contents = fs::read_to_string(path)?;
    let mut users = Vec::new();
    for line in contents.lines() {
        if line.is_empty() || line.starts_with('#') { continue; }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 7 { continue; }
        let name = parts[0].to_string();
        let uid = parts[2].parse::<u32>().unwrap_or(0);
        let gid = parts[3].parse::<u32>().unwrap_or(0);
        let full_name = if parts[4].is_empty() { None } else { Some(parts[4].to_string()) };
        let home_dir = parts[5].to_string();
        let shell = parts[6].to_string();
        users.push(SystemUser { uid, name, primary_gid: gid, full_name, home_dir, shell });
    }
    Ok(users)
}

fn parse_group<P: AsRef<Path>>(path: P) -> Result<Vec<SystemGroup>> {
    let contents = fs::read_to_string(path)?;
    let mut groups = Vec::new();
    for line in contents.lines() {
        if line.is_empty() || line.starts_with('#') { continue; }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 3 { continue; }
        let name = parts[0].to_string();
        let gid = parts[2].parse::<u32>().unwrap_or(0);
        let members = if parts.len() >= 4 && !parts[3].is_empty() {
            parts[3].split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect()
        } else { Vec::new() };
        groups.push(SystemGroup { gid, name, members });
    }
    Ok(groups)
}

// Note: NSS enumeration is not used at the moment; parsing /etc/passwd and
// /etc/group is the default approach and can be forced via the `file-parse` feature.

fn format_cli_error(cmd: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("{} returned non-zero status: {}", cmd, output.status)
    } else {
        format!("{} failed: {}", cmd, stderr)
    }
}

fn current_uid() -> u32 {
    // Linux: read from /proc; fallback to 0 if parsing fails
    if let Ok(contents) = std::fs::read_to_string("/proc/self/status") {
        for line in contents.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                if let Some(first) = rest.split_whitespace().next() {
                    if let Ok(uid) = first.parse() { return uid; }
                }
            }
        }
    }
    0
}

pub fn current_username() -> Option<String> {
    let uid = current_uid();
    parse_passwd("/etc/passwd").ok()?
        .into_iter()
        .find(|u| u.uid == uid)
        .map(|u| u.name)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

	fn tmp_path(tag: &str) -> PathBuf {
		let mut p = std::env::temp_dir();
		let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		p.push(format!("ugm_rs_{tag}_{}_{}", std::process::id(), n));
		p
	}

	#[test]
	fn parse_passwd_basic() {
		let path = tmp_path("passwd");
		let data = "\
root:x:0:0:root:/root:/bin/bash
jdoe:x:1000:1000:John Doe,,,:/home/jdoe:/bin/zsh
";
		fs::write(&path, data).unwrap();

		let users = parse_passwd(&path).unwrap();
		fs::remove_file(&path).ok();

		assert_eq!(users.len(), 2);
		assert_eq!(users[0].name, "root");
		assert_eq!(users[0].uid, 0);
		assert_eq!(users[0].full_name.as_deref(), Some("root"));
		assert_eq!(users[1].name, "jdoe");
		assert_eq!(users[1].uid, 1000);
		assert_eq!(users[1].full_name.as_deref(), Some("John Doe,,,"));
		assert_eq!(users[1].home_dir, "/home/jdoe");
		assert_eq!(users[1].shell, "/bin/zsh");
	}

	#[test]
	fn parse_group_basic() {
		let path = tmp_path("group");
		let data = "\
root:x:0:
wheel:x:998:root,jdoe
";
		fs::write(&path, data).unwrap();

		let groups = parse_group(&path).unwrap();
		fs::remove_file(&path).ok();

		assert_eq!(groups.len(), 2);
		assert_eq!(groups[0].name, "root");
		assert_eq!(groups[0].gid, 0);
		assert!(groups[0].members.is_empty());
		assert_eq!(groups[1].name, "wheel");
		assert_eq!(groups[1].gid, 998);
		assert_eq!(groups[1].members, vec!["root".to_string(), "jdoe".to_string()]);
	}
}