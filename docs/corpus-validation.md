# Corpus validation harness

Manifest-driven integration tests exercise real-world alignment scenarios: multiple containers/codecs, timing leaders, multi-track MP4, and multi-clip consistency.

**Case matrix:** [corpus-matrix.md](corpus-matrix.md)  
**Full dev guide (features, all test tiers):** [development.md](development.md)  
**Fixtures & commands:** [tests/corpus/README.md](../tests/corpus/README.md)  
**Archived plans:** [corpus implementation](archive/corpus-implementation-plan.md), [session reuse](archive/session-reuse-plan.md), [high-rate refinement](archive/high-rate-offset-refinement-plan.md)

---

## Quick start

```powershell
# PR alignment slice (committed corpus; same as pr-align tier)
.\scripts\test-tier.ps1 -Tier pr-align

# Or ad-hoc name filter on lib:
cargo test -p clip-sync corpus_committed

cargo test -p clip-sync corpus_                                          # committed + generated (~60s with ffmpeg)
cargo test -p clip-sync -- --ignored                                     # + external long smoke (CLIP_SYNC_CORPUS)
cargo test -p clip-sync --features he-aac,test-utils corpus_            # + HE-AAC cases
.\scripts\generate_corpus.ps1                                            # regenerate committed WAV fixtures
```

Full tier machinery: [development.md](development.md), [test-tier-remainder.md](test-tier-remainder.md).

- **Committed tier** — 3 cases, 6 WAV files under `tests/corpus/wav/` (~3.4 MB).
- **Generated tier** — 20 cases built at test time; ffmpeg / `he-aac` cases skip when unavailable.
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

## Option A false-pass evidence (2026-06-11, updated)

Archived [offset-verification plan](archive/offset-verification-plan.md) Phase 0 left the “does Option A false-pass on self-similar hold-out?” spike unchecked. Phase 3 of [verification-hardening plan](archive/verification-hardening-plan.md) closes the non-period case; **period-equivalent** wrong Δ was added 2026-06-11.

**Probe:** manifest case `verify_option_a_false_pass_probe` — 120 s mono WAV pair with a **10 s chirp loop** tiled across the file (true inter-file offset +3 s). The dedicated test injects recommended offsets and runs hold-out verification **without** asserting discovery output.

**Discovery alias:** the same fixture aliases in discovery to ≈ **+13 s** (+3 s true + 10 s loop period), not +3 s. That is cross-file fingerprint periodicity, not random noise. Do **not** use `looped_chirp_pair` for discovery offset oracles — use `offset_chirp_pair` instead.

**How verification can false-pass:** Option A compares hold-out segments placed at `window_A` and `window_B = window_A + injected_offset`. It then fingerprints both and requires lag ≈ 0 between them. When audio is **periodic with period T**, a placement error of **N×T** still yields segments that match at lag 0, so `verified == true` even though the inter-file offset is wrong.

**Regression:** `cargo test -p clip-sync corpus_verify_option_a_false_pass_probe`

| Injected Δ | `verified` | Notes |
|------------|------------|-------|
| +8 s (manifest `probe_wrong_verification_offset_secs`) | `false` | ≡ +8 mod 10; not equivalent to true +3 s |
| +18 s (+8 s + 10 s loop period) | `false` | Same residue mod 10 as +8 s |
| +13 s (true +3 s + one loop period) | **`false`** (gated) | Option A still scores a pass internally; periodic gating + PCM parallel recheck reject `verified` |

**Shipped mitigation (2026-06-11):** with `check_clip_repetition`, discovery sets `offset_ambiguous_mod_secs` when strong start-clip repetition is detected. Start-clip confidence is halved only when [`should_downgrade_periodic_ambiguity`](../../crates/clip-sync/src/domain/alignment.rs) applies (`|offset| ≥ T − 1`) or when the existing lag-near-offset rule fires — not for every periodic flag (e.g. `repeated_segment_in_clip` at +3 s with T ≈ 30 s sets the flag but keeps confidence). Hold-out verify runs calendar-parallel **PCM** recheck at the file edge (only when a repeat period is already known), compares to recommended Δ, and sets `verify_inconclusive` when they disagree by **N×T** (or beyond tolerance). See [archive/periodic-ambiguity-plan.md](archive/periodic-ambiguity-plan.md).

