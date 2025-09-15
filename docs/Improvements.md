## Purpose & Audience

This document captures product and engineering improvements for UsrGrp-Manager. It is intended for maintainers and contributors to align on scope, priorities, risks, and testing.

## Scope & Non‑Goals

- In scope: Local account and group management on UNIX‑like systems, TUI UX, safety, and testing.
- Non‑goals: Directory services management (LDAP/SSSD/AD), remote identity sources, `systemd-homed`, and full policy orchestration.

## At‑A‑Glance Summary

- Top priorities: Lock/Unlock and Shell Toggle; Password Set/Reset; Tests + CI; Fuzzy Search and Filters; Performance with large datasets; Safety groundwork.
- Milestones: See Roadmap for v0.3, v0.4, Later.
- Risks: Privilege escalation, data loss, admin lockout → see Risk & Mitigations.

## Notes

`UsrGrp-Manager` only works on UNIX-based OSes.

On macOS, the information reported will not be accurate. The tool relies on the `/etc/passwd` and `/etc/group` files, which are only consulted on macOS in single-user mode, and the system uses [Directory Services](https://developer.apple.com/documentation/devicemanagement/directoryservice) to manage users and groups.

## Table of Contents

- [Purpose & Audience](#purpose--audience)
- [Scope & Non‑Goals](#scope--non-goals)
- [At‑A‑Glance Summary](#at-a-glance-summary)
- [Notes](#notes)
- [Built With](#built-with)
- [Plan](#plan)
  - [Tech Stack](#tech-stack)
  - [Testing Strategy](#testing-strategy)
  - [Platform Notes](#platform-notes)
  - [Run Locally](#run-locally)
- [Roadmap](#roadmap)
- [Backlog](#backlog)
  - [UX & Navigation](#ux--navigation)
  - [Security & Compliance](#security--compliance)
  - [Integrations](#integrations)
  - [Operations](#operations)
- [Things to Consider](#things-to-consider)
  - [Security and Risk Concerns](#security-and-risk-concerns)
  - [Privilege Escalation Risks](#privilege-escalation-risks)
  - [System Integrity Considerations](#system-integrity-considerations)
  - [Philosophy](#philosophy)
- [Testing Overview](#testing-overview)
- [Platform Support](#platform-support)
- [Risk & Mitigations](#risk--mitigations)
- [Feature Status Overview](#feature-status-overview)
- [Privilege Model](#privilege-model)
- [Open Questions](#open-questions)
- [Decisions](#decisions)
- [Gaps to Cover in `README.md` (Features‑Focused)](#gaps-to-cover-in-readmemd-features-focused)
- [Metadata & Maintenance](#metadata--maintenance)
  - [How to Propose Improvements](#how-to-propose-improvements)
  - [Glossary](#glossary)

## Built With
 - [`ratatui`](https://github.com/ratatui-org/ratatui) and its ecosystem
 - [`crossterm`](https://github.com/crossterm-rs/crossterm)

## Plan

### Tech Stack
- UI: [`ratatui`](https://github.com/ratatui-org/ratatui) + [`crossterm`](https://github.com/crossterm-rs/crossterm)
- System users/groups: [`users`](https://github.com/ogham/rust-users) (respects NSS); optional file parsing fallback
- CLI/logging/error: `clap`, `tracing` + `tracing-subscriber`, `anyhow`/`thiserror`
- Search: `fuzzy-matcher` (optional), simple substring by default

### Testing Strategy
- Parser tests using fixture files; property tests for edge cases
- Integration tests driving the update loop with synthetic input events
- Snapshot tests for UI components using known tables

### Platform Notes
- Linux/BSD: primary targets. `users` uses libc calls and should honor NSS.
- macOS: behavior may differ due to Directory Services; keep file-parse as fallback via `--features file-parse`.

### Run Locally
```bash
cargo build --release
cargo run --release
cargo run --features file-parse
```

## Roadmap

See the milestone roadmap in [docs/roadmap.md](docs/roadmap.md) for v0.3, v0.4, and Later, including acceptance criteria and owners.


## Backlog

### UX & Navigation

- Core CRUD: Create/modify/delete users and groups; lock/unlock; login shell toggle; password set/reset with strength checks
- Search & Filtering: Fuzzy find users/groups; filters (system vs human, inactive, expired, locked, no home, no password)
- Layout & Interactions: Split view (list + detail), breadcrumbs, status bar hints, non‑blocking jobs panel
- Accessibility & Theming: High‑contrast theme, mouse support, resizable panes, configurable keymaps, persistent settings file

### Security & Compliance

- Account Policies: Password aging/expiry (chage), account expiry dates, lockout status (lastlog/faillog)
- Policy Checks: Shell whitelist, UID/GID ranges, reserved names
- Auditability & Safety: Dry‑run preview (diff for `/etc/passwd`, `/etc/group`, `/etc/shadow`, sudoers); automatic backups; structured audit log (JSON)

### Integrations

- Directory Services: Optional LDAP/SSSD read‑only view or sync hints
- Remote Management: Manage remote hosts over SSH for fleet operations (fan‑out with concurrency)

### Operations

- Batch Operations: Multi‑select for bulk add to groups, lock, shell change, expiry set; CSV/JSON import for mass user creation with validation
- Profiles & Defaults: Role‑based presets; default shell, home layout, groups, umask, password policy per template
- Home Management: Create/migrate home with skeleton, ownership fix; quotas (if supported), dotfiles bootstrap, optional encryption/systemd‑homed
- Group & Sudo: Add/remove primary/secondary groups; manage `sudoers.d` snippets with `visudo -c` validation; quick wheel/admin toggle
- SSH Keys: View/edit `authorized_keys` with key validation and comments; bulk add/remove; expiry metadata
- Diagnostics: Health checks for PAM, shadow permissions, `login.defs` anomalies; detect conflicting state (orphaned homes, duplicate UIDs)

## Things to Consider
### Security and Risk Concerns
User management is inherently high-risk from a security perspective

### Privilege Escalation Risks
- User management tools must run with elevated privileges to modify system files

- Any bugs or vulnerabilities in such tools can lead to root account exploits

- Race conditions in user management operations could compromise system security

### System Integrity Considerations
- Incorrect user modifications can lock administrators out of systems

- File permission changes can expose sensitive data

- Centralized authentication complexity makes comprehensive tools risky

### Philosophy

- "Fail noisily and as soon as possible"

- Provide transparent operation for debugging

- Handle partial failures gracefully in multi-user operations

## Testing Overview

See [docs/testing.md](docs/testing.md) for the complete testing plan, including unit/integration/snapshot tests and safety checks.

Targets:

- Datasets up to 10,000 users/groups
- Incremental search latency under 50 ms on large datasets
- Clear separation of privileged vs non‑privileged flows

## Platform Support

| Platform | Support | Notes |
| --- | --- | --- |
| Linux | Yes | Primary target; respects NSS via `users` crate. |
| BSD | Yes | Should honor NSS where available. |
| macOS | Partial | `/etc/passwd`/`/etc/group` consulted only in single‑user mode; Directory Services used. |

## Risk & Mitigations

| Risk | Mitigation |
| --- | --- |
| Privilege escalation via command execution | Strict argument escaping; minimal surface; dry‑run preview; tests for injection. |
| Data loss or corruption of system files | Backups before write; atomic operations where possible; rollback plan. |
| Administrator lockout (e.g., shell or password changes) | Confirmation modals; safe defaults; clear status in UI; dry‑run mode. |

## Feature Status Overview

| Feature Area | Status | Target |
| --- | --- | --- |
| Lock/Unlock and Shell Toggle | Planned | v0.3 |
| Password Set/Reset with Strength Checks | Planned | v0.3 |
| Fuzzy Search and Filters | Planned | v0.4 |
| Bulk Operations | Later | Later |
| Backups and Audit Log | Later | Later |

## Privilege Model

For read‑only usage, run normally. For write actions, run the TUI with elevated privileges, for example:

```bash
sudo usrgrp-manager
```

The application may integrate with `sudo`/polkit in the future; configuration details will be documented alongside the implementation.

## Open Questions

- Preferred elevation strategy: whole‑app under sudo vs per‑action elevation?
- Minimum terminal size and accessibility requirements to support by default?
- Sorting support scope (UID/GID/name) and keybindings to toggle?

## Decisions

- Use `ratatui` + `crossterm` for the TUI stack.
- Keep LDAP/SSSD/AD out of scope; local accounts only.

## Gaps to Cover in `README.md` (Features‑Focused)
- [ ] Feature matrix (read vs write). [Create issue](/../../issues/new?title=Docs%3A+README+Feature+Matrix&labels=docs,readme)
- [ ] Sorting and filtering details. [Create issue](/../../issues/new?title=Docs%3A+README+Sorting%2FFiltering&labels=docs,readme)
- [ ] Bulk operations. [Create issue](/../../issues/new?title=Docs%3A+README+Bulk+Operations&labels=docs,readme)
- [ ] Account lifecycle features. [Create issue](/../../issues/new?title=Docs%3A+README+Account+Lifecycle&labels=docs,readme)
- [ ] User creation options. [Create issue](/../../issues/new?title=Docs%3A+README+User+Creation+Options&labels=docs,readme)
- [ ] Group management options. [Create issue](/../../issues/new?title=Docs%3A+README+Group+Management&labels=docs,readme)
- [ ] Safety/permissions model. [Create issue](/../../issues/new?title=Docs%3A+README+Safety%2FPermissions+Model&labels=docs,readme)
- [ ] Data sources and scope (local only). [Create issue](/../../issues/new?title=Docs%3A+README+Data+Sources+%26+Scope&labels=docs,readme)
- [ ] Terminal support and UX. [Create issue](/../../issues/new?title=Docs%3A+README+Terminal+Support%2FUX&labels=docs,readme)
- [ ] Config and environment. [Create issue](/../../issues/new?title=Docs%3A+README+Config+%26+Env+Vars&labels=docs,readme)
- [ ] Known limitations. [Create issue](/../../issues/new?title=Docs%3A+README+Known+Limitations&labels=docs,readme)
- [ ] Roadmap summary. [Create issue](/../../issues/new?title=Docs%3A+README+Roadmap+Summary&labels=docs,readme)
- [ ] MSRV/compatibility. [Create issue](/../../issues/new?title=Docs%3A+README+MSRV%2FCompatibility&labels=docs,readme)
- [ ] Install from source/binaries. [Create issue](/../../issues/new?title=Docs%3A+README+Install+%28Source%2FBinaries%29&labels=docs,readme)
- [ ] Uninstall. [Create issue](/../../issues/new?title=Docs%3A+README+Uninstall&labels=docs,readme)
- [ ] Security note. [Create issue](/../../issues/new?title=Docs%3A+README+Security+Note&labels=docs,readme)