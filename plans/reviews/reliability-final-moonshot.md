# Final reliability validation review (round 2, reviewer lane: moonshot)

**Scope:** Final independent read-only validation of the integrated W1–W4 reliability work against the canonical plan `plans/planned/reliability-robustness.md`, its round-1 dispositions, both handoff pairs, and the exact acceptance targets. No project/source file was modified; only this configured review artifact was written. No privileged account tool, sudo invocation, host-account mutation, or destructive Git operation was run.

**Reviewer provider/model:** Not verifiable from inside this session. Round-1 runtime evidence showed artifact names did not match the actually resolved model (`openai-codex/gpt-5.6-sol` for both lanes). This lane was launched as `reliability-final-moonshot`, but the actual provider/model must be confirmed by the integration owner from runtime metadata before the provider-diversity gate (R2-ME8) is treated as satisfied. This review does not self-attest diversity.

**Worktree:** shared dirty worktree on `main` at base `0b154c1`; zero staged files (`git diff --cached --name-only` → empty); `git diff --check` clean.

## Review

### Correct (verified against the actual worktree)

- **Correct — no-real-tool enforcement is deterministic and passing.** `tests/core_static_guards.rs:1-60` recursively scans every `tests/**.rs` source for `LocalCommandRunner`, `std::process::Command`, `Command::new(`, `Command::spawn(` (runtime-composed patterns) and asserts the public adapter exposes no legacy mutation/password facade. It passes. An independent grep of `tests/` confirmed no production process boundary. The only spawn anywhere in the suite is `src/sys/command.rs:540-565`, which re-executes the current test binary with `--ignored` and proves timeout → kill → reap; no account tool is named. Suite-wide enforcement required by R2-ME5 is therefore present for integration tests; see Note 5 for the remaining unit-test seam.
- **Correct — numeric 10k/10k D7 benchmark is real and reproducible.** `Cargo.toml:17-19` now declares `[[bench]] harness = false`, so the plan-literal `cargo bench --bench search_and_render` executes `main` and prints evidence. Local release run: `samples=100 search_p95_ms=1.902 limit_ms=50 status=PASS render_p95_ms=0.145 limit_ms=16 status=PASS` on 10,000 fixture users + 10,000 fixture groups (`benches/search_and_render.rs:17-105`). This resolves round-1 R2-ME3 and W4 residual-risk #2 (the manifest declaration the W4 boundary could not make has since been applied and works).
- **Correct — composite operations and one-shot secrets.** `src/app/update.rs:1207-1351` compiles create+password+sudo-group, password+expiry, and bulk membership into one `OperationRequest::Composite` prepared once (`prepare_request`, `src/app/update.rs:1136-1159`), previewed from the same plan, executed once, and reported as one aggregate report. Passwords exist only as a non-cloneable `OneShotPassword` consumed during preparation (`src/app/mod.rs:681-693`, `1023-1027`); cancel/error paths call `clear_pending_operation` (`src/app/mod.rs:1030-1035`), and the adapter consumes the elevation secret per execution (`src/sys/mod.rs:1073-1087`). Round-1 R1-1/R1-6/R2-F1/F11 are verifiably resolved.
- **Correct — retry/idempotency is production-wired.** Every production plan in `src/sys/mod.rs:260-547` attaches typed `skip_if_satisfied` conditions (13 sites), and `tests/operation_retry_matrices.rs` proves retry skips already-satisfied steps before elevation/runner invocation. R1-2/R2-F2 resolved.
- **Correct — protected-identity policy.** `src/sys/mod.rs:104-191` implements injectable `ProtectedIdentityPolicy` with fail-closed defaults (UID/GID <1000, `sudo`/`wheel` membership) and unconditional root user/group/name blocking (`src/sys/mod.rs:148-183`). R1-4 resolved; D4 satisfied.
- **Correct — terminal lifecycle.** `src/terminal.rs:53-119` tracks raw mode/alternate screen/mouse independently, unwinds every acquired capability on partial acquisition failure, preserves primary+cleanup error context, and `Drop` is the panic backstop; `src/main.rs` returns `ExitCode::FAILURE` on init/run/cleanup failure. `tests/terminal_cleanup.rs` (2 tests) covers injected failure matrices. R1-5/R2-F4 resolved at the injection level; real-PTY evidence remains the external R2-ME6 gate.
- **Correct — child lifecycle.** `src/sys/command.rs:296-370` checks spawn, stdin write, pipe acquisition, bounded readers, wait/timeout, kill, and reap; `terminate_and_reap` (`src/sys/command.rs:474-497`) attempts kill and reap independently and preserves cleanup context in `PartialCompletion`. R1-7/R2-F5 resolved.
- **Correct — shadow semantics.** `src/search.rs:41-90` implements per-account `Known`/`Unknown`/`Unavailable`; `last_change == 0` is must-change (`src/search.rs:161-162`); shadow-dependent filters refuse to claim success when any visible account is unknown (`src/search.rs:222-238`). `tests/shadow_status.rs` (5 tests) passes. R1-9/R2-F3 resolved; D6 satisfied.
- **Correct — config durability/diagnostics.** `src/config/mod.rs:115-166` implements restricted same-directory temp file, flush, `sync_all`, rename, directory sync, symlink refusal, and `atomic_write_with_fault` injection at all five stages; `tests/config_atomicity.rs` (2 tests) proves complete-old-or-new and temp cleanup. Bounded reads (`MAX_CONFIG_BYTES`, `MAX_CONFIG_LINE_BYTES`) and source-line diagnostics are real; keymap round trip now asserts full binding equality. R1-10(config part)/R2-F12 resolved.
- **Correct — UI purity and bounds.** Static scan of `src/ui/**` and `src/terminal.rs` found no filesystem/process I/O (only string literals). Render functions take `&AppState`; tables slice to visible rows; modals bound candidates to 1,024 and rows to 12 (`src/ui/groups.rs:235-248`, `src/ui/users.rs:289-339`); group details consume cached summaries. Selections are identity-keyed (`selected_user_uid`/`selected_group_gid`/`selected_user_group_gid`/`selected_group_member_name`, `src/app/mod.rs:725-735`) with centralized `normalize_selections` (`src/app/mod.rs:937`). R1-10(render part)/R2-F6(render)/F8 resolved; see Note 4 for one residual per-frame scan.
- **Correct — snapshot retention.** `refresh_retaining` (`src/sys/data_source.rs:80-108`) keeps prior snapshots as `Stale`; refresh failure cannot empty a known-good list (`src/app/mod.rs:850-864`). Baseline P0-4 resolved.
- **Correct — manifest/MSRV/CI.** `rust-version = "1.89"` declared (`Cargo.toml:5`); empty `file-parse` feature removed; `libc`/`zeroize` documented as D9-approved. CI (`.github/workflows/rust.yml:24-45`) installs both 1.89.0 and stable minimal toolchains **plus explicit `rustfmt`/`clippy` components** (round-1 blocker F10/R1-11 resolved), gates fmt/build/test/all-features/no-default/docs/Clippy with `--locked`, keeps full-SHA checkout pins, `contents: read`, concurrency cancellation, and job timeouts. Local `cargo +1.89.0 test --workspace --all-targets --all-features --locked` passed (80 tests). Dependabot automation exists (`.github/dependabot.yml`).
- **Correct — supply chain commands pass with documented dispositions.** `cargo deny check` exit 0 (`advisories ok, bans ok, licenses ok, sources ok`); `deny.toml` records the `paste` exception with owner and 2027-01-31 review expiry and references the `lru`/target-specific dev `anyhow` warnings. `cargo audit` exit 0 with 3 allowed warnings (confirmed locally: `lru` RUSTSEC-2026-0002 via ratatui among them). `cargo tree --duplicates` exit 0; duplicate crossterm 0.28/0.29, rustix, linux-raw-sys, unicode-width families remain as documented non-forced deferrals (R2-ME7 disposition).
- **Correct — test-suite quality.** 80 test executions pass once through the library (binary consumes `usrgrp_manager` lib; no duplicate module tree). No tautology patterns (`is_ok() || is_err()`, `assert!(true)`) found in `tests/` or `src/`. All 19 named acceptance targets exist and pass individually, including the round-1-missing `action_targeting`, `terminal_cleanup`, `shadow_status`, `config_atomicity`, `ui_small_terminal` (R2-ME2 resolved).
- **Correct — docs.** README line 7 states Linux-local scope (no NSS/LDAP/cross-platform claims); line 49 documents the exact D7 benchmark and limits; SECURITY/CONTRIBUTING describe the fail-closed policy and deterministic-test requirements. Superseded `docs/Improvements.md`/`docs/roadmap.md` are deleted with no dangling references.
- **Correct — plan truthfulness.** The canonical plan remains `Status: In progress` with all milestone checkboxes open and round-1 dispositions recorded with evidence; handoffs accurately describe their boundaries (verified W3/W4 file lists against the diff).

