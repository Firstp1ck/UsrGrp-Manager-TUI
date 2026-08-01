# W2 Application-Integration Handoff

## 1. Workstream status and revision

- **Workstream/run:** W2 application-integration implementation; sequential writer 2 of 2
- **Status:** Implemented within the W2 boundary. All required build/test/doc/config/resource checks pass except the full all-target Clippy gate, which is blocked solely by three lint findings in W1-owned focused tests that this workstream is forbidden to edit.
- **Base/resulting revision:** `0b154c1c3a6a889b495ab50c268b5c3ae491087a` / uncommitted shared worktree (no commit created)
- **Artifact path:** `plans/handoffs/reliability-integration.md`
- **Plan/context validation:** `context.md` and `plan.md` remain absent. The canonical reliability plan and W1 handoff/actual bridge contracts were inspected. The inherited complex classification remains valid because this work spans trusted actions, state/effects, rendering, config durability, terminal lifecycle, tests, CI, diagnostics, and documentation.

## 2. Changed files and implementation summary

### Application, action bridge, state, rendering, and terminal

- `src/app/mod.rs`, `src/app/update.rs` — made default `AppState::new` pure and deterministic; added injected `with_adapter`, explicit system/config/account refresh effects, stale snapshot retention, cached diagnostic state, independent users/member-of/groups/member selections, redacted pending previews/reports, and zeroizing, non-`Clone`/non-`Debug` `SecretInput` modal input. All action paths now translate only into W1 `OperationRequest` values; they call `prepare_operation`, render the exact redacted plan before confirmation, call `execute_prepared_operation`, prompt only for `CoreError::AuthenticationRequired`, pass one-shot `SecretString` through `set_elevation_secret`, refresh cached data, and render honest completed/failed/unavailable reconciliation reports. No app/UI/main source retains or references the legacy sudo-password facade.
- `src/search.rs` — made filtering pure/cached, bounded search input to 256 bytes, preserved selection by stable UID/GID, and introduced explicit one-refresh shadow `Known`/`Unavailable` data rather than metadata guessing or silent filter no-ops.
- `src/ui/{mod,components,users,groups}.rs` — changed render entry points to immutable `&AppState`; removed all filesystem/process I/O; use cached home/shadow/group data; added small-terminal fallback, independent pane pagination, stale/shadow/config state presentation, and redacted operation confirmation/result modals.
- `src/terminal.rs`, `src/main.rs`, `src/lib.rs` — added RAII terminal ownership that cleans partial initialization, normal return, error, and panic unwinding; `main` now consumes the library module tree, reports failure with non-zero exit, and explicitly reports cleanup errors.

### Durable configuration and diagnostics

- `src/config/mod.rs` — added shared assignment parsing and same-directory restricted temporary write, flush/sync, atomic rename, directory sync, and existing-symlink refusal.
- `src/app/{filterconf,keymap}.rs` and `src/app/mod.rs` theme support — made theme/filter/keymap parse/write APIs return surfaced I/O/parse errors, use atomic writes, preserve supported fields, serialize actual filter enums, serialize all active key bindings, and round-trip RGB/indexed/named/reset colors. Configuration errors are retained in UI state rather than ignored.
- Application-visible classified diagnostics now use bounded stable `E-*` codes (authentication, validation, execution, refresh, partial, postcondition, etc.) backed by W1 non-secret `CoreError` values.

### Tests, benchmarks, CI, policy, and docs

- Replaced host-dependent/tautological `tests/{unit_test,integration_test}.rs` with pure deterministic construction/render/search/config tests; added `tests/{config_roundtrip,ui_invariants,resource_bounds}.rs`; added an app-level bridge/modal test in `src/app/update.rs`; and added `benches/search_and_render.rs` compile benchmark target. Tests do not spawn processes or invoke privileged tools.
- `.github/workflows/{rust,doc-rust}.yml`, `.github/dependabot.yml`, `deny.toml` — added locked CI checks for format, all-target/all-feature tests, no-default-features, docs, Clippy, MSRV 1.89.0, dependency policy/audit/tree; least-privilege read permissions, full-SHA checkout pin, cancellation, and time bounds; reviewed dependency update automation and policy.
- `README.md`, `SECURITY.md`, `CONTRIBUTING.md`, `example-configs/*`, `.github` PR/issue templates — corrected Linux-local, elevation, authentication, secret, test, configuration, and security-reporting claims.

