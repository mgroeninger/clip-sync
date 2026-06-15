# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/corpus-validation.md](docs/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling.

Last updated: 2026-06-11. Media-session redesign shipped.

**How this doc works**

- **Open** — actionable items below (problem / direction kept for open work only).
- **Done** — one-line index in [Completed](#completed); design detail lives in `docs/archive/*` and git history.
- **Plans** — active drafts under `docs/TEMP-*.md`; archive when shipped.

**Next:** [query-reference alignment](#active-plans) (unblocked); [Phase 6](#phase-6--architecture-cleanup) cleanup; repair follow-ups (`--dry-run` / `--write`, scratch-buffer test, streaming WAV encode).

**Active plans (2026-06-11)** — AC-3 backend and periodic-ambiguity plans are independent and can land in parallel.

| Plan | Covers |
|------|--------|
| [TEMP-ac3-backend-plan.md](docs/TEMP-ac3-backend-plan.md) | AC-3 capability gate + compile-time `ac3-oxideav` vs `ac3-ffmpeg` decode backends |
| [TEMP-query-reference-alignment-plan.md](docs/TEMP-query-reference-alignment-plan.md) | Short clip vs long video localization + repair mapped-region fill |

---

## Open work

### Phase 6 — Architecture cleanup

| # | Item | Direction |
|---|------|-----------|
| 13 | [Documentation drift](#documentation-drift-plan-vs-code) | PLAN audit after policy decisions |

#### Documentation drift (PLAN vs code)

Defaults, domain errors, purity claims out of sync with code.

**Refs:** `PLAN.md`, `application/config.rs`

---

### Defer / opportunistic

| Item | Direction |
|------|-----------|
| [Memory / PCM cloning](#memory-use-and-pcm-cloning-on-long-clips) | `Cow` / in-place prep when painful; parallel A/B decode when needed |
| [Log file appender](#log-file-appender) | `tracing-appender` in `logging/mod.rs` |
| [Committed test fixtures](#committed-test-fixtures) | Optional committed MP3; committed verify deferred — see [tests/corpus/README.md](tests/corpus/README.md) |
| [Resampler port shrink](#resampler-port-drop-unused-resample_interleaved) | Drop trait method if still unused; repair keeps facade fn |

#### Memory use and PCM cloning on long clips

15-minute default clips; full PCM in memory per extracted window; no streaming fingerprint API yet. **Decided (2026-06-11):** future streaming should reuse `scan_*_buckets` callbacks; `MediaSession: Send` allows one session per thread when parallel decode lands — see [PLAN.md](PLAN.md) § Media session semantics and [archive/media-session-redesign-plan.md](docs/archive/media-session-redesign-plan.md).

**Refs:** `application/align_videos.rs`, `domain/pcm_preparation.rs`

#### Log file appender

`--log-file` parsed but not implemented.

**Refs:** `infrastructure/logging/mod.rs`

#### Committed test fixtures

Tier B = 3× 30 s WAV pairs; ffmpeg for encoded formats. Hold-out verify on committed tier deferred — generated-only coverage documented in [tests/corpus/README.md](tests/corpus/README.md).

**Refs:** `tests/corpus/`, `Cargo.toml` features

#### Resampler port — drop unused `resample_interleaved`

Layer-purity (Phases 1–3) added `Resampler::resample_interleaved` for port completeness; nothing in production calls it (analyzer uses `resample_mono`; repair uses `clip_sync::resample_interleaved` on the facade). **Before removing:** grep for `resampler.resample_interleaved` and trait-object dispatch to this method — skip if any caller has appeared. Safe shrink: delete the trait method and matching `FakeResampler` / `RubatoResampler` impl blocks; keep the facade `resample_interleaved` fn and all `resample_mono` wiring.

**Refs:** `application/ports.rs`, `infrastructure/resample/rubato.rs`, `clip-sync-repair/src/application/patch_audio.rs`, [archive/layer-purity-plan.md](docs/archive/layer-purity-plan.md)

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

---

## Explicitly out of scope (initial version)

From [PLAN.md](PLAN.md) — not backlog unless scope changes:

- Video frame / visual sync
- Batch processing (> two files)
- Writing aligned output files from the **analyzer** (report offset only)
- Patched repair output is **`clip-sync-repair`** only — [archive/repair-write-path-plan.md](docs/archive/repair-write-path-plan.md)
- Network or streaming sources
