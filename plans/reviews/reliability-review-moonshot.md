# Reliability implementation review R2

## Review

### Verdict

- **Blocker:** Do not close or archive the reliability plan. The integrated tree has release-gating defects in multi-step mutation handling and CI, plus unresolved M1–M6 acceptance failures.
- **Correct:** The plan is still marked **In progress**, and all milestone checkboxes remain open. That is truthful for the inspected state.
- **Fixed:** None. This was a read-only review; only this configured review artifact was written.

### What is already correct

- **Correct:** `cargo fmt`, full locked tests, no-default-feature tests, doc tests/docs, and full all-target Clippy pass locally. The full suite executed 48 tests once through the library; the binary no longer redeclares the module tree.
- **Correct:** Rendering entry points accept `&AppState`, and the inspected `src/ui/**` code performs no filesystem/process I/O (`src/ui/mod.rs:15-17`; render signatures throughout `src/ui/{users,groups,components}.rs`).
- **Correct:** Typed account parsing does not coerce malformed numeric IDs to zero (`src/sys/data_source.rs:154-180,337-342`), effective UID is injectable/fail-closed, root targets are checked, and command selection is closed over `KnownProgram`.
- **Correct:** Password records are omitted from argv/previews and written through the typed stdin path; secret wrappers are non-`Debug` and zeroized. Authentication-required is separately classified.
- **Correct:** Known-good account snapshots are retained on refresh failure (`src/sys/data_source.rs:86-98`; `src/app/mod.rs:652-668`).
- **Correct:** Basic theme/filter/keymap round trips and same-directory restricted atomic replacement are implemented and the current policy commands complete successfully.

## Code defects and plan-compliance findings

### F1 — Multi-step actions are not one operation plan and later required steps are predictably dropped

- **Severity:** **Blocker**
- **Affected:** `src/app/update.rs::prepare_next_request/execute_pending_plan/operation_requests`, lines `1079-1151`, `1175-1216`, `1245-1286`; `src/sys/mod.rs::prepare_operation` password branches, lines `413-437`; `src/sys/operations.rs::execute_plan`, lines `345-371`.
- **Violated requirement/failure mode:** M2 requires create-user, password+expiry, and bulk membership to be ordered steps of an `OperationPlan`, previewed from the same plan, failure-injected at every boundary, and reported/retried as one operation (`plans/planned/reliability-robustness.md:190-205`).
- **Evidence/reproduction:** The app expands one user action into a `VecDeque<OperationRequest>` and previews/executes only one request at a time. It continues only when `report.is_complete()` is true (`src/app/update.rs:1126-1133`). Password and expiry plans use opaque postconditions (`src/sys/mod.rs:417-436`); `AccountSourceReconciler` cannot satisfy an opaque check, so after a successful password child, `execute_plan` records `PostconditionFailed` and returns a partial report. Consequently:
  - `SetPassword { must_change: true }` never reaches `ExpireUserPassword`;
  - create-user with a password and “add to wheel” never reaches wheel membership after the password step;
  - bulk operations are separate confirmations/reports rather than one exact planned action.
  No test covers these application flows.
- **Minimal remediation:** Compile each user-visible multi-step action into one plan with all ordered steps and one preview/confirmation. Model shadow-observable postconditions or explicitly mark an unobservable step without treating successful execution as a reason to discard later required steps. Add boundary tests for create+password+group, password+expiry, and bulk membership.

### F2 — Production operation plans do not implement idempotent retry/precondition behavior

- **Severity:** **High**
- **Affected:** `src/sys/mod.rs::prepare_operation`, lines `157-439`; `src/sys/operations.rs::OperationPlan::require/execute_plan`, lines `190-208`, `299-343`; `tests/partial_failure.rs:48-77`.
- **Violated requirement/failure mode:** M2 requires retries to skip already-satisfied steps and safe remediation after partial completion (`reliability-robustness.md:193-198`).
- **Evidence/reproduction:** The retry mechanism is demonstrated only with a synthetic test-created plan. No production plan in `SystemAdapter::prepare_operation` calls `.require(...)`. Existing create targets are rejected as validation errors, and membership requests do not attach an already-present/already-absent precondition. Retrying a partially completed app action therefore reconstructs new independent requests rather than skipping observed completed work.
- **Minimal remediation:** Attach typed per-step idempotency conditions in every production plan and retain/rebuild the whole action plan from reconciled state. Test retries after every multi-step boundary using the real `OperationRequest` compiler.

