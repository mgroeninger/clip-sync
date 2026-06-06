# Temporary plan: hold-out offset verification

> **Status:** Not started. Archive to `docs/archive/offset-verification-plan.md` when shipped.

**Problem:** With `num_clips == 1`, a single Chromaprint window is the only evidence for the recommended offset. A confident but wrong Δ has no independent check. Multi-clip runs compare offsets across windows but never test “at lag 0, do these shifted regions actually match?”

**Goal:** Optional second pass: given `recommended_offset_secs`, extract a hold-out window from each file (B shifted by Δ) and score **direct similarity at zero lag**. Off by default; enabled via config.

---

## Config

Same `ValidationConfig` section as [TEMP-clip-self-repetition-plan.md](TEMP-clip-self-repetition-plan.md):

```toml
[validation]
verify_offset = false   # default
min_verification_confidence = 0.5   # optional; default matches min_match_score scale
```

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationConfig {
    // check_clip_repetition: bool,  // sibling flag — see other plan
    /// After alignment, extract hold-out clips shifted by recommended offset and score lag-0 match.
    #[serde(default)]
    pub verify_offset: bool,
    /// Minimum lag-0 similarity to set verified = true.
    #[serde(default = "default_min_match_score")]
    pub min_verification_confidence: f32,
}
```

Optional CLI mirror (Phase 2): `--verify-offset`.

**Behaviour when verification fails:**

- Set `offset_verified: false` on `AlignmentResult`.
- If verification was the only extra check and score is below threshold: clear `recommended_offset_secs` **or** keep offset but mark unverified — **prefer keep + unverified** for v1 (user still sees the estimate with a warning). Document choice in implementation; corpus tests lock it in.

Skip verification when:

- No `recommended_offset_secs` (no alignment or `require_consistent_offsets` blocked recommendation).
- Hold-out window would extend past either file’s duration after applying Δ.
- Either hold-out extract fails (`InsufficientAudio` / `EmptyClip`).

---

## Phases

### Phase 1 — Hold-out extract + lag-0 score

- [ ] Plan doc
- [ ] `ValidationConfig` on `AppConfig` (shared struct with repetition plan)
- [ ] `pick_verification_window(duration, existing_windows, clip_length) -> Option<ClipWindow>`
- [ ] `verify_offset_at_holdout(session_a, session_b, Δ, window, ...) -> VerificationResult`
- [ ] Lag-0 score via fingerprint: `find_offset` on pair should return ≈0 with high confidence, **or** dedicated `similarity_at_lag_zero` helper (prefer explicit lag-0 to avoid re-searching Δ)
- [ ] Wire after `build_alignment_result` in `align_videos`
- [ ] Unit tests: known Δ + chirp → verified; wrong Δ → not verified

### Phase 2 — Reporting + CLI

- [ ] `OffsetVerification { window_a, window_b, confidence, verified }` on `AlignmentResult`
- [ ] Human + JSON output; progress phase: `Verifying offset at hold-out window...`
- [ ] CLI `--verify-offset`
- [ ] Document in PLAN.md; note redundancy when `num_clips >= 2` and offsets already agree

### Phase 3 — Corpus

- [ ] Manifest case `verify_offset_pass` (`num_clips = 1`, flag on, +3s leader)
- [ ] Manifest case `verify_offset_fail_wrong_delta` (synthetic: force wrong Δ in test hook or inconsistent pair)
- [ ] Archive this doc

---

## Design

### Hold-out window placement

Pick a window that does **not** overlap discovery windows when possible.

```text
pick_verification_window(duration, windows, clip_length):
  if duration < clip_length:
    return None   # same region as single discovery clip — skip or use shorter verify slice (defer)
  if windows.len() == 1:
    # Single discovery clip at start — use middle third
    T = duration / 3
    return [T, min(T + clip_length, duration))
  if windows.len() >= 2:
    # Default 2-clip: start [0,L) and end [dur-L, dur) — gap in middle
    gap_start = windows[0].end
    gap_end = windows.last().start
    if gap_end - gap_start >= clip_length:
      T = gap_start + (gap_end - gap_start - clip_length) / 2
      return [T, T + clip_length)
    else:
      # Short gap — use midpoint of full timeline, accept partial overlap risk
      T = (duration - clip_length) / 2
      return [T, T + clip_length)
```

Verification extracts:

```text
A: [T, T + L)
B: [T + Δ, T + L + Δ)   # clamp to [0, duration_b); fail gracefully if truncated
```

Sign convention: match existing `ClipMatchEstimate.offset_secs` (“seconds to add to video A’s timeline to align with video B”).

### Lag-0 similarity (preferred over re-estimating Δ)

```text
fp_a = fingerprint(extract A at hold-out)
fp_b = fingerprint(extract B at hold-out shifted by Δ)

Option A (minimal): find_offset(fp_a, fp_b) → require |offset_secs| < 0.5 and confidence >= threshold
Option B (clearer):  match_fingerprints at constrained lag 0 only / PCM correlation peak near 0

Start with Option A for reuse; switch to Option B if false passes appear in corpus.
```

Optional PCM sanity check (reuse `offset_refinement` cross-correlate on short slice) — Phase 2 only if fingerprint verification is flaky.

### Interaction with existing checks

| Existing | Relationship |
|----------|--------------|
| `num_clips >= 2` + `offsets_consistent` | Verification adds lag-0 evidence; optional — document as most useful for `num_clips == 1` |
| `require_consistent_offsets` | Runs before verification; no recommendation → skip verify |
| `refine_offset_with_pcm` | Refinement applies to discovery clips only; verification uses final recommended Δ |
| `check_clip_repetition` | If repetition flagged on hold-out clip, downgrade verification confidence (Phase 3) |

```text
execute():
  ... normal alignment → AlignmentResult with recommended_offset_secs
  if validation.verify_offset && recommended_offset_secs.is_some():
      pick hold-out window
      extract shifted pair
      score lag-0
      merge OffsetVerification into result
  return
```

---

## Tests

| Test | Asserts |
|------|---------|
| `pick_verification_window_middle_for_single_clip` | 60s media, 15s clip, one discovery window → hold-out in middle third |
| `pick_verification_window_fits_two_clip_gap` | 60s, start+end windows → hold-out in gap |
| `verify_offset_passes_known_leader` | +3s chirp, correct Δ → `verified = true` |
| `verify_offset_fails_wrong_delta` | Intentionally wrong Δ → `verified = false` |
| Corpus `verify_offset_pass` | End-to-end with `num_clips = 1` |

---

## References

- `src/application/align_videos.rs` — post-alignment hook
- `src/domain/alignment.rs` — extend `AlignmentResult`
- `src/domain/policies.rs` — `clip_windows` (hold-out must not assume same labels)
- `src/infrastructure/chromaprint/aligner.rs` — lag-0 scoring
- [TEMP-clip-self-repetition-plan.md](TEMP-clip-self-repetition-plan.md) — shared `ValidationConfig`
- [BACKLOG.md](../BACKLOG.md) — add items when work starts
