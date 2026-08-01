# Reliability implementation review R1

## Review

- **Correct:** The worktree compiles, formats, lints, and passes its current deterministic suite. `cargo fmt --all -- --check`, locked all-target/all-feature `cargo check`, locked all-target/all-feature `cargo test` (48 tests), and full locked Clippy with `-D warnings` all exited 0.
- **Correct:** Privileged command construction is materially safer. `KnownProgram` closes executable selection, validated values are passed as distinct argv entries, no shell pipeline is used, and `chpasswd` receives a non-`Debug` password record only on stdin (`src/sys/command.rs:13-42`, `90-190`, `245-288`; `src/sys/validation.rs:219-289`). Static scans found no UI filesystem/process access and no shell command construction outside the trusted runner.
- **Correct:** Existing mutation requests bind current users/groups to UID/GID/name and snapshot generation before execution; membership requests bind both entities, and execution revalidates them before elevation (`src/sys/mod.rs:157-215`, `447-462`, `788-834`). I found no additional stable-target mismatch in the currently reachable UI paths.
- **Correct:** Refresh failure retains prior account data as stale rather than replacing it with an empty list (`src/sys/data_source.rs:83-101`; `src/app/mod.rs:652-668`).
- **Correct:** Rendering entry points are immutable and I/O-free, and normal list tables render only visible rows (`src/ui/mod.rs:14-96`; `src/ui/users.rs:14-63`; `src/ui/groups.rs:14-59`).
- **Correct:** The config writer uses a same-directory `0600` create-new temporary file, file sync, rename, directory sync, and rejects an existing destination symlink (`src/config/mod.rs:34-99`). I found no additional atomic replacement or secret-disclosure defect in the normal successful config-save path.
- **Fixed:** None. Review-only scope was respected; only this review artifact was written.

### Blocker — multi-step user/password/membership actions are not one operation and lose their real partial outcome

- **Severity:** Blocker
- **Affected:** `operation_requests`, `prepare_next_request`, `execute_pending_plan` (`src/app/update.rs:1064-1156`, `1245-1286`); password plan construction (`src/sys/mod.rs:413-436`); opaque postcondition handling (`src/sys/mod.rs:879`); postcondition failure (`src/sys/operations.rs:345-371`).
- **Violated requirement:** M2 requires each mutation to be one `OperationPlan` with ordered steps, failure injection at every create-user/password+expiry/bulk-membership boundary, and a report containing completed/failed/skipped/remediation state (`plans/planned/reliability-robustness.md:186-205`).
- **Evidence/reproduction:** `CreateUserWithOptions` becomes up to three independent queued requests; `SetPassword { must_change: true }` becomes two. Only the current request is stored in `last_report`; after any incomplete report, remaining requests are simply cleared at `src/app/update.rs:1131-1135`. Password and expiry plans deliberately use `OperationCheck::new`, whose `Opaque` condition is always `Unavailable`; therefore a successful `chpasswd` always becomes `PostconditionFailed`, and `must_change`/sudo-group steps can never run. Earlier successful create or membership steps and later cleared steps are absent from the displayed report. Current tests construct synthetic multi-step plans directly but do not test these application workflows.
- **Smallest safe remediation:** Compile each user-visible multi-step action into one bridge-owned plan/report (or an equivalent bridge-owned composite report) before confirmation. Preserve one aggregate completed/failed/skipped list, include unexecuted downstream steps, and add per-boundary tests for create+password+group, password+expiry, and bulk membership. Until password/expiry state is observable, report it as unverified without silently discarding the rest of the operation.

### High — production plans do not implement retry preconditions/idempotency

- **Severity:** High
- **Affected:** `SystemAdapter::prepare_operation` (`src/sys/mod.rs:157-438`); `execute_plan` (`src/sys/operations.rs:299-375`).
- **Violated requirement:** M2 requires retry to skip already-satisfied steps (`plans/planned/reliability-robustness.md:190-198`).
- **Evidence/reproduction:** No production bridge plan calls `OperationPlan::require`; repository search finds `.require(` only in `tests/partial_failure.rs`. For example, membership plans at `src/sys/mod.rs:169-215` have only postconditions. If `gpasswd` succeeds but reconciliation is temporarily unavailable, preparing/retrying later executes `gpasswd` again even when membership is now satisfied. Create plans instead return “already exists” (`src/sys/mod.rs:220-246`), and shell/GECOS/rename plans also lack already-satisfied checks. The passing idempotency test uses a hand-built opaque plan, not any production `OperationRequest`.
- **Smallest safe remediation:** Add typed, bridge-generated already-satisfied conditions for every operation and evaluate them against the current snapshot before elevation/command execution; test retries through each real `OperationRequest` after an injected reconciliation failure.

