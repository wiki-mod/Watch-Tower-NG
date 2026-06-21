# Execution Status

Environment:
- Repository root: `C:\aunetic\ide\Watch-Tower-NG` (Windows) / `/c/aunetic/ide/Watch-Tower-NG` (Git Bash)
- Platform: Windows 11 Pro, Git Bash + PowerShell
- Active legacy source: `old-source/` (Go, read-only SoT)
- Migration workspace: `dev/work/watchtower-rs/` (Rust, Cargo workspace)
- Git remote: `git@github.com:wiki-mod/Watch-Tower-NG.git`

Trust status (as of 2026-06-21):
- All existing Rust files: **UNTRUSTED** (Codex-generated, not re-verified by Claude Code)
- `04_INDEX_TOS.md`: **UNTRUSTED** (Codex contradictions confirmed — e.g. IDX-0030 duplicate blocks, IDX-0023/IDX-0026 wrong target paths)
- Re-verification by Claude Code is required for every file before Quality A can be claimed.
- See `03_INCIDENTS_RCA.md` INC-001 for the full incident record.

Known build state:
- Several runtime phases return `RuntimeAdapterMissing` because `bollard` (Rust Docker SDK) is not yet in `Cargo.toml`.
- `src/cli.rs` has dead-code warnings that block downstream modules — prioritize early.
- `src/notify_upgrade.rs` had module wiring errors — verify current state before porting.

Legacy cleanup spots (carry into Rust port):
- `cmd/root.go`: TODO for configurable listen port.
- `internal/flags/flags.go`: FIXME for the `snakeswap` integration hack.
- Cleanup behavior is implemented in multiple places and must be rechecked for parity and safety.

Migration index:
- 153 files inventoried in `04_INDEX_TOS.md`.
- Trust field `Claude-Verifiziert-Hash` must be empty until Claude Code re-verifies each file.
- Priority order: source translations first, byte-identical assets (images, icons, docs) second.

Next steps:
- Complete foundation: CLAUDE.md + GOV rewrite + INDEX trust policy.
- Architectural decision: add `bollard` to `Cargo.toml` (Docker client, required for functional parity).
- Fix `src/cli.rs` dead-code warnings (blocks many downstream modules).
- Begin systematic re-verification per file (consult INDEX for order).
