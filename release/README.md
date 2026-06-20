# Release Workspace

Use this tree for captured build and validation state.

Recommended layout:
- `release/build-release/`
- `release/test-release/`
- `release/debug/`
- `release/release-candidates/`

Rules:
- Keep release state reproducible.
- Record the exact branch, commit, and verification evidence for every candidate.
- A push to GitHub is not a proof of functionality.

Each releaser should use a dedicated branch so a captured release can be recreated later.
