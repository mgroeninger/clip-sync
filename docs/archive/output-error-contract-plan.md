# Temporary plan: output & error contract

> **Status:** Shipped (2026-06-10), all phases complete. Plan 1 of 4 — see [BACKLOG.md](../../BACKLOG.md). Contract frozen in [docs/json-output.md](../json-output.md); unblocked [archive/media-session-redesign-plan.md](media-session-redesign-plan.md) (shipped 2026-06-11).

**Problem:** The analyzer's JSON output is the raw serde view of domain types — `AlignmentResult` and friends derive `Serialize` in `domain/`, so freezing the JSON contract (BACKLOG Phase 4 #7) would freeze the domain model's shape with it. Meanwhile the failure taxonomy is unsettled: `AlignmentError::NoMatch` / `AmbiguousMatch` are `#[allow(dead_code)]` and never constructed, port errors carry free-form `String`s with no `source()` chain, and the rubato→linear resample fallback degrades quality silently.

**Goal:** Decouple JSON output from domain types via application-layer report DTOs (byte-identical JSON), settle and document the error taxonomy, attach type-erased sources to port errors, warn on the resample fallback — then freeze the contract.

**Workspace split:** DTOs and error types in **`crates/clip-sync`** (application layer — serde is already used there for config). **`clip-sync-cli`** and **`clip-sync-repair`** switch to serializing the DTO. Exit codes unchanged in both CLIs.

---

## Current codebase baseline

| Area | Path | Current state | Target phase |
|------|------|---------------|--------------|
| Domain serde derives | `crates/clip-sync/src/domain/alignment.rs` ~22–119 | `RepetitionFinding`, `ClipRepetitionReport`, `ClipMatch`, `TimelineOverlap`, `OffsetVerification`, `HighRateRefinement`, `AlignmentResult` all derive `Serialize` (with `skip_serializing_if` attrs) | 2 |
| Domain serde derives | `crates/clip-sync/src/domain/clip_window.rs` ~5–10 | `ClipLabel` derives `Serialize` (`rename_all = "lowercase"`); `ClipWindow` does not | 2 |
| Analyzer JSON | `crates/clip-sync-cli/src/infrastructure/cli/output.rs` ~8–13 | `serde_json::to_string_pretty(result)` on `AlignmentResult` directly, no envelope | 2 |
| Repair JSON | `crates/clip-sync-repair/src/domain/gap.rs` ~66; `infrastructure/cli/output.rs` ~260–291 | `GapReport.alignment: AlignmentResult` embedded in `RepairJsonOutput { scan, patch }` | 2 |
| Dead error variants | `crates/clip-sync/src/application/error.rs` ~45–55 | `AlignmentError::NoMatch`, `AmbiguousMatch { candidates }` — `#[allow(dead_code)]`, zero constructors in code; only `EngineFailed(String)` is live | 1 |
| Stringly errors | `application/error.rs` ~21–65 | `MediaError::{UnsupportedFormat, OpenFailed, SeekFailed, Unsupported}(String)`, `DecodeFailed { detail: String }`; `FingerprintError::{InvalidPcm, EngineFailed}(String)`; `ConfigError::Parse(String)` | 3 |
| Symphonia mapping | `crates/clip-sync/src/infrastructure/symphonia/error_mapping.rs` ~193–296 | `format!`-stringifies every `SymphoniaError` / `io::Error`; no `#[source]`, underlying error dropped | 3 |
| Zero-confidence contract | `infrastructure/chromaprint/aligner.rs` ~41–83 | Three `Ok { confidence: 0.0 }` paths (empty fingerprint, no segment, computed zero); only error is `EngineFailed` (`FingerprintTooLong`) | 1 (document) |
| Silent fallback | `crates/clip-sync/src/domain/resample.rs` ~17–25, ~43–45, ~68–93 | `FftFixedIn::new` or `process_into_buffer` failure → `linear_resample_fallback`, no log | 1 |
| Exit codes | `clip-sync-cli/.../exit_code.rs` ~5–15; `clip-sync-repair/.../exit_code.rs` ~15–22 | Analyzer 2–6 by `AppError` branch; repair 2–6 by `RepairError` | unchanged |
| Contract doc | `docs/error-mapping.md` | Documents low-confidence ≠ error; reserves `NoMatch`/`AmbiguousMatch` for "engine failures" that cannot occur | 1, 4 |
| JSON shape tests | `clip-sync-cli/tests/cli_output.rs` ~137–543 | Shape/roundtrip tests exist but serialize domain types directly | 0 |

---

## Decisions

Locked before implementation. Change only with an explicit plan revision.