The pre-existing W1 core files, pre-existing dirty `.gitignore`, deleted superseded docs, and canonical plan were preserved. No W1-owned source/test/fixture was edited.

## 3. Tests added or updated

- `src/app/update.rs` — exact redacted plan is prepared before authentication prompt; pure index/report checks.
- `tests/config_roundtrip.rs` — theme (indexed and named colors), filter enum, keymap full-binding round trips; atomic replacement and symlink refusal.
- `tests/ui_invariants.rs` — immutable render state and small-terminal fallback.
- `tests/resource_bounds.rs` — bounded query behavior without host I/O.
- `tests/integration_test.rs` — injected pure state/render and stable-identity search.
- `tests/unit_test.rs` — pure app construction, deterministic shadow parsing, indexed theme, and keymap checks.
- `benches/search_and_render.rs` — release benchmark target over 10,000 synthetic users.

The final all-target/all-feature run executed 48 tests successfully (plus zero doc tests and zero benchmark harness tests). W1’s 26 focused trusted-core tests, including `operation_bridge`, also passed in that run.

## 4. Commands, exit codes, and validation evidence

| Command | Exit | Result |
|---|---:|---|
| `cargo fmt --all -- --check` | 0 | Final formatting check passed. |
| `cargo clippy --workspace --lib --bins --all-features --locked -- -D warnings` | 0 | W2 production source lint passed with warnings denied. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 101 | Failed only in forbidden W1 tests: `tests/partial_failure.rs:44` redundant pattern match; `tests/operation_bridge.rs:174` redundant pattern match and `:213` unnecessary lazy evaluation. No W2 source/test lint finding remains. |
| `cargo test --workspace --all-targets --all-features --locked` | 0 | Final full suite passed: 48 tests, including all W1 focused test targets, new W2 tests, and benchmark target compile/run harness. |
| `cargo test --workspace --no-default-features --locked` | 0 | Passed. |
| `cargo test --doc --locked` | 0 | Passed (zero doc tests). |
| `cargo doc --workspace --all-features --no-deps --locked` | 0 | Documentation built. |
| `cargo bench --bench search_and_render --no-run --locked` | 0 | Release benchmark target compiled. |
| `cargo +1.89.0 test --workspace --all-targets --all-features --locked` | 0 | MSRV full suite passed. |
| `cargo +1.85.0 check --workspace --all-targets --all-features --locked` | 101 | Initial proposed MSRV invalid: locked `darling`/`instability` require Rust 1.88. MSRV was accurately changed to tested 1.89.0 in docs/CI. |
| `cargo deny check` | 0 | Policy passed. It reports reviewed duplicate transitive dependency warnings. |
| `cargo audit` | 0 | Completed with three reported allowed warnings: unmaintained `paste` via Ratatui; `lru` IterMut and target-specific dev `anyhow` advisories. |
| `cargo tree --duplicates` | 0 | Reported existing Crossterm/Rustix/unicode-width duplicate families; no forced dependency upgrade was made. |
| static UI/test/credential scans | 0 | No `src/ui` filesystem/process calls; no W2 `sudo_password`/`with_sudo_password`; no normal-test `Command::new`/`std::process::Command`. |
| `git diff --check && git diff --cached --name-only` | 0 | No whitespace error and no staged files. |

**Omissions:** No real privileged account command, sudo, host mutation, deployment, publication, destructive Git operation, or disposable VM/container real-tool test was run. No PTY/failure-injection terminal test was run; terminal cleanup is covered by RAII implementation review and normal compile/test evidence only. `actionlint` was not installed/run.

## 5. Deviations, assumptions, unresolved decisions, and residual risks

