# Governance

Scope:
- `old-source/` is read-only legacy input.
- `dev/` is the active Rust migration workspace.
- `release/` stores build, test, debug, and release-candidate state.

Hard rules:
- Warnings are errors.
- Warnings must not be silently ignored.
- Do not change `old-source/` unless a task explicitly says so.
- Fanout is mandatory.
- Prefer one file, one agent, one translation slice.
- Each Go source file is an independent migration lane.
- Exactly one agent owns exactly one file for the full migration of that file.
- The assigned agent must read the file completely, translate it completely, verify it completely, and integrate the finished Rust file directly.
- Partial translations, partial verifications, and deferred remainder work are not accepted.
- Restliche offene Test-, Mock- und Preview-Flächen werden ebenfalls in einzelne Agenten geschnitten.
- When adding files under `dev/`, choose a meaningful subdirectory instead of the `dev/` root.
- Only agreed governance files may live directly under `dev/gov/`; everything else gets a purpose-specific child folder.
- The main thread is exclusively an orchestrator; it does not integrate code directly.
- The main thread stays exclusively an orchestrator and must not write or integrate code.
- Development happens through agents. The main thread coordinates scope, sequence, and acceptance.
- One agent per file is the default unit of work when the slice is independent.
- All suitable agents may be started in parallel; there is no artificial fanout limit.
- Do not use `wait_agent` unless the user explicitly requests waiting.
- Warten auf genau einen Agenten ist verboten, solange andere sinnvolle Arbeit oder weitere offene Lanes existieren; `wait_agent` darf dann nicht blockierend genutzt werden.
- Der Mainthread muss stattdessen weitere Lanes starten, Status anfordern oder parallel weiterarbeiten. Ausnahme nur, wenn die nächste zwingende Aktion ausschließlich von genau diesem Rücklauf abhängt und keine andere verwertbare Arbeit existiert.
- New tasks start with `gpt-mini-low` for sighting, sorting, risk marking, and scope cutting.
- Escalate immediately when the current model cannot reach Quality A for the concrete core question.
- Prefer the smallest qualifying model, but never keep a weak model if quality would suffer.
- Files may be missing at assignment time and may be delivered later after an agent has already started; agents must accept late file delivery and continue without treating it as a blocker unless the missing file is required for their own current file.
- After file-level work, review the project as a whole.
- Deploy success is not proof of behavior.
- Functional parity only counts when services start without warnings or errors and the feature set matches the legacy program.
- Git branches are mandatory for separation of concerns.
- Agent output must be Quality A or it is not accepted.
- Quality A is the only accepted quality level.
- Auch für Test-, Mock- und Preview-Flächen gilt: eine Datei, ein Agent, Qualität A als einzige akzeptierte Abschlussstufe.
- The agent status for each file must be reportable at minimum as: file name, fully read yes/no, translation complete yes/no, one-to-one checked yes/no, compile without errors yes/no, compile without warnings yes/no, directly integrated yes/no, Quality A yes/no, blocker.
- Anything below Quality A is not done and must be reworked.
- Quality A means full implementation, one-to-one migration parity, no gaps, no TODOs or placeholders, no errors or warnings, relevant tests green, and governance followed.
- One-to-one means literal behavioral and contract parity with the legacy source; inventing new abstractions, invented shortcuts, merged responsibilities, or synthetic replacement behavior is forbidden unless the user explicitly requests a change.
- Quality B, C, and D are explicitly not accepted.

Quality rule:
- Only code quality A is accepted.
- Anything below Quality A is not finished and must not be reported as complete.
- Quality A requires full implementation, one-to-one migration parity, no omitted features, error paths, or edge cases, no placeholders or TODOs, no known gaps or defects, no compiler errors or warnings, relevant tests and checks green, and governance followed.
- Any file translation that adds behavior, contracts, or abstractions that do not exist in the legacy source is not a valid one-to-one translation unless the user explicitly requested the change.
- Quality B, C, and D are explicitly not accepted and must be reworked or rebuilt.
- The main thread remains exclusively an orchestrator and must not write or integrate code.
- Agents accept only Quality A.
- A separate review round is mandatory and is distinct from implementation.
- Implementation stays aligned to Quality A; review does not replace the Quality A obligation.
- Review starts at minimum with `gpt-5.5-low` and escalates per GOV when the file cannot be assessed reliably.
- Review must not write code, integrate code, or apply silent fixes.
- Review may only confirm or reject Quality A, name blockers, trigger follow-up work, recommend escalation, and update Index-SoT status.
- A file is only final Quality A after implementation, compile gate, warning gate, and the separate review round all pass.
- When a file reaches Quality A in the translation lane, set its Index-SoT status to `Review Waiting`; the review is a separate, explicitly commissioned round and does not begin automatically.
- All GOV rules apply equally to the main thread and to agents unless a rule explicitly differentiates them.

Agent selection policy:
- Choose the agent/model with enough reasoning depth for the task.
- Small mechanical slices can use lighter agents.
- Cross-cutting, bug-sensitive, or architecture-sensitive slices require the strongest available agent/model.
- If the required reasoning depth is not available, do not pretend the task is ready.

Branch policy:
- Use a dedicated dev branch for migration work.
- Use a dedicated release branch for every releaser.
- Release branches must be reproducible from the captured `release/` state.

Verification policy:
- Evidence first: code, logs, tests, and reproducible artifacts only.
- Keep verification separate from mutation work.
- Treat helper-script contract mistakes as process failures.

External reference policy:
- This project is a rewrite based on the old source code, and the Rust code remains a one-to-one conversion of legacy behavior.
- External references, URLs, remote origins, and other external endpoints must be understood before they are changed.
- Update URLs, remote origins, and other external endpoints only when their semantics are clear; legacy compatibility strings must not be renamed blindly.
- URLs and external references must not preserve dead-project references blindly; they should be cleaned so they point to the new project or otherwise make semantic sense for the rewrite.
- Missing external assets, URLs, or other required external targets are a Bringschuld: they must be added or backfilled as a follow-up task, not deferred.
- This applies to documentation, badges, package metadata, container labels, notifications, and runtime-generated links as well; the technical effect of each external reference must be understood before a legacy reference is removed or replaced.

Tooling policy:
- `sccache` is mandatory for build and test execution in this workspace.
- Verwendung ist Pflicht.
- Redis endpoint: `192.168.1.230`
- sccache-dist endpoint: `192.168.1.220`
- Running builds or tests without `sccache` is not allowed.
- Redis must be reachable and configured before any build or test run starts.
- If a build compiles successfully with `sccache` while Redis is in use, lack of `sccache-dist` is an accepted operating state for this workspace.
- When `sccache-dist` is available, it may be used, but it is no longer a hard requirement for acceptance.

Delivery policy:
- Keep the code modern, commented, and understandable for humans.
- Fix bugs while porting.
- Research legacy problems and purge requests before claiming parity.
- Sort issues before implementation into parity bugs, migration blockers, cleanup/purge requests, and feature requests.
- Keep the legacy archive context in mind: the project was effectively abandoned for years and dependencies must be brought to current state, especially Docker-related ones.
- During active work, commit and push the current state at least every 15 minutes, and only at exact quarter-hour boundaries: `xx:00`, `xx:15`, `xx:30`, `xx:45`.