### F3 — Shadow state is not three-state, and `chage -d 0` is parsed incorrectly

- **Severity:** **High**
- **Affected:** `src/search.rs::ShadowState/parse_shadow_records`, lines `20-50`, `80-109`; `src/ui/users.rs::render_user_details`, lines `83-94`; `tests/unit_test.rs::shadow_parser_is_deterministic`, lines `38-43`.
- **Violated requirement/failure mode:** D6/M3 require known, per-account unknown, and unavailable states, with “must change” observed honestly (`reliability-robustness.md:74,220-232`).
- **Evidence/reproduction:** `ShadowState` has only `Known(HashMap)` and `Unavailable`. If a selected account is absent from an otherwise readable shadow file, `status()` returns `None` but the UI prints the overall label `known`, not `unknown`. Also `expired_by_age` requires `last_change > 0`; `chage -d 0` writes a zero last-change value to force password change, so the parser reports that account as not expired/must-change. Existing tests use last-change `1`, not `0` or a missing account.
- **Minimal remediation:** Represent per-account `Unknown` separately from source `Unavailable`; treat last-change zero as must-change; add table-driven tests for missing records, malformed fields, `0`, locked/empty passwords, and absolute expiry.

### F4 — Terminal setup can leave the process in the alternate screen after partial initialization

- **Severity:** **High**
- **Affected:** `src/terminal.rs::TerminalSession::enter`, lines `21-40`.
- **Violated requirement/failure mode:** M1 requires restoration after every partial initialization and PTY/failure-injection proof (`reliability-robustness.md:167,176-182`).
- **Evidence/reproduction:** `execute!(stdout, EnterAlternateScreen, EnableMouseCapture)` combines two acquisitions. If entering the alternate screen succeeds and enabling mouse capture fails, the error branch only disables raw mode (`lines 24-26`) and never sends `LeaveAlternateScreen`. Cleanup errors in both initialization failure branches are discarded.
- **Minimal remediation:** Acquire/track raw mode, alternate screen, mouse capture, and cursor independently in a guard that exists before the second acquisition; unwind every acquired resource and preserve cleanup errors. Add the required PTY/failure-injection test target.

### F5 — Child cleanup still ignores or bypasses kill/reap failures

- **Severity:** **High**
- **Affected:** `src/sys/command.rs::run_child/wait_with_timeout/kill_and_reap`, lines `296-354`, `422-456`.
- **Violated requirement/failure mode:** M1 explicitly requires every spawn, stdin close/write, wait, timeout, kill, and reap result to be checked (`reliability-robustness.md:160`).
- **Evidence/reproduction:** `kill_and_reap` discards both `kill()` and `wait()` results. Missing stdout/stderr pipe and `wait_with_timeout` error paths return with `?` after spawn without a guaranteed kill/reap guard; reader-thread join errors can similarly return after the child path without aggregating cleanup status. There is no production benign-helper lifecycle test.
- **Minimal remediation:** Introduce a child-process RAII guard that always performs checked termination/reaping, explicitly closes stdin, joins both readers on every path, and returns a classified primary+cleanup failure. Add deterministic benign-helper tests for write failure, timeout, output limit, wait/read failures, and reap behavior.

### F6 — Rendering is immutable but not bounded on the required 10k/10k profile

- **Severity:** **High**
- **Affected:** `src/ui/groups.rs::render_group_details/render_group_modal/multi_users`, lines `56-96`, `150-225`, `243-254`; `src/ui/users.rs::member_groups/render_user_modal/available_groups/multi_choices`, lines `119-170`, `173-329`; `src/ui/components.rs::render_keybinds_panel`, lines `40-66`.
- **Violated requirement/failure mode:** M4/D7 require visible-row-only rendering, bounded allocations, and p95 render ≤16 ms for 10,000 users plus 10,000 groups (`reliability-robustness.md:78,247-267`).
- **Evidence/reproduction:** Tables are sliced to visible rows, but modal rendering clones and joins every eligible user/group into a fresh string every 100 ms. Group details perform nested member-to-user scans (`members × users_all`) every frame; a 10k-member/10k-user group can require roughly 100 million comparisons per frame. User “member of” rendering scans all groups and members each frame. No cache/visible slice bounds these paths.
- **Minimal remediation:** Precompute indexed/cached diagnostics and eligible identities in explicit effects/reducers; render only the visible modal page; add operation/allocation-count assertions and a real 10k/10k render benchmark.

