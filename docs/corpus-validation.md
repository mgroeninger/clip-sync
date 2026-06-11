# Corpus validation harness

Manifest-driven integration tests exercise real-world alignment scenarios: multiple containers/codecs, timing leaders, multi-track MP4, and multi-clip consistency.

**Case matrix:** [corpus-matrix.md](corpus-matrix.md)  
**Full dev guide (features, all test tiers):** [development.md](development.md)  
**Fixtures & commands:** [tests/corpus/README.md](../tests/corpus/README.md)  
**Archived plans:** [corpus implementation](archive/corpus-implementation-plan.md), [session reuse](archive/session-reuse-plan.md), [high-rate refinement](archive/high-rate-offset-refinement-plan.md)

---

## Quick start

```powershell
cargo test -p clip-sync corpus_                                          # committed + generated (~60s with ffmpeg)
cargo test -p clip-sync -- --ignored                                     # + external long smoke (CLIP_SYNC_CORPUS)
cargo test -p clip-sync --features he-aac,test-utils corpus_            # + HE-AAC cases
.\scripts\generate_corpus.ps1                                            # regenerate committed WAV fixtures
```

- **Committed tier** — 3 cases, 6 WAV files under `tests/corpus/wav/` (~3.4 MB).
- **Generated tier** — 19 cases built at test time; ffmpeg / `he-aac` cases skip when unavailable.
- **External tier** — `long_smoke_60m` (3600 s); `#[ignore]` unless `CLIP_SYNC_CORPUS` is set.

Harness code: `crates/clip-sync/src/application/testing/corpus_fixtures.rs`, generators in `audio_fixtures.rs` and `ffmpeg_util.rs`.

---

## What the corpus proved (2026-06-06)

| Finding | Resolution |
|---------|------------|
| MP3 without Xing/duration tag | Opens and aligns (`mp3_no_duration_tag` passes) |
| Stereo AAC downmix | Works (`mp4_stereo_leader_3s`) |
| Dual-track MP4 with `try_all_tracks` | Works when program track is scored (`mp4_dual_track_decoy`) |
| Identical decoy on A/B caused false offset 0 | **Fixed:** distinct decoy tones (220 Hz vs 330 Hz) per file |
| Default `select_best_track` on dual MP4 | **Fixed:** first decodable track in mux order (`mp4_dual_track_wrong_default`); use `try_all_tracks` when program is not first |
| Two-clip offset agreement | `require_consistent_offsets` blocks bad recommendations (`two_clip_inconsistent`, `require_consistent_blocks`) |
| Redundant probe+open per clip window | **Fixed:** one probe per file per run; format reader + decoders reused ([session reuse plan](archive/session-reuse-plan.md)) |
| Sub-50 ms residual after discovery on WAV | **Fixed:** optional high-rate hold-out refine (`wav_high_rate_refine_3s`, ±50 ms) |

---

## Option A false-pass evidence (2026-06-11)

Archived [offset-verification plan](archive/offset-verification-plan.md) Phase 0 left the “does Option A false-pass on self-similar hold-out?” spike unchecked. Phase 3 of [verification-hardening plan](archive/verification-hardening-plan.md) closes it.

**Probe:** manifest case `verify_option_a_false_pass_probe` — 120 s mono WAV pair with a **10 s chirp loop** tiled across the file (true inter-file offset +3 s). The dedicated test runs hold-out verification with **deliberately wrong** injected recommended offsets (+8 s and +18 s = +8 s plus one loop period), independent of discovery output.

**Discovery note:** the same looped fixture aliases in discovery to ≈ **+13 s** (+3 s true offset + 10 s loop period), not +3 s. That is a separate fingerprint-ambiguity signal; this probe does not assert on discovery offset.

**Outcome:** Option A **does not** false-pass on wrong injected Δ (`verified == false` for both probe values). Confidence stays below the 0.5 threshold or lag exceeds 0.5 s tolerance. **Option B** (PCM lag-0 via `refine_holdout_segment_lag`) remains **deferred** — no corpus evidence that Chromaprint lag search is fooled on the verification probe.

**Regression:** `cargo test -p clip-sync corpus_verify_option_a_false_pass_probe`

| Wrong Δ | `verified` | Notes |
|---------|------------|-------|
| +8 s (manifest `probe_wrong_verification_offset_secs`) | `false` | Same wrong-Δ class as unit test `verify_offset_fails_wrong_delta` |
| +18 s (+8 s + 10 s loop period) | `false` | Loop-period alias does not fool lag-0 fingerprint check |

---

## Validation diagnostics (v1 contract, 2026-06-11)