| Topic | Decision |
|-------|----------|
| **Low-confidence contract** | **`Ok` with `confidence: 0.0` is the contract** for "clips didn't match" — matches `docs/error-mapping.md` and exit-code semantics (analysis success = exit 0). Domain assembly already maps zero confidence → `aligned: false`, `offset_secs: None`. |
| **`NoMatch` / `AmbiguousMatch`** | **Delete both variants.** The only engine failure (`MatchError::FingerprintTooLong`) maps to `EngineFailed`; nothing can construct the dead variants. Ambiguity is already expressed via `segment_confidence` halving (`matching.rs` ~63–78). *Rejected alternative:* wiring `AmbiguousMatch` to the ambiguous-cluster path — that would turn a successful low-confidence analysis into exit 6, contradicting the contract above. |
| **DTO location** | New **`application/report.rs`** in the lib: `AlignmentReport` DTO tree mirroring today's JSON exactly (`ClipMatchReport`, `OffsetVerificationReport`, …), with `From<&AlignmentResult>`. Application layer already uses serde (config.rs); domain becomes serde-free. Re-exported on the lib facade. |
| **JSON shape** | **Byte-identical** to current output: same field names (snake_case), same `skip_serializing_if` / `#[serde(default)]` behavior, `ClipLabel` still `"start"`/`"interior"`/`"end"`. Verified by golden tests captured in Phase 0 *before* the swap. |
| **Repair embedding** | `GapReport.alignment` switches from `AlignmentResult` to `AlignmentReport` (conversion at `scan_gaps` boundary). Repair JSON shape unchanged. |
| **Structured errors** | Keep current variant set and display strings; **add a type-erased source** where an underlying error exists: `source: Option<Box<dyn std::error::Error + Send + Sync>>` (via `#[source]`). Port errors stay in the application layer without leaking Symphonia types. *Rejected alternative:* structured sub-enums per Symphonia error class — high churn, no consumer needs to match on them today; revisit if a caller ever does. |
| **Resample warn** | One `tracing::warn!` at each fallback trigger in `infrastructure/resample/rubato.rs` (target rate, input rate, error). Shipped with [layer-purity-plan.md](layer-purity-plan.md). |
| **Exit codes** | Unchanged in both CLIs. Deleting dead variants does not alter any mapping (they were never produced). |
| **Error display text** | stderr messages may not change byte-for-byte (Phase 3 keeps `Display` strings stable; only `source()` is added). CLI tests asserting message text must keep passing. |
| **Contract freeze artifact** | New **`docs/json-output.md`**: authoritative field-by-field JSON contract for analyzer and repair outputs, versioned "v1". `docs/error-mapping.md` updated for the deleted variants. Freeze happens only as the **last** phase. |

---

## Phases

### Phase 0 — golden snapshot (guard rail)

- [x] Add a full-surface JSON golden test in `clip-sync-cli/tests/cli_output.rs`: an `AlignmentResult` exercising every optional field (`repetition`, `offset_verification`, `high_rate_refinement`, `start_overlap`, `offset_drift_secs`, null `offset_secs`) serialized and compared against a checked-in expected JSON string.
- [x] Same for repair: `RepairJsonOutput` with embedded alignment + patch summary in `clip-sync-repair/.../output.rs` tests.
- [x] These tests must survive Phases 2–3 **unmodified except for the type swap** — they define "byte-identical".

**Artifacts (2026-06-10):** `format_json_output` / `format_repair_json_output` (production paths); fixtures at `clip-sync-cli/tests/fixtures/full_surface_alignment.json` and `clip-sync-repair/tests/fixtures/full_surface_repair.json`; ignored generator tests `write_full_surface_*_golden` to refresh goldens after intentional contract changes.

### Phase 1 — tactical contract fixes

- [x] Delete `AlignmentError::NoMatch` and `AmbiguousMatch` (+ `#[allow(dead_code)]`); update `docs/error-mapping.md` (remove the "reserved for engine failures" rows; document `Ok`+zero-confidence as the no-match contract).
- [x] Add `tracing::warn!` to both fallback triggers in `domain/resample.rs`; unit test via a tracing subscriber capture or by asserting fallback output path is taken (existing tests cover the math).
- [x] Sweep `PLAN.md` § Plan-level notes (lines referencing `NoMatch`/`AmbiguousMatch`) and `BACKLOG.md` item 7.

### Phase 2 — report DTO split

