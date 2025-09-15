# Roadmap

Last Updated: 2025-09-15

This roadmap organizes high‑level work into milestone buckets with indicative targets. Dates are aspirational.

## v0.3

- Lock/unlock, enable/disable login shell
  - Acceptance: Lock/unlock state visible in table; actions in detail view; confirmation modal; dry‑run; applied via `usermod -L/-U` and `chsh` (or edit `/etc/passwd` in file‑parse mode)
- Password set/reset with strength checks
  - Acceptance: Masked prompt; basic rules or zxcvbn; optional “must change at next login”; clear PAM error messages
- Add tests: parsers, update loop, UI snapshots
- Set up CI and cross‑platform release builds

## v0.4

- Fuzzy find users/groups with incremental filtering and highlighting; toggle fuzzy vs substring
- Filters: system vs human, inactive, expired, locked, no home, no password (combinable chips; persisted per session)
- Optimize performance and memory usage across large datasets

## Later

- Multi‑select bulk ops (add to groups, lock, shell change, expiry set): selection mode with count; preview + confirmation; batched execution with per‑item results and rollback
- Profile templates and defaults (shell, home layout, groups, umask, password policy)
- SSH keys: view/edit `authorized_keys`, bulk operations, expiry metadata
- Auditability & safety: automatic timestamped backups + one‑click rollback; structured audit log (JSON)
- Integrations: optional LDAP/SSSD read‑only view; remote host management over SSH
- UX & navigation: split view, jobs panel, status bar hints, keybinding presets
- Accessibility & theming: high‑contrast theme, mouse support, resizable panes, configurable keymaps
- Scripting & extensibility: pre/post hooks, plugin points; export views as CSV/JSON; non‑interactive CLI subcommands
- Diagnostics: PAM/shadow/login.defs checks; detect conflicting state


