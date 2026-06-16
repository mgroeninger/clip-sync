# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/corpus-validation.md](docs/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling.

Last updated: 2026-06-16. Phase 6 complete (6A–6C).

**How this doc works**

- **Open** — actionable items below (problem / direction kept for open work only).
- **Done** — one-line index in [Completed](#completed); design detail lives in `docs/archive/*` and git history.
- **Plans** — active drafts under `docs/TEMP-*.md`; archive when shipped.

**Next:** [Repair R6](#repair-r6-follow-ups); AC-3 backend independent. Phase 6 closed.

**Active plans (2026-06-16)** — AC-3 backend can land in parallel with Phase 6.

| Plan | Covers |
|------|--------|
| [TEMP-ac3-backend-plan.md](docs/TEMP-ac3-backend-plan.md) | AC-3 capability gate + compile-time `ac3-oxideav` vs `ac3-ffmpeg` decode backends |

---

## Open work

### Phase 6 — Architecture & documentation cleanup

**Done when:** `PLAN.md` describes current paths (6A ✓); satellite docs current (6B ✓); clippy clean; no stale primary API names in active docs; 6C facade shrink optional.

| # | Item | Status | Direction |
|---|------|--------|-----------|
| 6A | [PLAN.md audit](#6a--planmd-audit) | **done** | Query-reference, repair write path, workflows, `AlignmentResult`/Report, config tables |
| 6B | [Satellite docs](#6b--satellite-docs) | **done** | `corpus-matrix`, `cli-output`, `error-mapping`; BACKLOG hygiene |
| 6C | [Facade / port shrink](#6c--facade--port-shrink) | **done** | Removed unused `Resampler::resample_interleaved`; facade fn retained |

#### 6A — PLAN.md audit

**Done (2026-06-16).** `PLAN.md` now documents query-reference + symmetric branches, repair write path (R0–R5), expanded `AlignmentResult` / `AlignmentConfig`, query-mode hold-out, and links to archive plans.

**Refs:** `PLAN.md`, `application/config.rs`, [archive/query-reference-alignment-plan.md](docs/archive/query-reference-alignment-plan.md)

#### 6B — Satellite docs

**Done (2026-06-16).** `corpus-matrix.md` lists all `wav_query_reference_*` cases; `cli-output.md` documents query-mode placement lines and B-longer `(donor on B: …)` suffix; `error-mapping.md` has complete repair exit codes and `RepairError` user messages.

- `docs/corpus-matrix.md` — `wav_query_reference_*` cases + coverage checklist
- `docs/cli-output.md` — query-mode default/verbose lines; B-longer donor suffix
- `docs/error-mapping.md` — repair exit codes + `RepairError` Display table

#### 6C — Facade / port shrink

**Done (2026-06-16).** Grep confirmed no `resampler.resample_interleaved` dispatch — repair uses facade `resample_interleaved`. Removed the port trait method and `RubatoResampler` impl; kept facade fn and `resample_mono` wiring.

**Refs:** `application/ports.rs`, `infrastructure/resample/rubato.rs`, [archive/layer-purity-plan.md](docs/archive/layer-purity-plan.md)

---

### Repair R6 follow-ups

Parallel track — **not** blocking Phase 6 closure. From [archive/repair-write-path-plan.md](docs/archive/repair-write-path-plan.md) post-ship gaps.

| Item | Direction |
|------|-----------|
| `--dry-run` / `--write` | Explicit CLI flags; today write mode is implied by `--wav` / `--mux` or TOML `dry_run = false` |
| Scratch-buffer regression test | Dedicated unit test for patch PCM path |
| Streaming / chunked WAV encode | Large multi-gap fills without holding full PCM |

---

### Defer / opportunistic

| Item | Direction |
|------|-----------|
| [Memory / PCM cloning](#memory-use-and-pcm-cloning-on-long-clips) | `Cow` / in-place prep when painful; parallel A/B decode when needed |
| [Log file appender](#log-file-appender) | `tracing-appender` in `logging/mod.rs` |
| [Committed test fixtures](#committed-test-fixtures) | Optional committed MP3; committed verify deferred — see [tests/corpus/README.md](tests/corpus/README.md) |
| [Verification cost knob](#verification-cost-knob) | `validation.max_verification_secs` — only on demonstrated friction |

#### Memory use and PCM cloning on long clips

15-minute default clips; full PCM in memory per extracted window; no streaming fingerprint API yet. **Decided (2026-06-11):** future streaming should reuse `scan_*_buckets` callbacks; `MediaSession: Send` allows one session per thread when parallel decode lands — see [PLAN.md](PLAN.md) § Media session semantics and [archive/media-session-redesign-plan.md](docs/archive/media-session-redesign-plan.md).

**Refs:** `application/align_videos.rs`, `domain/pcm_preparation.rs`

#### Log file appender

`--log-file` parsed but not implemented.

**Refs:** `infrastructure/logging/mod.rs`

#### Committed test fixtures

Tier B = 3× 30 s WAV pairs; ffmpeg for encoded formats. Hold-out verify on committed tier deferred — generated-only coverage documented in [tests/corpus/README.md](tests/corpus/README.md).

**Refs:** `tests/corpus/`, `Cargo.toml` features

#### Verification cost knob

Optional `validation.max_verification_secs` — deferred in [archive/verification-hardening-plan.md](docs/archive/verification-hardening-plan.md) (that plan’s “Phase 6”, not workspace Phase 6). Implement only if verify decode cost becomes painful in practice.

**Refs:** [corpus-validation.md](docs/corpus-validation.md) § Hold-out verification cost

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
| Internal layer purity (`Resampler` / `PcmCorrelator` ports, test_support, `Fingerprint` encapsulation) | 2026-06-11 | [archive/layer-purity-plan.md](docs/archive/layer-purity-plan.md) |
| Output & error contract (JSON freeze v1, report DTOs, error sources) | 2026-06-10 | [archive/output-error-contract-plan.md](docs/archive/output-error-contract-plan.md); contract in [docs/json-output.md](docs/json-output.md) |
| `AlignmentError::NoMatch` / `AmbiguousMatch` removed | 2026-06-10 | Contract frozen: low-confidence = `Ok(confidence: 0.0)`; `EngineFailed` is the only error variant |
| Resample rubato fallback warn | 2026-06-10 | [archive/output-error-contract-plan.md](docs/archive/output-error-contract-plan.md) Phase 1 — `domain/resample.rs` |
| `AudioTrack.bitrate` removed | 2026-06-10 | Symphonia doesn't expose encoding bitrate; field was always `None`; container-order heuristic is sufficient |
| `MediaSession` redesign + `MediaExtent` | 2026-06-11 | [archive/media-session-redesign-plan.md](docs/archive/media-session-redesign-plan.md): `&mut self` port, internal seek recovery, `media_scan.rs`, hold-out extent placement, duration-less open audit |
| Verification & validation hardening (phases 1–5) | 2026-06-11 | [archive/verification-hardening-plan.md](docs/archive/verification-hardening-plan.md): label-driven selection, verify retry + `candidates_tried`, Option A probe (no false-pass), corpus README / test dedupe / `alignment_fixtures`; v1 docs in [corpus-validation.md](docs/corpus-validation.md) |
| Periodic offset ambiguity | 2026-06-11 | [archive/periodic-ambiguity-plan.md](docs/archive/periodic-ambiguity-plan.md): `offset_ambiguous_mod_secs`, PCM parallel recheck, verify gating (`verify_inconclusive`); looped +13 s probe |
| Query-reference alignment (Q0–Q4) | 2026-06-15 | [archive/query-reference-alignment-plan.md](docs/archive/query-reference-alignment-plan.md): localization, repair mapped-region fill, CLI flags, generated corpus `wav_query_reference_45min_anchor` |
| Query-reference B-longer (B0–B4) | 2026-06-16 | [archive/query-reference-b-longer-plan.md](docs/archive/query-reference-b-longer-plan.md): either file may be shorter; offset sign + A/B span remapping |
| Phase 6C — facade port shrink | 2026-06-16 | Dropped `Resampler::resample_interleaved`; repair keeps facade `resample_interleaved` |
| Phase 6B — satellite docs | 2026-06-16 | `corpus-matrix`, `cli-output`, `error-mapping` aligned with query-reference + repair |
| Phase 6A — PLAN.md audit | 2026-06-16 | Query-reference workflows, repair write path, `AlignmentResult`/config tables |
| Region-bounded hold-out in query mode | 2026-06-16 | `resolve_holdout_candidates` / `mapped_region_holdout_candidates` in `domain/policies.rs` |
| `anchor_ref_secs` rename + fast B-long corpus | 2026-06-16 | JSON field rename with `anchor_a_secs` deserialize alias; `wav_query_reference_b_longer_fast` in default CI |

---

## Explicitly out of scope (initial version)

From [PLAN.md](PLAN.md) — not backlog unless scope changes:

- Video frame / visual sync
- Batch processing (> two files)
- Writing aligned output files from the **analyzer** (report offset only)
- Patched repair output is **`clip-sync-repair`** only — [archive/repair-write-path-plan.md](docs/archive/repair-write-path-plan.md)
- Network or streaming sources