- [x] Create `crates/clip-sync/src/application/report.rs`: DTO structs with all serde attrs moved from domain; `From<&AlignmentResult>` (and per-type `From`s). Facade export in `lib.rs`.
- [x] Remove `Serialize` derives (and the `serde` import) from `domain/alignment.rs` and `domain/clip_window.rs`.
- [x] `clip-sync-cli` `output.rs`: serialize `AlignmentReport::from(result)`.
- [x] `clip-sync-repair`: `GapReport.alignment` → `AlignmentReport`; convert at the `scan_gaps` alignment boundary.
- [x] Migrate test serialization sites: `align_videos.rs` tests ~1694–1728, `offset_verification.rs` tests ~836–885, `cli_output.rs`, repair output tests.
- [x] Phase 0 goldens pass unchanged.

**Artifacts (2026-06-10):** DTO tree `AlignmentReport` / `ClipMatchReport` / `ClipLabelReport` / `RepetitionReport` / `RepetitionFindingReport` / `TimelineOverlapReport` / `HighRateRefinementReport` / `OffsetVerificationReport` in `application/report.rs` (the repetition wrapper is `RepetitionReport` to avoid colliding with domain `ClipRepetitionReport`). `format_high_rate_refinement_lines` / `format_offset_verification_lines` moved there too (domain `alignment_report.rs` deleted) and now take report types; `GapReport.overlap` is `Option<TimelineOverlapReport>`. `format_json_output` / `format_repair_json_output` kept their signatures and convert internally, so Phase 0 goldens needed no edits.

### Phase 3 — error sources

- [x] Add `#[source]`-carrying forms to `MediaError` / `FingerprintError` / `ConfigError` variants where an underlying error exists (keep `Display` text stable; prefer adding an optional boxed source field over new variants).
- [x] `infrastructure/symphonia/error_mapping.rs`: attach the original `SymphoniaError` / `io::Error` as source instead of dropping it after `format!`.
- [x] Unit tests: `source()` chain reachable from `AppError` down to the io error for at least probe-failure and decode-failure paths.
- [x] Confirm CLI stderr output and exit-code tests unchanged.

**Artifacts (2026-06-10).** Two deliberate deviations from the decisions table, same intent:
- `source` is `Option<Arc<dyn Error + Send + Sync>>` (alias `ErrorSource`), not `Box` — the error enums must stay `Clone` because test fakes store an error and return a clone per call. `PartialEq`/`Eq` dropped from `MediaError`/`FingerprintError`/`ConfigError` (one test compared by `==`; now `matches!`).
- `MediaError`/`FingerprintError`/`ConfigError` implement `Display`/`Error` by hand instead of thiserror `#[source]`: `Arc<dyn Error>` itself implements `Error` (Rust ≥ 1.76), so thiserror would expose the `Arc` wrapper as the chain node and the wrapped error would not downcast. Display strings byte-identical to the old derives.
- Variants gaining a source moved to struct form with constructors (`MediaError::open_failed` etc.) for source-less sites. Config loaders now attach the `io::Error` / `toml::de::Error`; chromaprint `ResetError::CannotResample` attached on `FingerprintError::EngineFailed`. `AlignmentError` untouched per plan scope.

### Phase 4 — freeze

- [x] Write `docs/json-output.md` (analyzer + repair JSON, field semantics, optionality rules, v1 marker). Link from `README.md` and `docs/error-mapping.md`.
- [x] PLAN.md sync: Output section points at the DTO + contract doc; purity claim updated (domain serde-free).
- [x] BACKLOG cleanup: close item 7, the serde half of item 11, "stringly port errors", "silent resample fallback".

---

## Tests

| Layer | Coverage |
|-------|----------|
| Golden JSON | Phase 0 full-surface snapshots, both CLIs — the freeze artifact in executable form |
| DTO conversion | Unit tests for `From<&AlignmentResult>` edge cases (all-`None` optionals, skipped verification) |
| Error sources | `source()` chain assertions for probe + decode failures |
| Regression | Existing `cli_output.rs` shape/exit-code tests, corpus committed tier, repair output tests — all green at every phase boundary |

## Exit criteria

- No `Serialize`/`Deserialize` anywhere under `crates/clip-sync/src/domain/`.
- `AlignmentError` has no dead variants; `docs/error-mapping.md` and `docs/json-output.md` agree with code.
- Every adapter-mapped error exposes the underlying error via `source()`.
- Phase 0 goldens byte-identical before/after.

## Cross-plan sequencing

- **Blocks** the JSON freeze and should land **before** the media-session redesign (both touch `MediaError`) — satisfied; media-session shipped 2026-06-11 ([media-session-redesign-plan.md](media-session-redesign-plan.md)).
- Layer-purity shipped 2026-06-11 ([layer-purity-plan.md](layer-purity-plan.md)); resample warn lives in `rubato.rs`.
- [verification-hardening-plan.md](verification-hardening-plan.md) Phase 2 shipped `candidates_tried` as an additive JSON field under v1 (2026-06-11).
