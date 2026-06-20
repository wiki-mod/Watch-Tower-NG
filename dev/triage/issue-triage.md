# Issue Triage Matrix

Scope: legacy Watchtower issues that matter for the Rust rewrite and current parity target.

| Issue | Bucket | Why it belongs here | Rewrite action |
| --- | --- | --- | --- |
| #2103 `Cleanup option does not seem to work` | Cleanup/purge request | The report is specifically about `WATCHTOWER_CLEANUP=true` not removing old images, which is a cleanup path rather than a core launch/update failure. | Recheck image cleanup semantics and keep it in the cleanup parity lane. |
| #2113 `Multiple Instances - Only work if fired in particular order` | Parity bug | This is an observed runtime bug in multi-instance scope handling; the order-dependent cleanup/stop behavior is a product defect the rewrite must not regress. | Preserve scope isolation and instance conflict handling in the Rust runtime. |
| #2023 `Cannot connect to the Docker daemon after Docker daemon update` | Parity bug | The issue describes a concrete runtime regression after a host Docker update, so it is a behavior bug in the existing product, not a feature ask. | Verify socket reconnect and daemon-update resilience in the migration. |
| #2067 `project dead? no commits in 2+ years. alternatives?` | Stale/archive-only | This is not a product defect or implementation request; it is meta discussion about project maintenance and archive status. | Do not spend rewrite effort here unless it resurfaces as a concrete migration decision. |

## Current Sorting

- Parity bugs: #2113, #2023
- Migration blockers: none from the inspected set
- Cleanup/purge requests: #2103
- Feature requests: none from the inspected set
- Stale/archive-only items: #2067

## Notes

- Keep this list narrow: only promote items into the rewrite backlog when they map to a concrete runtime gap, migration dependency, or cleanup behavior we need to preserve.
- If later scan notes surface more legacy issues, add them only after they are tied to a specific rewrite surface.
