### Summary

Provide a concise summary of the change and its motivation. Reference related issues.

- Closes #
- Related to #

### Type of change

- [ ] fix
- [ ] feat
- [ ] refactor
- [ ] perf
- [ ] docs
- [ ] test
- [ ] build/ci
- [ ] chore
- [ ] breaking change

### Context and rationale

Why is this change needed? User story, UX improvement, bug fix details, etc.

### UI changes (if applicable)

Attach a screenshot or short GIF of the TUI change.

### How to test

Describe steps to verify locally. Include commands and expected behavior. If helpful, enable logs.

```bash
USRGRP_MANAGER_LOG=debug cargo run --release
```

### Safety and privileged operations

- Does this PR alter code paths that execute privileged commands (`useradd`, `usermod`, `userdel`, `groupadd`, `groupdel`, `gpasswd`, `chpasswd`, `chage`)? If so, explain the safeguards and confirmations.
- Note any considerations around system accounts/groups (e.g., low UIDs/GIDs). PRs should preserve confirmations and clear warnings for potentially destructive actions.

### Packaging impact (AUR/CLI)

Does this change affect build flags, runtime requirements, binary name, or CLI interface? If yes, briefly describe so maintainers can coordinate updates to `usrgrp-manager-git` and `usrgrp-manager-bin`.

### Documentation updates

List docs you updated (if applicable): `README`, wiki pages, or files in `docs/`.

### Breaking changes

- What breaks?
- Migration/upgrade notes for users, if any.

### Release notes (proposed)

One or two lines suitable for the Releases page.

### Checklist

- [ ] I read and followed `CONTRIBUTING.md`
- [ ] PR is focused and reasonably small
- [ ] Code is formatted and linted
  - [ ] `cargo fmt --check`
  - [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] Tests pass locally: `cargo test`
- [ ] New/updated tests added where practical (parsing, search, state helpers, error handling)
- [ ] Tests do not invoke privileged/destructive system commands
- [ ] UI change includes screenshot/GIF (if applicable)
- [ ] Docs updated where appropriate (`README`, wiki, or `docs/`)
- [ ] Packaging impact (AUR/CLI) called out above if applicable
- [ ] No secrets or sensitive data included in code, tests, or logs
- [ ] Security issues are not disclosed here; I will use private advisories per `SECURITY.md` if needed

### Additional context

Environment (distro, Rust version), logs, and any other notes for reviewers.

```bash
rustc --version
cargo --version
uname -a
```

<!--
Notes for contributors

- Keep rendering code in `src/ui/`, side-effectful system interactions in `src/sys/`, and state transitions in `src/app/update.rs`.
- Prefer small, testable helpers for logic. Avoid panics in normal flow; return `Result` with context.
- For UI involving destructive actions, ensure confirmations/warnings are clear (e.g., low GID/UID hints for system accounts).
- For security vulnerabilities, follow SECURITY.md and open a private advisory, not a public PR.
-->