---

## Validation diagnostics (v1 contract, 2026-06-11)

Shipped in [verification-hardening plan](archive/verification-hardening-plan.md) (phases 1–5, 2026-06-11). Behaviour summary for operators and test authors.

### Repetition downgrade vs `aligned`

When `check_clip_repetition` is on, hold-out / discovery confidence may be inflated by internal repeat. v1 applies **two separate downgrade rules** (each halves confidence once; they do not stack to ×0.25):

| Rule | Function | Fires when |
|------|----------|------------|
| Lag near offset | `should_downgrade_repetition_confidence` | Strong repeat lag within ±1 s of `\|clip offset\|` |
| Period alias | `should_downgrade_periodic_ambiguity` | Strong repeat **and** `\|offset\| ≥ T − 1` (offset likely a period alias, e.g. +13 s when T = 10 s) |

`offset_ambiguous_mod_secs` is set whenever strong start-clip repetition is detected (`periodic_ambiguity_period`), **independent** of whether confidence is halved.

Pipeline:

1. `build_alignment_result` sets `aligned`, `offset_secs`, `start_aligned`, and `recommended_offset_secs` from **pre-downgrade** fingerprint confidence.
2. `AlignVideos` may halve `ClipMatch.confidence` (either rule above) and attach `repetition` diagnostics; may set `offset_ambiguous_mod_secs` on the result.
3. JSON and human output show **post-downgrade** confidence; `aligned` does **not** flip when downgrade runs.

So a clip can show `aligned: true` with lowered confidence — by design in v1. Example: `repeated_segment_in_clip` (+3 s, T ≈ 30 s) sets the ambiguity flag but usually **does not** halve confidence. See `align_videos.rs` and `corpus_repeated_segment_sets_ambiguity_flag`.

### Periodic offset ambiguity

When `check_clip_repetition` finds strong internal repeat on the **start** clip:

1. `offset_ambiguous_mod_secs` is set to the repeat period **T** (diagnostic; may be normalized from harmonic/sub-octave autocorrelation lags).
2. Start-clip confidence is halved only when `should_downgrade_periodic_ambiguity` or lag-near-offset downgrade applies (see table above) — **not** automatically for every periodic flag.
3. Hold-out verify runs calendar-parallel **PCM** recheck at the file edge (`parallel_holdout_window_candidates`, `T=0` first) **only when** a repeat period is already known from discovery or start-clip repetition.
4. If parallel offset disagrees with recommended Δ by **N×T** (or beyond tolerance), `verified` is forced false and `verify_inconclusive` is set — even when Option A scored a pass on offset-shifted hold-out segments.

Human output (default): `Warning: offset ambiguous (repeats every ~N s) — …` and, when gated, `Verify: offset not independently verified (periodic content; …)`. Verbose adds parallel recheck offset lines — see [cli-output.md](cli-output.md).

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

## Query-reference corpus (`wav_query_reference_*`)

Shipped with [query-reference alignment](archive/query-reference-alignment-plan.md) (2026-06-15). Exercises query-reference mode — not symmetric offset chirp pairs. A-long cases embed short B on long A; B-long cases embed short A on long B (donor-longer repair scenario).

| Case | Tier | Generator | Asserts |
|------|------|-----------|---------|
| `wav_query_reference_b_longer_fast` | **generated** (default CI) | `query_reference_b_longer_chirp_pair` | 3 min B + 70 s A @ 1:30 on B; `clip_on_a_start_secs ≈ 0`, `anchor_ref_secs ≈ 90`, `recommended_offset_secs ≈ +90` |
| `wav_query_reference_45min_anchor` | **generated** (`#[ignore]`) | `query_reference_chirp_pair` | 60 min A + 8 min B @ 45:00; `anchor_ref_secs` and `recommended_offset_secs` within **±0.05 s** |
| `wav_query_reference_b_longer_anchor` | **generated** (`#[ignore]`) | `query_reference_b_longer_chirp_pair` | 60 min B + 8 min A @ 45:00 on B; `clip_on_a_start_secs ≈ 0`, `anchor_ref_secs ≈ 2700`, `recommended_offset_secs ≈ +2700` |

