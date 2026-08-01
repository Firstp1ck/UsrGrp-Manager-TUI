# Reliability and Robustness Improvement Plan

**Status:** Implemented locally; external release verification pending (not archival-ready)  
**Owner:** Maintainers  
**Integration owner:** Parent Pi session (final repository inspection, plan state, review dispositions, and archival)  
**Final report:** [`../../reports/reliability-robustness.html`](../../reports/reliability-robustness.html) (created after integrated validation)  
**Baseline:** `main` at `0b154c1` (commit date 2026-04-12; inspected 2026-07-31)  
**Canonical scope:** This is the only improvement/roadmap plan for the repository. It supersedes `docs/Improvements.md` and `docs/roadmap.md`.

## Goal

Make the existing local Linux user/group manager safe, diagnosable, deterministic to test, and honest about partial failure before adding more product features.

## Scope and non-goals

### In scope

- Existing account/group reads and privileged mutations.
- Authentication, secret handling, command execution, timeouts, and child cleanup.
- Input validation, target identity, confirmation, partial failure, reconciliation, and retry behavior.
- Terminal lifecycle, error reporting, config durability, UI-state invariants, bounded rendering, tests, CI, dependencies, and support diagnostics.

### Not part of this reliability program

- New account lifecycle features such as lock/unlock.
- LDAP/SSSD/AD writes, remote management, `systemd-homed`, sudoers editing, SSH-key editing, import/export, plugins, or fleet operations.
- Automatic deletion of users, groups, or homes as rollback.
- A claim of cross-platform or NSS support without an implemented backend and CI evidence.
- Visual polish such as fuzzy search, mouse support, resizable panes, or new themes unless required to fix an invariant.

## Reliability principles

1. Unknown privilege or identity state fails closed; it never means root.
2. Rendering is pure and bounded; system I/O happens in explicit refresh/effect paths.
3. Authentication failures are distinct from validation, execution, timeout, exit-status, refresh, and unsupported-platform failures.
4. A mutation is successful only when every required step and postcondition succeeds.
5. Partial changes are reconciled and reported; they are not hidden or blindly retried.
6. Previously valid UI data is retained and marked stale when refresh fails.
7. Secrets never enter argv, logs, `Debug`, snapshots, or long-lived application state.
8. Tests never invoke privileged host tools or depend on the host's accounts, HOME, UID, `/etc`, or `/proc`.

## Verified baseline

| Check | Result | Implication |
|---|---|---|
| `cargo fmt --all -- --check` | Pass | Formatting baseline is clean. |
| `cargo check --all-targets --all-features` | Pass | Current code compiles on the inspected Linux host. |
| `cargo test --all-features` | Pass: 86 executions | Twenty module tests run twice because the binary redeclares library modules; several public tests are tautological or host-dependent. |
| `cargo doc --all-features --no-deps` | Pass | Documentation builds. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Fail: 4 findings | CI does not enforce the repository's documented lint command. |
| `cargo audit --no-fetch` | 3 allowed warnings | `paste` is unmaintained; cached advisories also flag unsound `lru` and dev/target-specific `anyhow` versions. Reachability/upgrades require review. |
| `cargo tree --duplicates` | Duplicate Crossterm/Rustix families | Direct Crossterm 0.29 coexists with Ratatui's Crossterm 0.28. |
| `cargo deny check` | Not usable without policy | No `deny.toml`; default license policy rejects the graph and advisories include `paste`. |
| Tracked-file secret-pattern scan (`git ls-files` + grep for common OpenAI/AWS/GitHub/Slack/private-key/password-assignment patterns) | No tracked secret found | This bounded pattern scan is not proof of absence; add a configured scanner and redaction tests in M6. |
| Literal debt scan | No `TODO`/`FIXME`/`HACK` in tracked Rust/Markdown | Existing planning debt was concentrated in the two superseded docs, not inline markers. |

Repository size inspected: all 13 Rust source modules, both test files, manifest/lockfile, both workflows, examples relevant to parsing, security/contribution docs, and both superseded plans.

## Highest-priority findings

### P0 — release-blocking safety/correctness

1. **Password and sudo secret handling is unsafe.** Non-root password changes put `username:password` into a `bash -c` argument, subject to local process-argument visibility and shell/control-character hazards. Sudo credentials remain cloned in `AppState`; stdin write errors are ignored (`src/sys/mod.rs:275-342`, `src/app/mod.rs:503`, `src/app/update.rs:1978-1988`).
2. **Privilege detection fails open.** Failure to parse Linux `/proc/self/status` returns UID 0, bypassing the sudo path (`src/sys/mod.rs:501-514`).
3. **Multi-step mutations can leave silent partial state.** User creation and bulk membership loops stop after an earlier step has already changed the host; retry can repeat completed steps (`src/app/update.rs:2133-2142`, `2200-2254`).
4. **A successful mutation can erase the UI cache.** Post-write reads use `unwrap_or_default()`, replace known data with empty vectors, and still show success (`src/app/update.rs:2044-2254`).
5. **UI target-selection bugs can act on the wrong entity.** The member action stores a username as `group_name`; a single-select add indexes `groups_all` rather than the eligible list; group-member removal render/update target resolution differs (`src/app/update.rs:154-178`, `857-925`, `1729-1756`; `src/ui/groups.rs:543-558`).
6. **Inputs reaching privileged tools are not centrally validated.** Production lacks username/group grammar, length, control-character, delimiter, and leading-option checks; the only username validator exists inside a test (`src/app/update.rs:1178-1208`, `1368-1394`, `1912-1962`; `tests/unit_test.rs:641-678`).

