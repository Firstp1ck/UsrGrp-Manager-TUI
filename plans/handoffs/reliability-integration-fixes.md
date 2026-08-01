# W4 Application/Release Hardening Handoff

## 1. Status, scope, and plan validation

- **Workstream:** W4 application/release hardening, sequential writer 2 of 2.
- **Status:** Implemented in the authorized W4 boundary; no commit or staging was performed. The W3 contracts were inspected and consumed without modifying W3-owned core paths.
- **Revision/worktree:** Shared dirty worktree based on `0b154c1c3a6a889b495ab50c268b5c3ae491087a`. Existing user changes, canonical plan/reviews, prior handoffs, `.gitignore`, deleted superseded docs, Cargo manifest/lock, W3 source/tests/fixtures, and `src/search.rs` were preserved.
- **Plan inputs:** Requested root `context.md` and `plan.md` remain absent. Per accepted R2-ME1, `plans/planned/reliability-robustness.md` is canonical; it, both round-1 reviews, the W3 handoff, and the actual W3 composite/policy/report seam were read before edits.
- **Classification:** Complex remains validated: this slice spans secret lifetime and composite execution, protected-policy presentation, terminal resource unwind, bounded effect providers/config durability, selection invariants/render bounds, deterministic tests/benchmarking, CI policy, supply-chain evidence, and user documentation.

## 2. Changed files and implementation

### Application, policy, selection, diagnostics, and rendering

- `src/app/mod.rs`, `src/app/update.rs` — replace general `pending_requests`/`pending_plan` queues with one `PreparedOperation` capability. Multi-step create/password/membership and bulk membership now compile to one W3 `OperationRequest::Composite`, one redacted confirmation, and one aggregate report. `OneShotPassword` is non-cloneable and is consumed only while preparing the trusted plan; cancel/error drops the dedicated prepared operation/capability and no password enters general request/report state. The app mirrors W3’s default fail-closed protected policy in presentation, retains W3 preparation as enforcement, and reports protected user/group actions honestly.
- `src/app/mod.rs` — adds injected `Clock`, `ConfigRootProvider`, and `DiagnosticProvider` seams; `SystemDiagnosticProvider` bounds users, authorized-key bytes, total diagnostic bytes, group member diagnostics, and UI config messages. It precomputes group detail summaries outside rendering and retains stale/config state. Every pane now has a stable UID/GID/member identity source of truth plus centralized index normalization.
- `src/ui/{components,users,groups}.rs` — group detail rendering consumes cached summaries rather than nested user/group scans. Confirmation and candidate modal rendering are bounded to visible 12-row slices and 1,024 candidates/preview steps; large candidate strings are not built per frame. User details retain W3 per-account shadow distinction through cached state.

### Terminal and configuration

- `src/terminal.rs` — introduces independently tracked raw mode, alternate screen, and mouse capture through injectable `TerminalControl`/`TerminalResources`. Every partial acquisition failure attempts reverse cleanup for every acquired capability; cleanup attempts all resources even after a cleanup error and preserves primary/cleanup context.
- `src/config/mod.rs` — provides bounded UTF-8 reads, bounded source-line-aware assignment parsing, and `atomic_write_with_fault` stages for deterministic write/flush/file-sync/rename/directory-sync failures. Temporary cleanup and complete old-or-new outcomes are asserted by tests.
- `src/app/{filterconf,keymap}.rs` — use bounded config parsing; reject unknown/duplicate/invalid entries with line diagnostics. Keymap canonical serialization now preserves shifted BackTab distinctly, enabling full binding equality instead of a count-only round trip.

### CI, policy, benchmark, docs

