# Temporary plan: internal layer purity

> **Status:** Draft (2026-06-10). Plan 2 of 4 — see [BACKLOG.md](../BACKLOG.md). No behavior or contract changes; parallel-safe with [TEMP-output-error-contract-plan.md](TEMP-output-error-contract-plan.md) (coordinate only on the `resample.rs` warn).

**Problem:** PLAN.md claims "Domain depends on nothing external" and "application depends on domain only", but: `rubato` is used directly in `domain/resample.rs`; the `cross_correlate` crate is called directly from `application/offset_refinement.rs`; infrastructure tests import `application::testing::ffmpeg_util` (dependency arrow pointing the wrong way); and `Fingerprint` exposes a bare `pub data: Vec<u32>`.

**Goal:** Make the dependency rule true again — DSP engines behind ports with adapters in infrastructure, ffmpeg test helpers out of the application module, `Fingerprint` encapsulated — with zero behavior change.

**Workspace split:** All work in **`crates/clip-sync`**. **`clip-sync-repair`** keeps compiling via stable facade paths (`resample_interleaved`, `normalized_correlation`, `clip_sync::testing::ffmpeg_util`).

---

## Current codebase baseline

| Area | Path | Current state | Target phase |
|------|------|---------------|--------------|
| rubato in domain | `crates/clip-sync/src/domain/resample.rs` | `use rubato::{FftFixedIn, Resampler}` (L1); `resample_mono_pcm` ~9–66, `resample_interleaved` ~99–131, `linear_resample_fallback` ~68–93 | 1 |
| resample call sites | `application/align_videos.rs` ~589; `application/offset_refinement.rs` ~405–410; `application/offset_verification.rs` ~185–191; `clip-sync-repair/src/application/patch_audio.rs` ~149 (facade) | Free-function calls | 1 |
| cross_correlate in application | `application/offset_refinement.rs` L1, ~445–451 | Only inside `pcm_cross_correlate_lag` (`Correlate::create_real_f64`, `CrossCorrelationMode::Full`); `normalized_correlation` ~532–558 is hand-rolled (pure, stays) | 2 |
| correlate internal callers | `offset_refinement.rs` ~197, ~386, ~482 (`refine_offset_around_prior`, `pcm_lag_adjustment_secs`, `refine_holdout_segment_lag`) | All funnel through `pcm_cross_correlate_lag` | 2 |
| Use-case ports | `application/align_videos.rs` ~34–45 | `AlignVideos<'a, MR, FP, AL>` holds `MediaReader`/`Fingerprinter`/`Aligner` + `ProgressReporter`; no resampler/correlator port | 1–2 |
| Default wiring | `application/default_pipeline.rs` ~8–17 | Instantiates Symphonia + Chromaprint adapters | 1–2 |
| Infra tests → app testing | `infrastructure/symphonia/media_reader_tests.rs` ~380–934 | 6 `#[cfg(feature = "ffmpeg-tests")]` tests import `application::testing::ffmpeg_util` | 3 |
| ffmpeg_util consumers | `application/testing/corpus_fixtures.rs` ~17; `clip-sync-repair/tests/scan_gaps_integration.rs` ~13 (`clip_sync::testing::ffmpeg_util`) | Facade path `clip_sync::testing::ffmpeg_util` already public under `test-utils` | 3 |
| `Fingerprint` | `domain/alignment.rs` ~8–11 | `pub struct Fingerprint { pub data: Vec<u32> }`; constructed in `chromaprint/fingerprinter.rs` ~66, `testing/fakes.rs` ~189; field-read in `aligner.rs` ~46–54, `repetition.rs` ~74–100 | 4 |
| `anyhow` | — | **Already absent** from all Cargo.toml and code; BACKLOG entry is stale | 4 (doc only) |
| Float `PartialEq` / config truncation | `domain/alignment.rs` derives; `application/config.rs` | BACKLOG "type polish" residue | 4 |
| Deps | `crates/clip-sync/Cargo.toml` ~16–31 | `rubato = "0.16"`, `cross_correlate = "0.3"` — both stay, but only infrastructure may import them | 1–2 |

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Resampler port** | New `Resampler` trait in `application/ports.rs`: `fn resample_mono(&self, clip: &MonoPcmClip, target_rate: u32) -> MonoPcmClip` and `fn resample_interleaved(&self, pcm: &MultiChannelPcm, target_rate: u32) -> MultiChannelPcm`. Infallible signature — current code never propagates resample errors (fallback handles failure internally). |
| **Rubato adapter** | Move `domain/resample.rs` to `infrastructure/resample/rubato.rs` as `RubatoResampler` (implements `Resampler`), keeping the FFT path, the linear fallback, and the Phase-1 warn from the contract plan. Domain keeps **no** resample code. |
| **AlignVideos wiring** | `AlignVideos` gains a fourth port (`RS: Resampler`); `default_pipeline.rs` and test fakes updated. In-repo breaking change only — facade `align_with_defaults` signature unchanged. |
| **Repair facade compat** | Facade keeps exporting a `resample_interleaved` convenience fn backed by `RubatoResampler` (repair `patch_audio.rs` keeps its one-line call; no port injection needed there in v1). |
| **Correlator port** | New `PcmCorrelator` trait in `application/ports.rs`: `fn cross_correlate_lag(&self, a: &[f64], b: &[f64]) -> Option<isize>` (peak-lag semantics of the current `cross_correlate` call). Adapter `FftCorrelator` in `infrastructure/correlation.rs`. *Rejected alternative:* reclassifying `cross_correlate` as an allowed math utility — it is a third-party FFT engine, exactly what ports exist for. |
| **Threading the correlator** | `pcm_cross_correlate_lag` and the `refine_*` free functions take `&dyn PcmCorrelator` as a parameter (callers: `align_videos.rs` ~391–418, `high_rate_refinement.rs` ~147). `normalized_correlation` is pure Rust and **stays** in application. |
| **ffmpeg_util relocation** | Move `application/testing/ffmpeg_util.rs` → top-level `src/test_support/ffmpeg_util.rs` (same `#[cfg(any(test, feature = "test-utils"))]` gating). Re-export under the **existing** `clip_sync::testing::ffmpeg_util` path so `corpus_fixtures.rs` and repair's integration test need import-line changes only. Infrastructure tests then import `crate::test_support::ffmpeg_util` — no `application::testing` arrow from infrastructure. |
| **`Fingerprint` encapsulation** | Private field + `Fingerprint::new(Vec<u32>)`, `items(&self) -> &[u32]`, `len`, `is_empty`. Touches fingerprinter, aligner, repetition, fakes, a handful of tests. |
| **`anyhow`** | Nothing to do in code — delete the stale BACKLOG bullet. |
| **Float `PartialEq` / config truncation** | Out of scope for hard fixes; document the float-compare caveat where `PartialEq` derives exist and add a sub-second truncation test or fix in `config.rs` **only if trivial**. Otherwise leave the BACKLOG bullet with a pointer here. |
| **PLAN purity claim** | After Phases 1–2, PLAN.md's dependency rule is accurate again — update the Dependencies table (`rubato`, `cross_correlate` annotated "infrastructure adapters only"). |

