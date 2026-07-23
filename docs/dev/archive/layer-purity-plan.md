# Internal layer purity (shipped)

> **Status:** Shipped (2026-06-11). All phases complete. See [BACKLOG.md](../../../BACKLOG.md).

**Problem:** PLAN.md claimed "Domain depends on nothing external" and "application depends on domain only", but: `rubato` was used directly in `domain/resample.rs`; the `cross_correlate` crate was called directly from `application/offset_refinement.rs`; infrastructure tests imported `application::testing::ffmpeg_util` (dependency arrow pointing the wrong way); and `Fingerprint` exposed a bare `pub data: Vec<u32>`.

**Goal:** Make the dependency rule true again — DSP engines behind ports with adapters in infrastructure, ffmpeg test helpers out of the application module, `Fingerprint` encapsulated — with zero behavior change.

**Workspace split:** All work in **`crates/clip-sync`**. **`clip-sync-repair`** compiles via stable facade paths (`resample_interleaved`, `normalized_correlation`, `clip_sync::testing::ffmpeg_util`).

---

## Decisions (summary)

| Topic | Decision |
|-------|----------|
| **Resampler port** | `resample_mono` on trait; analyzer pipeline injects `RubatoResampler`. Repair uses facade `resample_interleaved` (no port injection in v1). |
| **Correlator port** | `PcmCorrelator` + `FftCorrelator`; `refine_*` functions take `&dyn PcmCorrelator`. |
| **Test support** | `src/test_support/` for `ffmpeg_util` + `audio_fixtures`; re-exported at `clip_sync::testing::*`. |
| **Fingerprint** | Private `data`; `new`, `items`, `len`, `is_empty`. |
| **Float `PartialEq` / config truncation** | Documented on `ClipMatchEstimate` / `RepetitionFinding`; `duration_secs` module rustdoc + round-trip test. |

---

## Phases

### Phase 1 — `Resampler` port

- [x] `Resampler` trait + `RubatoResampler` in `infrastructure/resample/` (moved from `domain/resample.rs`).
- [x] `AlignVideos` + `default_pipeline.rs` wiring; `FakeResampler` in `testing/fakes.rs`.
- [x] Call sites: `align_videos.rs`, `offset_verification.rs`, `offset_refinement.rs`.
- [x] Facade: `RubatoResampler` + `resample_interleaved` convenience fn; repair untouched.

### Phase 2 — `PcmCorrelator` port

- [x] `PcmCorrelator` + `FftCorrelator` in `infrastructure/correlation.rs`.
- [x] `refine_*` + `pcm_cross_correlate_lag` parameterized with ports.

### Phase 3 — test-support relocation

- [x] `ffmpeg_util` + `audio_fixtures` → `src/test_support/`; infrastructure imports fixed.

### Phase 4 — polish + docs

- [x] `Fingerprint` encapsulation.
- [x] Float `PartialEq` + `duration_secs` truncation documented (no behavior change).
- [x] `PLAN.md` dependency table + purity rule; `BACKLOG.md` synced.

---

## Exit criteria

- [x] `rubato` and `cross_correlate` referenced only under `infrastructure/`.
- [x] No `infrastructure → application::testing` imports.
- [x] `Fingerprint.data` not publicly reachable.
- [x] PLAN.md dependency rule matches reality.

## Follow-up (not in plan)

- [BACKLOG.md](../../../BACKLOG.md): optional `Resampler::resample_interleaved` trait shrink if still unused after grep.
