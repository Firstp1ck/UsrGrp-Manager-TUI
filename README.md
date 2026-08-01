# usrgrp-manager

A keyboard-driven TUI for **Linux local account files and standard account tools**. It reads local passwd/group/shell data and can prepare confirmed user/group changes.

## Safety model

- Linux-local scope only; this is not an NSS, LDAP, remote-management, or cross-platform tool.
- The UI renders cached data only. Refresh, shadow reads, home diagnostics, and process execution are explicit effects.
- Mutations use a closed, typed request bridge. The exact redacted command preview is shown before execution; targets are bound to the observed UID/GID/name and revalidated before elevation.
- Root user UID 0 and root group GID 0 are immutable. The default fail-closed policy also blocks service UID/GID targets below 1000 and `sudo`/`wheel` membership unless a reviewed runtime policy explicitly allowlists them.
- The application does not retain a reusable sudo password. A prompt appears only when the trusted runner returns `AuthenticationRequired`; its input is supplied once to `sudo -v`, then commands use `sudo -n`.
- Password records go only to `chpasswd` stdin. They are not put in shell commands, argv, previews, diagnostics, or logs.
- Create-with-password/membership, password+expiry, and bulk membership actions compile into one ordered bridge-owned composite plan, one redacted confirmation, and one aggregate report. Partial reports retain completed, failed, downstream-skipped, and unverified evidence rather than claiming success.

> Use a disposable VM/container for real account changes. Normal tests use injected data/command fakes and never run account tools.

## Build and run

```bash
cargo build --locked --release
./target/release/usrgrp-manager
```

Running as root is supported but unnecessary. For non-root write actions, the configured sudo policy must permit `sudo -v` followed by `sudo -n`; otherwise the action fails closed.

## Configuration

The application reads `theme.conf`, `filter.conf`, and `keybinds.conf` from `$XDG_CONFIG_HOME/UsrGrpManager`, then `~/.config/UsrGrpManager`, then `~/UsrGrpManager`.

Settings are parsed as bounded `key = value` files with source-line diagnostics for malformed, duplicate, unknown, or invalid entries. They are written in one canonical format through a restricted same-directory temporary file, synced, and atomically renamed. Existing symlinks are rejected. Configuration load/save errors are bounded and shown in the UI status state rather than ignored.

Example files are in [`example-configs/`](example-configs/). Theme colors support `#RRGGBB`, `index:N`, `named:color`, and `reset`.

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
