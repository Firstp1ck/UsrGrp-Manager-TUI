# Security Policy

usrgrp-manager is a local terminal application for managing Linux users and groups. It does not expose a network service. Security issues typically relate to local privilege handling, command execution, and file parsing of system databases.

## Supported Versions

Pre-1.0 policy: we support the latest release line and the development branch. Older pre-1.0 series are not maintained for security fixes.

| Channel / Version | Supported | Notes |
| ----------------- | --------- | ----- |
| main (git)        | ✅        | Security fixes land here first. |
| Latest 0.y.x      | ✅        | Currently: 0.1.x. Supported until the next minor (0.y+1). |
| Older 0.x         | ❌        | Please upgrade to the latest release. |

Distribution packages are updated after fixes are released:
- AUR: `usrgrp-manager-git` (tracks main), `usrgrp-manager-bin` (latest release)
- Source: GitHub releases and `cargo install --locked --git` for advanced users

## Reporting a Vulnerability

Please use private disclosure. Do not open a public issue for suspected vulnerabilities.

Preferred: open a private advisory via GitHub Security Advisories for this repo: [Report a vulnerability](https://github.com/firstpick/usrgrp-manager/security/advisories/new)

Include, when possible:
- A clear description of the issue and potential impact
- Exact version or package channel (e.g., 0.1.0, `usrgrp-manager-bin`, `usrgrp-manager-git`)
- OS/distro and environment details
- Reproduction steps or a minimal proof-of-concept
- Logs or screenshots (with sensitive data redacted)
- Whether elevated privileges (sudo/root) are required to reproduce

Acknowledgement
- We will coordinate disclosure and credit the reporter if desired

## Scope and Threat Model

- Local-only tool. No network listeners or remote protocol surfaces
- Write operations use standard system tools (`useradd`, `usermod`, `userdel`, `groupadd`, `groupdel`, `gpasswd`, `chpasswd`, `chage`)
- Running without privileges should not modify system state; privileged actions require sudo/root
- We treat command injection, unsafe argument construction, and insecure temporary file handling as in-scope
- Parsing of `/etc/passwd`, `/etc/shadow`, and `/etc/group` must be robust against malformed but well-formed text files (arbitrary binary data is out of scope)

Not considered vulnerabilities:
- Destructive actions that are guarded by confirmations (e.g., deleting users or groups) when invoked by a privileged user
- Safety reminders that do not hard-block actions by design (e.g., warnings for low GIDs such as GID < 1000) are intended UX, not a security boundary

## Getting Security Updates

- Binary package: update via your AUR helper (e.g., `yay -Syu usrgrp-manager-bin`)
- Git package: update to latest main (e.g., `yay -Syu usrgrp-manager-git`)
- From source: upgrade to the latest tagged release or rebuild from main

If you maintain downstream packages, please update to the fixed release as soon as practical after a security advisory is published.