---

## Phases

### Phase 1 — `Resampler` port

- [ ] Add `Resampler` trait to `application/ports.rs`; `RubatoResampler` in `infrastructure/resample/` (code moved from `domain/resample.rs`, tests move with it).
- [ ] `AlignVideos` + `default_pipeline.rs` wiring; `FakeResampler` (identity or rate-stamp) in `testing/fakes.rs` for use-case tests.
- [ ] Update call sites: `align_videos.rs`, `offset_verification.rs`, `offset_refinement.rs` (rate-mismatch path receives the port).
- [ ] Facade: export `RubatoResampler` + keep `resample_interleaved` convenience fn; `clip-sync-repair` compiles untouched or with import-only diffs.
- [ ] `cargo test --workspace` green; grep gate: no `rubato` outside `infrastructure/`.

### Phase 2 — `PcmCorrelator` port

- [ ] Add `PcmCorrelator` trait + `FftCorrelator` adapter in `infrastructure/correlation.rs` (move the `Correlate::create_real_f64` block from `pcm_cross_correlate_lag`).
- [ ] Parameterize `pcm_cross_correlate_lag`, `refine_offset_estimate`, `refine_offset_around_prior`, `refine_holdout_segment_lag` with `&dyn PcmCorrelator`; update `align_videos.rs` and `high_rate_refinement.rs` callers and `offset_refinement.rs` tests (real adapter is fine in tests — it's deterministic).
- [ ] Grep gate: no `cross_correlate` import outside `infrastructure/`.

### Phase 3 — test-support relocation

- [ ] Move `ffmpeg_util.rs` to `src/test_support/`; re-export at `clip_sync::testing::ffmpeg_util`; fix the 6 imports in `media_reader_tests.rs` and the one in `corpus_fixtures.rs`.
- [ ] `cargo test -p clip-sync --features ffmpeg-tests,test-utils` and repair's `scan_gaps_integration` compile-check.
- [ ] Grep gate: no `application::testing` import anywhere under `infrastructure/`.

### Phase 4 — polish + docs

- [ ] `Fingerprint` encapsulation (constructor + accessors; update ~6 construction/read sites).
- [ ] Decide-and-do or decide-and-document the float `PartialEq` / config truncation bullets.
- [ ] PLAN.md: dependency table + purity statements updated. BACKLOG: close item 11 remainder, "test helper coupling", "type/dependency polish" (delete stale `anyhow` claim).

---

## Tests

No new behavior — the test obligation is **equivalence**:

- Existing resample unit tests move with the code and pass unchanged.
- `offset_refinement.rs` test suite (~565–1085) passes with the adapter injected.
- Corpus committed tier + repair integration green at every phase boundary.
- Grep gates above enforced (consider a CI lint script later; manual for now).

## Exit criteria

- `rubato` and `cross_correlate` referenced only under `infrastructure/`.
- No `infrastructure → application::testing` imports.
- `Fingerprint.data` not publicly reachable.
- PLAN.md dependency rule matches reality.

## Cross-plan sequencing

- Parallel-safe with Plan 1 ([TEMP-output-error-contract-plan.md](TEMP-output-error-contract-plan.md)); if Plan 1 Phase 1 already added the resample warn, it moves here with the code.
- Land **before or after** [TEMP-media-session-redesign-plan.md](TEMP-media-session-redesign-plan.md) — no shared surface except `ports.rs` additions (merge-conflict-only risk).