### P1 — major reliability gaps

1. All initial mutation errors open a sudo prompt, even for invalid input, missing executables, operational failure, or partial completion (`src/app/update.rs:605-622`, `1978-1989`).
2. `must_change` ignores `chage` failure but reports success (`src/app/update.rs:2174-2192`).
3. Terminal setup/cleanup is not RAII or panic-safe; initialization can leave raw mode enabled, cleanup errors are ignored, and application failures return exit code 0 (`src/main.rs:22-49`).
4. Invalid passwd/group numbers become UID/GID 0 and malformed records disappear silently (`src/sys/mod.rs:427-486`).
5. Shadow readability is guessed from the world-readable bit; unavailable data becomes false or filters silently become no-ops (`src/search.rs:48-61`, `136-148`, `223-226`; `src/ui/users.rs:197-211`).
6. Config initialization/save errors are ignored; writes are non-atomic and lossy. Filter enum values are serialized as `None`, indexed theme colors are not parseable by the reader, and keymap writes emit only a fixed subset (`src/app/filterconf.rs:92-102`, `177-198`; `src/app/mod.rs:192-250`, `603-610`; `src/app/keymap.rs:153-231`).
7. Rendering repeatedly parses `/etc/passwd`, reads shadow/authorized keys, scans `/proc`, and computes group diagnostics on a 100 ms loop (`src/app/update.rs:23-34`; `src/ui/mod.rs:53`; `src/ui/users.rs:148-260`; `src/ui/groups.rs:100-229`).
8. Selection and pagination state is shared across unrelated panes; rendering mutates state; group-member paging always starts at zero (`src/ui/users.rs:26-34`, `303-330`; `src/ui/groups.rs:25-33`, `296-309`).
9. Production reads only `/etc/passwd` and `/etc/group`; the `file-parse` feature is empty. Earlier NSS/BSD claims were inaccurate (`Cargo.toml:24-26`; `src/sys/mod.rs:54-61`, `488-490`).
10. CI only builds/tests on Ubuntu and omits format, Clippy, docs, locked/all-feature modes, MSRV, advisory/license policy, timeouts, and immutable action pins (`.github/workflows/rust.yml`, `.github/workflows/doc-rust.yml`).
11. Group deletion has no protected-identity policy: GID 0 bypasses even the system-group warning, while other mutation paths use inconsistent UID/GID guards (`src/app/update.rs:1408-1453`, `2109-2117`; `src/ui/groups.rs:410-425`).

## Decisions required before behavior-changing work

The maintainer should record these decisions in this file before Milestone 1 starts. Recommended fail-closed interim choices are shown.

| ID | Decision | Recommended interim choice |
|---|---|---|
| D1 | Supported platform/data source | Linux local account files/tools only. Remove BSD/macOS/NSS claims until an NSS backend and CI exist. |
| D2 | Elevation model | Keep per-action elevation, but isolate it behind one typed runner. Do not require running the whole TUI as root. |
| D3 | Recovery semantics | Reconcile and report partial state; compensate only reversible membership changes whose prior state is known. Never auto-delete users/homes/groups. |
| D4 | Protected identities | Replace hard-coded UID/GID ranges with explicit policy/config and immutable target checks. Root remains unconditionally protected. |
| D5 | Sudo credential lifetime/transport | Prefer sudo's timestamp cache and one-shot secret input; do not retain a reusable password in `AppState`. If policy disables a reusable timestamp, fail with a typed authentication-capability error unless a separately approved, tested transport can keep sudo credentials and `chpasswd` payloads unambiguous. Never restore the shell pipeline. |
| D6 | Shadow-unavailable filter behavior | Show `unknown/unavailable` and refuse to claim an active status filter succeeded without data. |
| D7 | Responsiveness budget and benchmark owner | Before M4, record the benchmark host/profile, dataset, p95 render/search limits, allocation/I/O limits, and maintainer owner. Recommended starting targets: zero render-path I/O, visible-row-only rendering, p95 render ≤16 ms, and p95 search/filter ≤50 ms for 10,000 users plus 10,000 groups on the recorded runner. |

### Recorded implementation decisions (2026-07-31)

The user-authorized implementation adopts the fail-closed recommendations above after direct source validation. These are implementation defaults, not claims about unsupported platforms.

