use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;
use users::{get_current_uid, get_user_by_uid};

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
pub struct SystemAdapter;

#[allow(dead_code)]
impl SystemAdapter {
    pub fn new() -> Self { Self }

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
        let status = Command::new("gpasswd")
            .args(["-a", username, groupname])
            .status()
            .with_context(|| format!("failed to execute gpasswd -a {} {}", username, groupname))?;
        if status.success() { Ok(()) } else { anyhow::bail!("gpasswd -a returned non-zero status: {}", status) }
    }

    pub fn remove_user_from_group(&self, username: &str, groupname: &str) -> Result<()> {
        let status = Command::new("gpasswd")
            .args(["-d", username, groupname])
            .status()
            .with_context(|| format!("failed to execute gpasswd -d {} {}", username, groupname))?;
        if status.success() { Ok(()) } else { anyhow::bail!("gpasswd -d returned non-zero status: {}", status) }
    }

    pub fn create_group(&self, groupname: &str) -> Result<()> {
        let status = Command::new("groupadd")
            .arg(groupname)
            .status()
            .with_context(|| format!("failed to execute groupadd {}", groupname))?;
        if status.success() { Ok(()) } else { anyhow::bail!("groupadd returned non-zero status: {}", status) }
    }

    pub fn delete_group(&self, groupname: &str) -> Result<()> {
        let status = Command::new("groupdel")
            .arg(groupname)
            .status()
            .with_context(|| format!("failed to execute groupdel {}", groupname))?;
        if status.success() { Ok(()) } else { anyhow::bail!("groupdel returned non-zero status: {}", status) }
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
        let status = Command::new("usermod")
            .args(["-s", new_shell, username])
            .status()
            .with_context(|| format!("failed to execute usermod -s {} {}", new_shell, username))?;
        if status.success() { Ok(()) } else { anyhow::bail!("usermod -s returned non-zero status: {}", status) }
    }

    pub fn change_user_fullname(&self, username: &str, new_fullname: &str) -> Result<()> {
        let status = Command::new("usermod")
            .args(["-c", new_fullname, username])
            .status()
            .with_context(|| format!("failed to execute usermod -c {} {}", new_fullname, username))?;
        if status.success() { Ok(()) } else { anyhow::bail!("usermod -c returned non-zero status: {}", status) }
    }

    pub fn change_username(&self, old_username: &str, new_username: &str) -> Result<()> {
        let status = Command::new("usermod")
            .args(["-l", new_username, old_username])
            .status()
            .with_context(|| format!("failed to execute usermod -l {} {}", new_username, old_username))?;
        if status.success() { Ok(()) } else { anyhow::bail!("usermod -l returned non-zero status: {}", status) }
    }
}

pub fn current_username() -> Option<String> {
    let uid = get_current_uid();
    get_user_by_uid(uid).map(|u| u.name().to_string_lossy().into_owned())
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