- `.github/workflows/rust.yml` — explicit minimal-toolchain installation now adds `rustfmt` and `clippy`; a declared tested-policy step states locked/all-feature/no-default/format/Clippy/docs/no-real-test-process expectations. Existing full-SHA action pins, permissions, cancellation, and timeouts remain intact.
- `deny.toml` — records `paste` owner/reachability/review expiry and references the still-reviewed `lru` and target-specific dev `anyhow` warnings without forcing an unreviewed upgrade.
- `benches/search_and_render.rs` — adds a deterministic release measurement over 10,000 users plus 10,000 groups, 100 search+immutable-frame samples, and printed D7 p95 evidence.
- `README.md`, `SECURITY.md`, `CONTRIBUTING.md` — document fail-closed policy, composite/report semantics, bounded config diagnostics, deterministic injection/static-guard requirements, benchmark invocation, and external release gates accurately.

## 3. Tests and evidence added/updated

- Added exact targets:
  - `tests/action_targeting.rs` — table-style stable identity restoration through reorder/removal and W3 default protected-policy presentation.
  - `tests/terminal_cleanup.rs` — injected terminal acquisition/cleanup failure matrices with no PTY or host-terminal mutation.
  - `tests/config_atomicity.rs` — fault injection at every atomic write boundary; pre-rename complete-old and post-rename complete-old-or-new equality/temporary cleanup evidence.
  - `tests/ui_small_terminal.rs` — deterministic TestBackend fallback snapshot and immutable identity proof.
  - `tests/operation_retry_matrices.rs` — table-driven add/remove retry skips before elevation/runner invocation.
- Updated `tests/config_roundtrip.rs` for complete keymap binding equality and unknown/duplicate/invalid source-line diagnostics.
- Updated `tests/resource_bounds.rs` with injected fixed clock/config root/counting diagnostics evidence; existing W3-owned `tests/shadow_status.rs` remains the exact shadow target and was not edited.
- Added application unit coverage that password+expiry compiles as one composite and consumes its one-shot capability.
- The existing W3 `tests/core_static_guards.rs` now covers all newly added integration tests in the suite-wide normal-test process/runner guard; it passed.

## 4. Commands and validation outcomes

| Command | Exit | Result |
|---|---:|---|
| `cargo fmt --all -- --check` | 0 | Final formatting passed. |
| `cargo check --workspace --all-targets --all-features --locked` | 0 | Locked all-target/all-feature check passed. |
| `cargo test --workspace --all-targets --all-features --locked` | 0 | Full deterministic suite passed, including every named target and static guard. |
| `cargo test --workspace --no-default-features --locked` | 0 | No-default suite passed. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 | Full Clippy passed with warnings denied. |
| `cargo +1.89.0 test --workspace --all-targets --all-features --locked` | 0 | Documented MSRV suite passed. |
| `cargo test --doc --locked && cargo doc --workspace --all-features --no-deps --locked` | 0 | Doc tests and docs passed. |
| `cargo test --test action_targeting --test terminal_cleanup --test shadow_status --test config_atomicity --test ui_small_terminal --test resource_bounds --test operation_retry_matrices --locked` | 0 | All exact acceptance targets passed. |
| `cargo test --release --bench search_and_render -- --nocapture` | 0 | D7 numeric evidence: 100 samples, search p95 **2.302 ms** vs 50 ms; render p95 **0.119 ms** vs 16 ms; both PASS. |
| `cargo deny check` | 0 | Policy passed with documented duplicate-family warnings. |
| `cargo audit` | 0 | Completed with three allowed/reviewed warnings: `paste`, `lru`, target-specific dev `anyhow`. |
| `cargo tree --duplicates` | 0 | Existing Crossterm/Rustix/unicode-width duplicate families reported; no forced upgrade made. |
| Static UI-I/O, normal-test process/runner, and full-SHA workflow scans | 0 | No UI filesystem/process calls, no normal-test production process construction, and no unpinned workflow action found. |
| `actionlint` | unavailable | Not installed on this worker; workflow YAML was statically scanned, but hosted/actionlint execution remains pending. |
| `git diff --check && git diff --cached --name-only` | 0 | No whitespace errors and no staged files. |

