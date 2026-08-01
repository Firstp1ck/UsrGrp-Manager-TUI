# W1 Trusted-Core Handoff

- **Workstream/run:** W1 trusted-core implementation; sequential writer 1 of 2
- **Status:** Implemented within the W1 write boundary; focused validation passes. Repository-wide test validation is blocked by three legacy integration-test expectations that conflict with the approved W1 D4/D8 behavior and cannot be changed in this workstream.
- **Base/resulting revision:** `0b154c1c3a6a889b495ab50c268b5c3ae491087a` / uncommitted worktree (no W1 commit created)
- **Artifact path:** `plans/handoffs/reliability-core.md`

## Plan validation

`context.md` and `plan.md` requested by the task are absent. The canonical `plans/planned/reliability-robustness.md` is present and its recorded D1-D9 support this implementation: Linux-local account files/tools only; checked effective UID; D8 validation limits; one-shot sudo validation followed by `sudo -n`; no shell pipeline/password argv; no destructive compensation; and the D9-approved `libc` and `zeroize` dependencies. The inherited complex classification remains justified because the change spans account parsing, identity/elevation, process lifecycle, secret handling, operation semantics, compatibility, and deterministic integration tests.

## Changed files

- `Cargo.toml`, `Cargo.lock` — declared D9-approved `libc` and `zeroize` (lock resolves `zeroize 1.9.0`).
- `src/error.rs` — added classified, non-secret `CoreError` and `CoreResult` types.
- `src/sys/mod.rs` — rebuilt the legacy facade over the injectable trusted boundary while preserving safe application compatibility.
- `src/sys/validation.rs` — D8 `UserName`, `GroupName`, `ShellPath`, `Gecos`, non-`Debug`/non-`Clone` zeroizing `SecretString`, and `PasswordRecord`.
- `src/sys/data_source.rs` — typed Linux-local passwd/group/shell parsing with bounded source/line diagnostics, injectable source, and stale-snapshot retention.
- `src/sys/identity.rs` — injectable checked *effective* UID provider using Linux `geteuid(2)`; failure is never root.
- `src/sys/command.rs` — fixed-program typed command contracts, typed arguments, redacted previews, one-shot sudo authentication, direct `chpasswd` stdin records, bounded output, timeout/kill/reap, and typed result classification.
- `src/sys/operations.rs` — stable targets, plans, shared dry-run previews, step reports, reconciliation status, idempotent precondition skipping, and no automatic compensation.
- `tests/common/mod.rs`, `tests/fixtures/{passwd,group,shells}` — deterministic fixture source/identity and non-spawning fake runner.
- Focused tests: `tests/account_parsing.rs`, `tests/app_construction.rs`, `tests/command_contracts.rs`, `tests/dry_run_equivalence.rs`, `tests/operation_reports.rs`, `tests/partial_failure.rs`, `tests/reconciliation.rs`, `tests/secret_redaction.rs`.

No forbidden source/application/UI/config/workflow/documentation path was edited. Pre-existing dirty `.gitignore`, deleted `docs/Improvements.md`/`docs/roadmap.md`, and canonical plan changes were preserved untouched.

## Implementation summary

- The public `sys` boundary now exposes injectable `AccountDataSource`, `IdentityProvider`, and `CommandRunner` traits plus concrete Linux-local implementations and deterministic fixed/fake seams.
- Account-file parse failures produce diagnostics and omitted malformed records; malformed UID/GID fields cannot become UID/GID 0. Explicit successful `0` fields remain valid root identities.
- `SystemIdentityProvider` uses `geteuid`, so it observes effective—not real—UID and has no `/proc` parsing or root fallback.
- Command invocation is closed over `KnownProgram`; public builders accept reviewed fixed arguments or validated user/group/shell/GECOS types. `bash`, `echo`, free-form programs, and password-bearing argv are absent.
- `SecretString` and `PasswordRecord` do not implement `Debug`, `Display`, or `Clone`; secrets are zeroized on drop. Password records are written only as `username:password\n` to `chpasswd` stdin. `sudo -v` consumes a one-shot secret, then command execution uses `sudo -n`.
- Process execution has a non-zero timeout/output bound, checks spawn/stdin/wait/kill/reap/output-reader failures, kills and reaps timeouts, joins output readers after reap, and retains bounded output only.
- `OperationPlan` previews and execution use the same typed steps. `OperationReport` distinguishes completed, skipped, failed, compensated (always empty unless an explicit future D3-approved executor fills it), and reconciliation state. Retry preconditions skip already-satisfied work; no user/group/home deletion is used as compensation.
- The compatibility `SystemAdapter` is injected in tests, protects root UID/GID targets, and retains a transitional `with_sudo_password` facade for W2. It converts that compatibility string into a one-shot zeroizing secret immediately; W2 remains responsible for removing the long-lived app/modal password state.

