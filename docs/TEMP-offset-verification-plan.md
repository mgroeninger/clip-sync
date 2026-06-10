# Temporary plan: hold-out offset verification

> **Status:** Not started (verification module). **Partial infrastructure shipped:** hold-out window placement + feasibility helpers in lib `domain/policies.rs` (added for high-rate refinement). Workspace refactor (Phases 1–4) complete — paths below are **`crates/clip-sync`** + **`crates/clip-sync-cli`**. Archive to `docs/archive/offset-verification-plan.md` when shipped.

**Problem:** With `num_clips == 1`, a single Chromaprint window is the only evidence for the recommended offset. A confident but wrong Δ has no independent check. Multi-clip runs compare offsets across windows but never test “at lag 0, do these shifted regions actually match?”

**Goal:** Optional second pass: given `recommended_offset_secs`, extract a hold-out window from each file (B shifted by Δ) and score **direct similarity at zero lag**. Off by default (`align.validation.verify_offset` on `AlignConfig`); enabled via config or CLI flag.

**Workspace split:** Hold-out logic and `AlignmentResult.offset_verification` in **`crates/clip-sync`**. `--verify-offset`, TOML, and human lines in **`crates/clip-sync-cli`**. **`clip-sync-repair`** embeds `AlignmentResult` in `GapReport` — verification appears in nested JSON when enabled via repair TOML `[validation]`. See [Config](#config) and [Phases](#phases).

---

## Current codebase baseline

Audit against the tree **after** workspace migration (2026-06-08).

| Area | Path | Current state | Target phase |
|------|------|---------------|--------------|
| **AlignConfig** | `crates/clip-sync/src/application/config.rs` | `{ clip, alignment }` only — **no `validation` field** | 1 |
| **AppConfig** | `crates/clip-sync-cli/src/infrastructure/config.rs` | Flattens `align` + `output` + `logging` | 2 |
| **`execute()` hook** | `crates/clip-sync/src/application/align_videos.rs` | Align → `apply_high_rate_refinement(...)` → `log_alignment_summary` | 1 (insert verify **after** high-rate, before summary) |
| **AlignmentOutcome** | same | `{ result, track_a, track_b, discovery_windows, duration_a/b, decoded_extent_a/b }` — same shape as `HighRateRefinementInput` | 1 |
| **Hold-out placement** | `crates/clip-sync/src/domain/policies.rs` | **`pick_holdout_window`**, **`holdout_window_candidates`**, **`holdout_window_feasible`** — shipped for high-rate refine | 1 (reuse; do **not** add `pick_verification_window`) |
| **Hold-out tests** | `policies.rs` `#[cfg(test)]` | `pick_holdout_window_*`, `holdout_window_feasible_*`, candidate tests | 1 (extend for verify-specific cases) |
| **High-rate template** | `crates/clip-sync/src/application/high_rate_refinement.rs` | Post-align pass: candidates → feasibility → extract → score; sets `HighRateRefinement` on result | 1 (mirror pattern for verify) |
| **Verification module** | — | **Missing** (`application/offset_verification.rs`) | 1 |
| **OffsetVerification type** | `domain/alignment.rs` | **Missing** on `AlignmentResult` (has `high_rate_refinement`, `offset_drift_secs`, `start_overlap`) | 2 |
| **Lag-0 score** | `infrastructure/chromaprint/aligner.rs` | `find_offset` via `Aligner` port (Option A) | 1 |
| **PCM fallback** | `application/offset_refinement.rs` | `normalized_correlation` on facade (`clip_sync::normalized_correlation`) — Option B if Option A false-passes | 0 / defer |
| **CLI** | `clip-sync-cli/.../cli/args.rs` (`Cli`) | No `--verify-offset` | 2 |
| **Corpus** | `application/testing/corpus_fixtures.rs` | `tests/corpus/manifest.toml` at workspace root | 3 |

**Naming:** This plan originally said `pick_verification_window`; the implemented API is **`pick_holdout_window`**. Verification uses the same helpers as high-rate refinement, but segment length = **`clip.clip_length`** (not `alignment.high_rate_refine_secs`).

---
## Decisions

Locked before implementation. Change only with an explicit plan revision.

| Topic | Decision |
|-------|----------|
| **User-visible failure** | Keep `recommended_offset_secs`; set `offset_verification.verified = false`. Exit code stays **0**. Never clear the recommendation in v1. |
| **Report model** | `offset_verification: Option<OffsetVerification>` on `AlignmentResult`. `None` when `verify_offset` is off; `Some(...)` when flag is on (including skips). |
| **Lag-0 scoring (Phase 1)** | **Option A:** reuse `find_offset` on hold-out fingerprints; require `\|offset_secs\| ≤ 0.5` (same constant as `OFFSET_AGREEMENT_TOLERANCE_SECS`) and `confidence ≥ validation.min_verification_confidence`. Option B (explicit lag-0 / PCM) only if corpus or unit tests show false passes. |
| **PCM prep** | Same pipeline as discovery: resample to `target_sample_rate`, then `prepare_clip_for_fingerprint` (normalize / trim). **No** `expand_window_for_slide` on hold-out — fixed `[T, T + L)` extract only. **No** `select_aligned_subclip_pair` on hold-out. |
| **PCM refinement** | `refine_offset_with_pcm` applies to discovery clips only. Verification uses final recommended Δ; does not re-refine. |
| **Window placement duration** | Reuse **`holdout_window_candidates(min(dur_a, dur_b, decoded_extent_*), discovery_windows, clip_length, Δ)`** then **`holdout_window_feasible`** (same as `high_rate_refinement.rs`). Primary picker inside candidates: **`pick_holdout_window`**. Segment length = **`config.clip.clip_length`**, not `high_rate_refine_secs`. |
| **Short media (`duration < clip_length`)** | Skip verification (`pick_holdout_window` / empty candidates → skip). Hold-out would coincide with the sole discovery window; no shorter verify slice in v1. |
| **`num_clips > 2`** | Supported via gap heuristic: `[windows[0].end, windows.last().start)` when gap ≥ `clip_length`; else timeline midpoint (overlap risk accepted). Same as two-clip fallback. |
| **Overlap with discovery** | Prefer non-overlapping hold-out vs logical discovery `ClipWindow`s. When gap is too small, midpoint may overlap — still run; log `tracing::debug` overlap warning. |
| **Hook location** | `AlignVideos::execute()` **after** `apply_high_rate_refinement`, **before** `log_alignment_summary`, while `session_a` / `session_b` are still open. Pass `AlignmentOutcome` fields (same as `HighRateRefinementInput`). **Not** inside `align_extracted_pair`. |
| **`try_all_tracks`** | `align_best_track_pair` already returns **`AlignmentOutcome { track_a, track_b, … }`**. Verification uses that winning pair only (no search-loop changes). |
| **Skip / absent semantics** | Flag off → `offset_verification: None` (JSON field omitted via `skip_serializing_if`). Flag on + skip → `Some(OffsetVerification { verified: false, skipped: true, .. })`. Flag on + ran → `skipped: false`. |
| **Truncation** | Any hold-out extract shorter than `clip_length` after clamping → treat as skip (`skipped: true`), not partial scoring. |
| **Threshold knob** | `validation.min_verification_confidence` (default `0.5`). **No CLI flag for threshold in v1** — TOML / `AlignConfig` only. |
| **Architecture** | Window pickers in lib **`domain/policies.rs`** (**exist**). Hold-out extract + lag-0 score in lib **`application/offset_verification.rs`** (**new**); reuse `Aligner` / `Fingerprinter` ports + `extract_mono` (discovery rate). CLI formats `AlignmentResult` only. No new port trait in v1. |
| **Phase 1 scope** | Core logic + unit tests + `tracing::debug` only. No stdout / JSON / CLI / `AlignmentResult` field until Phase 2. |
| **Human / JSON (Phase 2)** | When flag on, always emit `offset_verification` in JSON. Human warning when `verified == false` and not skipped; `--verbose` shows skip reason. |
| **Repetition interaction (Phase 3)** | After lag-0 score, if `check_clip_repetition`: run `detect_clip_repetition` on hold-out prepared clips; if `should_downgrade(repetition_*, recommended_offset_secs)` (±1 s, same helper as repetition plan), multiply **verification** `confidence` by `0.5` before threshold check. |
| **Execution order** | Discovery alignment (and discovery repetition, if enabled) completes first; verification is a separate pass in `execute()`. |
| **Corpus fail path** | **Unit test** `verify_offset_fails_wrong_delta` only (inject wrong Δ). Corpus adds **pass** case `verify_offset_pass`; no manifest fail case in v1. |
| **POC risk** | Phase 0 spike: confirm Option A (`find_offset` on prepared hold-out clips) returns ≈0 lag + high confidence on matching hold-out chirp pair, and fails with intentional wrong Δ (+5s). Window placement de-risked by existing `pick_holdout_window` tests. |

---

## Config

Shared with [TEMP-clip-self-repetition-plan.md](TEMP-clip-self-repetition-plan.md). Implement the **full** `ValidationConfig` once when either feature lands.

### Library (`AlignConfig.validation`)

`ValidationConfig` is **not in the tree yet**. Add on **`AlignConfig`** in `crates/clip-sync/src/application/config.rs` — shared with [TEMP-clip-self-repetition-plan.md](TEMP-clip-self-repetition-plan.md).

**Current `AlignConfig` (today):**

```rust
pub struct AlignConfig {
    pub clip: ClipConfig,
    pub alignment: AlignmentConfig,
    // validation: ValidationConfig,  // Phase 1 — add with #[serde(default)]
}
```

**Target (Phase 1):**

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AlignConfig {
    #[serde(default)]
    pub clip: ClipConfig,
    #[serde(default)]
    pub alignment: AlignmentConfig,
    #[serde(default)]
    pub validation: ValidationConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ValidationConfig {
    #[serde(default)]
    pub check_clip_repetition: bool,          // repetition plan
    #[serde(default = "default_min_repetition_confidence")]
    pub min_repetition_confidence: f32,
    /// After alignment, extract hold-out clips shifted by recommended offset and score lag-0 match.
    #[serde(default)]
    pub verify_offset: bool,
    /// Minimum lag-0 similarity to set verified = true.
    #[serde(default = "default_min_verification_confidence")]
    pub min_verification_confidence: f32,
}

fn default_min_verification_confidence() -> f32 {
    0.5
}
```

- `AlignVideos::execute()` reads `config.validation.verify_offset`.
- `load_align_config` in `crates/clip-sync/src/infrastructure/config/file.rs` deserializes `[validation]` with `[clip]` and `[alignment]`.
- `AlignConfig::validate()` today only calls `clip.validate()` — no extra validation rules for `[validation]` in v1.

### CLI (`AppConfig` — TOML + flags only)

Top-level TOML unchanged — `[validation]` flattens via `AppConfig.align`:

```toml
[validation]
check_clip_repetition = false
min_repetition_confidence = 0.5
verify_offset = false
min_verification_confidence = 0.5
```

- **CLI Phase 2:** `--verify-offset` on `Cli` in `args.rs` → `config.align.validation.verify_offset = true` in `apply_cli_overrides` (`cli/mod.rs`).
- Human verbose / skip lines use `config.output.show_diagnostics` (`--verbose` sets this today).
- When flag is **off**, omit `offset_verification` from JSON via `#[serde(skip_serializing_if = "Option::is_none")]` on lib `AlignmentResult`.

**Behaviour when verification fails (score below threshold, not skipped):**

- **Lib:** `offset_verification.verified = false`; keep `recommended_offset_secs`.
- **CLI Phase 2:** exit code **0**; human line warns that offset was not independently verified.

### Skip conditions

Skip verification (emit `skipped: true`, `verified: false`) when:

- `verify_offset` is false (omit field entirely).
- No `recommended_offset_secs` (no alignment or `require_consistent_offsets` blocked recommendation).
- `holdout_window_candidates(...)` returns empty, or no candidate passes **`holdout_window_feasible`** (full `clip_length` required on both sides after Δ).
- Either hold-out extract fails (`InsufficientAudio` / `EmptyClip`).
- Either prepared hold-out clip yields an empty fingerprint.

---

## Types

All types in **`crates/clip-sync/src/domain/alignment.rs`** (library).

```rust
/// Hold-out lag-0 check (Phase 2 on AlignmentResult; Phase 1 internal struct may omit Serialize).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OffsetVerification {
    pub window_a_start_secs: f64,
    pub window_a_end_secs: f64,
    pub window_b_start_secs: f64,
    pub window_b_end_secs: f64,
    /// Lag-0 fingerprint match confidence (after Phase 3 repetition downgrade, if any).
    pub confidence: f32,
    pub verified: bool,
    /// True when verification did not run (no window, extract failure, etc.).
    #[serde(default)]
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

// AlignmentResult — add in Phase 2:
#[serde(skip_serializing_if = "Option::is_none")]
pub offset_verification: Option<OffsetVerification>,
```

Sign convention for B extract: match `ClipMatchEstimate.offset_secs` (“seconds to add to video A’s timeline to align with video B”).

---

## Phases

### Phase 0 — Spike

**Lib (`crates/clip-sync`)**

- [ ] Option A on synthetic hold-out: matching +3s chirp pair → `\|offset_secs\| ≤ 0.5`, confidence ≥ 0.5 (`Aligner::find_offset` on prepared hold-out clips @ `target_sample_rate`)
- [ ] Same pair with intentional wrong Δ (+5s) → fails threshold or `\|offset_secs\| > 0.5`
- [ ] Record whether false passes warrant Option B (`normalized_correlation` on prepared PCM) before Phase 1 ships
- [x] Hold-out **window** placement de-risked — `pick_holdout_window` + tests already in `domain/policies.rs`

**CLI:** none

### Phase 1 — Hold-out extract + lag-0 score (lib only) ✅

**Lib (`crates/clip-sync`)**

- [x] `ValidationConfig` on `AlignConfig` (full struct with repetition fields — shared with repetition plan)
- [x] `domain/policies.rs` — **`pick_holdout_window`**, **`holdout_window_candidates`**, **`holdout_window_feasible`** (reuse; extend tests only if verify needs new edge cases)
- [x] `application/offset_verification.rs` — **new** `apply_offset_verification(...)` (mirror `apply_high_rate_refinement` structure; segment length = `clip.clip_length`)
- [x] Hold-out extract via existing **`MediaSession::extract_mono`** @ `target_sample_rate` → `prepare_clip_for_fingerprint` (no `expand_window_for_slide`, no subclip slide)
- [x] Lag-0 score via `Aligner::find_offset` + tolerance check (Option A); compare lag to **`OFFSET_AGREEMENT_TOLERANCE_SECS` (0.5)** in `domain/alignment.rs`
- [x] `application/align_videos.rs` — call from `execute()` after `apply_high_rate_refinement`, before `log_alignment_summary`; pass `AlignmentOutcome` / shared input struct
- [x] `ProgressReporter::phase_verbose("Verifying offset at hold-out window...")` when running
- [x] Unit tests: verify pass/fail/wrong-Δ/skip/flag-off (see [Tests](#tests))
- [x] **`AlignmentResult.offset_verification` not set yet** — internal `VerificationOutcome` + `tracing::debug` only

**CLI (`clip-sync-cli`):** none

### Phase 2 — Reporting (lib domain + CLI stdout) ✅

**Lib (`crates/clip-sync`)**

- [x] Add `offset_verification: Option<OffsetVerification>` to `AlignmentResult` in `domain/alignment.rs`
- [x] `apply_offset_verification` writes `result.offset_verification` when flag on (including skip cases)
- [x] Lib tests: `AlignmentResult` JSON shape when flag on/off/skipped (`serde_json` in `align_videos` or domain tests)

**CLI (`crates/clip-sync-cli`)**

- [x] `infrastructure/cli/args.rs` — `--verify-offset` on `Cli`
- [x] `infrastructure/cli/mod.rs` — `apply_cli_overrides`: `config.align.validation.verify_offset = true`
- [x] `infrastructure/cli/output.rs` — after recommended-offset block: warn when `verified == false` and not skipped; verbose skip reason via `show_diagnostics`
- [x] `tests/fixtures/analyzer.toml` — optional `[validation]` example
- [x] `tests/config_roundtrip.rs` — TOML `[validation] verify_offset = true`
- [x] `tests/cli_output.rs` — human lines for verified / unverified / skipped

### Phase 3 — Corpus + repetition cross-check (lib only)

**Lib (`clip-sync`)**

- [ ] `detect_clip_repetition` on hold-out prepared clips when both flags on; apply `should_downgrade` to verification confidence (from repetition plan)
- [ ] Test `verification_downgrade_when_holdout_repeats`
- [ ] `application/testing/corpus_fixtures.rs` — case `verify_offset_pass`; extend `CorpusCase` with `verify_offset` / `expect_offset_verified`
- [ ] Archive this doc → `docs/archive/offset-verification-plan.md`

**CLI:** none (prints lib-populated `offset_verification` field)

---

## Design

### Hold-out window placement (already implemented)

Window pickers live in **`crates/clip-sync/src/domain/policies.rs`**. Verification reuses the same API as **`apply_high_rate_refinement`** (`high_rate_refinement.rs`), but with **`segment_length = config.clip.clip_length`**.

```text
pick_duration = min(duration_a, duration_b, decoded_extent_a, decoded_extent_b)
candidates = holdout_window_candidates(pick_duration, discovery_windows, clip_length, Δ)
for holdout in candidates:
  if holdout_window_feasible(holdout.start, clip_length_secs, Δ, dur_a, dur_b):
    try extract + score; break on success
```

Core placement logic (inside `holdout_window_candidates` → `pick_holdout_window`):

```text
pick_holdout_window(duration, windows, segment_length):
  if duration < segment_length: return None
  if windows.len() <= 1:
    T = duration / 3
    return [T, min(T + segment_length, duration))
  gap_start = windows[0].end; gap_end = windows.last().start
  if gap_end - gap_start >= segment_length:
    T = gap_start + (gap_end - gap_start - segment_length) / 2
  else:
    T = (duration - segment_length) / 2   # may overlap discovery — debug log
  return [T, T + segment_length)
```

`holdout_window_candidates` also prepends overlap-safe near-start windows (important for negative Δ and MKV seek quirks) — prefer trying candidates in order, same as high-rate refine.

Feasibility (existing **`holdout_window_feasible`**):

```text
A needs: 0 <= T  and  T + L <= dur_a
B needs: 0 <= T + Δ  and  T + L + Δ <= dur_b
If no candidate feasible → skip (skipped: true)
```

Verification extracts (winning tracks from `AlignmentOutcome`):

```text
A: extract_mono(track_a, [T, T + L)) @ target_sample_rate
B: extract_mono(track_b, [T + Δ, T + L + Δ)) @ target_sample_rate
→ prepare_clip_for_fingerprint → fingerprint → aligner.find_offset
```

### Lag-0 similarity

```text
estimate = find_offset(fp_a, fp_b)
verified = !skipped
  && estimate.confidence >= min_verification_confidence
  && estimate.offset_secs.abs() <= 0.5   // OFFSET_AGREEMENT_TOLERANCE_SECS in domain/alignment.rs (private; use same constant)
```

### Option A vs Option B (lag-0 scoring)

Both options answer the same question after hold-out extraction: given recommended Δ, do the shifted hold-out regions actually match?

**Shared setup (both options):**

1. Pick a hold-out window (not used in discovery).
2. Extract A at `[T, T + L)` and B at `[T + Δ, T + L + Δ)`.
3. Resample to `target_sample_rate`, then `prepare_clip_for_fingerprint` (same pipeline as discovery).

After that they diverge.

**Option A — Chromaprint `find_offset` (Phase 1 default):** Fingerprint both hold-out clips, then run the same alignment search used in discovery (`Aligner::find_offset` → `match_fingerprints` → best segment). Pass when the best-match lag is near zero (`|offset_secs| ≤ 0.5`) and confidence ≥ `min_verification_confidence`. Reuses existing ports and the discovery confidence model; still a lag search (not a literal “score at lag 0 only” check), so repetitive or ambiguous hold-out audio can false-pass — Phase 3 repetition downgrade targets that.

**Option B — Explicit lag-0 PCM correlation (fallback):** Skip fingerprint lag search; compare prepared PCM waveforms directly at lag 0 via `normalized_correlation` (same family as `refine_offset_with_pcm` / `refine_holdout_segment_lag` in `offset_refinement.rs`). Pass when correlation at lag 0 meets a threshold — no “best lag” step. Truly answers “how similar are these waveforms right now?”; less likely to be fooled by fingerprint ambiguity when waveforms are clearly misaligned. Different code path and threshold semantics; more sensitive to prep differences. **Defer unless Option A false-passes in Phase 0 spike or corpus.**

| | **Option A** (`find_offset`) | **Option B** (`normalized_correlation`) |
|---|---|---|
| **Signal** | Chromaprint fingerprints | Prepared PCM samples |
| **Operation** | Search all lags, pick best | Score only at lag 0 |
| **Pass means** | Best lag ≈ 0 + high confidence | High direct waveform similarity |
| **Reuses** | Discovery aligner | PCM refinement machinery |
| **Main risk** | Spurious ~0-lag fingerprint match | Prep / resampling sensitivity |
| **Plan status** | Phase 1 default | Fallback if A false-passes |

**Relation to high-rate refinement:** High-rate refinement (`apply_high_rate_refinement`) is a separate post-align pass that extracts **native-rate** PCM from a short hold-out, cross-correlates to find a small lag adjustment, and **may change** `recommended_offset_secs`. Verification uses the **final** Δ (post high-rate) and **does not change** it — it only judges whether that Δ is credible. Same hold-out window helpers (`holdout_window_candidates`, `holdout_window_feasible`), but different segment length (`high_rate_refine_secs` vs `clip_length`), extract rate (native vs `target_sample_rate`), and purpose: high-rate = “tweak Δ”; verification = “trust Δ?”

### Interaction with existing checks

| Existing | Relationship |
|----------|--------------|
| `num_clips >= 2` + `offsets_consistent` | Verification still runs when flag on; most useful for `num_clips == 1`. Document as supplementary lag-0 evidence. |
| `require_consistent_offsets` | Runs before verification; no recommendation → skip verify |
| `refine_offset_high_rate` | Runs **before** verification; verify uses **post-refinement** `recommended_offset_secs`. Same hold-out helpers, different segment length and scoring. |
| `refine_offset_with_pcm` | Discovery only; verification uses final recommended Δ |
| `check_clip_repetition` | Phase 3: repetition on hold-out clips may ×0.5 verification confidence (see Decisions) |

```text
execute() — current structure (align_videos.rs):
  open session_a, session_b
  outcome = align_single_track_pair(...) OR align_best_track_pair(...)
  mut result = outcome.result
  apply_high_rate_refinement(&HighRateRefinementInput { sessions, tracks, discovery_windows, durations, ... }, ...)
  // INSERT Phase 1:
  if config.validation.verify_offset:
      apply_offset_verification(&VerificationInput { same fields as high-rate + clip config }, &mut result, ...)
  log_alignment_summary(&result, ...)
  return AlignVideosResponse { result }
```

Inside `apply_offset_verification` when flag on and `recommended_offset_secs` is `Some(Δ)`:

```text
progress: "Verifying offset at hold-out window..."
candidates = holdout_window_candidates(..., clip_length, Δ)
if no feasible candidate:
    offset_verification = { verified: false, skipped: true, skip_reason: ... }
else:
    extract A/B hold-out → prepare → fingerprint → find_offset → confidence
    Phase 3: optional repetition downgrade on hold-out clips
    verified = confidence >= threshold && |estimate.offset_secs| <= 0.5
    offset_verification = { windows..., skipped: false, ... }
When flag off: leave result.offset_verification = None (Phase 2 serde omit)
When flag on but no recommendation: Some({ skipped: true, ... })
```

Phase 1: store result internally / `debug!` only; Phase 2: set `result.offset_verification`. CLI prints via existing JSON/human formatters (`print_success` / `serde_json::to_string_pretty`).

### Output (Phase 2)

**JSON** — lib `AlignmentResult` serde; CLI `output.rs` prints when `OutputFormat::Json`.

**JSON** (`--format json`, flag on, verification ran):

```json
{
  "recommended_offset_secs": 3.0,
  "offset_verification": {
    "window_a_start_secs": 20.0,
    "window_a_end_secs": 35.0,
    "window_b_start_secs": 23.0,
    "window_b_end_secs": 38.0,
    "confidence": 0.88,
    "verified": true,
    "skipped": false
  }
}
```

**JSON** (flag on, skipped):

```json
{
  "offset_verification": {
    "confidence": 0.0,
    "verified": false,
    "skipped": true,
    "skip_reason": "hold-out window unavailable"
  }
}
```

**Human** — **CLI only** (`crates/clip-sync-cli/src/infrastructure/cli/output.rs`, `print_human`):

Uses `show_diagnostics` for verbose skip lines (`--verbose` sets this today).

**Human** (unverified, not skipped):

```text
  Recommended offset: +3.000s (not independently verified; hold-out confidence 0.32)
```

**Human** (`--verbose` / `show_diagnostics`, skipped):

```text
  Offset verification skipped: hold-out window unavailable
```

---

## Tests

| Test | Phase | Crate | Asserts |
|------|-------|-------|---------|
| `pick_holdout_window_middle_for_single_clip` | — | lib | **Exists** in `policies.rs` |
| `pick_holdout_window_fits_two_clip_gap` | — | lib | **Exists** in `policies.rs` |
| `pick_holdout_window_none_when_shorter_than_segment` | — | lib | **Exists** in `policies.rs` |
| `holdout_window_feasible_respects_offset` | — | lib | **Exists** in `policies.rs` |
| `verify_offset_passes_known_leader` | 1 | lib | +3s chirp, correct Δ → `verified = true` |
| `verify_offset_passes_negative_delta` | 1 | lib | B-ahead chirp, correct negative Δ → `verified = true` |
| `verify_offset_fails_wrong_delta` | 1 | lib | Intentionally wrong Δ → `verified = false` |
| `verify_offset_skips_when_window_infeasible` | 1 | lib | Δ pushes B past `dur_b` → `skipped = true` |
| `alignment_result_json_offset_verification` | 2 | lib | Flag on → JSON includes `offset_verification`; flag off → field omitted |
| `cli_human_unverified_offset` | 2 | CLI | Human line when `verified == false` |
| `cli_human_verbose_skip_reason` | 2 | CLI | Verbose shows skip reason |
| `config_verify_offset_roundtrip` | 2 | CLI | TOML `verify_offset = true` via `AppConfig` |
| `verification_downgrade_when_holdout_repeats` | 3 | lib | Hold-out repeat lag ≈ Δ → confidence ×0.5 may flip `verified` |
| Corpus `verify_offset_pass` | 3 | lib | End-to-end `num_clips = 1`, flag on, +3s leader |

### Corpus

**Case `verify_offset_pass`**

| Field | Value |
|-------|-------|
| Base | Reuse `wav_leader_3s` or generated +3s chirp pair |
| `num_clips` | `1` |
| `verify_offset` | `true` |
| Assert | `recommended_offset_secs` within existing +3s tolerance |
| Assert | `offset_verification.verified == true`, `skipped == false` |
| Assert | `offset_verification.confidence >= 0.5` |

Extend `CorpusCase` with optional `verify_offset: bool` and `expect_offset_verified: Option<bool>` when implementing Phase 3 (lib `corpus_fixtures.rs`).

---

## References

### Library (`crates/clip-sync`)

| File | Status |
|------|--------|
| `src/application/align_videos.rs` | **Exists** — `execute()`, `AlignmentOutcome`; insert verify after high-rate |
| `src/application/high_rate_refinement.rs` | **Exists** — template for post-align hold-out pass |
| `src/application/offset_verification.rs` | **New** — `apply_offset_verification` |
| `src/domain/policies.rs` | **Exists** — `pick_holdout_window`, `holdout_window_candidates`, `holdout_window_feasible` + tests |
| `src/domain/alignment.rs` | **Exists** — add `OffsetVerification`; extend `AlignmentResult` |
| `src/application/config.rs` | **Exists** — add `ValidationConfig` on `AlignConfig` |
| `src/infrastructure/config/file.rs` | **Exists** — `load_align_config` |
| `src/infrastructure/chromaprint/aligner.rs` | **Exists** — `find_offset` (Option A) |
| `src/application/offset_refinement.rs` | **Exists** — `normalized_correlation` on facade (Option B fallback) |
| `src/application/testing/corpus_fixtures.rs` | **Exists** — corpus case (Phase 3) |

### CLI (`crates/clip-sync-cli`)

| File | Status |
|------|--------|
| `src/application/run_align.rs` | **Exists** — `align_with_defaults`; no verify logic here |
| `src/infrastructure/cli/args.rs` | **Exists** — add `--verify-offset` on `Cli` |
| `src/infrastructure/cli/mod.rs` | **Exists** — `apply_cli_overrides`, `run_inner` |
| `src/infrastructure/cli/output.rs` | **Exists** — extend `print_human` for verification lines |
| `src/infrastructure/config.rs` | **Exists** — `AppConfig` flattens `AlignConfig` |
| `tests/cli_output.rs`, `tests/config_roundtrip.rs` | **Exists** — extend for verification |

### Other

- [TEMP-clip-self-repetition-plan.md](TEMP-clip-self-repetition-plan.md) — shared `ValidationConfig`, `should_downgrade`, `detect_clip_repetition` (Phase 3)
- [PLAN.md](../PLAN.md) — workspace layout (`crates/clip-sync`, `crates/clip-sync-cli`)
- [BACKLOG.md](../BACKLOG.md) — Phase 5 validation diagnostics
- `crates/clip-sync-repair` — `GapReport.alignment` includes `offset_verification` when enabled
