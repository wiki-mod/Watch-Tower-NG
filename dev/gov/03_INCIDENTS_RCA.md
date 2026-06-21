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

---

## INC-001 — Codex Translation Trust Failure (2026-06-21)

**Problem**: All Rust files translated by Codex are untrustworthy.

**Symptom**: Confirmed contradictions in `04_INDEX_TOS.md` — IDX-0030 has two conflicting YAML blocks for the same file; IDX-0023 and IDX-0026 have target paths that do not match their Kurzbegründung. Files marked Quality A cannot be Quality A by their own stated field values (violates `01_GOVERNANCE_ADDITION.md` consistency rules).

**Root cause**: Codex took shortcuts during translation: merged Go files, invented abstractions, skipped edge cases and error paths, and marked work as complete without independent verification.

**Fix**: Full re-verification by Claude Code for every `.rs` file. No Codex claim is accepted without independent re-verification by Claude Code.

**Prevention**:
- Only Claude Code re-verified files may be marked Quality A in the Index-SoT.
- A new field `Claude-Verifiziert-Hash` is added to Index entries. Empty = untrusted. Filled = verified by Claude Code (holds the Go source hash at time of verification).
- Verification process per file: Go source read → Rust file read → behavioral diff → compile clean → tests green → Index updated with verified hash.

**Verification**: File is Quality A only when `Claude-Verifiziert-Hash` is non-empty AND all Quality A consistency rules in `01_GOVERNANCE_ADDITION.md` are satisfied.

---

## INC-003 — Codex Datei-Merge-Verstoß: 1:1-Regel systematisch verletzt (2026-06-21)

**Problem**: Codex hat ohne Autorisierung mehrere Go-Quelldateien zu einer einzigen Rust-Datei zusammengefasst. Das verstößt gegen die Kern-Portierungsregel `a.go → a.rs`, die explizit `NIEMALS a.go + b.go → c.rs` verbietet.

**Symptom**: Systematischer Abgleich Go-Dateien (`old-source/`) gegen Rust-Dateien (`src/`) ergibt folgende Verstöße:

| Typ | Ausmaß |
|---|---|
| Mehrere Go → eine Rust (Merge-Verstoß) | 6 Fälle, davon einer mit 9 Go-Dateien |
| Erfundene Rust-Dateien (kein Go-Äquivalent) | 5 Dateien |
| Fehlende Rust-Dateien (Go existiert, kein Rust) | 11+ Dateien |

**Konkrete Merge-Verstöße:**

- `internal/util/util.go` + `rand_name.go` + `rand_sha256.go` → **`src/util.rs`** (3 → 1)
- `internal/actions/check.go` + `update.go` → **`src/actions.rs`** (2 → 1)
- `pkg/session/container_status.go` + `progress.go` + `report.go` → **`src/session.rs`** (3 → 1)
- `pkg/types/*.go` (9 Dateien) → **`src/types.rs`** (9 → 1)
- `pkg/notifications/common_templates.go` + `email.go` + `gotify.go` + `json.go` + `model.go` + `msteams.go` + `notifier.go` + `shoutrrr.go` + `slack.go` → **`src/notifications.rs`** (9 → 1)
- `cmd/root.go` → Content auf `src/cli.rs` + `src/lib.rs` verteilt (1 → 2, invertierter Verstoß)

**Erfundene Rust-Dateien (kein Go-Pendant):**
- `src/startup.rs` — aus `cmd/root.go` herausgeschnitten, kein `startup.go`
- `src/wait.rs` — aus `pkg/container/client.go` herausgeschnitten, kein `wait.go`
- `src/registry/credentials.rs` — kein `pkg/registry/credentials.go`
- `src/registry/pull.rs` — kein `pkg/registry/pull.go`
- `src/notifications/runtime.rs` — kein `pkg/notifications/runtime.go`

**Fehlende Rust-Dateien (Go existiert, Rust fehlt vollständig):**
- `pkg/container/errors.go` → kein Rust-Äquivalent
- `pkg/container/metadata.go` → kein Rust-Äquivalent
- `pkg/notifications/templates/funcs.go` → kein Rust-Äquivalent
- `pkg/notifications/email.go`, `gotify.go`, `json.go`, `model.go`, `msteams.go`, `shoutrrr.go`, `slack.go`, `common_templates.go` → alle als Einzeldateien fehlend (in `notifications.rs` rechtswidrig zusammengefasst)

**Root cause**: Codex hat 40–50 Agenten parallel ohne Koordination laufen lassen. Die Agenten haben Go-Pakete als semantische Einheiten behandelt (ein Paket → eine Rust-Datei) statt als Datei-für-Datei-Übersetzung. Zusätzlich hat Codex Hilfsfunktionen in eigenständige Dateien ausgelagert, die im Go-Code Teil größerer Dateien waren — ebenfalls ein Verstoß gegen die 1:1-Regel.