## Tests added/updated

21 focused tests pass, including:

- malformed IDs are diagnosed rather than coerced to root; validated passwd/group/shell fixtures;
- injected account/identity construction and unknown-identity fail-closed behavior;
- only fake runner command observation in normal tests, redacted password stdin contracts, no `bash`/`-c` previews, and distinct authentication-required errors;
- shared preview/execution equivalence; ordered completed/failed/skipped reports; explicit partial/unavailable reconciliation; idempotent precondition skip; stale refresh retention; and password delimiter/redaction rules.

No focused W1 test instantiates `LocalCommandRunner` or invokes `sudo`, `useradd`, `usermod`, `userdel`, `groupadd`, `groupmod`, `groupdel`, `gpasswd`, `chpasswd`, or `chage` on the host.

## Commands run

| Command | Exit | Result / note |
|---|---:|---|
| `cargo fmt --all && cargo check --workspace --all-targets --all-features --locked` | 101 | Expected immediately after manifest edit: `--locked` correctly refused the stale lockfile. |
| `cargo check --workspace --all-targets --all-features` | 101 | Updated lockfile with `zeroize`; then exposed and corrected an implementation-only invalid `Copy` derive. |
| `cargo fmt --all && cargo check --workspace --all-targets --all-features` | 0 | Compiled after the derive fix; warning-only legacy binary module duplication. |
| `cargo fmt --all && cargo fmt --all -- --check && for test in account_parsing command_contracts operation_reports partial_failure reconciliation dry_run_equivalence secret_redaction app_construction; do cargo test --test "$test" --locked || exit $?; done` | 101 | Focused run exposed a W1 test-only fixed reconciler postcondition bug; corrected. |
| `cargo fmt --all && for test in operation_reports partial_failure reconciliation dry_run_equivalence secret_redaction app_construction; do cargo test --test "$test" --locked || exit $?; done` | 101 | Focused run exposed a test compile issue caused by intentionally non-`Debug` `CommandSpec`; corrected test to pattern-match. |
| `cargo fmt --all && for test in secret_redaction app_construction; do cargo test --test "$test" --locked || exit $?; done` | 0 | Focused tests passed. |
| `cargo fmt --all && cargo fmt --all -- --check && cargo check --workspace --all-targets --all-features --locked` | 0 | Required formatting/check pass after final trusted-core hardening. |
| `for test in account_parsing command_contracts operation_reports partial_failure reconciliation dry_run_equivalence secret_redaction app_construction; do cargo test --test "$test" --locked || exit $?; done` | 0 | All eight focused targets passed (21 tests). |
| `cargo test --workspace --all-targets --all-features --locked` | 101 | 17 lib + 17 binary module tests and all 21 W1 focused tests passed. Three existing `tests/integration_test.rs` assertions failed; see residual risks. No privileged tool was executed. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 101 | Exactly the four documented baseline findings in untouched `src/app/update.rs` (2 `collapsible_match`) and `src/search.rs` (2 `unnecessary_min_or_max`) blocked Clippy. |
| `git diff --check` | 0 | No whitespace errors. |
| `git diff --cached --name-only` | 0 | No staged files. |

**Omissions:** no privileged host account command, deployment, publication, destructive rollback, or host-account mutation was run. No `cargo doc`, audit, deny, benchmark, or external/disposable-environment command was requested for this W1 handoff.

## Residual risks, deviations, and required integration work

1. **W2 must remove legacy application secret retention.** The existing forbidden-to-W1 `src/app/**` still stores/clones `AppState.sudo_password` and controls prompt behavior. `SystemAdapter::with_sudo_password` remains only as a temporary bridge; W2 should construct/inject the trusted runner and submit a `SecretString` only to one-shot authentication.
2. **W2 must integrate typed reports/targets into UI behavior.** The public contracts are ready, but current app actions still use mutable selections, generic boxed errors, and pre-existing mutation flow. W2 owns stable confirmation targets, operation report rendering, stale-cache handling, classified prompt routing, and removal of legacy interfaces.
3. **Repository-wide tests are not green because W1 intentionally changed approved behavior but cannot edit `tests/integration_test.rs`.**
   - `delete_group_is_idempotent_without_sudo_when_missing` passes a >32-byte group name. D8 now correctly returns validation failure before a missing-group idempotency decision.
   - `privileged_ops_require_auth_without_sudo_password` and `privileged_ops_auth_required_extended_when_not_root` use `root` targets and expect authentication-required. D4 now blocks root user/group mutation before elevation. W2 must update/replace those assertions with valid non-root fixture targets and validation/root-protection expectations.