| ID | Recorded decision |
|---|---|
| D1 | Support Linux local account files and standard shadow-utils-compatible tools only. Remove the empty `file-parse` feature and unsupported NSS/BSD/macOS claims. |
| D2 | Keep per-action elevation behind a typed, injectable command runner. Whole-application root execution remains supported but is not required. |
| D3 | Reconcile and report partial state. Compensate only membership changes with a known prior state; never automatically delete a user, group, or home. |
| D4 | Root user UID 0 and root group GID 0 are immutable. Other protected thresholds remain explicit policy inputs rather than hidden command-runner behavior. |
| D5 | Authenticate once through sudo validation, immediately discard the secret, and execute with `sudo -n`. Password records go only to `chpasswd` stdin; a timestamp/policy failure is a typed authentication-capability error. |
| D6 | Shadow state is `known`, `unknown`, or `unavailable`; filters that require unavailable data report that limitation instead of silently acting as no-ops. |
| D7 | Benchmark owner is `Maintainers`. Reference profile: release-mode x86_64 Linux, 10,000 fixture users plus 10,000 fixture groups; zero render-path I/O, visible-row-only work, p95 render ≤16 ms, and p95 search/filter ≤50 ms. Correctness tests assert bounds and operation counts, not wall-clock timing. |
| D8 | Account names are 1–32 bytes, start with ASCII letter/underscore, then ASCII alphanumeric/underscore/hyphen, may have one trailing `$`, and reject leading `-`, controls, colon, comma, whitespace, and newline. GECOS rejects colon/control/newline and is bounded to 256 bytes. Shell paths must be absolute, control-free, and bounded to 4096 bytes. Password records reject NUL/CR/LF and username delimiters; passwords are bounded to 1024 bytes. |
| D9 | Small audited dependencies are permitted where the standard library lacks the required primitive: `libc` for checked effective UID, `zeroize` for secret drop behavior, and focused dev tooling only when it materially improves deterministic validation. Dependency upgrades are evidence-driven, not forced merely to remove duplicates. |

### Classification and execution governance

**Classification: complex (validated).** The work crosses process execution, privilege and secret transport, account parsing, application state, rendering, configuration, CI, and documentation; it has multiple independently verifiable slices and material security/reliability risk. Current evidence confirms the inherited complex classification: `src/sys/mod.rs` combines host reads and privileged execution, `src/app/update.rs` is a 2,596-line mutation/update surface, render modules perform host I/O, and current tests execute duplicated module tests and depend on host state.

**Success criteria:** all milestone exit gates pass; no privileged host tool is executed by normal tests; malformed IDs cannot become privileged identities; secrets are absent from argv/debug/errors/snapshots; partial operations and refresh failures retain honest state; rendering is immutable and I/O-free; configuration round-trips atomically; CI and documentation match the executable behavior.

**Execution DAG and ownership:**

1. `W1 trusted-core` (worker handoff: `plans/handoffs/reliability-core.md`) owns `Cargo.toml`, `Cargo.lock`, `src/error.rs`, `src/sys/**`, core operation/validation/secret/identity/data-source APIs, fake-runner/account fixtures, and their focused tests. It must not edit this canonical plan, UI/render files, workflows, or user documentation.
2. `W2 application-integration` (worker handoff: `plans/handoffs/reliability-integration.md`) starts only after W1 validates. It owns `src/app/**`, `src/config/**`, `src/terminal.rs`, `src/main.rs`, `src/lib.rs`, `src/search.rs`, `src/ui/**`, examples, remaining tests/benchmarks, workflows, dependency policy, and user documentation. It consumes W1's public contracts and must stop rather than silently redesign them; a minimal compatibility edit to W1-owned files requires integration-owner approval.
3. The integration owner inspects both handoffs and the actual diff, runs affected and cross-workstream checks, and records evidence below.
4. Two fresh-context, read-only reviewers from distinct provider families inspect the integrated result. Every finding receives an evidence-backed disposition before accepted fixes are applied and revalidated.
5. The integration owner creates `reports/reliability-robustness.html`, links it back here, and archives this plan only after every completion gate passes.

The dirty shared worktree requires sequential writers in one writer chain; automatic worktree fanout and concurrent writes are prohibited. Pre-existing `.gitignore`, deleted superseded plans, and this plan are preserved. Rollback uses path/hunk-scoped reverse patches only—never `reset --hard`, broad checkout, or clean. If elevation cannot satisfy D5, affected mutations fail closed; rollback never restores the shell pipeline. If refresh/reconciliation fails, prior data remains stale rather than being erased.

## Milestone plan

Milestones are dependency ordered. Do not start a later milestone until the prior milestone's exit gate passes.

### M0 — Freeze behavior and create deterministic seams

**Objective:** Make unsafe behavior reproducible without touching real host state.

- [ ] Record D1–D7 above and update acceptance wording if a recommendation is rejected.
- [ ] Introduce injectable `AccountDataSource`, `CommandRunner`, clock, config root, and diagnostic providers.
- [ ] Add fake command outputs, fixtures for passwd/group/shadow/shells, and deterministic temporary config roots.
- [ ] Add characterization tests for every current privileged command contract, target resolution path, config round trip, and displayed error class.
- [ ] Add fail-closed test enforcement: privileged executable names must only reach the fake runner under tests, and CI must run isolated fixture roots for effective-UID, HOME, account-file, and proc-data cases.
- [ ] Ensure `AppState` test construction never reads/creates real user config.

**Exit gate**

