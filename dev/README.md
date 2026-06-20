# Dev Workspace

This tree is the active migration workspace.

Suggested layout:
- `dev/gov/` for governance, execution status, and RCA notes
- `dev/triage/` for issue classification and backlog sorting
- `dev/work/` for implementation slices and agent handoffs
- `dev/runtime/` for local runbooks, environment notes, and validation context

Rules:
- Keep `old-source/` read-only.
- Fanout is mandatory.
- Use one agent per independent file or slice.
- The main thread is exclusively an orchestrator.
- Quality A is the only accepted handoff level.
- When you create new files under `dev/`, place them in a meaningful subdirectory by purpose or domain.
- Do not add new work files directly in `dev/` root unless they are one of the agreed governance files.
- Prefer stable folder names over ad-hoc dumps so agent work stays easy to route and review.
- Commit and push the current state at least every 15 minutes during active work.
- If `sccache` is used, Redis and sccache-dist are mandatory and must point to `192.168.1.230` and `192.168.1.220` respectively.
