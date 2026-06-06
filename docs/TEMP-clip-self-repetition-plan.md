# Temporary plan: clip self-repetition check

> **Status:** Not started. Archive to `docs/archive/clip-self-repetition-plan.md` when shipped.

**Problem:** A clip whose audio repeats internally (loop, rebroadcast, duplicated segment) can produce ambiguous Chromaprint matches — both cross-file alignment and offset verification may latch onto the wrong lag with high confidence.

**Goal:** Optional per-clip diagnostic that detects strong *non-zero* self-matches in a fingerprint and surfaces them in the alignment report. Off by default; enabled via config.

---

## Config

New `ValidationConfig` section on `AppConfig` (keeps alignment knobs separate from quality gates):

```toml
[validation]
check_clip_repetition = false   # default
```

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationConfig {
    /// Fingerprint each extracted clip against itself; warn when strong internal repeats exist.
    #[serde(default)]
    pub check_clip_repetition: bool,
}
```

Optional CLI mirror (Phase 2): `--check-clip-repetition`.

When enabled, run the check on every `MonoPcmClip` after extraction (before or after fingerprinting for alignment — fingerprint once, reuse for self-match).

**Behaviour when repetition is detected:**

- Do **not** fail the run by default (diagnostic only).
- Attach per-clip repetition metadata to verbose / JSON output.
- Optionally downgrade that clip’s alignment confidence by a fixed factor (e.g. ×0.5) when `repetition_lag_secs` is within ±tolerance of the cross-file offset — defer to Phase 2; document in plan only.

---

## Phases

### Phase 1 — Core detection

- [ ] Plan doc
- [ ] `ValidationConfig` + defaults on `AppConfig`
- [ ] `detect_clip_repetition(fingerprint, preset) -> Option<RepetitionFinding>` in Chromaprint adapter
- [ ] Wire into `align_videos` extract loop when flag is set
- [ ] Unit tests: silent clip (none), chirp with literal repeat segment (some lag), non-repeating chirp (none)

### Phase 2 — Reporting

- [ ] `RepetitionFinding { lag_secs, confidence, items_count }` on `ClipMatch` or nested diagnostic struct
- [ ] Human + JSON output when `show_diagnostics` / `--verbose`
- [ ] CLI `--check-clip-repetition`
- [ ] Document in PLAN.md and corpus-validation.md

### Phase 3 — Corpus + policy

- [ ] Generator: clip with 10s tone block copied to t=30s
- [ ] Manifest case `repeated_segment_in_clip` (expect repetition flagged; alignment may still pass)
- [ ] Optional confidence interaction with cross-file offset (Phase 1 deferral)
- [ ] Archive this doc

---

## Design

### Detection algorithm

Reuse existing Chromaprint stack — no new dependencies.

```text
fp = fingerprint(clip)
segments = match_fingerprints(&fp.data, &fp.data, config)

For each segment where |offset2 - offset1| > min_lag_items:
  cluster by internal lag (same logic as select_best_segment)
Pick strongest non-zero-lag cluster above threshold
→ RepetitionFinding { lag_secs, confidence, items_count }
```

Constants (tune in implementation):

| Constant | Suggested value | Rationale |
|----------|-----------------|-----------|
| `min_lag_items` | ~40 items (~5 s at default preset) | Ignore trivial adjacency / Chromaprint item overlap |
| `min_confidence` | same floor as `min_match_score` scale | Consistent with cross-file matching |
| `max_lags_reported` | 1 | Report primary repeat only; avoid noise from musical structure |

```text
align_videos (when check_clip_repetition):
  extract clip
  fingerprint clip                    # existing
  if validation.check_clip_repetition:
      finding = detect_clip_repetition(&fingerprint)
      attach to clip diagnostics
  find_offset vs other video          # existing
```

### False positives

Choruses, applause, and test tones may trigger repetition. Treat output as **warning**, not hard error, unless a future `validation.fail_on_repetition` flag is added (out of scope for v1).

---

## Tests

| Test | Asserts |
|------|---------|
| `detect_clip_repetition_none_on_chirp` | Monotonic chirp → `None` |
| `detect_clip_repetition_finds_copied_block` | 10s block at 0s and 30s → lag ≈ 30s |
| `align_with_check_enabled_emits_diagnostic` | Integration: flag on → JSON/human includes repetition field |
| Corpus `repeated_segment_in_clip` | Flag on; repetition reported |

---

## References

- `src/infrastructure/chromaprint/aligner.rs` — `match_fingerprints`, `select_best_segment`
- `src/application/align_videos.rs` — extract + fingerprint loop
- `src/application/config.rs` — new `ValidationConfig`
- [BACKLOG.md](../BACKLOG.md) — add item when work starts
