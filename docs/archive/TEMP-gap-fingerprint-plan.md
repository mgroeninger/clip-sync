# Gap fingerprint — licensing-safe numeric characterization — plan (DRAFT)

Status: **DONE — archived.** P0 + builder + B-sliced orchestrator + mono/selected-channel lag +
oracle `failure_stage` + `--gap-fingerprints` bin path + per-gap corpus library (per-file decoded-PCM
identity + `scan_recipe` + `manifest.json`) all landed; 8 module + 25 patch-integration tests green
(`decode_ab` extraction is behavior-preserving; patch path untouched). Validated on real media — the
tool answered the gap-3 investigation (a recoverable ~8–16 ms timing offset with drift, an upstream
alignment issue, **not** decorrelation).

**Resolved since the P0/P1 review:** the `seams.baseline_*` divergence (now taken from the zero-move
bracket, consistent with the brackets ≈ production; Full-tier cost N+3 → N+2); CLI collapsed to one
`--gap-fingerprints DIR` flag. Residual §6 items are minor known-limitations carried to BACKLOG, not
blockers. The **alignment-drift fix** for the real file is a separate, open thread (not part of this
plan).

Companions: [seam-scoring.md](../seam-scoring.md), [gap-fill-modes.md](../gap-fill-modes.md),
[TEMP-anchor-seam-plan.md](../TEMP-anchor-seam-plan.md), [cli-output.md](../cli-output.md),
[json-output.md](../json-output.md), [gap-fingerprint.md](../gap-fingerprint.md).

---

## 1. Problem / motivation