### High — cached elevation grants become permanently stale and bypass new authentication

- **Severity:** High
- **Affected:** `SystemAdapter::{grant,elevation_grant,set_elevation_secret}` (`src/sys/mod.rs:98-149`, `712-727`); sudo failure classification (`src/sys/command.rs:264-287`); app error routing (`src/app/update.rs:1138-1154`).
- **Violated requirement/failure mode:** The grant is documented as usable only for one operation, while D5 requires one-shot authentication and typed timestamp/capability failure (`plans/planned/reliability-robustness.md:108`).
- **Evidence/reproduction:** After the first successful `sudo -v`, `grant` remains `Some(SudoTimestamp)` for the lifetime of the adapter. Once the actual sudo timestamp expires, `sudo -n` returns `AuthenticationCapability`; the app prompts only for `AuthenticationRequired` and clears the plan for every other error. Future calls keep returning the stale cached grant at lines 716-717, so even a newly supplied pending secret would never be consumed.
- **Smallest safe remediation:** Scope `ElevationGrant` to a single execution and clear it on every completion/failure. Before each later operation, either validate a noninteractive timestamp and return `AuthenticationRequired` only when reauthentication is appropriate, or consume a newly supplied one-shot secret; retain `AuthenticationCapability` for policies that cannot support the approved transport.

### High — protected-identity policy was removed rather than made explicit

- **Severity:** High
- **Affected:** delete/modify UI and trusted target checks (`src/app/update.rs:182-195`; `src/sys/mod.rs:748-785`).
- **Violated requirement/regression:** D4 says root is always immutable and other protected thresholds remain explicit policy inputs (`plans/planned/reliability-robustness.md:107`). The prior UI blocked user deletion outside UID 1000–1999 and system-group rename; the new code retains only root checks.
- **Evidence/reproduction:** `open_delete` sets `allowed` for every account except UID 0/name `root`; `mutable_user_target` and `mutable_group_target` likewise accept UID/GID 1 and all other service identities. No protected-policy type, config, or injected threshold exists. Thus a confirmed action can now delete/rename non-root system accounts/groups without the planned explicit policy decision.
- **Smallest safe remediation:** Introduce one explicit, injected protected-identity policy used by preparation and UI presentation; default fail-closed for destructive service-identity operations, keep root unconditional, and add UID/GID boundary tests. Do not reintroduce scattered hard-coded checks.

### High — terminal initialization can leave the alternate screen/mouse mode active

- **Severity:** High
- **Affected:** `TerminalSession::enter` (`src/terminal.rs:20-40`).
- **Violated requirement:** M1 requires restoration after partial initialization and PTY/failure-injection proof (`plans/planned/reliability-robustness.md:171-183`).
- **Evidence/reproduction:** If the combined `execute!(EnterAlternateScreen, EnableMouseCapture)` returns after writing only part of the sequence, the error branch disables raw mode only; it never emits `LeaveAlternateScreen` or `DisableMouseCapture`. If `Terminal::new` fails, cleanup is attempted but all cleanup errors are discarded. No `tests/terminal_cleanup.rs` or PTY/failure-injection target exists.
- **Smallest safe remediation:** Track each acquired terminal capability in a guard from the first successful setup step, run best-effort reverse cleanup on every subsequent failure, return combined/first cleanup error without skipping later cleanup, and add injected writer/PTY tests for every setup boundary.

### Medium — account password material is retained in long-lived application state

- **Severity:** Medium
- **Affected:** secret-bearing `PendingAction` variants and `AppState::{pending_plan,pending_requests}` (`src/app/mod.rs:427-440`, `582-590`); queue/plan storage (`src/app/update.rs:1064-1100`, `1245-1286`).
- **Violated requirement:** Reliability principle 7 says secrets do not enter long-lived application state; `SecretString` documentation likewise says to pass it directly rather than retain it (`plans/planned/reliability-robustness.md:31-39`; `src/sys/validation.rs:219-223`).
- **Evidence/reproduction:** A create-user password is converted to `PasswordRecord` and can remain in `AppState.pending_requests` while the user confirms the create step. A direct password change remains inside `AppState.pending_plan` through confirmation and an authentication prompt, for an unbounded interactive duration. The wrappers are non-`Debug` and zeroize on drop, so this is a lifetime/minimization defect rather than an argv/log leak.
- **Smallest safe remediation:** Keep the validated secret in a dedicated one-shot, zeroizing execution capability with the shortest possible lifetime, not the general app queue/report state; confirmation state should contain only the redacted plan identity, and cancellation must drop the capability immediately.

