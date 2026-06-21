# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Projekt

Watch-Tower-NG ist ein Rust-Port von Watchtower — einem Tool das laufende Docker-Container
automatisch aktualisiert, wenn ein neueres Image im Registry verfügbar ist.

Original: `containrrr/watchtower` (Apache 2.0) — dieser Port steht unter derselben Lizenz.
Attribution: `NOTICE`-Datei beachten. Namespace: `wiki-mod/Watch-Tower-NG`.

## Verzeichnisstruktur

```
C:\aunetic\ide\Watch-Tower-NG\
├── dev/               ← ARBEITSVERZEICHNIS
│   ├── gov/           ← Governance-Dokumente
│   ├── runtime/       ← Lokale Konfigurationen (local.env gitignoriert)
│   └── work/
│       └── watchtower-rs/  ← aktiver Rust-Rewrite (Cargo-Workspace)
├── old-source/        ← Go-Referenz (SoT, NICHT ANFASSEN)
└── release/           ← fertige Public-Releases
```

## Build-Befehle

Alle Befehle aus `dev/work/watchtower-rs/`. sccache-Konfiguration via `dev/runtime/local.env` laden.

```bash
# Lokale Config laden (einmalig pro Shell, Git Bash)
source dev/runtime/local.env

# Build
cargo build

# Tests
cargo test

# Lint (Warnungen = Fehler)
cargo clippy -- -D warnings

# Einzelner Test
cargo test <testname>

# WASM-Binary (Template Preview)
cargo build --target wasm32-unknown-unknown --bin tplprev

# Dependency-Update
cargo update
```

**sccache ist Pflicht.** Konfiguration: `dev/runtime/local.env` (gitignoriert, nie committen).
Vorlage: `dev/runtime/local.env.example`.

**Fehler bei Befehlen sind keine Warnungen.** Jeder Befehl der mit einem Fehler abbricht (Exit-Code ≠ 0) oder eine Fehlermeldung ausgibt, muss untersucht und behoben werden bevor weitergearbeitet wird. Kein stilles Übergehen von Build-Fehlern, Test-Fehlern oder Tool-Fehlern.

## Arbeitsweise (Claude Code)

Claude Code schreibt und integriert Code **direkt** — kein Sub-Agent-Fanout, keine Parallelisierung.
Tools: Read, Edit, Grep, Glob, Bash, PowerShell.
Konvergenz und Idempotenz sind Pflicht — wiederholtes Ausführen ergibt dasselbe Ergebnis.
`run_in_background` ist der bevorzugte Ausführungsmodus für jeden Befehl der nicht sofort blockieren muss — blockierend nur wenn das Ergebnis für den nächsten Schritt zwingend gebraucht wird.

## Portierungsregeln (KRITISCH)

1. **1:1-Pflicht**: `a.go → a.rs`, `b.go → b.rs` — NIEMALS `a.go + b.go → c.rs`
2. Jede Go-Datei ist eine eigene Migrations-Lane.
3. Rust-Code übernimmt Go-Semantik, Typen, Fehlerpfade **exakt**.
4. `#![forbid(unsafe_code)]` gilt für das gesamte Crate.
5. Warnungen sind Fehler — keine stillen Ignorierer.

## Vertrauen in bestehenden Rust-Code

**ACHTUNG**: Kein bestehender Rust-Code ist vertrauenswürdig. Codex hat Abkürzungen genommen.
Jede Datei gilt als `Nicht übersetzt` bis Claude sie vollständig re-verifiziert hat.

Prüfablauf pro Datei:
1. Go-Quelldatei vollständig lesen
2. Rust-Zieldatei vollständig lesen
3. Vergleich: Verhalten, Typen, Fehlerpfade, Edge Cases
4. Bei Abweichung: Rust-Datei neu schreiben
5. Compile ohne Fehler, compile ohne Warnungen
6. Tests grün
7. `Claude-Verifiziert-Hash` in `dev/gov/04_INDEX_TOS.md` setzen

## Qualität A (einzig akzeptierter Standard)

