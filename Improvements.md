## Notes

`UsrGrp-Manager` only works on UNIX based OS.

On OSX, the information reported will not be accurate. The tool relies on the `/etc/passwd` and `/etc/group` files, which are only consulted in OSX in single-user mode, and the system uses [DirectoryService](https://developer.apple.com/documentation/devicemanagement/directoryservice) to manage user and groups.

## Built with
 - [bubbletea](https://github.com/charmbracelet/bubbletea) and its ecosystem
 - [bubble-table](https://github.com/Evertras/bubble-table)

## Plan

- **Tech Stack**
  - UI: [`ratatui`](https://github.com/ratatui-org/ratatui) + [`crossterm`](https://github.com/crossterm-rs/crossterm)
  - System users/groups: [`users`](https://github.com/ogham/rust-users) (respects NSS); optional file parsing fallback
  - CLI/logging/error: `clap`, `tracing` + `tracing-subscriber`, `anyhow`/`thiserror`
  - Search: `fuzzy-matcher` (optional), simple substring by default

- **Testing Strategy**
  - Parser tests using fixture files; property tests for edge cases
  - Integration tests driving the update loop with synthetic input events
  - Snapshot tests for UI components using known tables

- **Platform Notes**
  - Linux/BSD: primary targets. `users` uses libc calls and should honor NSS.
  - macOS: behavior may differ due to Directory Services; keep file-parse as fallback via `--features file-parse`.

- **Run locally**
  - Build: `cargo build --release`
  - Run: `cargo run --release`
  - Optional features: `cargo run --features file-parse`
  
  TODO:
- Add tests: parsers, update loop, UI snapshots
- Set up CI and cross-platform release builds
- Optimize performance and memory usage
- Lock/unlock, enable/disable login shell: show status in table; actions in detail view; confirm + dry-run; apply via `usermod -L/-U` and `chsh` (or edit `/etc/passwd` when in file-parse mode)
- Password set/reset with strength checks: masked prompt; basic rules or zxcvbn; optional "must change at next login"; respect PAM; clear error messages
- Fuzzy find users/groups: incremental filtering while typing; highlight matches; toggle fuzzy vs substring; performance guard for large datasets
- Filters (system vs human, inactive, expired, locked, no home, no password): quick filter menu; combinable chips; persisted per session; NSS-aware where possible
- Multi-select bulk ops (add to groups, lock, shell change, expiry set): selection mode with count; preview + confirmation; batched execution with per-item results and rollback on failure


## Possible Suggesstions

### High‑impact features for a TUI user manager

- **Core CRUD**
  - Create/modify/delete users and groups
  - Lock/unlock, enable/disable login shell
  - Password set/reset with strength checks

- **Search & filtering**
  - Fuzzy find users/groups
  - Filters: system vs human, inactive, expired, locked, no home, no password

- **Batch operations**
  - Multi‑select for bulk add to groups, lock, shell change, expiry set
  - CSV/JSON import for mass user creation with validation

- **Profile templates**
  - Role‑based presets (e.g., developer, service, admin)
  - Default shell, home layout, groups, umask, password policy per template

- **Home directory management**
  - Create/migrate home with skeleton, ownership fix
  - Quotas (if supported), dotfiles bootstrap, optional encryption/systemd‑homed

- **Group & sudo management**
  - Add/remove primary/secondary groups
  - Manage `sudoers.d` snippets with `visudo -c` validation
  - Quick wheel/admin toggle

- **Security & compliance**
  - Password aging/expiry (chage), account expiry dates
  - Last login, failed attempts (lastlog/faillog), lockout status
  - Policy checks (shell whitelist, uid/gid ranges, reserved names)

- **SSH keys**
  - View/edit `authorized_keys` with key validation and comments
  - Bulk add/remove keys, expiry metadata

- **Auditability & safety**
  - Dry‑run mode with preview of changes (show diff for `/etc/passwd`, `/etc/group`, `/etc/shadow`, sudoers)
  - Automatic timestamped backups + one‑click rollback
  - Structured audit log (JSON) of actions

- **Integrations**
  - Optional LDAP/SSSD read‑only view or sync hints
  - Remote host management over SSH for fleet operations (fan‑out with concurrency)

- **UX & navigation**
  - Vim/Emacs keybindings, fuzzy palette, breadcrumbs, status bar with hints
  - Split view: list on left, detail/preview pane on right
  - Non‑blocking jobs with progress (spinners/bars) and a jobs panel

- **Accessibility & theming**
  - High‑contrast theme, mouse support, resizable panes
  - Configurable keymaps and persistent settings file

- **Scripting & extensibility**
  - Pre/post action hooks (run scripts), plugin points
  - Export views as CSV/JSON; non‑interactive CLI subcommands for CI/automation

- **Diagnostics**
  - Health checks for PAM, shadow permissions, `login.defs` anomalies
  - Detect conflicting state (e.g., orphaned homes, duplicate UIDs)

## Things to consider
### Security and Risk Concerns
User management is inherently high-risk from a security perspective

### Privilege Escalation Risks
- User management tools must run with elevated privileges to modify system files

- Any bugs or vulnerabilities in such tools can lead to root account exploits

- Race conditions in user management operations could compromise system security

### System Integrity to consider
- Incorrect user modifications can lock administrators out of systems

- File permission changes can expose sensitive data

- Centralized authentication complexity makes comprehensive tools risky

### Philosophy

- "Fail noisily and as soon as possible"

- Provide transparent operation for debugging

- Handle partial failures gracefully in multi-user operations