Run the fast B-longer case in default PR checks:

```powershell
cargo test -p clip-sync corpus_query_reference_b_longer_fast
```

Run slow 60 min oracles alone:

```powershell
cargo test -p clip-sync corpus_query_reference_45min_anchor -- --ignored
cargo test -p clip-sync corpus_query_reference_b_longer_anchor -- --ignored
```

Included in `cargo test -p clip-sync corpus_generated -- --ignored`. Fields: `alignment_mode = "queryreference"`, `expect_clip_on_a_start_secs`, tight `tolerance_secs`.

**Test roles:** manifest-driven alignment oracle (lib `corpus_fixtures.rs`); repair integration uses smaller synthetic chirp pairs in `clip-sync-repair/tests/query_reference_integration.rs` (gap inside/outside mapped region).

---

## Gap fill / repair patch (`fill_mode = fit`)

Shipped with [fill-fitting plan](TEMP-fill-fitting-plan.md) (phases A–D, 2026-06-20). Exercises **patch** after alignment — not gap scan or offset discovery.

| Layer | Tier | Generator / fixture | Asserts |
|-------|------|---------------------|---------|
| `patch_audio_integration` | **committed** (CI) | Stereo sine + gap WAVs (`write_stereo_sine_with_gap`) | Patch count, skip reasons, fit marginal tier, gate regression, extension |
| `patch_audio_integration` | **ignored** | Same fixtures, production-like fit config (`fill_border_search_secs = 10`, full extension grid) | `patch_audio_fit_production_defaults_smoke` — run before release |
| `query_reference_integration` | **committed** | Short chirp pairs | Gap inside/outside mapped region under `fill_mode = gate` |
| `gap_corpus` | external / manual | `CLIP_SYNC_GAP_CORPUS` real media | Listen + skip/marginal counts (see gap corpus README) |
| `gap_corpus_patch_timing_committed` | **committed** (CI) | Gap corpus WAVs + generated clean B reference | Patch wall-time budget (`max_patch_wall_secs` in manifest) |
| `gap_corpus_patch_timing_production` | **ignored** | Same fixtures, `RepairConfig::default()` fit | Manual perf smoke before release |

**CI command:**

```powershell
cargo test -p clip-sync-repair patch_audio_integration
cargo test -p clip-sync-repair patch_audio_fit_production_defaults -- --ignored
cargo test -p clip-sync-repair gap_corpus_patch_timing_committed
cargo test -p clip-sync-repair gap_corpus_patch_timing_production -- --ignored --nocapture
```

**Patch summary fields to track** (JSON / `PatchSummary`): `patched_count`, `skipped_count`, `patched_marginal_count`, per-gap `confidence`, `gap_*_adjust_frames`.

### Manual acceptance (external pair or `CLIP_SYNC_GAP_CORPUS`)

Automated CI covers synthetic fixtures only (`patch_audio_integration`). This checklist is for **operator sign-off** after changing fit search, performance defaults, or `fill_repeat_penalty_weight` on **long-form or real media** (e.g. `CLIP_SYNC_GAP_CORPUS` — see [gap corpus README](../crates/clip-sync-repair/tests/gap_corpus/README.md)). Record date, config path, and media identifiers when complete.

| Check | Pass? | Notes |
|-------|-------|-------|
| Full repair (align + scan + patch) completes in acceptable wall time | | Note `fill_border_search_secs` and extension flags |
| Patch rate ≥ prior gate baseline (or fewer skips with same listen quality) | | Compare `patched_count` / `skipped_count` |
| No new audible repeat-at-seam vs last good build | | A/B listen on patched gaps |
| `-v`: marginal gaps show `! patched` and plausible structure/waveform slides | | |
| `patched_marginal_count` recorded if &gt; 0 | | |

