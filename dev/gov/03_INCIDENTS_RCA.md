# Incidents and RCA

This file is the initial backlog for legacy problems and purge requests that need to be carried into the Rust rewrite.

Known legacy signals from the codebase scan:
- `cmd/root.go`: TODO for configurable listen port.
- `internal/flags/flags.go`: FIXME for the `snakeswap` integration hack.
- Cleanup behavior is implemented in several places and must be rechecked for parity and safety.
- GitHub issues are disabled in the legacy repository, so the issue backlog is not directly recoverable from GitHub issue tracking.
- No open PRs were present during this init pass.

RCA format for future entries:
- Problem
- Symptom
- Root cause
- Fix
- Prevention
- Verification

Open work items:
- Collect legacy bug reports from code comments, commit history, and any external trackers.
- Classify purge requests into parity-preserving cleanup vs behavior-changing fixes.
- Track every confirmed defect with a prevention rule before closing it.
- Sort incoming issues into parity bugs, migration blockers, cleanup/purge requests, feature requests, and stale archive-only items.
- Verify the current upstream state before planning dependency updates or release slices.