### F7 — File/input bounds and injectable effect seams required by M0/M4 are incomplete

- **Severity:** **Medium**
- **Affected:** `src/sys/data_source.rs::LocalFileAccountDataSource::refresh`, lines `154-169`; `src/search.rs::read_shadow_state/apply_filters_and_search`, lines `55-63`, `114-123`; `src/app/{mod,filterconf,keymap}.rs` config readers at `mod.rs:99-102`, `filterconf.rs:49-52`, `keymap.rs:83-87`; `src/app/mod.rs::CachedDiagnostics::refresh_from`, lines `500-550`.
- **Violated requirement/failure mode:** M0 requires injected clock, config root, and diagnostic providers; M4 requires bounded system/config file sizes and allocations (`reliability-robustness.md:139-143,252-254`).
- **Evidence/reproduction:** Account, shadow, theme, filter, and keymap files are read wholesale with `read_to_string` and no byte/line limit. Diagnostic refresh directly accesses the clock/filesystem rather than an injected diagnostic provider and can perform 10,000 home metadata checks plus up to 640 MiB of authorized-key reads per refresh. `MAX_QUERY_BYTES` is enforced with `.chars().take(256)`, allowing up to 1,024 UTF-8 bytes; the only resource-bound test uses ASCII.
- **Minimal remediation:** Add bounded readers and explicit maximum record/file sizes; inject clock/config-root/diagnostic providers; cap total diagnostic work/bytes; truncate query at a valid UTF-8 byte boundary; test oversized and multibyte inputs.

### F8 — Pane selections remain indices and one pane is not normalized

- **Severity:** **Medium**
- **Affected:** `src/app/mod.rs::AppState`, lines `560-568`; `src/search.rs::apply_filters_and_search`, lines `114-120`, `187-219`; `src/ui/users.rs::render_user_groups`, lines `119-170`.
- **Violated requirement/failure mode:** M4 requires every pane’s selection/pagination to be keyed by stable identity with centralized normalization and invariant/property tests (`reliability-robustness.md:248-260`).
- **Evidence/reproduction:** All four selections are stored as indices. Filtering preserves only top-level user UID and group GID. Group-member index is merely clamped; user-group index is not normalized at all when the selected user/filter/refresh changes, so it can point past the new membership list and render an empty page. Tests check only that one render does not mutate five fields and that one selected user survives one search.
- **Minimal remediation:** Store stable user/group/member IDs per pane, normalize all panes after every transition, and add property/table tests across empty/filter/resize/refresh/delete and membership changes.

### F9 — The promised no-op feature removal was not performed

- **Severity:** **Medium**
- **Affected:** `Cargo.toml:27-29` (`file-parse = []`).
- **Violated requirement/failure mode:** Recorded D1 and M3 explicitly say to remove the empty `file-parse` feature and unsupported claims (`reliability-robustness.md:67,219`).
- **Evidence/reproduction:** The manifest still declares the empty feature. `cargo test --all-features` therefore gives false coverage signal for a feature with no behavior.
- **Minimal remediation:** Remove `file-parse` from the manifest and lock/documentation references, or implement a separately approved meaningful feature (not required by this plan).

### F10 — The required CI workflow fails on a clean runner toolchain installation

- **Severity:** **Blocker**
- **Affected:** `.github/workflows/rust.yml:27-43`.
- **Violated requirement/failure mode:** M6 requires reproducible required checks for MSRV/stable, format, and Clippy (`reliability-robustness.md:297-312`).
- **Evidence/reproduction:** The workflow installs each toolchain with `--profile minimal` but never adds `rustfmt` or `clippy`, then invokes both. I reproduced this with an isolated temporary `RUSTUP_HOME`: the installed components were only `cargo`, `rust-std`, and `rustc`; `cargo +1.89.0 fmt` failed with “cargo-fmt is not installed,” and Clippy failed analogously (both exit 1).
- **Minimal remediation:** Install `rustfmt` and `clippy` explicitly for each matrix toolchain (or use a fully SHA-pinned toolchain action/config that does so), then validate the workflow in CI/actionlint.

### F11 — Transitional public APIs still bypass the operation-plan contract and may retain a secret

