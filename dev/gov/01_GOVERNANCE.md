# Governance

This file supersedes all prior parallelism, fanout, and agent-delegation rules.
Written for Claude Code (sequential, tool-based execution). See `01_GOVERNANCE_ADDITION.md` for Quality A consistency rules.

Scope:
- `old-source/` is read-only legacy input (Source of Truth).
- `dev/` is the active Rust migration workspace.
- `release/` stores build, test, debug, and release-candidate state.

Hard rules:
- Warnings are errors.
- Warnings must not be silently ignored.
- Command errors must not be silently accepted. Any command that exits with a non-zero status or produces an error output must be investigated and resolved before proceeding. Do not continue past a failed build, failed test, or failed tool invocation.
- Do not change `old-source/` unless a task explicitly says so.
- When adding files under `dev/`, choose a meaningful subdirectory instead of the `dev/` root.
- Only agreed governance files may live directly under `dev/gov/`; everything else gets a purpose-specific child folder.

Execution model:
- Claude Code (the main AI model) is the primary executor.
- Claude Code reads, writes, and integrates code directly using tools: Read, Edit, Grep, Glob, Bash, PowerShell.
- No sub-agents, no fan-out, no parallel execution.
- Background tasks (Bash run_in_background) are the preferred execution mode for any command that does not need to block the next action. Use run_in_background by default; use blocking execution only when the result of the command is required before the next step can begin.
- Convergence and idempotency are mandatory: repeated execution of the same task must produce the same result.
- Each Go source file is an independent migration lane: `a.go → a.rs`, never `a.go + b.go → c.rs`.
- One file, one translation: multiple Go files must not be merged into one Rust file.
- Partial translations, partial verifications, and deferred remainder work are not accepted.
- After file-level work, review the project as a whole.
- Deploy success is not proof of behavior.
- Functional parity only counts when services start without warnings or errors and the feature set matches the legacy program.

Quality rule:
- Only code Quality A is accepted.
- Anything below Quality A is not finished and must not be reported as complete.
- Quality A requires: full implementation, one-to-one migration parity, no omitted features or error paths, no placeholders or TODOs, no known gaps or defects, no compiler errors or warnings, relevant tests and checks green, and governance followed.
- Any file translation that adds behavior, contracts, or abstractions not present in the legacy source is not a valid one-to-one translation unless the user explicitly requested the change.
- Quality B, C, and D are explicitly not accepted and must be reworked or rebuilt.
- A separate review pass is mandatory and is distinct from implementation.
- Review may only confirm or reject Quality A, name blockers, trigger follow-up work, and update Index-SoT status.
- Review must not silently fix defects or integrate code.
- A file is only final Quality A after implementation, compile gate, warning gate, and a separate review pass all succeed.
- All GOV rules apply equally unless a rule explicitly differentiates scopes.

One-to-one rule:
- One-to-one means literal behavioral and contract parity with the legacy source.
- Inventing new abstractions, shortcuts, merged responsibilities, or synthetic replacement behavior is forbidden unless the user explicitly requests a change.
- File status must be reportable as: file name, fully read, translation complete, one-to-one checked, compile without errors, compile without warnings, integrated, Quality A, blocker.

Trust rule:
- Existing Rust files translated by Codex are not trusted and must be re-verified by Claude Code before any Quality A claim is valid.
- A file is untrusted until its `Claude-Verifiziert-Hash` field is set in the Index-SoT after Claude Code re-verification.
- See `03_INCIDENTS_RCA.md` INC-001 for the incident record.

Commit policy:
- Commit after each meaningful, verified unit of work.
- Every commit must pass the pre-commit leak-test before being pushed.
- No time-based commit rhythm.
- Git branches are mandatory for separation of concerns.
- Use a dedicated dev branch for migration work.
- Use a dedicated release branch for every release.
- Release branches must be reproducible from the captured `release/` state.

Tooling policy (sccache):
- `sccache` is mandatory for build and test execution in this workspace.
- Endpoint configuration lives in `dev/runtime/local.env` (gitignored, never committed).
- Do not hardcode IP addresses or credentials in any committed file.
- Provide `dev/runtime/local.env.example` with placeholder values for documentation.
- If a build compiles successfully with sccache while Redis is in use, lack of sccache-dist is an accepted operating state.

Pre-push / pre-commit (leak-test):
- A leak-test is mandatory before every push or commit.
- The repository is public. Secrets, tokens, passwords, and IP addresses must not appear in committed diffs.
- See `.githooks/scan.sh` for the scanner implementation.
- The pre-commit and pre-push hooks invoke the scanner automatically.
- A commit or push that fails the leak-test must not proceed.

Verification policy:
- Evidence first: code, logs, tests, and reproducible artifacts only.
- Keep verification separate from mutation work.
- Treat helper-script contract mistakes as process failures.

External reference policy:
- This project is a rewrite based on the old source code; the Rust code is a one-to-one conversion of legacy behavior.
- External references, URLs, remote origins, and other external endpoints must be understood before they are changed.
- Update URLs, remote origins, and other external endpoints only when their semantics are clear; legacy compatibility strings must not be renamed blindly.
- URLs and external references must not preserve dead-project references blindly; clean them to point to the new project or make semantic sense for the rewrite.
- Missing external assets, URLs, or other required external targets are a Bringschuld: they must be added or backfilled as a follow-up task, not deferred.
- This applies to documentation, badges, package metadata, container labels, notifications, and runtime-generated links as well.

Delivery policy:
- Keep the code modern, commented, and understandable for humans.
- Fix bugs while porting.
- Research legacy problems and purge requests before claiming parity.
- Sort issues before implementation into parity bugs, migration blockers, cleanup/purge requests, and feature requests.
- Keep the legacy archive context in mind: the project was effectively abandoned for years and dependencies must be brought to current state, especially Docker-related ones.