### Medium — child cleanup errors can leave unreaped/detached processes

- **Severity:** Medium
- **Affected:** `run_child`, `wait_with_timeout`, `kill_and_reap` (`src/sys/command.rs:296-353`, `422-456`).
- **Violated requirement:** M1 requires every wait, timeout, kill, and reap result to be checked and children cleaned up.
- **Evidence/reproduction:** A `try_wait` error returns through `?` without killing/reaping or joining output readers. On the timeout race, a `kill` error returns before `wait`. The stdin-error cleanup explicitly ignores both `kill` and `wait` results. These paths contradict the handoff claim that all lifecycle results are checked and can leave a child/zombie or detached reader on exceptional OS errors.
- **Smallest safe remediation:** Centralize child finalization so every post-spawn exit path attempts close/kill/wait and joins readers, preserving the primary error plus cleanup classification; add a benign injected/helper process test for wait/kill/reap failures.

### Medium — valid Linux passwd records with an empty shell disappear

- **Severity:** Medium
- **Affected:** passwd parsing (`src/sys/data_source.rs:186-242`) and `ShellPath::new` (`src/sys/validation.rs:136-174`).
- **Violated requirement/failure mode:** The Linux-local data source must represent local passwd data honestly; privileged-input validation must not cause valid existing identities to vanish.
- **Evidence/reproduction:** `parse_passwd_records` applies mutation-input `ShellPath::new` to field 7, which rejects empty/nonabsolute values and drops the whole user as “invalid passwd field.” The installed Linux `passwd(5)` source explicitly calls the command interpreter optional and states an empty field defaults to `/bin/sh` (`/usr/share/man/man5/passwd.5.gz`, source lines 111 and 157-162 from the review command). Such an account is absent from UI/target resolution even though it is valid locally.
- **Smallest safe remediation:** Separate observed passwd shell representation from validated `usermod -s` input. Preserve empty observed shells (rendering the documented default/unknown explicitly) while continuing to require absolute paths for new mutation input; add an empty-shell fixture test.

### Medium — shadow “unknown” state is collapsed into “known,” and dependent filters silently no-op

- **Severity:** Medium
- **Affected:** `ShadowState`, status lookup/filtering (`src/search.rs:21-50`, `80-109`, `154-170`) and user detail rendering (`src/ui/users.rs:73-100`).
- **Violated requirement:** D6/M3 require known, unknown, and unavailable states and visibly distinct behavior (`plans/planned/reliability-robustness.md:109`, `214-232`).
- **Evidence/reproduction:** `ShadowState` has only `Known(map)` and `Unavailable`. If shadow is readable but a passwd user has no entry or its line is skipped, `status()` returns `None`, yet `availability_label()` returns `known`; user details therefore display “Shadow: known.” When any visible user is missing, a selected shadow filter performs no filtering at lines 154-170 and exposes no explicit per-filter limitation.
- **Smallest safe remediation:** Add an explicit per-account `Unknown` result (and reason/diagnostic), render it distinctly, and make a shadow-dependent filter surface an incomplete/unavailable result rather than leaving the unfiltered list under a “known” label; add missing/malformed-entry tests.

### Medium — resource bounds and visible-row-only rendering are incomplete

- **Severity:** Medium
- **Affected:** account/shadow/config/home reads (`src/sys/data_source.rs:154-169`; `src/search.rs:55-63`; `src/app/mod.rs:101`, `526`; `src/app/keymap.rs:85`; `src/app/filterconf.rs:50`); modal rendering (`src/ui/users.rs:194-217`, `295-329`; `src/ui/groups.rs:186-214`, `243-255`).
- **Violated requirement:** D7/M4 require bounded system/config files and allocations plus visible-row-only rendering for the 10k/10k profile (`plans/planned/reliability-robustness.md:110`, `243-267`).
- **Evidence/reproduction:** All listed reads use unbounded `read_to_string`; the authorized-keys metadata check is a TOCTOU size check followed by an unbounded read. Membership/shell modals ignore their `offset` fields and clone/format every candidate into one `String` each frame. `tests/resource_bounds.rs` checks only a 10,000-byte ASCII query, `tests/ui_invariants.rs` renders only the small-terminal/list path, and `benches/search_and_render.rs` benchmarks search only—no groups, rendering, p95, allocations, or I/O counts.
- **Smallest safe remediation:** Add bounded readers with explicit oversize errors, enforce the bound while reading authorized keys, slice modal candidates to visible rows using offset, and add 10k users + 10k groups render/search operation-count/allocation tests plus the recorded release benchmark.

### High — clean CI toolchains do not install the components the workflow invokes

