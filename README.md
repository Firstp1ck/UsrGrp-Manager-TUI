# usrgrp-manager

A keyboard-driven TUI for **Linux local account files and standard account tools**. It reads local passwd/group/shell data and can prepare confirmed user/group changes.

## Safety model

- Linux-local scope only; this is not an NSS, LDAP, remote-management, or cross-platform tool.
- The UI renders cached data only. Refresh, shadow reads, home diagnostics, and process execution are explicit effects.
- Mutations use a closed, typed request bridge. The exact redacted command preview is shown before execution; targets are bound to the observed UID/GID/name and revalidated before elevation.
- The built-in fail-closed policy blocks changes to the root user (UID 0) and root group (GID 0), to users and groups with a UID or GID below 1000, and to membership in those groups and in `sudo` and `wheel`. Blocked targets are refused before any command runs. An allowlist exists in code (`ProtectedIdentityPolicy`), but there is no runtime setting, flag, or file that changes the policy.
- The create-user form offers to add the new account to the group named by the `UGM_SUDO_GROUP` environment variable (default `wheel`). When the policy refuses that group, which it always does for `sudo` and `wheel`, the form says so and the box cannot be ticked. Groups the policy refuses are marked `(blocked: <reason>)` in the group and member lists and cannot be ticked or chosen.
- Start the app as a normal user. When a non-root user applies a change, the app asks for the sudo password for that one operation. It passes the password once to `sudo -v`, then runs each command with `sudo -n`, and does not keep it. If sudo rejects the password, the prompt reopens for the same prepared operation. After three rejections the operation is dropped without running. As root, commands run directly and no prompt appears.
- Password records go only to `chpasswd` stdin. They are not put in shell commands, argv, previews, diagnostics, or logs.
- Create-with-password/membership, password+expiry, and bulk membership actions compile into one ordered bridge-owned composite plan, one redacted confirmation, and one aggregate report. Partial reports retain completed, failed, downstream-skipped, and unverified evidence rather than claiming success.

> Use a disposable VM/container for real account changes. Normal tests use injected data/command fakes and never run account tools.

## Build and run

```bash
cargo build --locked --release
./target/release/usrgrp-manager
```

Run it as your normal user. Running as root is supported but unnecessary; as root the app can also read `/etc/shadow` for the lock, password, and expiry details. For non-root write actions, the sudo policy must permit `sudo -v` followed by `sudo -n`; otherwise the action fails closed.

Command-line options:

| Option | Effect |
| --- | --- |
| `--config-dir <PATH>` | Read and write `theme.conf`, `filter.conf`, `keybinds.conf`, and `settings.conf` in `PATH` only. The default search order below is skipped. Missing files are created there. |
| `--dry-run` | Every change still shows its exact command preview, but confirming it only reports "Dry run: nothing was executed." No sudo prompt appears and no command runs. |
| `--log-file <PATH>` | Append one line per prepared operation, executed operation (with its result summary), and error to `PATH`. The file is created with mode 0600 and symlinks are refused. Lines contain the same redacted previews the UI shows, never passwords. |
| `-h`, `--help` | Print usage and exit. |
| `-V`, `--version` | Print the version and exit. |

Path options also accept `--option=PATH`. An unknown or repeated option prints usage to stderr and exits with status 2 before the terminal is touched. So does a `--config-dir` that is an existing non-directory, or a `--log-file` that cannot be opened.

```bash
./target/release/usrgrp-manager --dry-run --log-file /tmp/ugm.log
```

## Using the interface

Press `?` for the help dialog. It lists every key as currently bound in `keybinds.conf`, the dialog keys, and where each configuration file was loaded from. Shift+K shows the same bindings in a side panel on terminals at least 90 columns wide. The ten keys you need most:

