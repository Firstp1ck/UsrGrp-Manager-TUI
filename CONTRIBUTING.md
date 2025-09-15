Contributing to usrgrp-manager
==============================

Thank you for your interest in improving usrgrp-manager, a Rust TUI for managing local UNIX users and groups. This document explains how to set up a dev environment, propose changes, and ensure high quality and safety.

Contents
--------
- Getting started
- Safety while developing
- Project structure and design guidelines
- Code style, formatting, and linting
- Testing
- Proposing changes (issues and PRs)
- Documentation updates
- AUR packaging notes
- Security policy
- Releases and versioning
- Code of Conduct
- License

Getting started
---------------

Prerequisites:
- Linux system (primary target)
- Rust toolchain (stable) and Cargo
- Standard user/group tools available on your system: `usermod`, `useradd`, `userdel`, `groupadd`, `groupdel`, `gpasswd`, `chpasswd`, `chage`

Build and run:

```bash
git clone https://github.com/firstpick/usrgrp-manager.git
cd usrgrp-manager

# Build
cargo build --release

# Run (read-only actions do not require sudo)
cargo run --release

# Or run the installed binary
usrgrp-manager

# Optional logging
USRGRP_MANAGER_LOG=info   # or debug, trace
```

Helpful links:
- README overview and usage
- Wiki: Home, Install, Quick Start, Keyboard Shortcuts, Troubleshooting / FAQ
- SECURITY.md for reporting vulnerabilities

Safety while developing
-----------------------

usrgrp-manager can perform privileged operations (creating/deleting users and groups, changing passwords, etc.). Keep these principles in mind:
- Read-only exploration is safe without elevated privileges. Write operations require appropriate privileges and prompt for confirmation.
- Prefer testing destructive paths on a non-production system, VM, or disposable container.
- Be especially careful with system users/groups. Deleting or modifying system accounts can break services. The UI includes confirmations and warnings, but caution is still required.
- Do not write automated tests that perform privileged or destructive system changes (see Testing below).

Project structure and design guidelines
--------------------------------------

High-level modules (see `README.md` for details):
- `src/app/`: App state and update logic (event handling, business rules)
- `src/ui/`: Rendering and widgets (ratatui)
- `src/sys/`: System adapter and parsing of `/etc/passwd`, `/etc/group`, and command construction/handling
- `src/search.rs`: Search/filter logic
- `src/error.rs`: Error types/utilities

Design guidelines:
- Keep rendering code in `src/ui/`, side-effectful system interactions in `src/sys/`, and state transitions/business rules in `src/app/update.rs`.
- Prefer pure helper functions for logic that can be unit-tested easily (index math, filtering, validation) and keep side effects isolated.
- Avoid panics in normal control flow; return `Result` with context-rich errors.
- Ensure privileged commands are constructed safely and defensively (escaping, validation) in one place.

Code style, formatting, and linting
-----------------------------------

- Use Rust 2024 edition with idiomatic, readable code.
- Run formatters and lints before committing:

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
```

Preferred style notes:
- Descriptive names for functions and variables; avoid abbreviations.
- Early returns and guard clauses; shallow nesting.
- Keep comments short and focused on the "why".
- Match the existing formatting and module organization.

Testing
-------

Run tests:

```bash
cargo test
```

What to test:
- Unit tests for parsing and search behavior (`src/sys/mod.rs`, `src/search.rs`).
- State machine and input handling logic in `src/app/update.rs` where feasible by extracting pure helpers.
- Error handling utilities in `src/error.rs`.
- Optional UI snapshot/sanity tests using `ratatui`’s test backend.

Important safety rule:
- Do not invoke privileged commands (e.g., `useradd`, `gpasswd`, `chpasswd`) from tests. Prefer pure logic and adapter construction tests. Use fake data inputs for `/etc/passwd`, `/etc/group` parsing. See `docs/testing.md` for a detailed plan and scenarios.

Proposing changes (issues and PRs)
----------------------------------

Issues:
- For bugs, include steps to reproduce, expected vs actual results, environment (distro, Rust version), and relevant logs (`USRGRP_MANAGER_LOG=debug`).
- For features, describe the problem and desired UX, including any new keybindings or modals.

Pull Requests:
- Use a feature branch and keep PRs focused and as small as practical.
- Recommended commit style: Conventional Commits (e.g., `feat:`, `fix:`, `refactor:`, `docs:`). Imperative mood, concise subjects.
- Include tests for new logic where practical.
- For UI changes, add a brief description and, if possible, a screenshot or small GIF.
- Update docs where appropriate (README, wiki pages, or `docs/`).
- Ensure the checklist below passes locally:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Documentation updates
---------------------

- README: update usage, keybindings, or project structure if user-visible behavior changes.
- Wiki: consider updating pages for Quick Start, Keyboard Shortcuts, and Troubleshooting when flows change.
- `docs/testing.md`: add scenarios for new modules or logic that you introduce.
- `docs/RELEASE.md`: maintainers update this during releases; contributors generally need not edit it in feature PRs.

AUR packaging notes
-------------------

This project provides AUR packages (`usrgrp-manager-git` and `usrgrp-manager-bin`).
- If your change affects build flags, runtime requirements, or the binary name/CLI, call it out in your PR description.
- Maintainers will coordinate updates to the `PKGBUILD` files in the AUR repositories. If you are familiar with Arch packaging and want to help validate, you can build locally via `makepkg -si` in the respective AUR repo directories.

Security policy
---------------

Please do not file public issues for security vulnerabilities. Follow the guidance in `SECURITY.md` to report privately and responsibly.

Releases and versioning
-----------------------

- The project is MIT-licensed and currently in alpha. Interfaces and keybindings may change.
- Version bumps and release notes are handled by maintainers (see `docs/RELEASE.md`).
- Please do not update the crate version in PRs unless explicitly requested for a release.

Code of Conduct
---------------

Be respectful and constructive. Assume good faith, collaborate openly, and focus on helping users and maintainers succeed. Harassment or discrimination is not tolerated.

License
-------

By contributing, you agree that your contributions will be licensed under the MIT License, consistent with the project’s `LICENSE` file.