### Fixed

- None by this review (read-only). Round-1 findings R1-1…R1-12 and R2-F1…F12/ME2/ME4/ME5/ME7 were re-verified as resolved by the W3/W4 fixes; no regression found.

### Blocker

- **None.** No code defect was found that must be fixed before the integration owner proceeds to milestone closure bookkeeping. The remaining items below are external-evidence or residual-risk items, not code defects.

### Note — residual risks and external (non-code) gates

1. **Note (external evidence, plan completion gate) — real-tool/PTY validation still absent.** R2-ME6's accepted external gate (disposable-environment account-database integrity with real shadow-utils, and real PTY terminal failure injection) has not been executed by design; no host mutation was authorized. Until it is, the plan's completion criteria ("Real-tool behavior has been validated in disposable environments") and archival cannot truthfully close. This is unavailable external evidence, not a code defect.
2. **Note (external evidence, plan completion gate) — final report and provider diversity.** `reports/reliability-robustness.html` does not exist yet (`reports/` is empty). Provider-diverse review quorum depends on runtime metadata confirming this lane's and the sibling lane's actual models — round 1 proved names are insufficient. Both are integration-owner tasks, not code defects.
3. **Note (Medium, external CI evidence) — workflow not executed on a hosted runner.** The rust.yml fix (explicit rustfmt/clippy components) is logically correct and locally reproduced as passing with both toolchains, but GitHub-hosted execution has not run, and `actionlint` is unavailable on this host (`which actionlint` → not found). First CI run on the real PR should be watched. Residual risk: low.
4. **Note (Low) — one un-cached per-frame scan remains.** `member_groups` (`src/ui/users.rs:159-170`) rebuilds the selected user's group list by scanning all filtered groups × their member lists on every frame (O(groups·members)); only the row rendering is sliced, not the collection. The D7 fixture's memberships are sparse (every 100th group has one member), so the 0.145 ms render p95 does not stress dense-membership hosts. Within the D7 fixture contract this passes; on a host with 10k groups × large member lists this path could exceed the 16 ms budget. Minimal remediation if desired: cache per-user membership in `CachedDiagnostics` like group summaries, or extend the bench fixture with dense memberships to bound the claim.
5. **Note (Low) — static guard covers integration tests only.** `tests/core_static_guards.rs` scans `tests/` but not `src/**` unit tests. Currently the only `src` spawn is the benign self-re-execution helper (`src/sys/command.rs:540-565`), which is safe and asserted. A future lib unit test constructing `LocalCommandRunner` would not be caught by the guard (only by code review). Minimal remediation: extend the scan to `src/` with an allowlist entry for the documented benign helper, or move the helper behind a test-only feature gate.
6. **Note (Low, unverifiable here) — root/non-root result identity.** M5's exit gate requires identical results as root/non-root. All identity is injected (`FixedIdentityProvider`), so host UID cannot affect results by construction, and the suite passed as UID 1000 locally; a literal root re-run was not performed (no privilege escalation authorized). Risk: negligible by design, but the literal evidence is absent.
7. **Note (informational) — benchmark harness nuance resolved but dual-path.** Both `cargo bench --bench search_and_render` (runs `main`, prints D7 evidence) and `cargo test --release --bench search_and_render` (runs the `#[test]` wrapper) work; the plan's literal M4 gate command is now satisfiable. No action needed.