- No test executes `sudo`, `useradd`, `usermod`, `userdel`, `groupadd`, `groupmod`, `groupdel`, `gpasswd`, `chpasswd`, or `chage`.
- Tests pass with arbitrary host UID, HOME, `/etc`, and `/proc` state.

```bash
cargo test --workspace --all-targets --all-features --locked
```

### M1 — Close immediate secret, privilege, terminal, and targeting hazards

**Objective:** Remove paths that can expose credentials, fail open, or operate on a mismatched target.

- [ ] Replace `/proc` parsing/fallback with a checked effective-UID API; unknown/unsupported is an error, never UID 0.
- [ ] Remove `bash -c`, `echo`, and password-bearing argv. After authentication, feed a bounded `username:password` record directly to `sudo -n chpasswd` stdin. If sudo policy makes the validated timestamp unusable, return the D5 typed capability/authentication result; do not guess, shell out, or mix two protocols on stdin without an approved contract test.
- [ ] Check every spawn, stdin write/close, wait, timeout, kill, and reap result; cap stderr/stdout retained in memory.
- [ ] Introduce a non-`Debug`, zeroizing secret wrapper and remove long-lived `AppState.sudo_password`.
- [ ] Add validated newtypes for user/group names, shell paths, GECOS text, and password records; reject leading options, delimiters, controls, newlines, and over-limit input.
- [ ] Add typed errors for authentication-required/denied, validation, unsupported platform, missing executable, timeout, I/O, non-zero exit, refresh, and partial completion.
- [ ] Prompt for sudo only on `AuthenticationRequired`; show the original classified error otherwise.
- [ ] Fix the three target-selection defects and bind confirmations to stable UID/GID/name identity plus the exact planned action.
- [ ] Apply D4 consistently to delete/rename/modify paths; unconditionally block the root user and root group, and test the current GID-0 deletion bypass.
- [ ] Add an RAII terminal session that restores raw mode, screen, mouse, and cursor after partial initialization, normal return, error, and panic; return non-zero on application failure.
- [ ] Make `main.rs` consume the library instead of redeclaring its module tree.
- [ ] Fix the four baseline Clippy findings so this milestone's lint gate measures new regressions rather than hidden debt.

**Exit gate**

- Secrets are absent from argv, errors, logs, `Debug`, and snapshots.
- Unknown UID cannot enter the root path.
- Operational failures never reopen the sudo prompt.
- PTY/failure-injection tests prove terminal restoration.
- Each target-regression test executes against the identity shown to the user.