Understanding a real production gap failure (e.g. the W5-style "structure aligns at 0.99 but the
waveform seam tops out at ~0.11" skip) currently means hand-building synthetic fixtures and guessing.
Real media cannot be checked in (licensing). But a **numeric characterization** of a gap — levels,
floor, anchors, structure/seam scores, and a lag fingerprint — contains **no audio or transcript**
and *can* be committed as a regression/calibration corpus.

Goal: a runnable mode that decodes a real soundtrack (A, optionally reference B), enumerates its
gaps, and emits a per-gap **fingerprint** as JSON.

Non-goals (v1): no patch/mux; no new DSP (reuse domain functions verbatim); no crate split of
gap-identify / gap-fill (decide separately, later).

---

## 2. Placement (decided)

- **Fingerprint logic = library module** `application/gap_fingerprint.rs` inside `clip-sync-repair`.
  Required there (not a separate crate) because the per-bracket scoring + `failure_stage` machinery
  it reuses (`derive_seam_gate_geometry`, `oracle_build_fit_cache`, `oracle_score_fit_candidate`,
  `oracle_evaluate_fit_joint`, `oracle_anchor_seam_would_run` in `patch_region`) is **`pub(crate)`** —
  reachable only from within the crate. This is `score_w5_fixture` generalized to real decoded PCM.
- **Entrypoint = flag on the existing `clip-sync-repair` bin**: `--dump-gap-fingerprints <path>`
  (+ `--fingerprint-gap <idx>`, repeatable). The bin already decodes, aligns, scans, and builds
  per-gap geometry; the flag diverts after gate-eval to serialize fingerprints. **No change to the
  patch path.**
- **Rejected:** a bin in `clip-sync-repair-harness` — that crate is deliberately a lean
  PCM-in/numbers-out lib with no media-decode or alignment deps; a characterize bin would force
  symphonia + the aligner onto it.

---

## 3. Detail tiers (cost management)

Re-running the gate per gap is the expensive part (~20 s/bracket observed). So:

- **Summary** (all gaps, cheap): geometry + intrinsic A-side (levels/floor/contour/anchors) +
  baseline structure & seam + outcome. No per-bracket enumeration, no lag.
- **Full** (only gaps named via `--fingerprint-gap`): adds feasible brackets, per-bracket
  structure/seam/`failure_stage`, and the `lag` fingerprint.

`--dump-gap-fingerprints out.json` → all gaps in summary; `--fingerprint-gap 3` → gap 3 in full.

---

## 4. Schema (the durable artifact — see `application/gap_fingerprint.rs`)

`GapCorpus { source: SourceMeta, gaps: [GapFingerprint] }`. Every field a number/enum; no samples.

Per `GapFingerprint`: `index`, `tier`, `sample_rate`, `channels`, and:
- `geometry` — A reported/refined edges, duration; B mapped edges + fill offset (when B present).
- `levels` — `bin_ms`, `profile_db[]` (RMS dBFS across pre..post context), speech-peak / noise-floor
  / gap-floor dB.
- `silence` — collar `rms/peak` ratio + whether it clears the **relative** silence test (the
  border walk-off discriminator).
- `contour` — `has_anchor_seam_contour`, pre/post envelope flatness.
- `anchors` — pre/post candidates `{ time, source, prominence, rms_db }`.
- `brackets` (full) — `{ pre_time, post_time, span_secs, move_frames, structure_*, seam_*,
  failure_stage }`.
- `structure` / `seams` (B present) — baseline scores + per-channel + selected channels.
- `lag` (full, B present) — per pre/post anchor: `{ window_ms, max_lag_ms, channel, lag0_r, peak_r,
  peak_lag_samples, frac_lag_samples, frac_lag_ms, verdict }`. **The probe**, promoted to a
  first-class characteristic.
- `outcome` (B present) — plan_kind, tier, seam_shape, fit_path, signature_mode, skip_reason.

### Lag verdict thresholds (v1, tunable)

`peak`/`lag0` are the parabolic-interpolated peak and the lag-0 correlation:
- **TimingOffset** — `peak ≥ 0.5` and the peak is *away* from lag 0 (`|peak_lag|>1` or
  `peak − lag0 > 0.2`): recoverable shift (read `frac_lag_ms`).
- **Decorrelated** — `peak < 0.3`: no shift recovers correlation; sources genuinely differ.
- **Ambiguous** — otherwise.

---

## 5. Phasing

- **P0** — `gap_fingerprint.rs`: serde schema + `lag_correlation_curve` / `summarize_lag_curve`
  (parabolic peak + verdict) + unit tests; `docs/gap-fingerprint.md`. **No behavior wired.**
- **P1 (library core — DONE)** — `build_gap_fingerprint` (intrinsic + pairwise via direct domain
  fns) + `characterize_gaps` (per-gap B-sliced orchestrator) + mono **and** gate-selected-channel
  lag. Unit-tested on synthetic PCM; no patch-path changes.
- **P1 (bin glue — DONE)** — `--dump-gap-fingerprints <path>` / `--fingerprint-gap <idx>` in
  `args.rs`; `composition::dump_gap_fingerprints` decodes A/B via the shared `decode_ab` (extracted
  from `PatchAudio::run`, patch path preserved), calls `characterize_gaps_with_gate` (oracle pass for
  Full gaps), and writes pretty JSON. **Still needs a real-file run to validate end-to-end** (no media
  available in-repo).
- **P2** — batch ergonomics + any schema gaps surfaced by real runs.

---

## 6. Outstanding issues

Tags: **[RESOLVED]** addressed in P1 · **[OPEN]** remains · **[OPEN·NEW]** surfaced by the
`apply_oracle`/`decode_ab` review (turn after the P0/P1 review) and not previously tracked.

### Fidelity gaps
- **`failure_stage` fidelity** — **[RESOLVED]**. `characterize_gaps_with_gate` builds Full gaps'
  brackets **directly from the gate enumeration**, scoring each with `oracle_score_fit_candidate`
  (authoritative seam + `failure_stage`); the baseline seam is authoritative, and `outcome` is derived
  from baseline/bracket pass (no joint eval). The heuristic `classify_bracket_stage` remains only for
  the A-only / no-gate path. *(The pre-restructure version matched gate brackets to builder brackets
  by frame — a silent-fallback fragility flagged in review; building brackets from the gate directly
  eliminated it, so that smell never shipped.)*
- **Per-bracket structure on the gate path** — **[OPEN · by design]**. Consequence of the
  `failure_stage` resolution: `structure_pre/post` is `None` per bracket on Full gaps (the gate
  doesn't surface structure). **Baseline** structure remains.
- **Placement weights / lag placement** — **[OPEN]**. `place_on_b` uses structure-only weights
  (1.0/0.0), not the run's fit weights, so the baseline structure **and the lag placement** sit at the
  structure-best, not the gate's weighted placement.
- **Gate geometry vs production** — **[OPEN · NEW]**. The gate pass builds geometry from the report's
  *reported* B offset (`video_b_start − video_a_start`) with `anchor_search_prior: None`, whereas
  production resolves the alignment offset and may pass a prior. *Related to "Placement weights": both
  are diagnostic geometry that approximates, but does not exactly reproduce, the production gate's
  placement.* Negligible where the report B mapping is tight (e.g. gap 3 ≈ −5.552 s).
- **Lag coverage** — **[OPEN]**. Lag is computed only at the best **energy-peak** bracket; a gap whose
  anchors are all bool-transition / scan-refined gets `lag: None` even with B.
- **Signature mode forced to `Energy`** — **[OPEN]** for contour/structure; a bool-mode gap is
  mischaracterized.
- **Lag channel** — **[RESOLVED]**. Was mono-only (`LagChannel::Selected` a dead variant); now mono +
  gate-selected channel.

### Smells
- **Full-B haystack** — **[RESOLVED]**. B is now sliced to a per-gap window (`b_extract_start_secs`).
- **Per-gap search cost (Full tier)** — **[RESOLVED · reduced]**. ~3N → **N + 3**. Residual minor:
  **[OPEN]** B is sliced twice (summary build + gate pass).
- **`noise_floor_db`** — **[OPEN]**. Median of *all* context bins (speech included) overstates the
  floor in speech-heavy context; a low percentile would be truer.
- **`rms_db` for non-energy anchors** — **[OPEN]**. Derived from a sentinel `rms` (bool/scan), so it's
  meaningless for those sources — should be floored.
- **`collar_rms_peak_ratio`** — **[OPEN]**. Proxy for the gate's border (no standoff/walk-off), so it
  won't exactly match the gate's walk-off decision.
- **Builder test runtime** (~32–40 s) — **[OPEN]**. Heavy for a unit test (the standalone
  `build_gap_fingerprint(Full)` test still does per-bracket `place_on_b`); consider a smaller fixture
  or a heavier test tier.
- **`RepairError::Config` for IO/serialize errors** — **[OPEN · NEW]**. `dump_gap_fingerprints` maps
  file-create / `serde_json` failures to the `Config` variant (semantic mismatch).
- **`max_refine_secs: 0.75` duplicated** — **[OPEN · NEW]**. `FingerprintConfig::from_request`
  hard-codes the value of `patch_audio::GAP_EDGE_REFINE_SECS` (private); drifts if that const changes.
- **Unnecessary `PathBuf` clone** — **[OPEN · NEW]**. `composition.rs` clones
  `args.dump_gap_fingerprints`; `.as_deref()` avoids it.
- **Generic `skip_reason`** — **[OPEN · NEW]**. Outcome on a skip is `"gate skipped"`; it could carry
  the baseline `failure_stage` for the actual reason.

## 7. Open items

- v1 reuses the **`oracle_*`** functions; if this becomes the supported backbone, de-`oracle_`-ify
  the names (cosmetic, defer to P1+).
- Output is **JSON** (serde_json). TOML not planned.

---

## 8. Recommendation: validate before polishing

The implementation is functionally complete and green, but **nothing has run on real media**. The
highest-value next step is a **real-file run on gap 3**, because:

1. It validates the entire bin path end-to-end (decode → slice → gate → JSON) — currently unproven.
2. It directly answers the original investigation: the authoritative `failure_stage` + the
   selected-channel `lag` verdict tell us whether gap 3 is a recoverable timing offset or genuine
   decorrelation.
3. None of the **[OPEN]** issues corrupt output or block a run — they are accuracy/hygiene refinements.
   Several (lag coverage, signature-mode, `noise_floor_db`) may prove irrelevant or differently shaped
   once we see real output, so fixing them first risks polishing the wrong things.

Run:

```
clip-sync-repair A.mkv B.m4v --dump-gap-fingerprints gaps.json --fingerprint-gap 3
```

Then triage from the JSON. **Do not** front-load the smell fixes. The only ones worth batching
opportunistically (zero-risk, ~minutes) are the **[OPEN · NEW]** code-hygiene items
(`RepairError::Config`, `PathBuf` clone, `0.75` dup, generic `skip_reason`); they can land anytime and
need no real media. Everything else waits on what the real run shows.