1. **Full Clippy gate remains blocked (ownership-constrained).** The only failures are three new/current lint diagnostics in W1-owned `tests/partial_failure.rs` and `tests/operation_bridge.rs`. W2 is expressly forbidden to edit W1 focused tests. The production-source Clippy gate passes; integration owner/W1 must apply the trivial test-only lint fixes before treating the all-target Clippy gate as green.
2. **MSRV changed from the obsolete 1.85.0 proposal to 1.89.0.** Direct 1.85 validation showed locked dependencies require Rust 1.88. No manifest/lock change was permitted or made; 1.89.0 is documented and fully tested locally/CI.
3. **Audit warnings remain reviewed residuals.** `paste` is an unmaintained Ratatui transitive dependency with no safe upgrade in the locked graph; `lru` is transitive and the reported `IterMut` path is not used here; `anyhow` is reached through Tempfile’s target-specific WASI test dependency. `cargo audit` reports them, and the committed deny policy narrowly ignores only the active `paste` advisory. Re-evaluate on Ratatui/Tempfile dependency review.
4. **Duplicate dependency families remain.** `cargo tree --duplicates` reports direct Crossterm 0.29 alongside Ratatui’s 0.28 tree. The plan prohibits forcing upgrades without compatibility evidence; this remains a review item.
5. **Shadow semantics are honest but bounded.** A refresh reads shadow once. Unavailable/partial shadow data is shown as unavailable and dependent filters do not claim complete application. Password/expiry execution remains partial/unavailable as specified by W1 because the local account source has no shadow postcondition reader.
6. **Real privilege lifecycle remains deliberately unexecuted.** Fakes/static guards prove normal tests do not reach account tools. A future release gate still needs disposable Linux VM/container validation and PTY cleanup failure injection.

## 6. Integration notes and next steps

- Use only `OperationRequest`, `SystemAdapter::prepare_operation`, `execute_prepared_operation`, `refresh_state`, and one-shot `set_elevation_secret` for future mutations. Do not reintroduce adapter legacy mutation helpers, runner/grant exposure, or stored credentials.
- `AppState::new`/`with_adapter` are deterministic test seams; `AppState::load_system` is the explicit runtime-effect constructor.
- Rendering accepts immutable cached state. New diagnostics must be produced by an explicit refresh/effect path and bounded before display.
- The integration owner should request a W1-owned micro-fix for the three test-only full-Clippy findings, rerun the exact full Clippy command, and then proceed with the required independent reviewer gate. No W2 source change is needed for that fix.

