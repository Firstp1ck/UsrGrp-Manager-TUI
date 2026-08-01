# Security Policy

usrgrp-manager is a local Linux TUI for local account-file inspection and standard account-tool requests. It exposes no network service.

## Supported scope

Security fixes target the current `main` branch and latest release line. The supported implementation is Linux local passwd/group/shell data plus standard `useradd`, `userdel`, `usermod`, `groupadd`, `groupdel`, `groupmod`, `gpasswd`, `chpasswd`, and `chage` contracts. NSS/LDAP/remote backends and non-Linux platforms are unsupported.

## Privilege and secret handling

- Root UID/GID targets are blocked before elevation. The default fail-closed policy also protects service identities below UID/GID 1000 and `sudo`/`wheel` membership unless a reviewed runtime policy explicitly allowlists them.
- Commands are closed typed contracts; no shell fragment or arbitrary program is accepted.
- A sudo password is requested only for typed `AuthenticationRequired`, passed once to `sudo -v`, then discarded. Commands run through `sudo -n`.
- Password changes use `chpasswd` stdin only. Do not include passwords in reports, screenshots, terminal captures, or issue text.
- Related user-visible steps are one ordered composite operation with a single redacted confirmation. Partial execution, downstream skips, unverified postconditions, and unavailable reconciliation are reported as incomplete, not successful.

## Reporting a vulnerability

Please do **not** open a public issue. Use [GitHub private security advisories](https://github.com/firstpick/usrgrp-manager/security/advisories/new) and include:

- affected revision/package and Linux distribution;
- impact and minimal non-destructive reproduction;
- whether elevation is involved; and
- redacted logs only (never account or sudo passwords).

We will acknowledge reports, coordinate a fix, and credit reporters on request.

## Out of scope

Expected destructive account changes performed after a clear confirmation by an authorized administrator are not vulnerabilities by themselves. Unsafe confirmation binding, root-protection bypass, password disclosure, command injection, data-parser privilege confusion, and unsafe config writes are in scope.