4. **Clippy remains blocked by the four recorded untouched application findings** named above. They are outside W1's write boundary and not caused by trusted-core code.
5. **Production process lifecycle has implementation coverage but not an OS-level benign-helper test.** The fake-runner suite deliberately proves ordinary tests do not launch host account tools. A later safe test could exercise timeout/output mechanics with a dedicated benign helper only if approved; no such executable was added to the product allowlist.
6. **Linux-local scope only.** `geteuid` returns `UnsupportedPlatform` outside Linux; no NSS/remote/backend/platform support is claimed.

## Integration notes

- Preferred new seams: `sys::{AccountDataSource, AccountSnapshot, IdentityProvider, CommandRunner, CommandSpec, OperationPlan, OperationReport, SnapshotState}` and `error::{CoreError, CoreResult}`.
- Use `refresh_retaining(source, prior)` when an app refresh fails; render stale state instead of substituting empty vectors.
- Use `CommandSpec::{fixed_arg,user_name,group_name,shell_path,gecos,password_record}` only. `password_record` is accepted only for argument-free `KnownProgram::ChPasswd`.
- `CommandPreview` is safe to show; never format `SecretString`, `PasswordRecord`, `CommandSpec`, or `CommandResult` with Debug/display.
- Keep the operation-plan object as the sole source for preview and post-confirm execution; use `execute_plan` to obtain honest partial/reconciliation reports.

