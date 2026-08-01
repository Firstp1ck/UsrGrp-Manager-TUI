# W3 Trusted-Core Hardening Handoff

## 1. Status, scope, and plan validation

- **Workstream:** W3 trusted-core hardening, sequential writer 1 of 2 in the accepted-fix wave.
- **Status:** Implemented within the assigned trusted-core boundary. The locked all-target/all-feature check/test/lint gates and no-default-feature tests pass.
- **Revision:** Shared uncommitted worktree based on `0b154c1c3a6a889b495ab50c268b5c3ae491087a`; no commit or staging was performed.
- **Plan inputs:** Requested root `context.md` and `plan.md` are absent. Per the accepted R2-ME1 disposition, the canonical plan is `plans/planned/reliability-robustness.md`; it, both R1/R2 reviews, and both earlier handoffs were read before edits.
- **Classification:** Complex remains validated: this work crosses secret/elevation lifetime, typed command execution, stable account identities, bounded account/shadow input, composite operation semantics, retry/reconciliation reporting, and deterministic lifecycle tests.

Only accepted findings were implemented: R1-1/2/3/4/7/8/9/10/12; R2-F1/F2/F3/F5/F7/F9/F11; and core-relevant R2-ME2/ME4/ME5. Terminal/config/CI/render/selection/benchmark/product integration work remains W4-owned and untouched.

## 2. Changed files and implementation

- `Cargo.toml` — declares tested `rust-version = "1.89"` and removes the empty `file-parse` feature (R1-12/R2-F9). `Cargo.lock` was not changed in this run.
- `src/sys/operations.rs` — adds adapter-compiled `OperationRequest::Composite`, ordered exact-plan aggregation, per-step typed already-satisfied checks, preflight-before-elevation behavior, typed skip reasons, downstream-skipped evidence, and per-completed-step `Verified`/`Unverified` evidence. Successful password/expiry commands with unobservable postconditions now remain honest but do not suppress later required steps.
- `src/sys/mod.rs` — compiles composites against one captured account snapshot; preserves stable target binding; scopes each `ElevationGrant` to one execution; adds injectable `ProtectedIdentityPolicy`; defaults service users/groups and `sudo`/`wheel` membership to fail-closed; keeps UID/GID 0 unconditional; and restricts the former public direct-mutation/password facade to crate-private compatibility only. Public callers use only prepare/execute/report APIs.
- `src/sys/command.rs` — centralizes checked post-spawn cleanup, independently attempts kill and reap on every exceptional lifecycle path, joins both reader threads after wait/timeout paths, preserves primary cleanup context, and checks a final reap after observed exit. A safe test-only helper runs the current test executable, never a host account tool.
- `src/sys/data_source.rs` and `src/sys/validation.rs` — bound Linux-local account-file reads to 1 MiB during read, bound records/counts, reject oversized input before unbounded allocation, and separate strict mutation `ShellPath::new` from crate-private observed passwd parsing. An empty observed passwd shell survives parsing and renders explicitly as `(default /bin/sh)`.
- `src/search.rs` — bounds shadow-file reads/records and truncates queries at a valid 256-byte UTF-8 boundary. Adds account-level `Known`/`Unknown`/`Unavailable` shadow state and treats shadow `last_change = 0` as must-change.
- `tests/{account_parsing,app_construction,command_contracts,core_static_guards,operation_bridge,operation_reports,shadow_status}.rs`, `tests/fixtures/passwd` — add deterministic coverage for the new boundaries. Existing core tests were migrated from restricted direct mutation calls to the public plan/report bridge.

The W4-owned `src/app/**`, `src/ui/**`, `src/config/**`, `src/terminal.rs`, `src/main.rs`, `src/lib.rs`, CI/workflows/docs/benches, canonical plan, reviews, and prior handoffs were not edited.

## 3. Tests and matrices added/updated

- `operation_bridge` now exercises every closed request variant, exact composite previews for create+password+membership and password+expiry, create/password/membership and bulk-membership failure boundaries, retry skipping before elevation, protected service/elevation-group policy injection, and one-execution elevation followed by required one-shot reauthentication.
- `operation_reports` proves unobservable successful password/expiry-like steps still execute downstream work and a failed step records every downstream skipped step.
- `shadow_status` is the required focused target: missing readable-shadow records are `Unknown`, source failures are `Unavailable`, zero last-change is must-change, and multibyte input respects the UTF-8 byte bound.
- `account_parsing` covers valid empty passwd shells, strict mutation-shell rejection, and bounded account-file reads.
- `core_static_guards` scans all integration-test Rust sources for production runner/process construction and asserts the public adapter has no legacy direct-mutation or stored-password facade.
- The `src/sys/command.rs` unit test launches only an ignored current-test-binary helper, forces the bounded timeout path, and verifies it is reaped. No test invokes `sudo`, `useradd`, `usermod`, `userdel`, `groupadd`, `groupmod`, `groupdel`, `gpasswd`, `chpasswd`, or `chage`.

