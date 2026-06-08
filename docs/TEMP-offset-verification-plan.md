# Temporary plan: hold-out offset verification

> **Status:** Not started. Archive to `docs/archive/offset-verification-plan.md` when shipped.

**Problem:** With `num_clips == 1`, a single Chromaprint window is the only evidence for the recommended offset. A confident but wrong Δ has no independent check. Multi-clip runs compare offsets across windows but never test “at lag 0, do these shifted regions actually match?”

**Goal:** Optional second pass: given `recommended_offset_secs`, extract a hold-out window from each file (B shifted by Δ) and score **direct similarity at zero lag**. Off by default (`align.validation.verify_offset` on `AlignConfig`); enabled via config or CLI flag.

**Workspace split:** Hold-out logic and `AlignmentResult.offset_verification` in **`crates/clip-sync`**. `--verify-offset`, TOML, and human lines in **`crates/clip-sync-cli`**. See [Config](#config) and [Phases](#phases).

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
| **Window placement duration** | `pick_verification_window(min(duration_a, duration_b), discovery_windows, clip_length)`. After picking `T`, skip if either file cannot supply a full `clip_length` slice at the shifted positions (see [Skip conditions](#skip-conditions)). |
| **Short media (`duration < clip_length`)** | Skip verification (`pick_verification_window` → `None`). Hold-out would coincide with the sole discovery window; no shorter verify slice in v1. |
| **`num_clips > 2`** | Supported via gap heuristic: `[windows[0].end, windows.last().start)` when gap ≥ `clip_length`; else timeline midpoint (overlap risk accepted). Same as two-clip fallback. |
| **Overlap with discovery** | Prefer non-overlapping hold-out vs logical discovery `ClipWindow`s. When gap is too small, midpoint may overlap — still run; log `tracing::debug` overlap warning. |
| **Hook location** | `AlignVideos::execute()` after `align_single_track_pair` / `align_best_track_pair` returns, while sessions are still open. **Not** inside `align_extracted_pair`. |
| **`try_all_tracks`** | Re-extract hold-out on the **winning** `(track_a, track_b)` only. Record winning track indices during search (same pattern as repetition plan Phase 2). |
| **Skip / absent semantics** | Flag off → `offset_verification: None` (JSON field omitted via `skip_serializing_if`). Flag on + skip → `Some(OffsetVerification { verified: false, skipped: true, .. })`. Flag on + ran → `skipped: false`. |
| **Truncation** | Any hold-out extract shorter than `clip_length` after clamping → treat as skip (`skipped: true`), not partial scoring. |
| **Threshold knob** | `validation.min_verification_confidence` (default `0.5`). **No CLI flag for threshold in v1** — TOML / `AlignConfig` only. |
| **Architecture** | `pick_verification_window` in lib `domain/policies.rs` (pure). Hold-out extract + score in lib `application/offset_verification.rs`; reuse `Aligner` / `Fingerprinter` ports. CLI formats `AlignmentResult` only. No new port trait in v1. |
| **Phase 1 scope** | Core logic + unit tests + `tracing::debug` only. No stdout / JSON / CLI / `AlignmentResult` field until Phase 2. |
| **Human / JSON (Phase 2)** | When flag on, always emit `offset_verification` in JSON. Human warning when `verified == false` and not skipped; `--verbose` shows skip reason. |
| **Repetition interaction (Phase 3)** | After lag-0 score, if `check_clip_repetition`: run `detect_clip_repetition` on hold-out prepared clips; if `should_downgrade(repetition_*, recommended_offset_secs)` (±1 s, same helper as repetition plan), multiply **verification** `confidence` by `0.5` before threshold check. |
| **Execution order** | Discovery alignment (and discovery repetition, if enabled) completes first; verification is a separate pass in `execute()`. |
| **Corpus fail path** | **Unit test** `verify_offset_fails_wrong_delta` only (inject wrong Δ). Corpus adds **pass** case `verify_offset_pass`; no manifest fail case in v1. |
| **POC risk** | Phase 0 spike: confirm Option A returns ≈0 lag + high confidence on matching hold-out chirp pair, and fails on same pair with Δ + 5s. |

---

## Config

Shared with [TEMP-clip-self-repetition-plan.md](TEMP-clip-self-repetition-plan.md). Implement the **full** `ValidationConfig` once when either feature lands.

### Library (`AlignConfig.validation`)

`ValidationConfig` on **`AlignConfig`** in `crates/clip-sync/src/application/config.rs`:

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
- `load_align_config` deserializes `[validation]` with `[clip]` and `[alignment]`.

### CLI (`AppConfig` — TOML + flags only)

Top-level TOML unchanged — `[validation]` flattens via `AppConfig.align`:

```toml
[validation]
check_clip_repetition = false
min_repetition_confidence = 0.5
verify_offset = false
min_verification_confidence = 0.5
```

- **CLI Phase 2:** `--verify-offset` → `config.align.validation.verify_offset = true` in `apply_cli_overrides`.
- When flag is **off**, omit `offset_verification` from JSON via `#[serde(skip_serializing_if = "Option::is_none")]` on lib `AlignmentResult`.

**Behaviour when verification fails (score below threshold, not skipped):**

- **Lib:** `offset_verification.verified = false`; keep `recommended_offset_secs`.
- **CLI Phase 2:** exit code **0**; human line warns that offset was not independently verified.

### Skip conditions

Skip verification (emit `skipped: true`, `verified: false`) when:

- `verify_offset` is false (omit field entirely).
- No `recommended_offset_secs` (no alignment or `require_consistent_offsets` blocked recommendation).
- `pick_verification_window` returns `None` (`duration < clip_length`, or zero duration).
- Hold-out window would extend past either file’s timeline after applying Δ (full `clip_length` required on both sides).
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

**Lib**

- [ ] Option A on synthetic hold-out: matching +3s chirp pair → `\|offset_secs\| ≤ 0.5`, confidence ≥ 0.5
- [ ] Same pair with intentional wrong Δ (+5s) → fails threshold or `\|offset_secs\| > 0.5`
- [ ] Record whether false passes warrant Option B before Phase 1 ships

**CLI:** none

### Phase 1 — Hold-out extract + lag-0 score (lib only)

**Lib (`clip-sync`)**

- [ ] `ValidationConfig` nested in `AlignConfig` (full struct with repetition fields — shared with repetition plan)
- [ ] `domain/policies.rs` — `pick_verification_window(duration, existing_windows, clip_length) -> Option<ClipWindow>`
- [ ] `application/offset_verification.rs` — **new** `verify_offset_at_holdout(...)` → internal `VerificationResult`
- [ ] Lag-0 score via `find_offset` + tolerance check (Option A)
- [ ] `application/align_videos.rs` — wire in `execute()` after alignment, before `log_alignment_summary`; winning tracks for `try_all_tracks`
- [ ] `ProgressReporter::phase("Verifying offset at hold-out window...")` when running (lib port; no CLI code)
- [ ] Unit tests: window picker, known Δ + chirp → verified; wrong Δ → not verified; negative Δ; skip when `duration < clip_length`
- [ ] **`AlignmentResult.offset_verification` not set yet** — debug / internal struct only

**CLI:** none

### Phase 2 — Reporting (lib domain + CLI stdout)

**Lib (`clip-sync`)**

- [ ] Add `offset_verification: Option<OffsetVerification>` to `AlignmentResult`
- [ ] Merge Phase 1 `VerificationResult` into returned `AlignmentResult`
- [ ] `try_all_tracks`: record winning tracks; verification uses that pair only
- [ ] Lib tests: `AlignmentResult` JSON shape when flag on/off/skipped (`serde_json`)
- [ ] Document in [PLAN.md](../PLAN.md); note redundancy when `num_clips >= 2` and offsets already agree

**CLI (`clip-sync-cli`)**

- [ ] `infrastructure/cli/args.rs` — `--verify-offset`
- [ ] `infrastructure/cli/mod.rs` — `apply_cli_overrides`: `config.align.validation.verify_offset = true`
- [ ] `infrastructure/cli/output.rs` — human warning when `verified == false` and not skipped; verbose skip reason
- [ ] `tests/config_roundtrip.rs` — TOML `[validation] verify_offset = true`
- [ ] `tests/cli_output.rs` — human lines for verified / unverified / skipped

### Phase 3 — Corpus + repetition cross-check (lib only)

**Lib (`clip-sync`)**

- [ ] `detect_clip_repetition` on hold-out prepared clips when both flags on; apply `should_downgrade` to verification confidence (from repetition plan)
- [ ] Test `verification_downgrade_when_holdout_repeats`
- [ ] `application/testing/corpus_fixtures.rs` — case `verify_offset_pass`; extend `CorpusCase` with `verify_offset` / `expect_offset_verified`
- [ ] Archive this doc → `docs/archive/offset-verification-plan.md`

**CLI:** none (prints lib-populated `offset_verification` field)

---

## Design

### Hold-out window placement

Pick a window on the **shorter** file’s timeline that does not overlap discovery windows when possible.

```text
pick_verification_window(duration, windows, clip_length):
  if duration < clip_length:
    return None
  if windows.len() == 1:
    # Single discovery clip at start — use middle third
    T = duration / 3
    return [T, min(T + clip_length, duration))
  if windows.len() >= 2:
    # Start/end (or start/interior/end): gap between first.end and last.start
    gap_start = windows[0].end
    gap_end = windows.last().start
    if gap_end - gap_start >= clip_length:
      T = gap_start + (gap_end - gap_start - clip_length) / 2
      return [T, T + clip_length)
    else:
      # Short gap — timeline midpoint; may overlap discovery windows
      T = (duration - clip_length) / 2
      return [T, T + clip_length)
```

Feasibility after picking `T` (use per-file durations `dur_a`, `dur_b` and recommended Δ):

```text
A needs: 0 <= T  and  T + L <= dur_a
B needs: 0 <= T + Δ  and  T + L + Δ <= dur_b
If either fails → skip (skipped: true)
```

Verification extracts (same track indices as winning alignment):

```text
A: [T, T + L)
B: [T + Δ, T + L + Δ)
```

Then resample → `prepare_clip_for_fingerprint` → fingerprint → `find_offset`.

### Lag-0 similarity

```text
estimate = find_offset(fp_a, fp_b)
verified = !skipped
  && estimate.confidence >= min_verification_confidence
  && estimate.offset_secs.abs() <= OFFSET_AGREEMENT_TOLERANCE_SECS  // 0.5
```

Optional PCM sanity check (reuse `offset_refinement` cross-correlate on hold-out slice) — defer unless Option A produces false passes in corpus.

### Interaction with existing checks

| Existing | Relationship |
|----------|--------------|
| `num_clips >= 2` + `offsets_consistent` | Verification still runs when flag on; most useful for `num_clips == 1`. Document as supplementary lag-0 evidence. |
| `require_consistent_offsets` | Runs before verification; no recommendation → skip verify |
| `refine_offset_with_pcm` | Discovery only; verification uses final recommended Δ |
| `check_clip_repetition` | Phase 3: repetition on hold-out clips may ×0.5 verification confidence (see Decisions) |

```text
execute():
  open sessions
  result = align_single_track_pair(...) or align_best_track_pair(...)
  if validation.verify_offset && result.recommended_offset_secs.is_some():
      progress: "Verifying offset at hold-out window..."
      T = pick_verification_window(min(dur_a, dur_b), discovery_windows, L)
      if T is None or !feasible(T, Δ, dur_a, dur_b):
          offset_verification = { verified: false, skipped: true, skip_reason: ... }
      else:
          extract A[T, T+L), B[T+Δ, T+L+Δ) on winning tracks
          prepare + fingerprint + find_offset → confidence
          Phase 3: optional repetition downgrade on hold-out clips
          verified = confidence >= threshold && |lag| <= 0.5
          offset_verification = { ..., skipped: false }
  else if verify_offset:
      offset_verification = None   // flag off
  log_alignment_summary
  return
```

Phase 1: store result internally / debug log only; Phase 2: attach to lib `AlignmentResult`. CLI prints via existing JSON/human formatters.

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

**Human** — **CLI only** (`crates/clip-sync-cli/src/infrastructure/cli/output.rs`):

**Human** (unverified, not skipped):

```text
  Recommended offset: +3.000s (not independently verified; hold-out confidence 0.32)
```

**Human** (`--verbose`, skipped):

```text
  Offset verification skipped: hold-out window unavailable
```

---

## Tests

| Test | Phase | Crate | Asserts |
|------|-------|-------|---------|
| `pick_verification_window_middle_for_single_clip` | 1 | lib | 60s media, 15s clip, one discovery window → hold-out in middle third |
| `pick_verification_window_fits_two_clip_gap` | 1 | lib | 60s, start+end windows → hold-out in gap |
| `pick_verification_window_none_when_shorter_than_clip` | 1 | lib | `duration < clip_length` → `None` |
| `verify_offset_passes_known_leader` | 1 | lib | +3s chirp, correct Δ → `verified = true` |
| `verify_offset_passes_negative_delta` | 1 | lib | B-ahead chirp, correct negative Δ → `verified = true` |
| `verify_offset_fails_wrong_delta` | 1 | lib | Intentionally wrong Δ → `verified = false` |
| `verify_offset_skips_when_window_infeasible` | 1 | lib | Δ pushes B window past `dur_b` → `skipped = true` |
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

- `src/application/align_videos.rs` — `execute()` post-alignment hook
- `src/application/offset_verification.rs` — **new** hold-out verify module
- `src/domain/alignment.rs` — `OffsetVerification`, extend `AlignmentResult`
- `src/domain/policies.rs` — `clip_windows`, `pick_verification_window`
- `src/application/config.rs` — `ValidationConfig` on `AlignConfig`
- `src/infrastructure/chromaprint/aligner.rs` — `find_offset` / Option A
- `src/application/testing/corpus_fixtures.rs` — corpus case (Phase 3)

### CLI (`crates/clip-sync-cli`)

- `src/infrastructure/cli/args.rs`, `mod.rs` — `--verify-offset`, `apply_cli_overrides`
- `src/infrastructure/cli/output.rs` — human verification lines
- `src/infrastructure/config.rs` — `AppConfig`
- `tests/cli_output.rs`, `tests/config_roundtrip.rs`

### Other

- [TEMP-clip-self-repetition-plan.md](TEMP-clip-self-repetition-plan.md) — shared `ValidationConfig`, `should_downgrade`, `detect_clip_repetition`
- [PLAN.md](../PLAN.md) — target architecture
- [BACKLOG.md](../BACKLOG.md) — tracking item