**Confidence: 89/100.** Focused contracts, formatting, locked compile, full test execution, diff hygiene, and no-staged-file state are directly verified. Confidence is reduced by the three legacy integration-test expectation conflicts, the untouched four Clippy findings, and the intentionally absent real privileged/disposable-environment execution.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Only W1-authorized Cargo/error/sys/test/fixture paths plus the mandated handoff artifact changed; trusted-core contracts implement D1-D9 constraints without product features or host-account execution."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "This handoff lists every material validation command and exit code, 21 passing focused tests, exact repository-wide test/Clippy blockers, changed paths, residual risks, and no-staged-files evidence."
    }
  ],
  "changedFiles": [
    "Cargo.toml",
    "Cargo.lock",
    "src/error.rs",
    "src/sys/mod.rs",
    "src/sys/command.rs",
    "src/sys/data_source.rs",
    "src/sys/identity.rs",
    "src/sys/operations.rs",
    "src/sys/validation.rs",
    "tests/common/mod.rs",
    "tests/fixtures/passwd",
    "tests/fixtures/group",
    "tests/fixtures/shells",
    "tests/account_parsing.rs",
    "tests/app_construction.rs",
    "tests/command_contracts.rs",
    "tests/dry_run_equivalence.rs",
    "tests/operation_reports.rs",
    "tests/partial_failure.rs",
    "tests/reconciliation.rs",
    "tests/secret_redaction.rs",
    "plans/handoffs/reliability-core.md"
  ],
  "testsAddedOrUpdated": [
    "tests/account_parsing.rs",
    "tests/app_construction.rs",
    "tests/command_contracts.rs",
    "tests/dry_run_equivalence.rs",
    "tests/operation_reports.rs",
    "tests/partial_failure.rs",
    "tests/reconciliation.rs",
    "tests/secret_redaction.rs",
    "tests/common/mod.rs",
    "tests/fixtures/passwd",
    "tests/fixtures/group",
    "tests/fixtures/shells"
  ],
  "commandsRun": [
    {
      "command": "cargo fmt --all -- --check",
      "result": "passed",
      "summary": "Passed as part of final format/check command."
    },
    {
      "command": "cargo check --workspace --all-targets --all-features --locked",
      "result": "passed",
      "summary": "Passed after lock update and final trusted-core hardening."
    },
    {
      "command": "focused cargo test loop for account_parsing, command_contracts, operation_reports, partial_failure, reconciliation, dry_run_equivalence, secret_redaction, app_construction",
      "result": "passed",
      "summary": "21 deterministic focused tests passed without host privileged tools."
    },
    {
      "command": "cargo test --workspace --all-targets --all-features --locked",
      "result": "failed",
      "summary": "Only three legacy integration expectations conflict with approved D4/D8 behavior; all W1 focused targets passed."
    },
    {
      "command": "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
      "result": "failed",
      "summary": "Blocked by four known, untouched src/app/update.rs and src/search.rs findings."
    },
    {
      "command": "git diff --check && git diff --cached --name-only",
      "result": "passed",
      "summary": "No whitespace errors and no staged files."
    }
  ],
  "validationOutput": [
    "Locked check and format check passed.",
    "All eight W1-focused test targets passed (21 tests).",
    "Full locked test run failed only at three legacy integration assertions incompatible with approved D4/D8 behavior.",
    "Clippy -D warnings failed only at the four documented untouched application findings."
  ],
  "residualRisks": [
    "W2 must remove app-level stored/cloned sudo secrets and consume one-shot SecretString authentication.",
    "W2 must integrate stable targets, plans/reports, stale snapshots, and classified errors into src/app/**.",
    "W2 must update legacy integration-test expectations for D4 root protection and D8 32-byte name validation.",
    "Real privileged tool behavior was intentionally not executed; process lifecycle lacks a benign OS-helper integration test."
  ],
  "noStagedFiles": true,
  "diffSummary": "Adds a Linux-local fail-closed trusted core with typed account parsing/validation/identity/commands/secrets/operation reports, deterministic non-spawning fakes and focused integration tests.",
  "reviewFindings": [
    "no blockers in W1-owned diff; integration owner should verify W2 migration of legacy app secret and action flows",
    "repository-wide test and Clippy blockers are documented above and require W2/out-of-boundary updates"
  ],
  "manualNotes": "Pre-existing dirty .gitignore and deleted docs were preserved. No files are staged."
}
```

## Follow-up: adapter-owned operation bridge

- **Follow-up status:** Implemented after the integration-owner accepted the W2 blocker.
- **Bridge-owned public contract:** `OperationRequest`, `SystemAdapter::prepare_operation`, `SystemAdapter::execute_prepared_operation`, and `SystemAdapter::refresh_state`.
- **Changed follow-up files:** `src/sys/mod.rs`, `src/sys/operations.rs`, `tests/operation_bridge.rs`, and this handoff.

### Bridge behavior

- `OperationRequest` is a closed enum for every existing mutation: membership add/remove, user/group create/delete/rename, shell/GECOS changes, password update/expiry, and home-create/home-delete flags.
- `prepare_operation` takes ownership of the request, refreshes the injected source once, validates D8 values, blocks D4 root user/group targets, converts commands through the existing typed `CommandSpec` builders, binds each existing user/group by UID/GID/name and a deterministic snapshot generation, and attaches typed observable postconditions.
- Membership operations bind *both* user and group, preventing a group name from being re-resolved to a different GID after preview. Creation binds an absence target; execution rejects a name that becomes occupied between preview and confirmation.
- `execute_prepared_operation` refreshes and validates every bound target before obtaining any elevation. It obtains `Direct` or one-shot sudo internally, executes the exact plan previewed through the adapter-owned runner, and reconciles only through the adapter-owned account source. It never exposes the runner or `ElevationGrant` to application callers.
- `refresh_state(prior)` delegates to the adapter-owned account source and returns `Fresh`, `Stale { prior, error }`, or `Unavailable`; it never converts refresh failure into an empty snapshot.
- `set_elevation_secret(SecretString)` remains the application-facing setter for one-shot elevation. The transitional `with_sudo_password` remains only because current W2-owned application code and pre-existing integration tests still compile against it; W2 must stop using it when adopting this bridge.
- Password and expiry commands are deliberately reported as partially/unobserved after child success because the current Linux-local source has no shadow-state reader. This is fail-closed: the bridge does not falsely claim a verified password-state postcondition. `OperationRequest::SetUserPassword` accepts the existing validated `PasswordRecord` so password material remains typed, non-debug, redacted from previews, and stdin-only; this remains the required W2 secret-transport integration point.

### Follow-up tests

Added `tests/operation_bridge.rs` (5 deterministic, non-spawning tests):

1. preview/execution use the same stable target and exact redacted command;
2. `AuthenticationRequired` is returned before a command reaches the fake runner, then `set_elevation_secret` enables one-shot authentication;
3. root and changed targets are rejected before runner use;
4. a command failure returns an explicit partial report with reconciliation;
5. `refresh_state` retains a stale known-good snapshot.

The test source and runner are injected fakes. No follow-up test constructs `LocalCommandRunner` or invokes a host account tool.

### Follow-up commands

| Command | Exit | Result / note |
|---|---:|---|
| `cargo fmt --all && cargo check --workspace --all-targets --all-features --locked` | 0 | Compiled bridge implementation. |
| `cargo fmt --all && cargo test --test operation_bridge --locked` | 0 | Initial five bridge tests passed. |
| `cargo fmt --all && for test in account_parsing command_contracts operation_reports partial_failure reconciliation dry_run_equivalence secret_redaction app_construction operation_bridge; do cargo test --test "$test" --locked || exit $?; done` | 0 | All nine focused test targets passed (26 tests). |
| `cargo fmt --all -- --check && cargo check --workspace --all-targets --all-features --locked` | 0 | Required format and locked all-target check passed. |
| `cargo test --workspace --all-targets --all-features --locked` | 101 | Same three out-of-boundary legacy integration assertions fail; all lib, bridge, and other focused targets passed. |
| `git diff --check && git diff --cached --name-only` | 0 | No whitespace errors; no staged files. |

### Follow-up residual risks and W2 notes

- W2 must construct only `OperationRequest` values and call `prepare_operation`/`execute_prepared_operation`; it must not select commands, root policy, reconcilers, runners, or elevation grants.
- W2 must replace all direct `SystemAdapter` mutation calls and legacy sudo-password storage with the bridge and `set_elevation_secret`; the legacy constructor remains solely to retain current compile compatibility during the handoff.
- Password/expiry verification remains unavailable until a D6-compatible explicit shadow refresh model is integrated. The bridge safely reports it as partial/unavailable rather than success.
- The original three full-suite test failures and four untouched Clippy findings remain as already documented above; no W2-owned file was changed to mask them.

**Follow-up confidence: 90/100.** The requested public bridge and every requested deterministic behavior have direct focused-test evidence, locked compile/format evidence, no-staged-file evidence, and no host mutation. Confidence is reduced only by the deliberately retained legacy compatibility facade, unavailable shadow postconditions, and out-of-boundary full-suite blockers.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "The follow-up changes only W1-owned src/sys modules, one focused trusted-core test, and the mandatory W1 handoff. The bridge centralizes closed request-to-command compilation, root/D8 checks, elevation, target binding, reconciliation, and stale state without touching W2-owned code."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "The follow-up handoff records exact commands and exit codes, 26 passing focused tests, the unchanged full-suite blocker, bridge behavior, residual risks, changed files, and a post-change no-staged-files check."
    }
  ],
  "changedFiles": [
    "src/sys/mod.rs",
    "src/sys/operations.rs",
    "tests/operation_bridge.rs",
    "plans/handoffs/reliability-core.md"
  ],
  "testsAddedOrUpdated": [
    "tests/operation_bridge.rs"
  ],
  "commandsRun": [
    {
      "command": "cargo fmt --all && cargo check --workspace --all-targets --all-features --locked",
      "result": "passed",
      "summary": "Bridge implementation compiled."
    },
    {
      "command": "cargo fmt --all && cargo test --test operation_bridge --locked",
      "result": "passed",
      "summary": "Five deterministic bridge tests passed."
    },
    {
      "command": "focused nine-target cargo test loop",
      "result": "passed",
      "summary": "26 focused trusted-core tests passed without host account tools."
    },
    {
      "command": "cargo fmt --all -- --check && cargo check --workspace --all-targets --all-features --locked",
      "result": "passed",
      "summary": "Formatting and locked all-target check passed."
    },
    {
      "command": "cargo test --workspace --all-targets --all-features --locked",
      "result": "failed",
      "summary": "Only the same three legacy integration assertions outside W1's write boundary failed."
    },
    {
      "command": "git diff --check && git diff --cached --name-only",
      "result": "passed",
      "summary": "No whitespace errors and no staged files."
    }
  ],
  "validationOutput": [
    "The bridge compiles with locked workspace/all-target/all-feature checks.",
    "All requested bridge behaviors are covered by deterministic fake-source/fake-runner tests.",
    "No bridge test invokes sudo or a privileged host account tool.",
    "The full suite remains blocked only by the three pre-existing legacy assertion conflicts documented in the original handoff."
  ],
  "residualRisks": [
    "W2 must migrate app calls and stored sudo state to OperationRequest plus set_elevation_secret.",
    "Password/expiry postconditions are intentionally unavailable until an explicit shadow refresh source is integrated.",
    "The transitional public with_sudo_password constructor remains temporarily for current compile compatibility."
  ],
  "noStagedFiles": true,
  "diffSummary": "Adds an adapter-owned closed operation bridge that binds stable targets, owns elevation/runner/reconciliation, and exposes stale refresh state for W2.",
  "reviewFindings": [
    "no blocker in the follow-up W1-owned bridge diff",
    "W2 migration and the documented legacy full-suite assertions remain integration work"
  ],
  "manualNotes": "Pre-existing dirty .gitignore and deleted docs were preserved. No files are staged."
}
```