## Milestone/checklist closure assessment (evidence-based)

| Gate | Can truthfully close? | Evidence |
|---|---|---|
| M0 | **Yes** | Injectable source/runner/clock/config-root/diagnostic seams verified (`src/app/mod.rs:742-744`, `src/sys/mod.rs:214-245`); fixtures in `tests/fixtures/`; suite-wide guard passes; `AppState::new()`/`with_dependencies` are pure. Exit command passes (80 tests). |
| M1 | **Yes** (PTY leg via external R2-ME6 gate) | Checked eUID (`src/sys/identity.rs:17-23`), no password argv/`bash -c` anywhere, zeroizing non-Debug secrets, validated newtypes (`src/sys/validation.rs`), typed errors, sudo prompt only on `AuthenticationRequired` (`src/app/update.rs:1179-1187`), targeting fixes with identity-bound confirmation, D4 policy, RAII terminal, lib-consuming main, Clippy `-D warnings` clean. All four M1 exit commands pass. |
| M2 | **Yes** | Composite plans/reports, per-step typed skip/postcondition, reconciliation with stale retention, failure matrices (`operation_bridge` 14 tests, `partial_failure`, `operation_reports`), dry-run from same plan (`dry_run_equivalence`), partial-success UI message. All four exit commands pass. |
| M3 | **Yes** | Typed parsing rejects malformed IDs (`tests/account_parsing`), D1 file backend with feature/claims removed, three-state shadow, bounded config with diagnostics and fault-injected atomicity, keymap full-equality round trip. All four exit commands pass. |
| M4 | **Yes** (with Note 4) | Update module split into reducer-style handlers; identity-keyed selections + normalization; read-only renderers; pagination fixes; cached diagnostics; bounded inputs/outputs; numeric D7 benchmark PASS via the literal `cargo bench` command. All exit commands pass. |
| M5 | **Yes** (with Note 6) | No tautologies; single library execution; table-driven matrices; redaction assertions (`secret_redaction`); deterministic construction; parallel-safe (suite passes default parallel). Full/no-default/doc commands pass. |
| M6 | **No — external items pending** | Local gates all pass (fmt/clippy/build/test/no-default/docs/deny/audit/tree; MSRV declared and locally tested; SHA pins; timeouts; dependabot; docs corrected). Open: hosted-CI execution unobserved (actionlint unavailable locally), disposable-environment real-tool validation (R2-ME6), final HTML report, provider-diversity confirmation. These are explicitly external/integration-owner items, not code defects. |
| Plan completion/archive | **No** | Requires M6 completion plus real-tool evidence, final report, dispositioned diverse reviews, and then archival by the integration owner. |