```bash
cargo test --test command_contracts --locked
cargo test --test action_targeting --locked
cargo test --test terminal_cleanup --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

### M2 — Typed operations, reconciliation, and partial-failure recovery

**Objective:** Make every mutation's actual outcome explicit and safely retryable.

- [ ] Represent each mutation as an `OperationPlan` with validated target, preconditions, ordered steps, expected postconditions, and a redacted preview.
- [ ] Return an `OperationReport` with completed, failed, skipped, and compensated steps plus reconciliation status.
- [ ] Re-read all affected data after success or partial failure; retain prior snapshots and mark them stale when refresh fails.
- [ ] For create-user, password+expiry, and bulk membership, inject failure at every step and document final state/remediation.
- [ ] Never claim “must change,” group membership, or full success until the required postcondition is observed.
- [ ] Add idempotency/precondition checks so retry skips already-satisfied steps.
- [ ] Add a partial-success UI with completed work, failed step, current known state, and safe retry/manual remediation.
- [ ] Add dry-run/preview from the same plan executed after confirmation; do not create a separate preview code path.
- [ ] Compensate only approved reversible membership steps; never auto-delete an account or home.

**Exit gate**

- Failure matrices cover every multi-step boundary.
- A refresh failure cannot empty a known-good list.
- Preview and execution have identical validated targets/arguments.
- Partial outcomes are visible and deterministic.

```bash
cargo test --test operation_reports --locked
cargo test --test reconciliation --locked
cargo test --test partial_failure --locked
cargo test --test dry_run_equivalence --locked
```

### M3 — Trustworthy account data and durable configuration

**Objective:** Stop fabricating identities and silently losing settings.

- [ ] Parse passwd/group records into typed results with source and line diagnostics; invalid UID/GID never becomes 0.
- [ ] Implement D1: either a real NSS-aware backend or an explicitly Linux-local file backend. Remove the no-op feature and inaccurate support claims.
- [ ] Read shadow once per explicit refresh; rely on actual open/read results rather than permission-bit guesses.
- [ ] Model shadow fields as known/unknown/unavailable and define filter behavior for each state.
- [ ] Add one shared config layer with parse diagnostics and lossless theme/filter/keymap serialization.
- [ ] Preserve legacy reads for one release, but emit one canonical parseable format; support or reject inline comments consistently.
- [ ] Write config through a restricted same-directory temporary file, flush/sync, and atomic rename; define symlink policy and surface errors.
- [ ] Correct filter enum persistence, indexed-color round trips, hard-coded keymap subsets, and example configs.

**Exit gate**

- Malformed records cannot masquerade as privileged identities.
- Parse/write/parse equivalence holds for every config type.
- Interrupted writes leave either the complete old or complete new file.
- Unknown account status is visibly distinct from false.

```bash
cargo test --test account_parsing --locked
cargo test --test shadow_status --locked
cargo test --test config_roundtrip --locked
cargo test --test config_atomicity --locked
```

Operational reference: ArchWiki documents seven-field local passwd records, recommends account tools for updates, and uses `pwck`/`grpck` for integrity checking (`/usr/share/doc/arch-wiki/html/en/Users_and_groups.html` — “User database”, “Automatic integrity checks”).

### M4 — Pure UI state and bounded responsiveness

**Objective:** Make rendering deterministic and keep the TUI responsive on large hosts.

- [ ] Split the 2,596-line update module into a pure reducer, modal reducers, effects, reconciliation, and result presentation.
- [ ] Give every pane independent selection/pagination keyed by stable identity; centralize post-transition normalization.
- [ ] Preserve selection through refresh/filter when its identity remains visible; otherwise choose a documented neighbor.
- [ ] Make all render functions read-only; remove modal/password clones and state mutation from render.
- [ ] Fix group-member pagination and small-terminal behavior.
- [ ] Cache username, shadow, home, SSH-key, process, and group diagnostics outside the 100 ms frame path with explicit refresh intervals.
- [ ] Bound input/query lengths, system/config file sizes, process scans, command duration/output, retained reports, and allocations.
- [ ] Replace clone/lowercase-heavy search with a measured design only where profiling shows benefit.

**Exit gate**

- Rendering performs no filesystem/process I/O.
- Property tests preserve all selection invariants across empty/filter/resize/refresh/delete transitions.
- Small terminals render a stable fallback.
- The D7 benchmark profile is recorded and the 10,000-user/10,000-group fixture meets its numeric p95, allocation, and I/O limits without wall-clock assertions in normal correctness tests.

```bash
cargo test --test ui_invariants --locked
cargo test --test ui_small_terminal --locked
cargo test --test resource_bounds --locked
cargo bench --bench search_and_render
```

### M5 — Deterministic reliability test suite

**Objective:** Replace passing-but-weak tests with meaningful failure coverage.

- [ ] Remove tautologies such as `is_ok() || is_err()` and the test-only username implementation.
- [ ] Verify the M1 binary/library consolidation: module tests execute once and no binary-local duplicate module tree remains.
- [ ] Convert parser/config/validation/reducer/operation cases to table-driven tests; add property tests where state space warrants them.
- [ ] Add deterministic command/data-source failure matrices, clocks, fixture roots, redaction assertions, and operation postconditions.
- [ ] Add focused Ratatui snapshots for classified errors, partial success, stale data, unknown shadow state, and small terminals.
- [ ] Keep latency measurements in benchmarks; correctness tests assert operation counts and explicit bounds.
- [ ] Ensure safe parallel execution without global thread-local providers or host state.

**Exit gate**

- Results are identical as root/non-root and across arbitrary host account/config state.
- Every operation step and error variant has a deterministic assertion.
- Tests require neither privileged tools nor serial execution.

```bash
cargo test --workspace --all-targets --all-features --locked
cargo test --doc --locked
```

### M6 — CI, supply-chain, diagnostics, and documentation gates

**Objective:** Make regressions fail closed before merge and keep support claims accurate.

- [ ] Declare MSRV and test MSRV plus stable.
- [ ] Gate format, Clippy `-D warnings`, build, tests, docs, all-features, no-default-features, and `--locked` modes.
- [ ] Add a reviewed `deny.toml` for advisories/licenses/sources; decide policy for unmaintained/unsound warnings and review the `paste`, `lru`, and `anyhow` paths.
- [ ] Review dependency upgrades and remove duplicate Crossterm/Rustix families when compatible; do not force an upgrade without tests.
- [ ] Pin GitHub Actions to full commit SHAs, minimize permissions, add cancellation and job timeouts.
- [ ] Add dependency/Actions update automation with lockfile review.
- [ ] Add structured, size-bounded, secret-redacted diagnostics and stable error codes; do not advertise logging until implemented.
- [ ] Correct README/security/contribution/issue-template claims: version, platform/NSS mode, confirmation coverage, privilege model, logging, tests, missing docs, and support fields.
- [ ] Validate account database integrity in disposable VM/container release tests where real privileged tools can be used safely; normal CI remains fake-runner only.

**Exit gate**

- Required PR checks are reproducible, least-privilege, immutable, and time-bounded.
- Advisory/license/source policy is committed and passes or has narrowly documented exceptions.
- Diagnostics are proven to redact both account and sudo passwords.
- User-facing documentation matches executable behavior.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --workspace --no-default-features --locked
cargo doc --workspace --all-features --no-deps --locked
cargo audit
cargo deny check
cargo tree --duplicates
```

## Old-plan disposition