Shipped in [verification-hardening plan](archive/verification-hardening-plan.md) (phases 1–5, 2026-06-11). Behaviour summary for operators and test authors.

### Repetition downgrade vs `aligned`

When `check_clip_repetition` is on and internal repeat lag is within 1 s of the clip offset, hold-out / discovery confidence may be inflated. v1 handles this as follows:

1. `build_alignment_result` sets `aligned`, `offset_secs`, `start_aligned`, and `recommended_offset_secs` from **pre-downgrade** fingerprint confidence.
2. `AlignVideos` then may halve `ClipMatch.confidence` and attach `repetition` diagnostics.
3. JSON and human output show **post-downgrade** confidence; `aligned` does **not** flip when downgrade runs.

So a clip can show `aligned: true` with lowered confidence — by design in v1. See `align_videos.rs` (downgrade after `build_alignment_result`) and corpus case `repeated_segment_in_clip`.

### Hold-out verification cost

`--verify-offset` / `validation.verify_offset` extracts hold-out windows of length `clip.clip_length` on **both** files per scored candidate. With the Phase 2 retry cap, up to **three** candidates may be scored before reporting the best attempt.

**Rough decode budget per run:** up to `3 × 2 × clip_length` of mono PCM (e.g. default 15 min clips → up to ~90 minutes of audio decoded for verification alone, in addition to discovery clips). Shorter `clip_length` or early `verified == true` reduces cost. Optional `validation.max_verification_secs` remains a future knob (deferred Phase 6 in [verification-hardening plan](archive/verification-hardening-plan.md)) if this becomes painful in practice.

Committed-tier WAVs (30 s) cannot satisfy default 60 s minimum hold-out — see [tests/corpus/README.md](../tests/corpus/README.md) § Hold-out verification on committed tier. Generated cases `verify_offset_pass` and `mkv_tail_decodable_extent_gap` cover CI.

### Test roles (+3 s chirp)

Avoid duplicating the same E2E assertion in multiple suites. Intended split:

| Layer | Responsibility | Examples |
|-------|----------------|----------|
| **Corpus (manifest)** | End-to-end alignment + optional verify through `AlignVideos` | `wav_leader_3s`, `verify_offset_pass`, `corpus_verify_option_a_false_pass_probe` |
| **`align_videos` integration** | One real Symphonia + Chromaprint pipeline smoke | `execute_detects_known_offset_through_real_wav_pipeline` |
| **`align_videos` integration** | PCM refine / high-rate paths (not verify dedupe) | `cross_layer_high_rate_refine_*`, `high_rate_refine_*` |
| **`offset_verification` unit** | Hold-out pass/fail/skip/retry branches with fakes or temp WAVs | `verify_offset_*`, `verify_offset_retries_until_verified` |
| **`clip-sync-repair` integration** | Repair-specific concerns | `scan_gaps_integration`, `patch_audio_integration` (own chirp copies) |

**Removed (2026-06-11):** `execute_runs_offset_verification_when_flag_on` — overlapped `corpus_verify_offset_pass`.

Test fixtures: prefer `application/testing/alignment_fixtures.rs` (`minimal_alignment_result`, `start_clip_match`) over hand-built `AlignmentResult` in lib and CLI tests.

---

## Multi-track containers (`try_all_tracks`)

`select_best_track` picks the **first decodable audio track** in container mux order. When the main program is muxed first, dual-track MP4/MKV aligns correctly without extra flags (`mp4_dual_track_wrong_default`). When commentary or a decoy is muxed **before** the program, use `try_all_tracks`.

When `try_all_tracks` is enabled, the aligner decodes every decodable track pair on A and B, scores each alignment, and keeps the highest-confidence result. The same media session and format reader are reused across track pairs and clip windows.

**Enable via CLI:**

```powershell
clip-sync --try-all-tracks video_a.mp4 video_b.mp4
```

**Or in a config file** (`[alignment]` section):

```toml
try_all_tracks = true
```

Default is `false` because track-pair brute force multiplies decode work. Prefer enabling it when you know a container has multiple audio tracks or alignment looks wrong with the default pick.

---

## Follow-up

Tracked in [BACKLOG.md](../BACKLOG.md):

- Tighten `max_wall_secs` on other multi-clip cases if regressions are caught
- Dual-track case when decoy is muxed first (default pick still wrong; needs `try_all_tracks`)
- Optional shorter verification segment (`validation.max_verification_secs`) — deferred Phase 6 in [verification-hardening plan](archive/verification-hardening-plan.md); implement only on demonstrated friction