No sudo, account-management executable, host account mutation, deployment/publication, or destructive Git operation was run. The command-lifecycle helper remains W3’s benign current-test executable only.

## 5. Residual risks and external gates

1. **External release gates remain pending, never passed:** real privileged account-tool/account-database integrity validation in an explicitly approved disposable Linux environment and real PTY terminal validation. Normal tests intentionally use fakes/TestBackend only.
2. **Benchmark manifest constraint:** The original W4 boundary forbids `Cargo.toml` edits, so this run could not explicitly declare `[[bench]] harness = false`. The existing auto-discovered bench target executes its release numeric test through `cargo test --release --bench ...`; `cargo bench --bench search_and_render` still uses Cargo’s standard harness and does not execute `main`. Numeric D7 evidence is real and reproducible, but the literal `harness = false` manifest declaration remains a parent ownership decision outside the approved W4 write boundary.
3. **Supply chain:** `cargo audit` warns about `paste` via Ratatui, `lru` via Ratatui, and target-specific dev `anyhow` via Tempfile/WASI; duplicate families remain. `deny.toml` records owner/reachability/2027-01-31 review expiry for the narrow ignored `paste` advisory. No dependency upgrade was forced without compatibility review.
4. **Hosted workflow syntax:** `actionlint` was unavailable locally. CI now installs the required rustfmt/Clippy components, but a reviewer/CI run should execute the workflow.
5. **Review gate:** A fresh required reviewer must inspect the integrated W3/W4 diff and this evidence before plan closure. Do not archive the canonical plan or claim the external release gates passed.

## 6. Integration notes and next step

- Future application mutations must keep using `OperationRequest` plus W3 `prepare_operation`/`execute_prepared_operation`; do not restore request queues, direct commands, or stored credentials. `PreparedOperation` is the sole confirmation capability and must be dropped on cancel/error.
- Use `AppState::with_dependencies` for deterministic tests; production uses system providers only through explicit refresh/load effects. New diagnostics should be bounded/cached before rendering.
- Treat `selected_*_uid/gid/member_name` as selection truth; call centralized `sort_and_filter`/`normalize_selections` after transitions rather than introducing pane-index action targeting.
- Request the required independent reviewer now. The reviewer should specifically assess the application one-shot plan lifetime, whether the default protected UI presentation matches policy exceptions, the literal benchmark-harness boundary conflict, and external release gates.