## Validation performed (this review)

Read-only inspection plus safe build/test/bench commands (all as UID 1000, no sudo, no account tools):

- `git status`, `git diff --cached --name-only` (0 staged), `git diff --check` (clean), `git diff Cargo.toml`, `git log`.
- `cargo fmt --all -- --check` — pass.
- `cargo check --workspace --all-targets --all-features --locked` — pass.
- `cargo test --workspace --all-targets --all-features --locked` — pass, 80 tests, 0 failed (run 3× for counting; identical).
- `cargo test --workspace --no-default-features --locked` — pass, 80 tests.
- `cargo +1.89.0 test --workspace --all-targets --all-features --locked` — pass, 80 tests (documented MSRV).
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass.
- `cargo doc --workspace --all-features --no-deps --locked` — pass; `cargo test --doc --locked` — pass (0 doc tests).
- `cargo bench --bench search_and_render` — pass; printed `samples=100 search_p95_ms=1.902/50 PASS render_p95_ms=0.145/16 PASS`.
- All 19 named `cargo test --test <target> --locked` acceptance targets — pass individually (counts recorded in session output).
- `cargo deny check` — exit 0; `cargo audit` — exit 0, 3 allowed warnings; `cargo tree --duplicates` — exit 0, documented duplicate families.
- Static scans: UI/terminal I/O grep (clean), tests process-boundary grep (clean), tautology grep (clean), superseded-doc reference grep (clean).
- `actionlint` — not run (not installed on this host).

**Limitations:** no privileged/real-tool execution (prohibited), no PTY injection, no hosted CI observation, no root re-run, reviewer provider/model not self-verifiable. `cargo audit` used the local advisory DB already present on this host.