- **Severity:** High
- **Affected:** `.github/workflows/rust.yml:27-43`.
- **Violated requirement/failure mode:** M6 requires reproducible format/Clippy gates on MSRV and stable (`plans/planned/reliability-robustness.md:293-318`).
- **Evidence/reproduction:** The workflow installs each toolchain with `--profile minimal`, which installs Cargo/rustc/std but not rustfmt or Clippy, then immediately invokes `cargo +toolchain fmt` and `cargo +toolchain clippy`. A clean 1.89.0 runner therefore lacks required components even though the commands pass on this pre-provisioned development host.
- **Smallest safe remediation:** Install `rustfmt` and `clippy` explicitly for each matrix toolchain (or use a fully pinned toolchain action/config that declares them), then validate the workflow in a clean runner.

### Low — declared implementation metadata still contradicts D1/M6

- **Severity:** Low
- **Affected:** `Cargo.toml:1-6`, `27-29`.
- **Violated requirement:** D1 says remove the empty `file-parse` feature; M6 says declare MSRV (`plans/planned/reliability-robustness.md:104`, `297`).
- **Evidence/reproduction:** `file-parse = []` remains, and `[package]` has no `rust-version` despite README/CI claiming 1.89.0. `--all-features` therefore still exercises a meaningless compatibility claim, and Cargo cannot enforce the stated MSRV for consumers.
- **Smallest safe remediation:** Remove `file-parse` and add `rust-version = "1.89"` after the locked MSRV test remains green.

## Areas with no additional findings

- **Architecture boundaries:** No additional arbitrary-command or render-side-effect bypass found; application mutations use the bridge. The material architecture gap is the multi-request composition reported above.
- **Privileged argv and transport:** No password-bearing argv, shell interpolation, raw stderr retention in UI reports, or sudo secret in `AppState` was found. Remaining secret issue is lifetime in pending password plans/queues.
- **Stable target binding:** No additional target-selection mismatch found beyond the retry/composition issues above. Membership binds both user and group, and group modal actions retain GID.
- **Partial refresh:** No known-good-list erasure found; stale retention is implemented.
- **Terminal cleanup:** Normal explicit restore/drop logic is sound; the finding is limited to partial setup/error injection.
- **Config durability/privacy:** No additional normal-path atomicity, destination-symlink, permissions, or config-secret issue found. Resource-size limits remain open as reported.
- **Regressions:** Findings above cover the observed protected-policy and valid-passwd parsing regressions. No extra root UID/GID bypass, binary module duplication, or rendering mutation was found.

## Commands and evidence

- `git status --short`; `git diff --stat`; `git diff --name-status`; `git diff --numstat`; `git diff --check`; `git ls-files --others --exclude-standard`; `git rev-parse HEAD`.
- Read canonical `plans/planned/reliability-robustness.md`, both `plans/handoffs/*.md`, all changed/untracked Rust/config/workflow/test files, and relevant full diff/surrounding baseline code. Requested root `plan.md` and `progress.md` do not exist; both handoffs independently record the same absence.
- `cargo fmt --all -- --check && cargo check --workspace --all-targets --all-features --locked` — pass.
- `cargo test --workspace --all-targets --all-features --locked` — pass, 48 tests total; no privileged tool was invoked.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass.
- Static scans for `bash`, `sudo_password`, `with_sudo_password`, `Command::new`, process/filesystem calls in UI, production `.require(` usage, request queues, and secret-bearing derives.
- Local `passwd(5)` source inspection with `gzip -cd /usr/share/man/man5/passwd.5.gz | grep ...` — confirms empty shell is valid and defaults to `/bin/sh`.
- No sudo, account-management executable, privileged tool, host mutation, network lookup, PTY mutation, disposable VM, benchmark execution, `cargo audit`, or `cargo deny` was run in this review.

## Provider, model, limits, confidence

- **Provider/model:** `openai-codex` / `gpt-5.6-sol` (reported from runtime `PI_PROVIDER`/`PI_MODEL`; the configured artifact filename says “anthropic” but does not match the actual runtime).
- **Revision:** `0b154c1c3a6a889b495ab50c268b5c3ae491087a` plus the full uncommitted worktree shown by `git status`.
- **Limits:** No real privileged-command behavior or PTY failure injection was executed by design. CI component failure was verified from workflow/toolchain-profile semantics, not by launching a clean GitHub runner. Performance defects were established structurally; p95/allocation measurements were not run because the supplied benchmark does not implement them.
- **Confidence:** **96/100.** Full source/diff/plan/handoff inspection and local compile/test/lint/static evidence support the findings. Residual uncertainty is limited mainly to unexecuted real sudo/PTY/CI-host behavior.