- **Severity:** **Medium**
- **Affected:** `src/sys/mod.rs::SystemAdapter`, lines `89-150`, and legacy direct mutation methods `499-650`.
- **Violated requirement/failure mode:** The completion criteria require existing mutations to use typed plans/reports and secrets not to remain in long-lived application state; CONTRIBUTING now says callers must use the operation bridge.
- **Evidence/reproduction:** `with_sudo_password` remains public and stores a `SecretString` in `pending_secret` until some later elevation. Public legacy methods execute `CommandSpec` directly and return `Result<()>`, bypassing target binding, plan preview, postcondition reconciliation, and `OperationReport`. The app no longer calls them, but the trusted public boundary still exposes the unsafe architectural alternative.
- **Minimal remediation:** Remove or make the compatibility facade non-public/test-only after migration; expose only one-shot secret submission and prepared operation APIs.

### F12 — Config diagnostics/atomicity claims exceed current behavior and tests

- **Severity:** **Medium**
- **Affected:** `src/app/mod.rs::Theme::from_file`, lines `99-129`; `src/app/filterconf.rs::from_file`, lines `49-82`; `src/app/keymap.rs::from_file`, lines `83-108`; `tests/config_roundtrip.rs:9-81`; `README.md:30`.
- **Violated requirement/failure mode:** M3 requires parse diagnostics and interrupted-write old-or-new evidence; documentation says configuration errors are surfaced (`reliability-robustness.md:222-232`).
- **Evidence/reproduction:** Unknown theme/filter/keymap keys/actions are silently ignored, so misspellings are not surfaced. The test named `failed_atomic_write_leaves_old_file_complete` performs a successful replacement and asserts only the new value; it injects no failure/interruption. Keymap round-trip asserts only binding count, not binding equality.
- **Minimal remediation:** Return bounded source/line diagnostics for unknown/duplicate/invalid entries; compare full keymap bindings; add fault injection around write/flush/sync/rename/directory-sync and assert complete old-or-new contents plus temporary cleanup.

## Missing mandatory evidence (separate from code defects)

1. **Requested inputs absent:** repository-root `plan.md` and `progress.md` do not exist. Both handoffs acknowledge this. The canonical plan was available and reviewed, but the requested progress artifact could not be inspected.
2. **Required test targets absent:** `action_targeting`, `terminal_cleanup`, `shadow_status`, `config_atomicity`, and `ui_small_terminal` do not exist. Running each plan-specified `cargo test --test … --locked` command exited 101 with the available-target list. Some behaviors have weaker coverage under other target names; the mandated PTY/targeting/shadow/atomicity evidence does not.
3. **No performance evidence:** `benches/search_and_render.rs` is an autodiscovered harness target containing `fn main`; `cargo bench --bench search_and_render --locked` reported **0 tests, 0 measured** and did not produce latency/allocation/I/O measurements. It creates 10,000 users only—no groups—and never renders. No numeric p95 evidence exists.
4. **No required matrices/snapshots/properties:** only `ChangeUserShell` is exercised through a literal `OperationRequest` in tests; 11 other request variants have no bridge test. Authentication-denied/capability, missing executable, timeout, output-limit, partial-completion, and postcondition-failed variants lack direct deterministic assertions. There are no property-test or snapshot dependencies/usages, no every-step create-user/password+expiry/bulk failure matrix, and no classified-error/partial/stale/unknown-shadow Ratatui snapshots.
5. **No normal-test fail-closed enforcement:** current tests use fakes, but CI has no structural guard preventing a future test from constructing `LocalCommandRunner`; the “ordinary tests prove…” test demonstrates one fake path, not suite-wide enforcement. No isolated HOME/config-root/proc/effective-UID CI matrix is present.
6. **No real-tool/PTY release evidence:** both handoffs explicitly omit disposable VM/container account-database integrity checks and PTY terminal failure injection. This is mandatory before plan completion.
7. **Supply-chain dispositions incomplete:** current `cargo audit` exits 0 but reports unmaintained `paste` and unsound `lru`/target-specific dev `anyhow` warnings. `deny.toml` explicitly documents only the `paste` exception; duplicate Crossterm/Rustix families remain. The handoff discusses these, but there is no final committed report with owner/expiry/reachability or compatibility-upgrade evidence.
8. **Final governance artifacts absent:** `reports/reliability-robustness.html` does not exist; no prior review artifact/disposition was present in `plans/reviews/`; there is no evidence of two distinct-provider reviews with every finding dispositioned. This R2 artifact alone cannot satisfy that gate.