**Confidence: 93/100.** Every claim above is backed by direct file/line inspection or a locally executed command with recorded output; the round-1 disposition list was re-verified item by item against the integrated tree. Confidence is reduced by the intentionally unexecuted external gates (real-tool/PTY/hosted CI), inability to self-verify reviewer provider/model (relevant to R2-ME8), and the un-stressed dense-membership render path (Note 4).

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Severity-tagged findings with file/line evidence above: no blockers; Notes 1-7 with paths (src/ui/users.rs:159-170 residual per-frame scan; tests/core_static_guards.rs unit-test seam; external R2-ME6/report/diversity gates) plus a verified Correct list covering no-real-tool enforcement, D7 benchmark, composites/secrets, retry, policy, terminal, child lifecycle, shadow, config, UI, CI, supply chain, and docs."
    }
  ],
  "changedFiles": [
    "plans/reviews/reliability-final-moonshot.md"
  ],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "cargo fmt --all -- --check",
      "result": "passed",
      "summary": "Formatting clean."
    },
    {
      "command": "cargo test --workspace --all-targets --all-features --locked",
      "result": "passed",
      "summary": "80 tests passed, 0 failed; single library execution."
    },
    {
      "command": "cargo test --workspace --no-default-features --locked",
      "result": "passed",
      "summary": "80 tests passed."
    },
    {
      "command": "cargo +1.89.0 test --workspace --all-targets --all-features --locked",
      "result": "passed",
      "summary": "Documented MSRV 1.89 suite passed (80 tests)."
    },
    {
      "command": "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
      "result": "passed",
      "summary": "Warnings denied; clean."
    },
    {
      "command": "cargo doc --workspace --all-features --no-deps --locked && cargo test --doc --locked",
      "result": "passed",
      "summary": "Docs build; 0 doc tests."
    },
    {
      "command": "cargo bench --bench search_and_render",
      "result": "passed",
      "summary": "D7 numeric evidence: samples=100, search p95 1.902 ms/50 ms PASS, render p95 0.145 ms/16 ms PASS on 10k users + 10k groups."
    },
    {
      "command": "cargo test --test <each of 19 named acceptance targets> --locked",
      "result": "passed",
      "summary": "All plan-named targets exist and pass individually."
    },
    {
      "command": "cargo deny check && cargo audit && cargo tree --duplicates",
      "result": "passed",
      "summary": "Policy clean; 3 allowed audit warnings with documented dispositions; duplicates documented, not forced."
    },
    {
      "command": "static scans (UI I/O, tests process boundary, tautologies, stale doc refs); git diff --check; git diff --cached --name-only",
      "result": "passed",
      "summary": "No UI host I/O, no production process boundary in tests, no tautologies, no dangling doc refs, no whitespace errors, no staged files."
    },
    {
      "command": "actionlint",
      "result": "not-run",
      "summary": "Not installed on this host; workflow verified by inspection and local reproduction of each step instead."
    }
  ],
  "validationOutput": [
    "No blocker found in the integrated W1-W4 worktree; all locally executable plan exit-gate commands pass.",
    "D7 benchmark via literal plan command: 10k/10k fixture, 100 samples, search p95 1.902 ms, render p95 0.145 ms, both within limits.",
    "Deterministic no-real-tool enforcement present and passing (core_static_guards + independent grep); only benign self-re-execution helper spawns.",
    "Milestones M0-M5 can truthfully close; M6/plan completion remain gated on external items (hosted CI observation, disposable real-tool/PTY validation, final HTML report, provider-diversity confirmation)."
  ],
  "residualRisks": [
    "External gate: disposable-environment real shadow-utils and PTY validation (R2-ME6) not executed; required before plan completion/archival.",
    "reports/reliability-robustness.html missing; provider-diverse review quorum needs runtime-metadata confirmation of actual models.",
    "Hosted GitHub CI for the updated workflow not yet observed; actionlint unavailable locally.",
    "Low: src/ui/users.rs:159-170 member_groups scans all groups x members per frame; D7 fixture memberships are sparse, so dense-membership hosts are unmeasured.",
    "Low: core_static_guards scans tests/ only; src unit tests rely on review plus the one documented benign helper.",
    "Low: literal root re-run of the suite not performed (no privilege escalation authorized); identity is injected by design."
  ],
  "noStagedFiles": true,
  "diffSummary": "Review-only: wrote plans/reviews/reliability-final-moonshot.md. The integrated W1-W4 worktree (uncommitted, base 0b154c1) was inspected unchanged; it implements composite operations with one-shot secrets, typed retry/policy, checked child/terminal lifecycles, bounded config/UI, deterministic guards, MSRV/stable CI with components, deny/audit dispositions, and a working numeric 10k/10k benchmark.",
  "reviewFindings": [
    "no blockers",
    "note-medium: hosted-CI execution and actionlint verification outstanding (.github/workflows/rust.yml) - watch first real CI run",
    "note-low: src/ui/users.rs:159-170 per-frame O(groups x members) membership scan unmeasured under dense memberships",
    "note-low: tests/core_static_guards.rs does not scan src/ unit tests; benign helper in src/sys/command.rs:540-565 is the only spawn",
    "external (not code defects): R2-ME6 real-tool/PTY gate, final HTML report, provider-diversity confirmation remain integration-owner tasks"
  ],
  "manualNotes": "Reviewer provider/model could not be self-verified from inside this session; given the round-1 mismatch where both lanes resolved to openai-codex/gpt-5.6-sol, the integration owner must confirm this lane's actual model from runtime metadata before treating the diversity quorum (R2-ME8) as satisfied. All commands ran as UID 1000 with no sudo and no privileged account tools. Confidence: 93/100."
}
```