## 4. Commands and exits

| Command | Exit | Result |
|---|---:|---|
| `cargo check --workspace --all-targets --all-features --locked` (early after temporary incompatible public error expansion) | 101 | Expected compile failure; reverted the incompatible public-enum expansion because W3 may not edit `src/app/**`. |
| `cargo check --workspace --all-targets --all-features --locked` (after core fixes) | 0 | Passed. |
| Focused `cargo test` for account parsing/shadow/operation reports/partial failure/command contracts/app construction | 101 | Initial fixture line expectation failed; corrected fixture assertion. |
| Same focused test set | 0 | Passed. |
| `cargo test --test operation_bridge --locked` (during failure-matrix construction) | 101 | Two fixture source sequences were initially incorrect; corrected deterministic refresh sequences. |
| `cargo test --test operation_bridge --locked` | 0 | Final 14 bridge/matrix tests passed. |
| `cargo test --test core_static_guards --locked` (initial) | 101 | Guard initially matched its own literal; made patterns runtime-composed, then fixed a `String` borrow compile error. |
| `cargo test --test core_static_guards --locked` | 0 | Final static guard passed. |
| `cargo test --lib sys::command::tests::timeout_reaps_only_a_benign_test_helper --locked` | 0 | Safe benign helper timeout/kill/reap proof passed. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 | Passed. |
| `cargo fmt --all -- --check && cargo check --workspace --all-targets --all-features --locked && cargo test --workspace --all-targets --all-features --locked && cargo test --workspace --no-default-features --locked && cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 | Final required full validation passed: 68 test executions plus the ignored helper marker; no-default suite and all-target Clippy are green. |
| `git diff --check && git diff --cached --name-only` plus static test/process/facade scans | 0 | No whitespace errors, no staged files, no integration-test production runner/process construction, and no public legacy facade. |

**Omissions:** No privileged account command, sudo invocation, host-account mutation, deployment, publication, destructive rollback, network operation, or disposable-environment account test ran. The benign helper is this test binary only and does not execute an account-management program.

## 5. Residual risks and W4 seam

1. **W4 must consume `OperationRequest::Composite` as one confirmation/execution.** Current forbidden-to-W3 `src/app/update.rs` still expands actions into a queue. W4 must replace that with one composite request for create+password+sudo-group, password+expiry, and bulk membership, present the one exact redacted plan, and retain the aggregate report.
2. **W4 owns the final application secret lifetime.** The core request consumes non-`Clone`, non-`Debug`, zeroizing `PasswordRecord`/`SecretString`; reports/previews never contain it. W4 must create that capability immediately before bridge preparation and must not place it in general queue/report/modal state. Cancellation must drop it.
3. **W4 must render the new evidence.** `CompletedStep.verification`, `SkipKind::DownstreamFailure`, and `ShadowState::account_status` now expose honest states; the existing W4-owned UI must present per-account unknown and per-step unverified/skipped evidence.
4. **Protected policy is intentionally fail-closed.** Production defaults block service-ID modifications and `sudo`/`wheel` membership. A deployment intentionally managing such identities must instantiate/inject `ProtectedIdentityPolicy` with the explicit numeric/group allowlists; no hidden threshold or automatic privilege-membership bypass exists.
5. **Core cleanup test is safe but not an OS fault injector.** It proves timeout/reap with a benign helper. Real account-tool behavior and PTY/failure-injection release evidence remain the accepted external R2-ME6 gate.
6. **Bounded reads are tested at the account and pure shadow/query layers.** W4 still owns all config/diagnostic/modal/render bounds and quantitative benchmark evidence.

## 6. Integration notes and next step

- Use `OperationRequest::Composite { requests }`; call `prepare_operation` once, show `OperationPlan::redacted_preview()`, then call `execute_prepared_operation` on that exact plan. Do not reconstruct commands, runners, target bindings, elevation grants, policy, or reconciliation in application code.
- A report can contain completed steps with `StepVerification::Unverified`, typed `SkipKind::AlreadySatisfied` retry evidence, and `SkipKind::DownstreamFailure`; `is_complete()` remains false until all required postconditions are observed.
- `ProtectedIdentityPolicy::fail_closed()` is the sole default policy. Use `from_components_with_policy` in explicitly configured runtime/test construction where a reviewed service/elevation exception is intended.
- The next writer should perform W4 application/release integration only, retain the existing dirty worktree content, rerun cross-workstream validation, then request the required reviewer gate.

**Confidence: 92/100.** Direct source/review/plan inspection, deterministic failure/retry/policy/lifecycle matrices, final locked checks/tests/Clippy, static enforcement, and no-staged-file evidence support the result. Confidence is reduced only by intentionally deferred W4 UI secret/composite integration and the approved external real-tool/PTY release gate.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Changes remain in Cargo manifest, trusted sys/search code, fixtures, and trusted-core tests. No forbidden app/UI/config/terminal/main/lib/plan/review/workflow/doc paths were edited; no host account tool ran."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "This handoff records changed files, focused matrices, intermediate and final command exits, full locked validation, static no-host-tool enforcement, omissions, residual risks, and no-staged-files evidence."
    }
  ],
  "changedFiles": [
    "Cargo.toml",
    "src/search.rs",
    "src/sys/mod.rs",
    "src/sys/command.rs",
    "src/sys/data_source.rs",
    "src/sys/operations.rs",
    "src/sys/validation.rs",
    "tests/account_parsing.rs",
    "tests/app_construction.rs",
    "tests/command_contracts.rs",
    "tests/core_static_guards.rs",
    "tests/fixtures/passwd",
    "tests/operation_bridge.rs",
    "tests/operation_reports.rs",
    "tests/shadow_status.rs",
    "plans/handoffs/reliability-core-fixes.md"
  ],
  "testsAddedOrUpdated": [
    "tests/account_parsing.rs",
    "tests/app_construction.rs",
    "tests/command_contracts.rs",
    "tests/core_static_guards.rs",
    "tests/operation_bridge.rs",
    "tests/operation_reports.rs",
    "tests/shadow_status.rs",
    "tests/fixtures/passwd",
    "src/sys/command.rs unit tests"
  ],
  "commandsRun": [
    {
      "command": "cargo fmt --all -- --check",
      "result": "passed",
      "summary": "Final formatting check passed."
    },
    {
      "command": "cargo check --workspace --all-targets --all-features --locked",
      "result": "passed",
      "summary": "Locked all-target/all-feature check passed."
    },
    {
      "command": "cargo test --workspace --all-targets --all-features --locked",
      "result": "passed",
      "summary": "Full deterministic suite passed, including 14 operation-bridge and 5 shadow-status tests."
    },
    {
      "command": "cargo test --workspace --no-default-features --locked",
      "result": "passed",
      "summary": "No-default-feature suite passed."
    },
    {
      "command": "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
      "result": "passed",
      "summary": "All-target Clippy warnings denied successfully; no W2-only failure remains."
    },
    {
      "command": "cargo test --lib sys::command::tests::timeout_reaps_only_a_benign_test_helper --locked",
      "result": "passed",
      "summary": "Only the current test binary helper was spawned and reaped."
    },
    {
      "command": "git diff --check; git diff --cached --name-only; core static scans",
      "result": "passed",
      "summary": "No whitespace errors, no staged files, no normal-test production process boundary, and no public legacy facade."
    }
  ],
  "validationOutput": [
    "Final chained format/check/full-test/no-default/Clippy command exited 0.",
    "No privileged host account executable or sudo was invoked.",
    "Static guard scans every integration-test Rust source for LocalCommandRunner or process construction."
  ],
  "residualRisks": [
    "W4 must migrate application queues to one composite request and shortest-lived secret capability.",
    "W4 must render per-step unverified/downstream-skipped and per-account shadow-unknown states.",
    "Real privileged-tool/disposable environment and PTY fault-injection evidence remain an approved external release gate.",
    "Fail-closed service/elevation policy requires an explicit runtime allowlist where such mutations are intentional."
  ],
  "noStagedFiles": true,
  "diffSummary": "Hardens the trusted operation bridge with one-snapshot composites, typed retry/partial evidence, one-execution elevation, injected protected policy, bounded reads, honest shell/shadow data, and checked child lifecycle cleanup.",
  "reviewFindings": [
    "no blocker found in the W3-owned diff after final locked validation",
    "W4 integration is required before app-visible composite/secret/report behavior can be accepted end-to-end"
  ],
  "manualNotes": "Pre-existing dirty shared-worktree files, canonical plan, reviews, prior handoffs, user-authored .gitignore, and deleted superseded docs were preserved. No files are staged."
}
```
