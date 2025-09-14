## Notes

`UsrGrp-Manager` only works on UNIX based OS.

On OSX, the information reported will not be accurate. The tool relies on the `/etc/passwd` and `/etc/group` files, which are only consulted in OSX in single-user mode, and the system uses [DirectoryService](https://developer.apple.com/documentation/devicemanagement/directoryservice) to manage user and groups.

## Built with
 - [bubbletea](https://github.com/charmbracelet/bubbletea) and its ecosystem
 - [bubble-table](https://github.com/Evertras/bubble-table)

## Plan: Rewrite to Rust

- **Goals**
  - Reach feature parity with the Go TUI: users/groups tabs, paging, search, vim/arrow keys.
  - Keep startup fast and memory usage modest; support Linux and BSDs. macOS remains best‑effort.

- **Tech Stack**
  - UI: [`ratatui`](https://github.com/ratatui-org/ratatui) + [`crossterm`](https://github.com/crossterm-rs/crossterm)
  - System users/groups: [`users`](https://github.com/ogham/rust-users) (respects NSS); optional file parsing fallback
  - CLI/logging/error: `clap`, `tracing` + `tracing-subscriber`, `anyhow`/`thiserror`
  - Search: `fuzzy-matcher` (optional), simple substring by default

- **Proposed Crate Layout**
  - `src/main.rs`: entry, args, tracing init, runs app
  - `src/app/`: app state, actions, update loop, key handling
  - `src/ui/`: table, status bar, search box, theming
  - `src/sys/`: adapters for users/groups via `users` crate
  - `src/parsers/`: `passwd.rs`, `group.rs` (feature `file-parse`, e.g. with `nom`)
  - `src/search.rs`: filter and optional fuzzy matching
  - `src/theme.rs`: colors and styles

- **Keybindings (parity)**
  - Exit: `Ctrl+C` / `q` / `Esc`
  - Switch tab: `Tab`
  - Navigation: `↑/k`, `↓/j`, `←/h`, `→/l`
  - Search: `/` to enter, `Enter` to apply

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

If you share your top 3 priorities (e.g., safety, batch ops, LDAP), I can help turn them into a concrete MVP scope.

## Refactor Structure
```
src/
  main.rs                    # thin entry: parse CLI, init logging, run app
  cli.rs                     # Clap args and env
  app/
    mod.rs                   # re-exports
    state.rs                 # AppState, ActiveTab, focus enums, settings
    msg.rs                   # Msg enum (events/commands)
    update.rs                # TEA-style update() handling keys/actions
    services.rs              # high-level operations orchestrating sys + state
  ui/
    mod.rs
    layout.rs                # root layout (header, split panes, footer)
    header.rs                # top header/hints
    status_bar.rs            # bottom status line
    help.rs                  # optional help popup
    widgets/
      table.rs               # generic table helpers
      modal.rs               # generic modal helpers
    users/
      table.rs               # users list
      details.rs             # user details pane
      member_of.rs           # groups the user belongs to
      modals.rs              # users actions: modify, groups add/remove, shell, etc.
    groups/
      table.rs
      details.rs
      members.rs
      modals.rs              # create/delete group, add/remove members
    theme.rs                 # colors/styles
  domain/
    mod.rs
    user.rs                  # core types independent of UI/sys
    group.rs
    filters.rs               # predicates for system vs human, locked, etc.
  search/
    mod.rs
    substring.rs
    fuzzy.rs                 # behind feature flag `fuzzy`
  sys/
    mod.rs                   # SystemAdapter facade
    users.rs                 # list users, user attrs
    groups.rs                # list groups, membership
    shells.rs                # read /etc/shells
    commands.rs              # wrappers: usermod, gpasswd, groupadd/del
    parsers/                 # only with feature `file-parse`
      mod.rs
      passwd.rs
      group.rs
  keymap.rs                  # keybindings -> Msg mapping (rebindable later)
  errors.rs                  # shared error types (thiserror) if you move off anyhow
  config.rs                  # persistent settings (rows/page, theme)
  logging.rs                 # tracing subscriber setup (optional)

tests/
  integration/
    update_loop.rs           # drive Msg/update and assert state
    sys_commands.rs          # command wrappers with dry-run
  ui_snapshots/
    users_table.snap         # snapshot tests for rendering
fixtures/
  etc/
    passwd.sample
    group.sample
```