**Suggested command** (adjust paths and config):

```powershell
clip-sync-repair --config repair.toml -v scan-gaps <video_a> <video_b>
clip-sync-repair --config repair.toml -v patch-audio <video_a> <video_b>
```

Optional: tune `fill_repeat_penalty_weight` in repair config after listen pass (default **0.4**; set to `0` to disable; increase only with evidence).

---

## Energy signature production corpus

Shipped with [archive/energy-corpus-plan.md](archive/energy-corpus-plan.md) (Phases A–G complete). Pure-Rust **F1/F2/F3-long** + **F4-decoy** fixtures @ 48 kHz exercise **structure signature** discrimination (`bool` / `energy` / `auto`) at production geometry — orthogonal to **gap_corpus** chirp scan tests. **EC-6 met:** F4-decoy separates `energy` (slides to the true pause) from `bool` (stays at the decoy) through the full patch path; the shipped **mode-coupled `fill_fit_energy_nominal_bias_scale`** (default 0.25) lets energy auto-correct a drifted nominal map.

**Vocabulary:** tag names and derivation rules live in [gap-repair-guide.md](gap-repair-guide.md) § Vocabulary. Use **guide P0–P7** for plan-time gap types; use **EC-1–EC-6** for corpus acceptance IDs (not the same namespace).

### Corpus tiers

| Layer | Tier | Fixture / test | Asserts |
|-------|------|----------------|---------|
| Domain oracle | **integration** (oracle label) | `tests/oracle_energy.rs` — U1–U8 (8 s integration fixtures), **EC-3** (`p3_`), **EC-6** score (`p4_f4_decoy_energy_separates_but_bool_ties`); **EC-1/EC-2** production geometry (`p1_`/`p2_`, `#[ignore]` — PR uses **U3/U5** instead) | Unified match on full B; energy vs bool discrimination |
| Scan path | **integration** | `tests/integration_energy_smoke.rs` — `scan_detects_f1_production_gap`, `f1_production_scan_and_domain_smoke` | `ScanGaps` finds gap within ±0.35 s |
| **End-to-end (CI smoke)** | **integration** | `corpus_scan_patch_smoke` in `integration_energy_smoke.rs` (~5 s) | Full scan → patch on 16 kHz / 32 s production-geometry F1; asserts one gap detected and patched |
| Integration (fast) | **integration** | I1–I4 @ 8 s in `patch_audio_integration.rs` | Patch with oracle `GapReport`, structure-heavy weights |
| Mode matrix + EC-6 patch | **validation** / **diagnostic** | `validate_residual_gate.rs` (`f4_decoy_patch_discrimination`, …); `diag_energy_matrix.rs` (`energy_signature_mode_matrix`, …) | CSV rows (fixture × mode × context); **EC-6:** energy → true pause / bool → decoy |