**Konsequenz**: Alle betroffenen zusammengefassten Rust-Dateien sind als **ungültig** einzustufen. Der enthaltene Code ist möglicherweise teilweise nutzbar, muss aber in die korrekte 1:1-Dateistruktur aufgeteilt werden. Der Code darf nicht im zusammengefassten Zustand als Quality A zertifiziert werden — unabhängig von Compile-Status oder Test-Status.

**Fix (Reihenfolge):**

1. **`src/util.rs`** aufteilen → `src/util.rs` (nur util.go-Inhalt) bleibt als Datei, Inhalt von rand_name.go → neue `src/rand_name.rs`, Inhalt von rand_sha256.go → neue `src/rand_sha256.rs`
2. **`src/actions.rs`** aufteilen → `src/actions/check.rs` (check.go) + `src/actions/update.rs` (update.go) + `src/actions/mod.rs`
3. **`src/session.rs`** aufteilen → `src/session/container_status.rs` + `src/session/progress.rs` + `src/session/report.rs` + `src/session/mod.rs`
4. **`src/types.rs`** aufteilen → 9 separate Dateien in `src/types/` entsprechend der 9 Go-Quelldateien + `mod.rs`
5. **`src/notifications.rs`** aufteilen → 9+ separate Dateien in `src/notifications/` entsprechend der Go-Quelldateien
6. Erfundene Dateien (`startup.rs`, `wait.rs`, `registry/credentials.rs`, `registry/pull.rs`, `notifications/runtime.rs`) prüfen: Inhalt dem korrekten Go-Pendant zuordnen oder entfernen
7. Fehlende Dateien (`container/errors.go` → `src/container_errors.rs`, `container/metadata.go` → `src/container_metadata.rs`, etc.) neu portieren
8. `src/lib.rs` bereinigen: Orchestrierungslogik aus `cmd/root.go` gehört vollständig nach `src/cli.rs`; `lib.rs` nur Modul-Deklarationen + Error-Typ
9. Nach jeder Aufteilung: `cargo check` muss grün sein, Import-Pfade anpassen

**Prevention:**
- Bei jedem neuen Portierungsauftrag: Abgleich Go-Dateien ↔ Rust-Dateien als erstes — bevor ein einziger Code-Inhalt geprüft wird.
- Die 1:1-Regel ist nicht verhandelbar: Ein Go-Paket mit 9 Dateien ergibt 9 Rust-Dateien plus `mod.rs`. Keine Ausnahmen.
- Erfundene Dateien (kein Go-Pendant) sind sofort zu kennzeichnen und zu hinterfragen.

**Verification**: Abgleich-Tabelle Go ↔ Rust ist vollständig ohne Merge-Verstöße, erfundene Dateien oder fehlende Dateien. `cargo check` grün. Alle aufgeteilten Dateien einzeln mit Go-Quelle verglichen und in INDEX mit `Claude-Verifiziert-Hash` versehen.

**Status**: Offen — Korrektur läuft.

---

## INC-002 — Metrics API als „Experimental" markiert (offen, 2026-06-21)

**Problem**: In `docs/metrics.md` steht `!!! warning "Experimental feature"` mit dem Hinweis, dass das Feature seit v1.0.4 experimental ist und Verhaltensauffälligkeiten gemeldet werden sollen.

**Symptom**: Die Docs-Warnung ist in der Rust-Port-Doku übernommen worden (IDX-0021, `pkg/metrics/` → `src/metrics.rs`). Unklar ob dieser Status im Rust-Port noch gilt oder ob das Feature zwischenzeitlich stable wurde.

**Root cause**: Unbekannt — muss im Go-Quellcode geprüft werden:
- `old-source/pkg/metrics/` und `old-source/pkg/api/metrics/` lesen
- Prüfen ob die Experimental-Warnung im Go-Code selbst verankert ist oder nur in der Doku
- Prüfen ob es offene Issues/PRs gab die diesen Status ändern sollten

**Fix**: Nach Code-Analyse:
- Falls Experimental-Status berechtigt → im Rust-Port beibehalten, ggf. Bedingungen klären unter denen es stable wird
- Falls Experimental-Status überholt → `!!! warning`-Block aus `docs/metrics.md` entfernen

**Prevention**: Vor Übernahme von Experimental-Warnungen in Rust-Port-Doku: immer Go-Quelle prüfen ob der Status noch aktuell ist.

**Verification**: Go-Quellcode `old-source/pkg/metrics/` und `old-source/pkg/api/metrics/` gelesen, Status geklärt, Docs entsprechend aktualisiert.

**Status**: Offen — prüfen bei IDX-Verifikation von `pkg/metrics/` und `pkg/api/metrics/`.
