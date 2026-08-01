# Contributing

## Local development

The supported target is Linux local account files/tools. Build with the repository lockfile:

```bash
cargo build --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
```

Rust 1.89.0 is the documented MSRV; CI also tests stable.

## Safety rules

- **Never** write a normal test that invokes `sudo`, `useradd`, `userdel`, `usermod`, `groupadd`, `groupdel`, `groupmod`, `gpasswd`, `chpasswd`, or `chage`.
- Use injected `AccountDataSource`, `IdentityProvider`, `CommandRunner`, clock, config-root, terminal-control, and diagnostic-provider fakes plus fixture snapshots. Tests must not depend on host HOME, UID, `/etc`, `/proc`, user configuration, or a real terminal.
- Keep account command selection, root protection, elevation, stable target binding, and reconciliation in `src/sys/`. Application code must use `OperationRequest`, `prepare_operation`, `execute_prepared_operation`, `refresh_state`, and one-shot `set_elevation_secret` rather than legacy command helpers.
- Rendering must accept immutable cached state and must not read files, scan processes, or spawn commands. Render only visible table/modal rows; compute diagnostics and candidate lists in bounded explicit effects.
- Do not log credentials. Use redacted previews/reports only.

## Configuration and UI changes

Configuration saves must use `src/config::atomic_write`; deterministic durability tests use `atomic_write_with_fault` at write/flush/sync/rename boundaries. Do not reintroduce direct `fs::write` or silent error handling. Preserve all supported theme/filter/keymap values on parse/write/parse round trips and surface bounded source-line diagnostics.

For destructive UX, show the exact redacted prepared operation before confirmation. Bind confirmations to stable identities rather than list indices.

## Pull requests

Keep PRs focused. Include targeted deterministic tests and update documentation when behavior/support claims change. Run `cargo test --test core_static_guards --locked` after adding tests: it rejects production runner/process construction anywhere under `tests/`. State any packaging or compatibility impact. Security issues belong in the private process described by [SECURITY.md](SECURITY.md), not public issues.