## Milestone/completion checkbox assessment

| Gate | Can close? | Reason |
|---|---|---|
| M0 | **No** | Clock/config-root/diagnostic seams and suite-wide host/tool enforcement are missing; command/target/error characterization is incomplete. |
| M1 | **No** | Partial terminal initialization and child cleanup are not proven/correct; required targeting/PTY tests are absent. |
| M2 | **No** | F1/F2: multi-step actions are split and later steps are dropped; production idempotency and failure matrices are absent. |
| M3 | **No** | F3/F9/F12: shadow unknown/must-change semantics, no-op feature removal, and config diagnostic/atomicity evidence remain. |
| M4 | **No** | F6–F8: render work is unbounded, stable pane identity is incomplete, process diagnostics were removed rather than cached, and no valid benchmark evidence exists. |
| M5 | **No** | Required matrices, property tests, snapshots, operation/error coverage, and root/non-root/environment equivalence evidence are absent. |
| M6 | **No** | F10 makes CI fail on a clean toolchain; real-tool evidence, final report/review dispositions, and supply-chain dispositions are incomplete. |
| Plan completion/archive | **No** | Completion criteria require all above gates, real-tool evidence, two dispositioned reviews, matching final docs/report, and archival. |

The current all-open checkboxes and `Status: In progress` should remain. The implementation has meaningful correct slices, but no milestone can truthfully be marked fully complete yet.

## Validation performed

Provider/model: **OpenAI / GPT-5.2**.

Commands run (read-only except build caches, advisory database, temporary clean-toolchain installation, and this allowed review artifact):

- Repository/diff inspection: `git status --short --branch`, `git diff --stat`, `git diff --name-status`, `git diff --numstat`, targeted/full-file `git diff`, `git diff --check`, `git diff --cached --name-only`, `git log -5`, `git ls-files --others --exclude-standard`, `find`, `rg`, `wc`, `nl`, and `git show HEAD:<path>` comparisons.
- `cargo fmt --all -- --check` — pass.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass (the W2 handoff’s three test-lint blockers are stale in the integrated tree).
- `cargo test --workspace --all-targets --all-features --locked` — pass, 48 tests total; benchmark harness reports zero tests.
- `cargo test --workspace --no-default-features --locked` — pass.
- `cargo test --doc --locked` — pass, zero doc tests.
- `cargo doc --workspace --all-features --no-deps --locked` — pass.
- `cargo deny check` — exit 0 with duplicate warnings.
- `cargo audit` — exit 0 with 3 allowed warnings (`paste`, `lru`, target-specific dev `anyhow`).
- `cargo tree --duplicates` — exit 0; duplicate Crossterm, Rustix/Linux raw sys, unicode-width, and related target families remain.
- `cargo bench --bench search_and_render --locked` — exit 0 but **0 tests/0 measured**, so no benchmark evidence.
- Plan-specified missing targets (`action_targeting`, `terminal_cleanup`, `shadow_status`, `config_atomicity`, `ui_small_terminal`) — each exit 101 because the target does not exist.
- Clean minimal 1.89.0 toolchain reproduction in isolated `/tmp` `RUSTUP_HOME` — only cargo/rustc/rust-std installed; format and Clippy commands each exit 1 because their components are missing.

## Limitations

- No privileged account tool, host mutation, real elevation, disposable VM/container, or destructive action was invoked, by instruction and plan safety policy.
- No PTY failure injection or live terminal lifecycle test was performed; the required target is absent.
- GitHub-hosted CI and `actionlint` were not run; the clean-toolchain component failure was reproduced locally.
- Review was performed on the Linux worktree at uncommitted `main` base `0b154c1`; no commit/PR metadata exists for this integrated change.
- Online source research was not needed. `cargo audit` updated its advisory data as part of the requested local policy validation.

**Confidence: 96/100.** The full worktree inventory/diff, canonical plan, both handoffs, implementation hotspots, all test targets, local quality gates, policy tools, missing named gates, and clean CI-toolchain failure were directly inspected or reproduced. Confidence is reduced only because privileged/PTY/disposable-environment behavior and hosted GitHub execution were intentionally not run.
