### Summary

Describe the focused change and its reliability/safety motivation.

### Verification

List exact local commands and results:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
```

### Privileged-operation safety

- [ ] This change does not cause normal tests to invoke account tools or sudo.
- [ ] Mutation changes use the typed `OperationRequest` bridge and show an exact redacted preview before confirmation.
- [ ] Root UID/GID protections, one-shot authentication, partial reports, and stale refresh behavior were considered.
- [ ] No password, secret, command output, or user-private data was added to diagnostics or docs.

### Configuration/UI/docs

- [ ] Rendering remains immutable and free of filesystem/process I/O.
- [ ] Config writes use the shared atomic writer and errors are surfaced.
- [ ] User-facing support/platform claims were updated when needed.

### Checklist

- [ ] Focused deterministic tests were added/updated where warranted.
- [ ] `cargo fmt`, applicable Clippy, tests, and docs were run.
- [ ] Dependency/security-policy changes include reviewed evidence.
- [ ] Security issues are reported privately per `SECURITY.md`, not in this PR.