| Superseded item | Disposition here |
|---|---|
| Parser tests | Present but insufficient; replaced by M0/M3/M5 because malformed IDs currently become 0. |
| Update-loop tests | Partially present; replaced by injected reducer/effect and targeting/failure tests in M0–M5. |
| CI setup | Basic build/test exists; expanded into fail-closed M6 gates. |
| User/group CRUD | Mostly implemented; reliability of current paths is handled in M1–M3 rather than tracked as a new feature. |
| Password set/reset | Implemented but unsafe/partial; handled in M1–M3. Strength policy remains a separate product decision. |
| Search and filters | Substring search exists; shadow filters/persistence are unreliable and handled in M3/M4. Fuzzy search is deferred polish. |
| 10,000-item/50 ms claim | Replace the inconsistent 100 ms wall-clock test with explicit bounds and benchmarks in M4/M5. |
| NSS/BSD/file-parse support | Claim was stale; resolve through D1/M3 before documenting support. |
| Backups/audit log/one-click rollback | Deferred; requires retention/privacy/recovery policy after operation reports are reliable. |
| Bulk, profiles, SSH keys, remote hosts, hooks/plugins, CSV/JSON, home skeleton/dotfiles/quotas | Product features; outside this reliability plan. |
| Sorting/keybinding scope and PAM/`login.defs`/orphaned-home health checks | Deferred product/diagnostic design; require a separate approved feature plan after M6 diagnostics are stable. |
| README “Change Permissions easily” future item | Unscoped product idea; not reliability work. |

## Independent review disposition

Two fresh-context reviewers checked this plan against the repository. Findings are advisory; the maintainer/integration owner retains final decisions.

| Finding | Disposition | Evidence/rationale |
|---|---|---|
| Group deletion lacks protected-identity handling and GID 0 bypasses its warning | **accepted** | Verified in `src/app/update.rs:1408-1453`, `2109-2117` and `src/ui/groups.rs:410-425`; added P1/M1 coverage. |
| Baseline date could be read as the commit date | **accepted** | `0b154c1` is HEAD but its commit date is 2026-04-12; header now separates commit and inspection dates. |
| Empty-feature Cargo citation ended before line 26 | **accepted** | Corrected to `Cargo.toml:24-26`. |
| Host-state independence/no-real-tools gates were not enforced by one `cargo test` command | **accepted** | Added fake-runner executable enforcement and isolated UID/HOME/account/proc fixture cases to M0; M6 owns CI matrix execution. |
| M4 performance gate lacked a numeric owner/budget | **accepted** | Added D7 and made the M4 exit gate depend on recorded numeric limits. |
| Secret scan method was unspecified | **accepted** | Baseline now names its bounded tracked-file grep method and its limitation. |
| Four existing Clippy findings were hidden work under the M1 gate | **accepted** | Added an explicit M1 cleanup item. |
| `sudo -n` assumes a usable timestamp cache | **accepted with modified remediation** | The assumption is valid; fail closed with a typed D5 result when policy disables it. The suggested automatic mixed-stdin fallback was not adopted because its credential/payload boundary needs separate proof. |
| M1/M5 repeated binary/library consolidation | **accepted** | M1 performs the change; M5 now verifies it. |
| Sorting, home-management, and PAM/`login.defs` items lacked explicit disposition | **accepted** | Added explicit deferred/out-of-scope rows. |

### Integrated implementation review — round 1 dispositions (2026-07-31)

Review run `d929e71d-80e3-46a2-ad98-ca6c968517f0` produced two fresh-context outputs, but runtime evidence shows both resolved to `openai-codex/gpt-5.6-sol`; therefore they are independent outputs but **do not yet satisfy provider diversity**. Source artifacts: [`../reviews/reliability-review-anthropic.md`](../reviews/reliability-review-anthropic.md) and [`../reviews/reliability-review-moonshot.md`](../reviews/reliability-review-moonshot.md).

