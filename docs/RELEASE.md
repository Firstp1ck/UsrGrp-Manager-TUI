## v0.1.0 — 2025-09-14

First public alpha of UsrGrp-Manager-TUI: a keyboard‑driven terminal app to view and manage users and groups on Linux.

### Features
- Users tab: list users, view details (UID/GID, home, shell, name), view member‑of groups
- User management: create user (optional home), delete user (optional remove home), set/change/reset password, modify username, full name (GECOS), and login shell
- Groups tab: list groups with members, create/delete groups, add/remove users
- Search: quick substring filter on Users and Groups
- Safety: read‑only browsing works without privileges; write actions prompt/require appropriate privileges
- Logging: set `USRGRP_MANAGER_LOG=info|debug|trace` (default: `info`)

### Status and Notes
- Alpha: interfaces and keybindings may change; error handling and performance still improving
- Linux focused; write operations call system tools like `usermod`, `gpasswd`, `groupadd`, `groupdel`, `useradd`, `userdel`, `chpasswd`, `chage`
- User deletion requires confirmation; optional home removal

For usage and keybindings, see `README.md`.
