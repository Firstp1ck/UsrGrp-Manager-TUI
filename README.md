UsrGrp-Manager-TUI (Rust Users/Groups Manager TUI)
================

## Description
Keyboard‑driven terminal app to view and manage users and groups. Browse accounts, see memberships, search, and make common changes: rename users, update names or shells, adjust group membership. Safe to explore without admin rights; asks for permission to apply changes. Linux‑focused.

## Status
Alpha. Read‑only browsing is safe; write operations require privileges and are limited (no user deletion yet).

Alpha means:
- Interfaces and keybindings may change without notice.
- Some actions are intentionally missing or guarded (e.g., user deletion not implemented).
- Error handling, edge cases, and performance are still being improved.
- Expect to run with `sudo` for any write operation (`usermod`, `gpasswd`, `groupadd`, `groupdel`).

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

## What’s implemented
------------------

- Users tab
  - Table of users (from `/etc/passwd`), selection, paging
  - Detail pane: UID, GID, name, home, shell
  - Member‑of pane: primary and supplementary groups
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
- Write actions call system tools and require appropriate privileges (root or sudo): `usermod`, `gpasswd`, `groupadd`, `groupdel`.
- User deletion is not implemented yet (guarded with a confirmation and an informational message).

## TODO (next steps)
-----------------

- Lock/unlock, enable/disable login shell: show status in table; actions in detail view; confirm + dry‑run; apply via `usermod -L/-U` and `chsh` (or edit `/etc/passwd` when in file‑parse mode)
- Password set/reset with strength checks: masked prompt; basic rules or zxcvbn; optional "must change at next login"; respect PAM; clear error messages
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