| ID | Disposition | Evidence/rationale |
|---|---|---|
| R1-1 / R2-F1 — multi-step actions are separate plans and downstream steps are dropped | **accepted** | Verified in `operation_requests`, `prepare_next_request`, and `execute_pending_plan`; opaque password postconditions force a partial report and clear queued required work. Fix requires one composite bridge-owned plan/report. |
| R1-2 / R2-F2 — production retry/idempotency preconditions are absent | **accepted** | `.require(...)` is used only by a synthetic test. Real membership/create/modify requests do not skip already-satisfied work after reconciliation. |
| R1-3 — sudo elevation grant is cached beyond one operation | **accepted** | `SystemAdapter.grant` survives operation completion and causes expired timestamp capability errors without consuming a new one-shot secret. |
| R1-4 — non-root protected-identity policy is absent | **accepted** | Only UID/GID 0 is blocked; D4 also requires an explicit injected policy for service identities. Implement one source of truth with root unconditional. |
| R1-5 / R2-F4 — partial terminal initialization cleanup is incomplete | **accepted** | Alternate-screen and mouse acquisition share one `execute!`; failure can unwind only raw mode. Cleanup errors are discarded and no failure-injection target exists. |
| R1-6 — password material can remain in general pending application state | **accepted** | `PasswordRecord` is zeroizing/redacted but can remain in `pending_requests`/`pending_plan` through interactive confirmation. Move secrets into a shortest-lived one-shot capability and drop on cancel. |
| R1-7 / R2-F5 — child cleanup errors are ignored or bypassed | **accepted** | `kill_and_reap` ignores kill/wait results and several post-spawn `?` paths do not guarantee checked finalization and reader joins. |
| R1-8 — valid observed passwd records with an empty shell are dropped | **accepted** | Mutation `ShellPath` validation is incorrectly reused for observed data. Preserve an empty observed shell/default distinctly while retaining strict mutation validation. |
| R1-9 / R2-F3 — shadow unknown/must-change semantics are incorrect | **accepted** | Source state has no per-account `Unknown`; missing entries appear `known`, dependent filters can no-op, and last-change `0` is not treated as must-change. |
| R1-10 / R2-F6/F7 — file, diagnostic, modal, and render work is insufficiently bounded | **accepted** | Multiple `read_to_string` calls are unbounded, modal rendering formats all candidates, group diagnostics include nested scans, and the supplied benchmark measures neither rendering nor p95. |
| R1-11 / R2-F10 — clean CI lacks rustfmt/Clippy components | **accepted** | Workflow installs `--profile minimal` then invokes missing components. Add explicit components and retain immutable action pins. |
| R1-12 / R2-F9 — manifest metadata contradicts D1/MSRV | **accepted** | Empty `file-parse` remains and `rust-version` is missing despite tested 1.89.0 policy. |
| R2-F8 — pane selections are index-based/incompletely normalized | **accepted** | Top-level identities are preserved opportunistically, but member-pane state is not identity-keyed and user-group selection is not normalized after transitions. |
| R2-F11 — transitional public mutation/secret facade bypasses plan/report contracts | **accepted** | `with_sudo_password` and direct `Result<()>` mutation methods remain public after app migration. Remove or restrict them to trusted internal/test compatibility. |
| R2-F12 — config diagnostics and atomic-failure tests overclaim coverage | **accepted** | Unknown/invalid entries are mostly ignored; the named interrupted-write test does not inject interruption and keymap equality is not fully asserted. |
| R2-ME1 — repository-root `plan.md`/`progress.md` absent | **rejected** | The repository convention is this canonical file under `plans/planned/`; session progress is managed by Pi's checklist. No root-level duplicate is required. |
| R2-ME2 — named targeting/terminal/shadow/atomic/small-terminal targets absent | **accepted** | Some behavior is covered elsewhere, but the plan's exact acceptance commands cannot run. Add/rename focused targets rather than claiming equivalence. |
| R2-ME3 — no quantitative performance evidence | **accepted** | Current bench runs zero measured tests and does not render or include 10,000 groups. |
| R2-ME4 — request/error/failure matrices, properties, and snapshots incomplete | **accepted** | Current tests cover representative paths, not every operation/error/transition boundary required by M2/M5. Add focused deterministic matrix coverage; use property/snapshot tooling only where it materially strengthens invariants. |
| R2-ME5 — suite-wide no-real-tool enforcement absent | **accepted** | Fake tests are present but no structural CI guard rejects future production-runner/process construction in normal tests. |
| R2-ME6 — real-tool and PTY release evidence absent | **accepted; external gate pending** | Must be obtained only in an explicitly approved disposable environment. Normal CI remains fake-runner only; no host mutation is authorized. |
| R2-ME7 — supply-chain warning/duplicate dispositions incomplete | **accepted with deferred upgrades** | Current commands pass with warnings. Record owner/reachability/expiry for narrow exceptions; do not force incompatible upgrades without tests. |
| R2-ME8 — final report/review quorum/dispositions absent | **accepted; in progress** | These dispositions address round 1, but provider-diverse review, final report, and archival remain gated on accepted fixes and revalidation. |

**Accepted-fix DAG:** `W3 trusted-core hardening` owns composite operations, idempotency, elevation lifetime, protected policy, observed account/shadow types, child lifecycle, bounds, and trusted tests. `W4 application/release hardening` starts after W3 and owns app secret/selection/composition integration, terminal/config/render/resource/benchmark tests, CI/metadata/policy/docs, and exact named acceptance targets. Both remain sequential in the dirty shared worktree and may not edit this canonical plan.

### Final review and integration disposition (2026-08-01)

The bounded review gate returned **2/2 qualifying read-only successes from distinct provider families** after one non-qualifying provider-diversity attempt. Runtime metadata—not artifact names—identifies the qualifying reviewers:

- `00732c51-3eb1-4107-9bae-ad9006c52a11` — Anthropic `claude-sonnet-4-5`, high thinking; artifact [`../reviews/reliability-final-anthropic.md`](../reviews/reliability-final-anthropic.md).
- `3b4a5a53-e8fd-41d1-afbd-c067877c7c84` — Moonshot `kimi-k3`, high thinking; artifact [`../reviews/reliability-final-moonshot.md`](../reviews/reliability-final-moonshot.md).

