# Testing

Last Updated: 2025-09-15

This document centralizes test plans and scenarios for UsrGrp-Manager. It complements the brief Testing Overview in Improvements.md.

## Goals

- Validate correctness across parsing, state updates, UI rendering, and command execution paths
- Ensure safety around privileged operations
- Provide performance confidence for large datasets

## Target Dataset Sizes and Performance

- Users/groups: up to 10,000 entries each
- Search and filter latency: target under 50 ms for incremental queries on 10k datasets
- Idle CPU usage near 0% and bounded memory footprint during typical navigation

## Test Plan

### High‑Value Additions

- Parsing edge cases (unit, in [src/sys/mod.rs](../src/sys/mod.rs))
  - Empty lines, comments, missing fields, extra fields
  - Invalid numeric UIDs/GIDs → parsed as 0, no panics
  - `/etc/shells` parsing ignores comments/blank lines
  - `groups_for_user` returns primary group and supplementary memberships
  - `format_cli_error` formats empty vs non‑empty stderr correctly
  - Malformed passwd entries with colons in field values
  - Group parsing with empty member lists vs single member vs multiple members
  - Handling of extremely large UIDs/GIDs (u32::MAX boundary tests)
  - Unicode/UTF‑8 handling in usernames, full names, and paths

- Search behavior (unit, in [src/search.rs](../src/search.rs))
  - Empty query resets lists and selection index to 0
  - Numeric queries match UID/GID string forms
  - Case‑insensitive matches on full name, home, shell, group members
  - Selection index clamping after filter (stays at 0 regardless of length)
  - Partial matches across different fields simultaneously
  - Special character handling in search queries (regex escaping)
  - Performance tests with large user/group lists (1000+ entries)

- State machine/input handling (unit, in [src/app/update.rs](../src/app/update.rs))
  - Tab/BackTab toggles `active_tab` and `users_focus`
  - Arrow keys/page moves clamp within bounds for users vs member‑of lists
  - `n` opens `UserAddInput` with `create_home = true`
  - Non‑privileged flows that only open/close modals (no system calls)
  - Optional refactor: extract small pure helpers for index math to test easily
  - Modal state transitions (all valid paths through modal states)
  - Input validation in text fields (username, fullname, password)
  - Keyboard shortcut conflicts and precedence
  - Page navigation boundary conditions (empty lists, single item, exact page size)
  - Focus management when switching between tabs/panes

- Error handling (unit, in [src/error.rs](../src/error.rs))
  - `WithContextError` properly chains error sources
  - Context messages are properly formatted and displayed
  - `SimpleError` creation and display
  - Error propagation through the `Context` trait
  - Nested error contexts (multiple layers of with_ctx)
  - Memory safety with dynamic error boxing

- Password management (unit, in [src/sys/mod.rs](../src/sys/mod.rs))
  - Password escaping for shell injection prevention
  - Special characters in passwords (quotes, backslashes, dollar signs, backticks)
  - Empty password handling
  - Password confirmation mismatch detection
  - `set_user_password` command construction for root vs non‑root
  - Sudo password handling and timeout scenarios

- Command execution safety (unit, in [src/sys/mod.rs](../src/sys/mod.rs))
  - Shell injection prevention in all privileged commands
  - Argument escaping for usernames/groupnames with special characters
  - Command timeout handling
  - Stderr/stdout parsing for different error conditions
  - Sudo authentication failures vs command failures
  - Race condition handling in rapid command execution

- Modal input validation (unit, in [src/app/update.rs](../src/app/update.rs))
  - Username validation (allowed characters, length limits, reserved names)
  - Group name validation
  - Path validation for shell changes
  - Text input buffer overflow prevention
  - Backspace/delete at boundaries
  - Cursor position management in text fields
  - Copy/paste handling (if implemented)

- UI rendering sanity (integration/snapshot)
  - Use `ratatui::backend::TestBackend` to render a small `AppState` and assert key labels/titles/row highlights. Consider `insta` for snapshots
  - Table rendering with empty data
  - Column width calculations and text truncation
  - Color theme application
  - Modal overlay rendering and clearing
  - Status bar message updates
  - Scroll position preservation during updates

- System state consistency (integration)
  - State refresh after operations (`list_users`, `list_groups`)
  - Selection index adjustment after list changes
  - Search query persistence across operations
  - Modal cleanup on escape/cancel
  - Undo/redo state management (if implemented)
  - Concurrent modification detection

- Command‑line parsing (unit)
  - If `clap` is used in `main.rs`, test flags/env via `Command::try_get_matches_from` (no TUI needed)
  - Environment variable precedence
  - Configuration file loading (if implemented)
  - Feature flag combinations

- Architecture for privileged ops (mockable)
  - Introduce a `trait System` that `SystemAdapter` implements; inject into `perform_pending_action` so tests can verify
    - Correct command path chosen (add/remove user to group, change shell/name)
    - App state refreshes (`users_all`, `groups_all`), and info messages
  - Without this, keep privileged paths out of tests
  - Dry‑run mode verification
  - Rollback mechanism testing
  - Audit log generation

- Terminal handling (integration)
  - Raw mode enable/disable
  - Terminal restoration on panic
  - Mouse event handling (if enabled)
  - Terminal resize handling
  - Alternate screen buffer management
  - Signal handling (SIGINT, SIGTERM)

- Performance and resource tests
  - Memory leak detection in long‑running sessions
  - CPU usage during idle vs active states
  - File descriptor leak prevention
  - Large dataset handling (10,000+ users/groups)
  - Search performance optimization validation

- Platform‑specific behavior (integration)
  - Linux/BSD differences in user management commands
  - File permission handling across filesystems
  - NSS integration when available
  - PAM configuration respect
  - systemd‑homed compatibility (if applicable)

- Data integrity tests
  - Backup creation before modifications
  - Atomic operations (all‑or‑nothing)
  - File lock handling for `/etc/passwd` and `/etc/group`
  - Concurrent access prevention
  - Data corruption recovery

## Running Tests

From the repository root:

```bash
cargo test
```