- Vollständige 1:1-Portierung: keine Lücken, keine TODOs, keine Platzhalter
- Keine Compilerfehler, keine Compiler-Warnungen
- Alle relevanten Tests grün, Fehlerpfade und Edge Cases implementiert
- Governance eingehalten — **Widersprüche = kein Qualität A**
- Siehe `dev/gov/01_GOVERNANCE_ADDITION.md` für Konsistenzregeln

## Source of Truth (SoT)

`old-source/` ist die einzige Referenz. Sie wird nicht mehr gepflegt.
Bekannte Bugs aus Issues/PRs des Originals sollen im Rust-Port behoben werden.
Vor Portierung eines Moduls: offene Issues/PRs im Original prüfen.

## Docker-Client

Das Projekt benötigt `bollard` (Rust Docker SDK) — diese Abhängigkeit fehlt noch in `Cargo.toml`.
Das ist der Hauptblocker für alle `RuntimeAdapterMissing`-Phasen.
Architekturentscheidung: `bollard` verwenden (safe, async, tokio-kompatibel).

## Modulkorrespondenz Go → Rust

| Go-Pfad | Rust-Modul |
|---------|------------|
| `cmd/root.go` | `src/cli.rs` |
| `cmd/notify-upgrade.go` | `src/notify_upgrade.rs` |
| `internal/actions/` | `src/actions.rs` |
| `internal/flags/` | `src/flags.rs` |
| `internal/meta/` | `src/meta.rs` |
| `internal/util/` | `src/util.rs` |
| `pkg/api/api.go` | `src/api.rs` |
| `pkg/api/metrics/` | `src/api_metrics.rs` |
| `pkg/api/update/` | `src/api_update.rs` |
| `pkg/container/container.go` | `src/container.rs` |
| `pkg/container/client.go` | `src/docker_client.rs` |
| `pkg/container/cgroup_id.go` | `src/cgroup.rs` |
| `pkg/filters/` | `src/filters.rs` |
| `pkg/lifecycle/` | `src/lifecycle.rs` |
| `pkg/metrics/` | `src/metrics.rs` |
| `pkg/notifications/` | `src/notifications/` |
| `pkg/registry/` | `src/registry/` |
| `pkg/session/` | `src/session.rs` |
| `pkg/sorter/` | `src/sorter.rs` |
| `pkg/types/` | `src/types.rs` |
| `tplprev/` | `src/bin/tplprev.rs` |

## Bekannte Blocker

- `src/cli.rs`: dead-code-Warnungen blockieren viele Module — früh angehen.
- `bollard` fehlt in `Cargo.toml` — alle Docker-Phasen geben `RuntimeAdapterMissing` zurück.
- `src/notify_upgrade.rs`: Modul-Verdrahtungsfehler — Status vor Portierung prüfen.

## Vor jedem Push/Commit

1. **Leak-Test**: Keine Secrets, Tokens, Passwörter, IP-Adressen im Diff
2. Repo ist PUBLIC — alles was gepusht wird ist öffentlich sichtbar
3. `.githooks/scan.sh` läuft automatisch via `pre-commit` und `pre-push`
4. `cargo clippy -- -D warnings` muss grün sein
5. `cargo test` muss grün sein

## Abhängigkeiten

Alle Rust-Crates sind veraltet (`cargo update` für Patches, Major-Bumps manuell prüfen).
`cargo audit` für Security-Scan (CI + vor Release).
`bollard` muss hinzugefügt werden (Docker-Client, fehlt komplett).

## Governance

- Regeln: `dev/gov/01_GOVERNANCE.md` (ersetzt alle Codex-Parallelisierungsregeln)
- Qualität-A-Konsistenz: `dev/gov/01_GOVERNANCE_ADDITION.md`
- Migration-Index: `dev/gov/04_INDEX_TOS.md` (Codex-generiert, untrusted bis `Claude-Verifiziert-Hash` gesetzt)
- Incidents: `dev/gov/03_INCIDENTS_RCA.md`