**Confidence: 92/100.** Final locked full/no-default/MSRV tests, docs, full Clippy, exact focused targets, static guards, release numeric benchmark, policy/audit/tree, diff hygiene, and direct W3 seam inspection support the implementation. Confidence is reduced by the intentionally unexecuted privileged/PTY external gates, unavailable actionlint, reviewed supply-chain warnings, and the manifest-bound `harness = false` limitation.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "W4 changes are confined to the approved app/config/terminal/UI/bench/tests/CI/docs/policy boundary, consume W3 public APIs, and preserve forbidden core, manifest/lock, plan/review, .gitignore, and deleted-doc paths."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "This handoff records changed paths, focused target/matrix evidence, final locked commands, numeric benchmark output, static scans, supply-chain results, residual gates, and no-staged-files proof."
    }
  ],
  "changedFiles": [
    ".github/workflows/rust.yml",
    "CONTRIBUTING.md",
    "README.md",
    "SECURITY.md",
    "benches/search_and_render.rs",
    "deny.toml",
    "src/app/filterconf.rs",
    "src/app/keymap.rs",
    "src/app/mod.rs",
    "src/app/update.rs",
    "src/config/mod.rs",
    "src/terminal.rs",
    "src/ui/components.rs",
    "src/ui/groups.rs",
    "src/ui/users.rs",
    "tests/action_targeting.rs",
    "tests/config_atomicity.rs",
    "tests/config_roundtrip.rs",
    "tests/operation_retry_matrices.rs",
    "tests/resource_bounds.rs",
    "tests/terminal_cleanup.rs",
    "tests/ui_small_terminal.rs",
    "plans/handoffs/reliability-integration-fixes.md"
  ],
  "testsAddedOrUpdated": [
    "src/app/update.rs unit tests",
    "tests/action_targeting.rs",
    "tests/config_atomicity.rs",
    "tests/config_roundtrip.rs",
    "tests/operation_retry_matrices.rs",
    "tests/resource_bounds.rs",
    "tests/terminal_cleanup.rs",
    "tests/ui_small_terminal.rs",
    "benches/search_and_render.rs"
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
      "summary": "Full deterministic suite passed, including all named acceptance targets and static guards."
    },
    {
      "command": "cargo test --workspace --no-default-features --locked",
      "result": "passed",
      "summary": "No-default-feature suite passed."
    },
    {
      "command": "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
      "result": "passed",
      "summary": "All-target Clippy passed with warnings denied."
    },
    {
      "command": "cargo +1.89.0 test --workspace --all-targets --all-features --locked",
      "result": "passed",
      "summary": "Documented MSRV suite passed."
    },
    {
      "command": "cargo test --doc --locked && cargo doc --workspace --all-features --no-deps --locked",
      "result": "passed",
      "summary": "Doc tests and docs passed."
    },
    {
      "command": "cargo test --release --bench search_and_render -- --nocapture",
      "result": "passed",
      "summary": "100-sample 10k-user+10k-group D7 benchmark printed search p95 2.302 ms and render p95 0.119 ms, both within limits."
    },
    {
      "command": "cargo deny check && cargo audit && cargo tree --duplicates",
      "result": "passed",
      "summary": "Policy passed; audited warnings and duplicates are documented residual risks."
    },
    {
      "command": "static UI-I/O/no-real-tool/full-SHA workflow scans; git diff --check; git diff --cached --name-only",
      "result": "passed",
      "summary": "No UI host I/O, no normal-test runner/process construction, immutable action pins, no whitespace errors, and no staged files."
    },
    {
      "command": "actionlint",
      "result": "not-run",
      "summary": "Unavailable on this worker."
    }
  ],
  "validationOutput": [
    "W3 composite, retry, policy, shadow, and report contracts are consumed by W4 without trusted-core edits.",
    "Release D7 evidence: samples=100, search p95=2.302 ms/50 ms, render p95=0.119 ms/16 ms; both PASS.",
    "No privileged account tool, sudo, host account mutation, deployment, or publication ran."
  ],
  "residualRisks": [
    "Disposable-environment real account-tool/account-database and real PTY release gates remain external pending checks.",
    "The literal Cargo manifest harness=false declaration cannot be made within the forbidden Cargo.toml boundary; release numeric evidence runs through cargo test --release --bench instead.",
    "actionlint is unavailable locally.",
    "paste/lru/target-specific dev anyhow warnings and duplicate dependency families remain reviewed but unupgraded.",
    "Required independent reviewer gate remains pending."
  ],
  "noStagedFiles": true,
  "diffSummary": "W4 consumes W3 composite/policy/report APIs with one-shot app capabilities, independently unwound terminal resources, bounded injected diagnostics/configuration, stable pane identities, visible modal slices, exact deterministic targets, release D7 evidence, clean CI components, and honest supply-chain/docs updates.",
  "reviewFindings": [
    "no new W4-source blocker found by final locked checks, exact targets, full Clippy, static safety scans, or release benchmark",
    "review required: inspect one-shot prepared-plan lifetime and manifest-bound benchmark harness limitation",
    "external pending: disposable real-tool/account-database and PTY release evidence"
  ],
  "manualNotes": "Pre-existing dirty shared-worktree changes were preserved. actionlint is not installed; cargo audit/network advisory refresh ran as an explicitly requested validation command. Confidence: 92/100."
}
```