| Key | Action |
| --- | --- |
| `?` | Help |
| Tab | Switch between the Users and Groups tabs |
| `/` | Search the current tab; Esc in the main view clears the search |
| `f` | Filter menu |
| Enter | Menu for the highlighted row, or bulk actions when users are marked |
| `n` | Create a user or group |
| Delete, `d` | Delete the highlighted user or group, or remove the highlighted membership |
| Space | Mark a user for bulk actions |
| `r` | Reread users and groups |
| `q`, Ctrl+C | Quit (Ctrl+C works everywhere) |

Table titles show the search, the match count, marks, the sort order, and active filters, for example `[Users]  /al  3 of 45  sort: name  [human]`. Every change opens a command preview first. A preview with more than 12 steps must be scrolled to its last step before Apply works. In dialogs, Esc goes back one level; on the preview or sudo prompt it cancels the operation. The wiki's Keyboard Shortcuts page lists the rest, including `s` to sort and `y` to copy a name through OSC 52.

Below 70 columns the details pane moves under the table and the member pane is hidden. Below 30x8 the app only asks for a larger terminal.

## Configuration

The application reads four files: `theme.conf`, `filter.conf`, `keybinds.conf`, and `settings.conf`. For each one it uses the first file found in `$XDG_CONFIG_HOME/UsrGrpManager`, then `~/.config/UsrGrpManager`, then `~/UsrGrpManager`. Changes are saved to the first of those directories, and missing files are created there with the defaults. `--config-dir` replaces the search with one directory. The Configuration section of the help dialog shows each file's loaded path and, when it differs, the save path.

Settings are parsed as bounded `key = value` files with source-line diagnostics for malformed, duplicate, unknown, or invalid entries. They are written in one canonical format through a restricted same-directory temporary file, synced, and atomically renamed. Existing symlinks are rejected. When a file cannot be loaded, the app uses its defaults, shows the errors in a dialog at startup and at the top of the help dialog, and the status bar says `config: N errors (see ? help)`.

Example files are in [`example-configs/`](example-configs/).

`keybinds.conf` has one `Action = Key` line per binding, for example `Refresh = r`, `DeleteSelection = d`, or `ToggleKeybindsPane = Shift+K`. An action can have several keys. Ctrl+C always quits, whatever the file says.

`filter.conf` stores the filter state and is rewritten whenever a filter changes. The search and the sort order are not saved.

`settings.conf` has one key, `wrap_selection = true|false` (default `true`). With `false`, moving past the first or last row of the users, groups, or member tables stops there instead of wrapping to the other end; page keys stop there too.

`theme.conf` keys. Colors take `#RRGGBB`, `index:N`, `named:color`, or `reset`.

| Key | Used for |
| --- | --- |
| `preset` | Optional. `mocha` (the default) or `dark` picks the base palette; the color keys in the file override it. Written back only if the file had it. `dark` uses named colors, for terminals without truecolor. |
| `text` | Table rows and the details panes |
| `muted` | Secondary text: header key hints, dialog footers, empty tables, unavailable values, filters that cannot apply. The old spelling `_muted` is still read. |
| `title` | Table column headings |
| `border` | Pane and dialog borders |
| `header_fg`, `header_bg` | The header bar at the top |
| `status_fg`, `status_bg` | The status bar at the bottom |
| `highlight_fg`, `highlight_bg` | The selected row in every table |

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --workspace --no-default-features --locked
cargo test --doc --locked
cargo doc --workspace --all-features --no-deps --locked
cargo deny check
cargo audit
cargo test --test action_targeting --test terminal_cleanup --test shadow_status --test config_atomicity --test ui_small_terminal --test resource_bounds --test operation_retry_matrices --locked
cargo test --release --bench search_and_render -- --nocapture
```

The release benchmark builds 10,000 users plus 10,000 groups, draws immutable Ratatui frames, and prints sample count plus search/render p95 against D7's 50 ms/16 ms limits. Correctness tests assert explicit resource and operation bounds rather than elapsed time.

See [CONTRIBUTING.md](CONTRIBUTING.md) for deterministic-test rules and [SECURITY.md](SECURITY.md) for private vulnerability reporting.
