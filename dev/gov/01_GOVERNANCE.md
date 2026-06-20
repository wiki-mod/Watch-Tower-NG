# Governance

Scope:
- `old-source/` is read-only legacy input.
- `dev/` is the active Rust migration workspace.
- `release/` stores build, test, debug, and release-candidate state.

Hard rules:
- Warnings are errors.
- Do not change `old-source/` unless a task explicitly says so.
- Fanout is mandatory.
- Prefer one file, one agent, one translation slice.
- When adding files under `dev/`, choose a meaningful subdirectory instead of the `dev/` root.
- Only agreed governance files may live directly under `dev/gov/`; everything else gets a purpose-specific child folder.
- The main thread is exclusively an orchestrator; it does not integrate code directly.
- Development happens through agents. The main thread coordinates scope, sequence, and acceptance.
- One agent per file is the default unit of work when the slice is independent.
- After file-level work, review the project as a whole.
- Deploy success is not proof of behavior.
- Functional parity only counts when services start without warnings or errors and the feature set matches the legacy program.
- Git branches are mandatory for separation of concerns.
- Agent output must be Quality A or it is not accepted.
- Quality A means: no known defects, no warning-level output, and directly integrable behavior.
- Anything below Quality A is returned for rework.

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

Tooling policy:
- `sccache` is mandatory for build and test execution in this workspace.
- Verwendung ist Pflicht.
- Redis endpoint: `192.168.1.230`
- sccache-dist endpoint: `192.168.1.220`
- Running builds or tests without `sccache`, or with only one of the two endpoints configured, is not allowed.
- Both Redis and `sccache-dist` must be reachable and configured before any build or test run starts.

Delivery policy:
- Keep the code modern, commented, and understandable for humans.
- Fix bugs while porting.
- Research legacy problems and purge requests before claiming parity.
- Sort issues before implementation into parity bugs, migration blockers, cleanup/purge requests, and feature requests.
- Keep the legacy archive context in mind: the project was effectively abandoned for years and dependencies must be brought to current state, especially Docker-related ones.
- During active work, commit and push the current state at least every 15 minutes.
