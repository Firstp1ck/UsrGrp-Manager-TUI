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

## Further Tests

### High‑value additions

- **Parsing edge cases (unit, in `src/sys/mod.rs`)**
  - Empty lines, comments, missing fields, extra fields.
  - Invalid numeric UIDs/GIDs → parsed as 0, no panics.
  - `/etc/shells` parsing ignores comments/blank lines.
  - `groups_for_user` returns primary group and supplementary memberships.
  - `format_cli_error` formats empty vs non‑empty stderr correctly.
  - Malformed passwd entries with colons in field values
  - Group parsing with empty member lists vs single member vs multiple members
  - Handling of extremely large UIDs/GIDs (u32::MAX boundary tests)
  - Unicode/UTF-8 handling in usernames, full names, and paths

- **Search behavior (unit, in `src/search.rs`)**
  - Empty query resets lists and selection index to 0.
  - Numeric queries match UID/GID string forms.
  - Case‑insensitive matches on full name, home, shell, group members.
  - Selection index clamping after filter (stays at 0 regardless of length).
  - Partial matches across different fields simultaneously
  - Special character handling in search queries (regex escaping)
  - Performance tests with large user/group lists (1000+ entries)

- **State machine/input handling (unit, in `src/app/update.rs`)**
  - Tab/BackTab toggles `active_tab` and `users_focus`.
  - Arrow keys/page moves clamp within bounds for users vs member‑of lists.
  - `n` opens `UserAddInput` with `create_home = true`.
  - Non‑privileged flows that only open/close modals (no system calls).
  - Optional refactor: extract small pure helpers for index math to test easily.
  - Modal state transitions (all valid paths through modal states)
  - Input validation in text fields (username, fullname, password)
  - Keyboard shortcut conflicts and precedence
  - Page navigation boundary conditions (empty lists, single item, exact page size)
  - Focus management when switching between tabs/panes

- **Error handling (unit, in `src/error.rs`)**
  - `WithContextError` properly chains error sources
  - Context messages are properly formatted and displayed
  - `SimpleError` creation and display
  - Error propagation through the `Context` trait
  - Nested error contexts (multiple layers of with_ctx)
  - Memory safety with dynamic error boxing

- **Password management (unit, in `src/sys/mod.rs`)**
  - Password escaping for shell injection prevention
  - Special characters in passwords (quotes, backslashes, dollar signs, backticks)
  - Empty password handling
  - Password confirmation mismatch detection
  - `set_user_password` command construction for root vs non-root
  - Sudo password handling and timeout scenarios

- **Command execution safety (unit, in `src/sys/mod.rs`)**
  - Shell injection prevention in all privileged commands
  - Argument escaping for usernames/groupnames with special characters
  - Command timeout handling
  - Stderr/stdout parsing for different error conditions
  - Sudo authentication failures vs command failures
  - Race condition handling in rapid command execution

- **Modal input validation (unit, in `src/app/update.rs`)**
  - Username validation (allowed characters, length limits, reserved names)
  - Group name validation
  - Path validation for shell changes
  - Text input buffer overflow prevention
  - Backspace/delete at boundaries
  - Cursor position management in text fields
  - Copy/paste handling (if implemented)

- **UI rendering sanity (integration/snapshot)**
  - Use `ratatui::backend::TestBackend` to render a small `AppState` and assert key labels/titles/row highlights. Consider `insta` for snapshots.
  - Table rendering with empty data
  - Column width calculations and text truncation
  - Color theme application
  - Modal overlay rendering and clearing
  - Status bar message updates
  - Scroll position preservation during updates

- **System state consistency (integration)**
  - State refresh after operations (`list_users`, `list_groups`)
  - Selection index adjustment after list changes
  - Search query persistence across operations
  - Modal cleanup on escape/cancel
  - Undo/redo state management (if implemented)
  - Concurrent modification detection

- **Command‑line parsing (unit)**
  - If `clap` is used in `main.rs`, test flags/env via `Command::try_get_matches_from` (no TUI needed).
  - Environment variable precedence
  - Configuration file loading (if implemented)
  - Feature flag combinations

- **Architecture for privileged ops (mockable)**
  - Introduce a `trait System` that `SystemAdapter` implements; inject into `perform_pending_action` so tests can verify:
    - Correct command path chosen (add/remove user to group, change shell/name).
    - App state refreshes (`users_all`, `groups_all`), and info messages.
  - Without this, keep privileged paths out of tests.
  - Dry-run mode verification
  - Rollback mechanism testing
  - Audit log generation

- **Terminal handling (integration)**
  - Raw mode enable/disable
  - Terminal restoration on panic
  - Mouse event handling (if enabled)
  - Terminal resize handling
  - Alternate screen buffer management
  - Signal handling (SIGINT, SIGTERM)

- **Performance and resource tests**
  - Memory leak detection in long-running sessions
  - CPU usage during idle vs active states
  - File descriptor leak prevention
  - Large dataset handling (10,000+ users/groups)
  - Search performance optimization validation

- **Platform-specific behavior (integration)**
  - Linux/BSD differences in user management commands
  - File permission handling across filesystems
  - NSS integration when available
  - PAM configuration respect
  - systemd-homed compatibility (if applicable)

- **Data integrity tests**
  - Backup creation before modifications
  - Atomic operations (all-or-nothing)
  - File lock handling for /etc/passwd and /etc/group
  - Concurrent access prevention
  - Data corruption recovery