# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/corpus-validation.md](docs/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling.

Last updated: 2026-06-10. Items 7, 8 done.

**How this doc works**

- **Open** — actionable items below (problem / direction kept for open work only).
- **Done** — one-line index in [Completed](#completed); design detail lives in `docs/archive/*` and git history.
- **Plans** — active drafts under `docs/TEMP-*.md`; archive when shipped.

**Next:** [Phase 4](#phase-4--edge-cases-and-semantics) edge cases → [validation open concerns](#validation-diagnostics--open-concerns) → [Phase 6](#phase-6--architecture-cleanup) cleanup; repair follow-ups (`--dry-run` / `--write`, scratch-buffer test, streaming WAV encode).

**Active plans (2026-06-10)** — most open items below are owned by one of four drafts; land order: 1 → (2 ∥ 4) → 3.

| Plan | Covers |
|------|--------|
| [TEMP-output-error-contract-plan.md](docs/TEMP-output-error-contract-plan.md) | serde half of item 11, stringly port errors, JSON freeze (Phases 1–2 done) |
| [TEMP-layer-purity-plan.md](docs/TEMP-layer-purity-plan.md) | Ports half of item 11, test helper coupling, type / dependency polish |
| [TEMP-media-session-redesign-plan.md](docs/TEMP-media-session-redesign-plan.md) | Items 6, 8, 12; hold-out container duration, unused `decoded_extent_*`, `reset_io` ignored |
| [TEMP-verification-hardening-plan.md](docs/TEMP-verification-hardening-plan.md) | Remaining [validation open concerns](#validation-diagnostics--open-concerns), committed-fixture gap, test dedupe, doc drift |

---

## Open work

### Phase 4 — Edge cases and semantics

| # | Item | Direction |
|---|------|-----------|
| 6 | [Duration-less files at open](#duration-less-files-at-open) | Audit remaining gaps; relax open when decodable |

#### Duration-less files at open

Partially addressed (`scan_container_audio_duration`, `mp3_no_duration_tag`). Audit paths that still fail at open; fail at clip planning if duration unknown after scan.

**Refs:** `crates/clip-sync/src/infrastructure/symphonia/session.rs`, `tests/corpus/manifest.toml`

---

### Validation diagnostics — open concerns

Core flags ship (2026-06-10). Follow-ups from hardening pass and code review.

| Concern | Direction |
|---------|-----------|
| Hold-out placement uses container duration | MKV tail regression test; consider hybrid extent policy |
| Short media vs min `clip_length` (30 s committed WAV, 60 s min) | Regenerate fixtures ≥ 60 s or accept generated-only `corpus_verify_offset_pass` |
| First hold-out candidate wins when `verified == false` | Try next candidate or log chosen window in verbose |
| Default 15 min `clip_length` + `verify_offset` | Document cost; optional shorter verification segment (future) |
| Option A (`find_offset`) false passes | Option B PCM lag-0 only if corpus proves need |
| Committed corpus + `verify_offset` | `wav_leader_3s` = alignment only |
| Test overlap (+3 s chirp) | Dedupe corpus vs integration vs unit roles |
| Unused `decoded_extent_*` on hold-out input | Remove or revive extent-aware placement |
| `reset_io` ignored in high-rate refinement | Match verification (log / propagate) |
| Headline confidence uses `clips.first()` | Select start clip by label |
| `AlignmentResult` test builder drift | Use `application/testing/alignment_fixtures.rs` |
| Repetition downgrade vs `aligned` | Document intentional v1 in corpus-validation |
| Plan doc drift | Archive [TEMP-offset-verification-plan.md](docs/TEMP-offset-verification-plan.md); sync PLAN |

**Refs:** `offset_verification.rs`, `high_rate_refinement.rs`, `domain/policies.rs`, CLI/repair `output.rs`, `tests/corpus/manifest.toml`

---

### Phase 6 — Architecture cleanup

| # | Item | Direction |
|---|------|-----------|
| 11 | [Layer leaks](#architecture-domain-and-application-layer-leaks) | `Resampler` / `OffsetRefiner` ports; DTO serde; update PLAN |
| 12 | [`MediaSession` interior mutability](#mediasession-interior-mutability) | `&mut self` or explicit handle; drop `expect()` |
| 13 | [Documentation drift](#documentation-drift-plan-vs-code) | PLAN audit after policy decisions |

#### Architecture: domain and application layer leaks

`rubato` in domain, `cross_correlate` in application, `Serialize` on domain types — conflicts with PLAN purity claim.

**Refs:** `domain/resample.rs`, `application/offset_refinement.rs`, `application/ports.rs`, `PLAN.md`

#### `MediaSession` interior mutability

`RefCell` + `extract_mono(&self)`; not `Sync`. Breaking port change when touching session code.

**Refs:** `infrastructure/symphonia/session.rs`, `application/ports.rs`

#### Documentation drift (PLAN vs code)

Defaults, domain errors, purity claims out of sync with code.

**Refs:** `PLAN.md`, `application/config.rs`

---

### Defer / opportunistic

| Item | Direction |
|------|-----------|
| [Memory / PCM cloning](#memory-use-and-pcm-cloning-on-long-clips) | Document in PLAN; `Cow` / in-place prep when painful |
| [Log file appender](#log-file-appender) | `tracing-appender` in `logging/mod.rs` |
| [Committed test fixtures](#committed-test-fixtures) | Optional committed MP3; WAV ≥ 60 s if verify on fixtures needed |
| [Test helper coupling](#test-helper-cross-layer-coupling) | `tests/support/` when refactoring media tests |
| [Type / dependency polish](#type-and-dependency-polish) | `Fingerprint` newtype; drop unused `anyhow` |
| [Stringly port errors](#stringly-typed-port-errors) | Structured sub-enums |

#### Memory use and PCM cloning on long clips

15-minute default clips; full PCM in memory; no streaming fingerprint. Structural ceiling until API changes.

**Refs:** `application/align_videos.rs`, `domain/pcm_preparation.rs`

#### Log file appender

`--log-file` parsed but not implemented.

**Refs:** `infrastructure/logging/mod.rs`

#### Committed test fixtures

Tier B = 3× 30 s WAV pairs; ffmpeg for encoded formats. See validation open concerns for verify gap.

**Refs:** `tests/corpus/`, `Cargo.toml` features

#### Test helper cross-layer coupling

Infrastructure tests import `application::testing::ffmpeg_util`.

**Refs:** `infrastructure/symphonia/`, `application/testing/`

#### Type and dependency polish

Bare `Fingerprint` vec; float `PartialEq`; sub-second config truncation.

**Refs:** `domain/alignment.rs`, `Cargo.toml`, `application/config.rs`

#### Stringly-typed port errors

Free-form `String`; no `source()` chain from Symphonia.

**Refs:** `application/error.rs`, `infrastructure/symphonia/error_mapping.rs`

---

## Completed

| Item | Done | Detail |
|------|------|--------|
| Workspace extraction (3 crates) | 2026-06-07 | [archive/workspace-refactor-plan.md](docs/archive/workspace-refactor-plan.md) |
| Repair Phase 4 (report-only) | 2026-06-08 | [archive/workspace-refactor-gaps.md](docs/archive/workspace-refactor-gaps.md) |
| Repair write path R0–R5 | 2026-06-08–09 | [archive/repair-write-path-plan.md](docs/archive/repair-write-path-plan.md) |
| Phase 1 — track selection + decode skips | 2026-06-06 | `policies.rs`, `symphonia/extract.rs`, CLI skip JSON |
| Phase 2 — symphonia split + sorted extract | 2026-06-06 | `infrastructure/symphonia/` |
| Phase 3 — large-offset PCM discover | 2026-06-06 | `offset_refinement.rs`; corpus 15/30/60 s leaders |
| High-rate hold-out refinement | 2026-06-06 | [archive/high-rate-offset-refinement-plan.md](docs/archive/high-rate-offset-refinement-plan.md) |
| Symphonia extract loop hardening | 2026-06-09 | [archive/extract-scaffold-plan.md](docs/archive/extract-scaffold-plan.md) |
| Session reuse + probe dedup | 2026-06-06 | [archive/session-reuse-plan.md](docs/archive/session-reuse-plan.md) |
| `try_all_tracks` docs + CLI | 2026-06-06 | `docs/corpus-validation.md` |
| Decode shortfall limits | 2026-06-06 | `symphonia/session.rs` |
| Clip self-repetition check | 2026-06-10 | [archive/clip-self-repetition-plan.md](docs/archive/clip-self-repetition-plan.md) |
| Hold-out offset verification | 2026-06-10 | [archive/offset-verification-plan.md](docs/archive/offset-verification-plan.md) |
| `AlignmentError::NoMatch` / `AmbiguousMatch` removed | 2026-06-10 | Contract frozen: low-confidence = `Ok(confidence: 0.0)`; `EngineFailed` is the only error variant |
| Resample rubato fallback warn | 2026-06-10 | [TEMP-output-error-contract-plan.md](docs/TEMP-output-error-contract-plan.md) Phase 1 — `domain/resample.rs` |
| `AudioTrack.bitrate` removed | 2026-06-10 | Symphonia doesn't expose encoding bitrate; field was always `None`; container-order heuristic is sufficient |

---

## Explicitly out of scope (initial version)

From [PLAN.md](PLAN.md) — not backlog unless scope changes:

- Video frame / visual sync
- Batch processing (> two files)
- Writing aligned output files from the **analyzer** (report offset only)
- Patched repair output is **`clip-sync-repair`** only — [archive/repair-write-path-plan.md](docs/archive/repair-write-path-plan.md)
- Network or streaming sources