**Confidence: 91/100.** Direct final evidence covers locked builds, full/no-default/MSRV test matrices, docs, benchmark compilation, policy/audit/tree, static guards, exact bridge integration, config durability, and no-staged-file state. Confidence is reduced by the W1-owned all-target Clippy blocker, audit/duplicate dependency residuals, and intentionally absent real privileged/PTY disposable-environment tests.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "W2 changes stay within its authorized app/config/terminal/lib/search/UI/test/bench/CI/doc boundary. App code consumes only the approved adapter-owned OperationRequest bridge, removes stored sudo credentials, makes rendering immutable/I/O-free, and preserves Linux-local existing mutation capabilities without adding product scope."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "This handoff records base state, changed paths, deterministic tests, every material command/exit code, exact full-Clippy ownership blocker, static safety scans, dependency/audit outcomes, omissions, residual risks, and final no-staged-files evidence."
    }
  ],
  "changedFiles": [
    ".github/ISSUE_TEMPLATE/bug_report.md",
    ".github/ISSUE_TEMPLATE/custom.md",
    ".github/ISSUE_TEMPLATE/feature_request.md",
    ".github/dependabot.yml",
    ".github/pull_request_template.md",
    ".github/workflows/doc-rust.yml",
    ".github/workflows/rust.yml",
    "CONTRIBUTING.md",
    "README.md",
    "SECURITY.md",
    "benches/search_and_render.rs",
    "deny.toml",
    "example-configs/filter.conf",
    "example-configs/keybinds.conf",
    "example-configs/theme.conf",
    "src/app/filterconf.rs",
    "src/app/keymap.rs",
    "src/app/mod.rs",
    "src/app/update.rs",
    "src/config/mod.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/search.rs",
    "src/terminal.rs",
    "src/ui/components.rs",
    "src/ui/groups.rs",
    "src/ui/mod.rs",
    "src/ui/users.rs",
    "tests/config_roundtrip.rs",
    "tests/integration_test.rs",
    "tests/resource_bounds.rs",
    "tests/ui_invariants.rs",
    "tests/unit_test.rs",
    "plans/handoffs/reliability-integration.md"
  ],
  "testsAddedOrUpdated": [
    "src/app/update.rs",
    "tests/config_roundtrip.rs",
    "tests/integration_test.rs",
    "tests/resource_bounds.rs",
    "tests/ui_invariants.rs",
    "tests/unit_test.rs",
    "benches/search_and_render.rs"
  ],
  "commandsRun": [
    {
      "command": "cargo fmt --all -- --check",
      "result": "passed",
      "summary": "Final formatting check passed."
    },
    {
      "command": "cargo clippy --workspace --lib --bins --all-features --locked -- -D warnings",
      "result": "passed",
      "summary": "W2 production source is warning-free."
    },
    {
      "command": "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
      "result": "failed",
      "summary": "Only three forbidden W1 focused-test lint findings remain in tests/partial_failure.rs and tests/operation_bridge.rs."
    },
    {
      "command": "cargo test --workspace --all-targets --all-features --locked",
      "result": "passed",
      "summary": "48 deterministic tests passed without privileged host tools."
    },
    {
      "command": "cargo test --workspace --no-default-features --locked",
      "result": "passed",
      "summary": "Passed."
    },
    {
      "command": "cargo test --doc --locked && cargo doc --workspace --all-features --no-deps --locked",
      "result": "passed",
      "summary": "Doc tests and documentation build passed."
    },
    {
      "command": "cargo bench --bench search_and_render --no-run --locked",
      "result": "passed",
      "summary": "Release benchmark target compiled."
    },
    {
      "command": "cargo +1.89.0 test --workspace --all-targets --all-features --locked",
      "result": "passed",
      "summary": "Documented MSRV full suite passed."
    },
    {
      "command": "cargo deny check && cargo audit && cargo tree --duplicates",
      "result": "passed",
      "summary": "Policy passed; audit/duplicate warnings are documented residual risks."
    },
    {
      "command": "static UI I/O, credential, privileged-test scans; git diff --check; git diff --cached --name-only",
      "result": "passed",
      "summary": "No UI filesystem/process calls, no W2 legacy sudo credential path, no normal-test process spawn, no whitespace errors, and no staged files."
    }
  ],
  "validationOutput": [
    "All locked full/no-default/MSRV tests passed.",
    "No normal test can construct a process command or invoke a privileged tool.",
    "Full all-target Clippy is not green solely because W1-owned focused tests require trivial lint cleanup outside W2 authority."
  ],
  "residualRisks": [
    "W1-owned full-Clippy test findings require an owner-approved follow-up.",
    "cargo audit reports paste, lru, and target-specific anyhow warnings; cargo tree reports existing duplicate dependency families.",
    "No real privileged command/disposable environment/PTY failure-injection test was run."
  ],
  "noStagedFiles": true,
  "diffSummary": "Integrates the approved trusted operation bridge across pure application state/effects, immutable cached rendering, atomic durable configuration, RAII terminal cleanup, deterministic coverage, least-privilege CI/policy, diagnostics, and corrected documentation.",
  "reviewFindings": [
    "blocker: tests/partial_failure.rs:44 and tests/operation_bridge.rs:174,213 - full Clippy fails in W1-owned focused tests; W2 cannot edit them.",
    "no W2-source blocker found by locked compile, source Clippy, full tests, MSRV, docs, benchmark, static safety scans, or diff hygiene."
  ],
  "manualNotes": "Pre-existing dirty .gitignore, W1 trusted-core work, and deleted superseded docs were preserved. No files are staged."
}
```