| Final finding | Disposition | Evidence/rationale |
|---|---|---|
| All round-1 blocker/high/medium code findings | **accepted fixes verified** | Both qualifying final reviews found no blocker and independently rechecked composites, retry, grant lifetime, protected identities, secret transport, cleanup, shadow, bounds, config, CI, and metadata against the actual worktree. |
| Dense per-frame user-membership scan | **accepted and fixed** | Added bounded `AppState.user_group_gids` precomputation; `src/ui/users.rs` now consumes the cache. Revalidation passed full fmt/Clippy/tests and a dense 100,000-membership-edge release benchmark: search p95 10.197 ms and render p95 0.206 ms. |
| Static guard covered integration tests but not source unit tests | **accepted and fixed** | `tests/core_static_guards.rs` now scans `#[cfg(test)]` source sections for production-runner/direct account-tool construction while allowing only the documented benign current-test helper. Focused and full suites pass. |
| Literal root-host rerun absent | **rejected as a required host action** | Identity behavior is exercised through injected root/non-root/error providers and normal tests are required to be host-independent. Running the suite as host root would add risk without improving the trusted seam evidence. |
| Supply-chain warnings and duplicate dependency families | **deferred, accepted risk** | `cargo deny check` passes; `cargo audit` reports the documented `paste`, Ratatui `lru`, and target-specific dev `anyhow` warnings. Owner/reachability/review expiry are recorded; incompatible upgrades remain evidence-driven. |
| Hosted CI/actionlint execution absent | **external verification pending** | Updated workflows install required components, use SHA pins, minimal permissions, cancellation, and timeouts; every local command passes. `actionlint` is unavailable and no hosted run exists in this worktree-only session. |
| Disposable real account-tool/database validation absent | **external verification pending; blocks archival** | Docker is available but no image pull, privileged container, account mutation, or integrity check was started without explicit external-side-effect approval. Normal CI remains fake-runner only. |
| Real PTY failure-injection evidence absent | **external verification pending; blocks archival** | Injected terminal acquisition/cleanup matrices pass without touching the host terminal. A real PTY run must use explicitly approved disposable infrastructure. |

### Current integrated evidence

- `cargo fmt --all -- --check`, locked all-target check/test, no-default tests, docs, MSRV 1.89 tests, and full Clippy `-D warnings`: pass.
- Exact acceptance targets for account parsing, command contracts, action targeting, terminal cleanup, operation reports/retry/reconciliation/dry-run, shadow status, config round-trip/atomicity, UI invariants/small terminals, resources, redaction, and static guards: pass.
- `cargo bench --bench search_and_render -- --nocapture`: pass on 10,000 users, 10,000 groups, and 100,000 membership edges; search p95 10.197 ms ≤ 50 ms, render p95 0.206 ms ≤ 16 ms.
- `cargo deny check`: pass. `cargo audit` and duplicate-tree warnings are dispositioned above.
- Final self-contained report: [`../../reports/reliability-robustness.html`](../../reports/reliability-robustness.html).

The locally executable code gates and provider-diverse review gate are satisfied. This plan intentionally remains under `plans/planned/`: the disposable real-tool/database, real PTY, and hosted-workflow evidence is not present, so the completion and archival criteria are not yet met.

## Suggested module boundaries

Apply incrementally; do not perform a single mass rewrite.

- `src/terminal.rs` — RAII terminal lifecycle.
- `src/sys/{data_source,command,validation,operations,error}.rs` — trusted boundaries and reports.
- `src/app/{reducer,effects,selection}.rs` — pure transitions, effects, and invariants.
- `src/config/{mod,atomic,theme,filters,keymap}.rs` — shared durable config.
- `tests/common/` plus fixture and fake-runner modules — deterministic test infrastructure.

## Risks and controls

| Risk | Control |
|---|---|
| Compensation causes more damage than the original failure | Default to reconcile/report; compensate only known reversible membership changes. |
| Refactor changes privileged behavior while improving structure | Characterization tests and command snapshots precede behavior changes. |
| Distribution-specific account tools differ | Use fake contracts in CI and disposable distro VM/container validation before release. |
| Secret wrappers imply stronger guarantees than possible | State limits honestly; test argv/log/debug redaction and minimize copies/lifetime. |
| Config migration destroys customization | Preserve legacy read compatibility, back up in tests, and prove atomic round trips. |
| PTY/performance tests become flaky | Assert invariants/resource bounds; isolate benchmarks from correctness tests. |
| Scope expands into old roadmap features | Enforce the non-goals above and require a separate approved feature plan. |

## Completion criteria

This plan is complete only when:

- All M0–M6 checkboxes and exit gates pass on the supported platform matrix.
- No P0/P1 finding remains open without an explicit accepted risk and evidence.
- Real-tool behavior has been validated in disposable environments without exposing secrets.
- Every review finding is dispositioned with evidence.
- Final documentation and package metadata match observed behavior.
- This file is moved from `plans/planned/` to `plans/archive/`; `plans/archive/` remains Git-ignored.

## Evidence confidence

**94/100.** Source, tests, workflows, manifests, dependencies, configs, and old plans were inspected; build/test/doc/lint/audit commands were run. Confidence is reduced because destructive account commands were intentionally not executed and platform/elevation/recovery decisions remain open. Local operational references: `/usr/share/doc/arch-wiki/html/en/Users_and_groups.html` and `/usr/share/doc/arch-wiki/html/en/Sudo.html`.
