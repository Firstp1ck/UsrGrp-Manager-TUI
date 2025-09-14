UsrGrp-Manager-TUI (Rust Users/Groups Manager TUI)
================

## Description
Keyboard‑driven terminal app to view and manage users and groups. Browse accounts, see memberships, search, and make common changes: rename users, update names or shells, adjust group membership. Safe to explore without admin rights; asks for permission to apply changes. Linux‑focused. Written in Rust.

## Status
Alpha. Read‑only browsing is safe; write operations require privileges and are still limited.

Alpha means:
- Interfaces and keybindings may change without notice.
- Some actions are intentionally guarded (e.g., user deletion requires confirmation; optional home removal).
- Error handling, edge cases, and performance are still being improved.
- Expect to run with `sudo` for any write operation (`usermod`, `gpasswd`, `groupadd`, `groupdel`, `useradd`, `userdel`).

## Install / Build
---------------

- Build: `cargo build --release`
- Run: `cargo run --release`
- Logging: set `USRGRP_MANAGER_LOG=info|debug|trace` (default: `info`)
- Feature flags: `file-parse` exists, but enumeration currently parses `/etc/passwd` and `/etc/group` by default.

## Usage & Keybindings
-------------------

- Quit: `q`
- Switch tab: `Tab` (Users ↔ Groups)
- Users tab focus: `Shift+Tab` toggles Users list ↔ Member‑of list
- Move: `↑/k`, `↓/j`
- Page: `←/h` (previous page), `→/l` (next page)
- Search: `/` to start, type query, `Enter` to apply, `Esc` to cancel
- Open actions on selection: `Enter`
- In popups: `↑/k`, `↓/j`, `PageUp`, `PageDown`, `Enter`, `Esc`

- New user: `n` (toggle "Create home" with `Space`) 
- Delete confirmation: `Space` toggles "Also delete home"
- Password: Actions → Modify → Password
  - Set/change: masked input with confirm; toggle "must change at next login" with `Space`; select Submit and press `Enter`
  - Reset: expire password immediately (forces change at next login)

## What’s implemented
------------------

- Users tab
  - Table of users (from `/etc/passwd`), selection, paging
  - Detail pane: UID, GID, name, home, shell
  - Member‑of pane: primary and supplementary groups
  - Create user (`useradd`; optional `-m` to create home)
  - Delete user (`userdel`; optional `-r` to remove home)
  - Password management:
    - Set/change (masked via `chpasswd`, optional "must change at next login")
    - Reset (expire now via `chage -d 0`)
  - Actions → Modify:
    - Add user to groups (via `gpasswd -a`)
    - Remove user from groups (via `gpasswd -d`, excluding primary group)
    - Change username (`usermod -l`)
    - Change full name (GECOS, `usermod -c`)
    - Change login shell (pick from `/etc/shells`, `usermod -s`)

- Groups tab
  - Table of groups (from `/etc/group`), selection, paging
  - Detail pane and members list
  - Actions:
    - Create group (`groupadd`)
    - Delete group (`groupdel`)
    - Modify members (add/remove users)

- Search
  - Simple substring filter for Users and Groups tabs

## Notes & requirements
--------------------

- Linux/BSD only. macOS behavior may differ (Directory Services).
- Write actions call system tools and require appropriate privileges (root or sudo): `usermod`, `gpasswd`, `groupadd`, `groupdel`, `useradd`, `userdel`, `chpasswd`, `chage`.
- User deletion is implemented with confirmation and optional home removal.

## TODO (next steps)
-----------------

- Lock/unlock, enable/disable login shell: show status in table; actions in detail view; confirm + dry‑run; apply via `usermod -L/-U` and `chsh` (or edit `/etc/passwd` when in file‑parse mode)
- Password strength checks/validation: enforce basic rules or integrate zxcvbn; respect PAM policies; clearer error messages
- Fuzzy find users/groups: incremental filtering while typing; highlight matches; toggle fuzzy vs substring; performance guard for large datasets
- Filters (system vs human, inactive, expired, locked, no home, no password): quick filter menu; combinable chips; persisted per session; NSS‑aware where possible
- Multi‑select bulk ops (add to groups, lock, shell change, expiry set): selection mode with count; preview + confirmation; batched execution with per‑item results and rollback on failure
- Add tests (parsers, update loop, UI snapshots)
- Set up CI and cross‑platform release builds
- Optimize performance and memory usage

## Run locally
-----------

- Build: `cargo build --release`
- Run: `cargo run --release`

## Tests
-----

- Run all tests: `cargo test`

What’s covered today:
- Unit tests for parsers in `src/sys/mod.rs` (fake `/etc/passwd` and `/etc/group` files).
- Unit tests for filtering in `src/search.rs` (case‑insensitive user/group search, membership matching).

Guidelines:
- Keep small unit tests inline next to the code with `#[cfg(test)]` for private helpers and pure logic.
- For broader or cross‑module tests, add a `src/lib.rs` exposing modules (e.g., `pub mod app; pub mod search; pub mod sys;`) and place integration tests in `tests/`.
- Avoid invoking privileged commands (`useradd`, `gpasswd`, etc.) in tests. Prefer testing pure parts (parsing, filtering) or introduce a trait to mock `SystemAdapter` in higher‑level tests.

Optional:
- UI snapshot/sanity tests can be written using `ratatui`’s test backend (and, if desired, a snapshot tool like `insta`). These should render minimal views and assert on expected labels/highlights, not terminal specifics.

## Project Structure
```
src/
  main.rs                    # Entry point
  app/
    mod.rs                   # AppState, core types
    update.rs                # Event handling, business logic
  ui/
    mod.rs                   # Main render function, layout
    users.rs                 # Users tab (table + details + modals)
    groups.rs                # Groups tab (table + details + modals)
    components.rs            # Shared UI helpers (status bar, etc.)
  sys/
    mod.rs                   # Current SystemAdapter
  search.rs                  # Search functionality
```