**CI commands** (prefer [test-tier.ps1](development.md#default--ci-commands); bare `cargo test -p clip-sync-repair` runs **`--lib` only** after Phase 3):

```powershell
# PR repair slice (integration + oracle label)
.\scripts\test-tier.ps1 -Tier pr-repair

# Ad hoc — integration binaries (PR runs U3/U5 for EC-1/EC-2; production geometry via --ignored)
cargo test -p clip-sync-repair --test oracle_energy u5 f2 p3_f3 p4_f4
cargo test -p clip-sync-repair --test oracle_energy p1_f1_production_energy_unified_finds_true_offset p2_f2_production_energy_unified_finds_pause_one -- --ignored --nocapture
cargo test -p clip-sync-repair --test integration_energy_smoke corpus_scan_patch_smoke

# Validation (EC-6 patch layer)
cargo test -p clip-sync-repair --features validation-tests --test validate_residual_gate f4_decoy_patch_discrimination

# Diagnostic (mode matrix CSV)
cargo test -p clip-sync-repair --features diagnostic-tests --test diag_energy_matrix energy_signature_mode_matrix -- --nocapture
```

### Fixture scenarios → oracles and tags

Record **fixture oracle** (what the test asserts) separately from **run tags** (what `-v` would show on a production-default patch).

| `fixture_scenario` | Geometry | Domain oracle (EC-*) | Typical run tags (production default, if patched) |
|--------------------|----------|----------------------|---------------------------------------------------|
| `F1-long` | Decoy dropout inside 10 s border; wrong nominal B map | **EC-1:** energy/`auto` → true offset (`p1_` production domain `#[ignore]`; **U3** on PR; `f1_production_scan_patch_smoke` patch) | `plan_kind=fillable`, `signature_mode=energy`, non-zero slide |
| `F2-long` | Dual pause; nominal → pause₂, truth → pause₁ | **EC-2:** energy → pause₁ (`p2_` production domain `#[ignore]`; **U5** on PR; `f2_production_oracle_patch_smoke` patch, slide ≈ 0 from A-aligned nominal) | `plan_kind=fillable`, `signature_mode=energy`, slide ≈ 0 |
| `F3-long` | Steady drone | **EC-3:** `auto` → resolved `bool` | `signature_mode=bool`, `content_hint=flat` |
| `F4-decoy` | A gap at decoy; truth shifted +7 s in B; identical bool pattern, anti-correlated energy contour, waveform-neutral inner border | **EC-6:** `energy`/`auto` patch the true pause (slide +7 s), `bool` stays at the decoy (slide 0) — `p4_*` domain; `f4_decoy_patch_discrimination` patch | `signature_mode=energy`, slide → true pause |

Short fixtures **F1–F3** @ 11.025 Hz / 8 s integration (**U\***, **I1–I4**) use the same geometry at `integration_fast()` scale.

### Matrix row format (tuning record)

When running the ignored mode matrix or manual CLI on written WAVs, log one line per run:

```text
fixture,mode_config,context_secs,gap_report_source,patched,skipped,marginal,wall_ms,tags,notes
F1-long,auto,3,scan_derived,0,1,0,8420,"plan=fillable tier=structure_fail seam=n/a sig=energy","domain OK; haystack structure 0/0"
```

**Tags column:** copy from `-v` `gap tags:` when patch runs; otherwise derive from JSON skip reason + scores ([gap-repair-guide.md](gap-repair-guide.md) § Deriving tags).

**Run metadata columns:** `mode_config` = `signature_mode_config`; `gap_report_source` = `scan_derived` (after `ScanGaps`) or `oracle_injected` (`gap_report_from_energy_fixture`, I1 pattern).

### Manual acceptance (energy corpus / real media)

After matrix or config changes, extend the [gap fill checklist](#manual-acceptance-external-pair-or-clip_sync_gap_corpus) with:

| Check | Pass? | Notes |
|-------|-------|-------|
| EC-1–EC-3 oracle tests green | | `cargo test -p clip-sync-repair --test oracle_energy production` |
| Mode matrix recorded (diagnostic run) | | `.\scripts\test-tier.ps1 -Tier diagnostic -Package clip-sync-repair -Nocapture` |
| `-v` `gap tags:` match expected tier for listen-approved gaps | | Compare to fixture table above |
| `auto`/`energy` patch cases where `bool` skips or mis-places | | **EC-6 met** on synthetic F4-decoy (`f4_decoy_patch_discrimination`); confirm on real drift-heavy media |

---

## Follow-up

Tracked in [BACKLOG.md](../BACKLOG.md):

- ~~Periodic offset ambiguity~~ — shipped 2026-06-11 ([archive/periodic-ambiguity-plan.md](archive/periodic-ambiguity-plan.md))
- Tighten `max_wall_secs` on other multi-clip cases if regressions are caught
- Dual-track case when decoy is muxed first (default pick still wrong; needs `try_all_tracks`)
- Optional shorter verification segment (`validation.max_verification_secs`) — deferred Phase 6 in [verification-hardening plan](archive/verification-hardening-plan.md); implement only on demonstrated friction
