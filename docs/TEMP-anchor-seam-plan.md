# Anchor-based seam placement — plan (DRAFT)

Status: **in progress** — P0–P4 + Batch A–D (Pearson gate, xcorr, observability tags). Remaining:
user docs polish, optional corpus/diag rows.

Companions: [seam-scoring.md](seam-scoring.md), [gap-repair-guide.md](gap-repair-guide.md) § W5 /
Vocabulary, [gap-fill-modes.md](gap-fill-modes.md) § extension / `baseline_only`, [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md).

---

## 1. Problem (one paragraph)

Today a **good seam** means `min(pre, post)` Pearson on **fixed** (or incrementally extended) scan
gap edges, using ~250 ms windows after standoff/trim at the dropout **throat**. Placement uses a
**wider** representation (3 s energy envelope or bool pause pattern) to slide B, but approval uses a
**narrow, often silent** slice that may not contain the salient audio an editor would use as the cut
(speech peaks, onset before/after a dropout). When throat and contour disagree, we get **structure
placement + waveform skip** — the wrong decomposition for “find matchable cut points, then fill
between them.”

Incremental `gap_*_extend_*` nudges the hole in ≤500 ms steps after Pearson fails; it does not
**search for anchor pairs** where A and B carry identifiable, matchable signal.

---

## 2. Definition: anchor seam (target)

Per gap, choose four linked points (two editorial boundaries):

| Anchor | Meaning |
|--------|---------|
| **A_pre** | Last kept sample on A before the B fill |
| **A_post** | First kept sample on A after the B fill |
| **B_pre** | B audio that must correspond across **A_pre** |
| **B_post** | B audio that must correspond across **A_post** |

**Same editorial boundary:** `(A_pre ↔ B_pre)` and `(A_post ↔ B_post)` are the same story moment
(modulo clip offset). **Matchable:** short windows at each anchor have **signal** (not codec noise
alone) and **agree** between A and B under at least one metric (envelope, waveform, residual cancel).

The **fill region** on A is `[A_pre, A_post]`; on B it is the mapped interior between the matched
anchors. Seam validation runs **at the chosen anchors**, not by assumption at the scan silence floor.

---

## 3. Non-goals

- **Per-gap chromaprint / landmark FFT** — clip-level offset already exists; too coarse/slow for
  sub-second cuts.
- **Replacing scan** — scan still finds “there is a hole”; anchors **refine where to cut** inside/near
  that hole.
- **Removing waveform Pearson** — keep as a validator when anchors carry waveform; compose with
  envelope/residual when throat Pearson is uninformative.
- **Unbounded gap growth** — anchor brackets must stay near the scan hole (prior + max span).
- **Gate-mode-only trust** — fit mode should get an explicit anchor-trust path, not only legacy
  `structure_trusted` in `fill_mode = gate`.

---

## 4. Current behavior (summary)

```text
scan hole (min_gap_ms floor)
  → refine_gap_frames (silence walk on A)
  → fixed A bracket (± extend grid only under --full / full_grid)
  → structure slide on B (energy/bool, 3 s context)
  → seam Pearson 250 ms at scan throat → tier / skip
```

| Mechanism | Sees salient peaks ~1 s away? | Chooses cut points? |
|-----------|------------------------------|---------------------|
| Energy / bool structure | Often (bins in 3 s context) | No — slides B only |
| Seam Pearson | No (250 ms at throat) | No — grades fixed edges |
| `gap_*_extend_*` | No — local nudge | Slightly — only if grid/retry runs |
| Residual rescue | Raw window at throat | No |

**Default profile (`baseline_only`):** extension grid inactive; failed baseline → skip with no
anchor search.

---

## 5. Proposed design

### 5a. Principle

> **Propose a small set of anchor candidates on A from existing representations; score joint
> (A_pre, A_post, B placement) for matchability; pick the bracket where both boundaries are
> verifiable; fill and splice between them.**

Reuse decode + haystack infrastructure. New logic is **candidate generation**, **matchability
scoring**, and **fit-mode approval** at chosen anchors.

### 5b. Anchor candidates on A (Tier 1 — reuse)

Within `[gap − context, gap + context]` on A (default 3 s, center-weighted channels for 5.1):

| Source | Candidate | Existing code |
|--------|-----------|---------------|
| Bool | Silence ↔ active transitions | `activity_bins`, `build_gap_context_signature` |
| Energy | Local maxima in `pre_energy` / `post_energy` | `energy_bins`, `build_gap_energy_signature` |
| Scan | Refined gap start/end | `refine_gap_frames` (always a fallback candidate) |

**Filter (matchable on A):** bin energy or RMS above `absolute_silence_rms` / scan silence floor;
optional minimum prominence vs neighbors (envelope peak − local median).

Keep **K ≤ 5** candidates per side; always include scan-refined edges as fallback.

### 5c. Match B and score pairs (Tier 1 + optional Tier 2)

For each feasible `(A_pre_anchor, A_post_anchor)` with `A_pre < scan hole < A_post` (or containing
scan interior) and span ≤ `max_anchor_bracket_secs` (config, e.g. 5 s):

1. Build `GapSignature` / energy halves for **that** bracket (not only scan edges).
2. Run existing unified B slide (`UnifiedFillSearchInput`) — same haystack, new signature geometry.
3. **Matchability** at anchors (both required unless policy allows one-strong for short gaps):

| Metric | Use | Existing |
|--------|-----|----------|
| Envelope similarity | Primary placement score | `score_pre/post_energy_match` |
| Waveform Pearson | Validator at anchor windows | `seam_pearson` — window width **adaptive** (e.g. 250 ms–1 s, capped by local energy) |
| Residual headroom | Same-master confirm / rescue | `SeamResidualVerdict`, `apply_residual_to_confidence` |

**Optional Tier 2 (later):** short local PCM xcorr (`PcmCorrelator` port) on center channel at anchor
windows — one lag peak per anchor, few candidates only.

**Ranking:** existing unified score + penalties for distance from scan hole (prior nominal bracket) +
distance from scan center (don’t swallow unrelated speech).

### 5d. Approval (replace throat-only tier)

When **both** anchors pass matchability on B:

- **High / marginal** from anchor Pearson or envelope+residual compose (extend
  `classify_fill_waveform_confidence` / sibling).
- When anchor windows are **low-RMS but residual cancels** → marginal via rescue (same invariant as
  [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) §2).
- When structure confident at anchors but Pearson dead at throat → **`anchor_trusted`** patch tier
  (fit mode; vocabulary tag), with residual veto unchanged.

Emit `A gap (refined)` from **winning anchor bracket**, not only silence walk on scan edges.

### 5e. Relation to extension grid

Anchor search **subsumes** “move the cut to matchable audio” for many W5 cases. Keep `gap_*_extend_*`
grid as a fallback when anchor candidate set is empty or all fail; do not rely on 40 ms steps to reach
peaks ~1 s away.

---

## 6. Implementation phases

**Naming:** use `gap_anchor_seam.rs` / `SeamAnchor` / `AnchorBracket` — **not** `patch_anchor.rs`,
which is offset anchors for `anchored_retry`.

| Phase | Scope | Primary touch |
|-------|--------|---------------|
| **P0** | Domain: candidates, brackets, A-side matchability | `domain/gap_anchor_seam.rs` (new), `gap_structure.rs`, `gap_energy.rs` |
| **P1** | Fit-path integration + config | `patch_region.rs`, `patch_audio.rs`, `gap_fill_fit.rs`, `config.rs` |
| **P2** | Adaptive seam window + `anchor_trusted` vocabulary | `policies.rs`, `gap_tags.rs`, `cli-output.md` |
| **P3** | Oracle + corpus rows | `tests/`, `test_support/energy_signature_fixtures.rs` |
| **P4** (optional) | Local PCM xcorr at anchors | `PcmCorrelator` adapter |

**Default behavior:** ship behind `repair.anchor_seam_mode = off | auto | force`;
`auto` enables when baseline throat `min(pre,post) < marginal_floor` and signature contour present:
energy mode uses `energy_envelope_is_flat`; bool mode uses activity transitions or mixed
active/silent bins (`GapSignature::has_anchor_seam_contour`).

**Target call chain** (P1, in `evaluate_seam_gate_fit_joint`):

```text
record_fit_joint_candidate(baseline)           // existing
  → if baseline High → return                  // existing
  → if baseline Marginal + baseline_only → return
  → if should_run_anchor_seam:
       list_anchor_candidates_a → list_feasible_anchor_brackets
       for bracket: record_fit_joint_candidate(bracket.refined, …)
  → if anchor winner → return
  → if fit_boundary_search == BaselineOnly → skip
  → joint boundary grid loop                    // existing fallback
```

### P0 — Domain: candidates + A-side matchability

**New module:** `crates/clip-sync-repair/src/domain/gap_anchor_seam.rs`

| # | Task | File(s) | Functions / types |
|---|------|---------|-------------------|
| P0.1 | Add types | `gap_anchor_seam.rs` | `AnchorSeamSide`, `AnchorSource`, `AnchorCandidate`, `AnchorCandidateSet`, `AnchorSeamParams`, `AnchorBracket` |
| P0.2 | Promote shared bin helper | `gap_structure.rs` | `activity_bins` → `pub(crate) fn activity_bins(...)` |
| P0.3 | Raw-bin peak picker | `gap_energy.rs` | `pub(crate) fn local_energy_peaks(...)` on **pre-`peak_normalize`** bins from `energy_bins` |
| P0.4 | Bool transition picker | `gap_anchor_seam.rs` | `bool_transition_candidates(activity, …)` |
| P0.5 | Main list API | `gap_anchor_seam.rs` | `pub fn list_anchor_candidates_a(...)` |
| P0.6 | Bracket enumeration | `gap_anchor_seam.rs` | `pub fn list_feasible_anchor_brackets(...)` — contain-hole + `max_bracket_frames` |
| P0.7 | A-side matchability | `gap_anchor_seam.rs` | `pub fn anchor_matchable_on_a(...)` — `is_silent_frame`, optional prominence |
| P0.8 | Wire module | `domain/mod.rs` | `pub mod gap_anchor_seam;` + re-exports |
| P0.9 | Unit tests | `gap_anchor_seam.rs` `#[cfg(test)]` | Peaks ±1 s from throat, bool onset, scan fallback, contain-hole rejection, K-cap dedup |

**Reuse (no new logic):**

| Helper | Location | Role in P0 |
|--------|----------|------------|
| `energy_bins` | `gap_energy.rs` | Raw envelope for peak picking |
| `build_gap_energy_signature` geometry | `gap_energy.rs` | `pre_start` / `pre_end` / `post_start` / `post_end` frame ranges |
| `activity_bins` | `gap_structure.rs` | Bool transitions (after promote) |
| `build_gap_context_signature` geometry | `gap_structure.rs` | Same context window as energy |
| `StructureMatchParams` | `gap_structure.rs` | `bin_frames`, silence thresholds |
| `RefinedGapFrames` | `policies.rs` | Scan hole bracket |

**Explicitly not in P0:** B-side scoring, `evaluate_seam_gate_fit_joint` integration, config surface.

### P1 — Fit-path integration

| # | Task | File(s) | Functions |
|---|------|---------|-----------|
| P1.1 | Config enum | `gap_anchor_seam.rs` or `repair_profile.rs` | `AnchorSeamMode { Off, Auto, Force }` + `FromStr` |
| P1.2 | Config fields | `infrastructure/config.rs` | `anchor_seam_mode`, `max_anchor_bracket_secs` (5.0), `max_anchors_per_side` (5), `anchor_seam_min_prominence` |
| P1.3 | CLI / TOML | `cli/args.rs`, `cli/mod.rs` | `--anchor-seam-mode`, serde defaults |
| P1.4 | Gate params | `patch_region.rs` | Extend `SeamGateParams` with anchor fields |
| P1.5 | Build params | `patch_audio.rs` (~`SeamGateParams { … }`) | Map `RepairConfig` → `SeamGateParams` |
| P1.6 | Auto trigger | `gap_signature.rs`, `patch_region.rs` | `GapSignature::has_anchor_seam_contour()` + `should_run_anchor_seam(...)` |
| P1.7 | Anchor search loop | `patch_region.rs` | `evaluate_anchor_seam_brackets(...)` → `record_fit_joint_candidate` per `AnchorBracket` |
| P1.8 | Orchestration | `evaluate_seam_gate_fit_joint` (~530) | Insert after baseline fail, before `BaselineOnly` early return (~635) |
| P1.9 | Ranking penalty | `gap_fill_fit.rs` | `anchor_bracket_ranking_penalty` / `fit_anchor_candidate_ranking_score` |
| P1.10 | B-side matchability | `gap_anchor_seam.rs` or `patch_region.rs` | `matchability_at_anchor(...)` — envelope + `fill_seam_correlations` + deferred residual |
| P1.11 | Outcome fields | `SeamGateOutcome` | `anchor_seam_used`, `anchor_bracket_move_frames` |
| P1.12 | Integration test | `tests/anchor_seam_oracle.rs` | A5/A5b: `baseline_only` + `anchor_seam_mode=auto` (energy + bool) patches speech-at-peaks |

### P2 — Adaptive window + `anchor_trusted` vocabulary

| # | Task | File(s) | Functions |
|---|------|---------|-----------|
| P2.1 | Adaptive seam window | `policies.rs` | `adaptive_seam_window_frames(local_energy_frames, min_w, max_w, cap)` |
| P2.2 | Use at anchor | `evaluate_seam_gate_fit_candidate` (~939) | Replace fixed `waveform_gate_frames` when `anchor_seam_used` |
| P2.3 | Anchor-trust classifier | `gap_fill_fit.rs` | `classify_anchor_trusted_confidence(...)` — sibling to `classify_fill_waveform_confidence` |
| P2.4 | Apply in candidate eval | `evaluate_seam_gate_fit_candidate` (~1063) | Throat Pearson dead + anchor windows pass → `anchor_trusted`; residual veto unchanged |
| P2.5 | Outcome flag | `SeamGateOutcome` | `anchor_trusted: bool` (distinct from gate-mode `structure_trusted`) |
| P2.6 | Tags | `gap_tags.rs` | `PatchTier::AnchorTrusted`, derive in `derive_gap_tags_from_patch_outcome` |
| P2.7 | CLI output | `cli/output.rs` | Human line for `anchor_trusted` |
| P2.8 | Docs | `cli-output.md`, `gap-repair-guide.md` | `patch_tier=anchor_trusted` vocabulary |
| P2.9 | Profile notes | `repair_profile.rs` | Warn when `anchor_seam_mode=off` under failing baseline |

### P3 — Oracle + corpus

| # | Task | File(s) | Notes |
|---|------|---------|-------|
| P3.1 | Synthetic fixture | `energy_signature_fixtures.rs` | `speech_peaks_offset_from_throat(secs)` |
| P3.2 | Domain oracle | `tests/anchor_seam_oracle.rs` (new) | A1: candidates pick peak frames, not throat |
| P3.3 | Pipeline oracle | `tests/anchor_seam_oracle.rs` | A1 end-to-end: `PatchAudio` + `anchor_seam_mode=force` |
| P3.4 | Regression rows | `tests/anchor_seam_oracle.rs` | A2–A4 (A3 domain + pipeline in oracle) |
| P3.5 | Production corpus row | `gap_corpus_fixtures.rs` + manifest | One real W5 row |
| P3.6 | Diagnostic (optional) | `tests/diag_anchor_seam.rs` | Per-gap candidate dump (`diagnostic-tests`) |

Pattern to copy: `tests/seam_residual_oracle.rs`.

### P4 — Optional PCM xcorr (Tier 2 rescue)

| # | Task | File(s) | Status |
|---|------|---------|--------|
| P4.1 | Port adapter | `infrastructure/pcm_correlator.rs` — wrap `clip_sync::PcmCorrelator` | optional / skipped (`clip_sync::FftCorrelator` used directly) |
| P4.2 | Anchor lag probe | `local_anchor_xcorr_peak` in `gap_anchor_seam.rs` | **done** |
| P4.3 | Tier-2 gate | `matchability_at_anchor` when Pearson ambiguous; production via `anchor_bracket_both_matchable_at_gate` + `residual_max_lag_frames` | **done** (Batch C) |

Unit tests: `xcorr_rescues_ambiguous_pearson_pre_anchor`, `local_anchor_xcorr_peak_finds_lag_alignment`,
`xcorr_not_run_when_pearson_deep_fail` in `gap_anchor_seam.rs`.

### Batch D — Observability (done)

| Surface | Fields | Location |
|---------|--------|----------|
| `GapPatchStatus::Patched` | `anchor_seam_used`, `anchor_bracket_move_frames` | `patch_result.rs` (JSON status) |
| `GapTags` | same (tags mirror status for vocabulary) | `gap_tags.rs` |
| Verbose stderr | `anchor_seam=true`, `anchor_move_frames=N` | `format_gap_tags_verbose_line` |
| Human gap table | `patched (anchor …)` when `anchor_seam_used` | `cli/output.rs` `format_patched_gap_detail` |
| Tag derivation | `GapTagsPatchContext` + `derive_gap_tags_from_status` | `patch_audio.rs`, `gap_tags.rs` |

---

## 6b. P0 API sketch

Types live in `domain/gap_anchor_seam.rs`. Geometry matches
[`build_gap_energy_signature`](../../crates/clip-sync-repair/src/domain/gap_energy.rs) /
[`build_gap_context_signature`](../../crates/clip-sync-repair/src/domain/gap_structure.rs).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorSeamSide {
    /// Boundary before the fill (`A_pre`); candidates at or before `scan_hole.start_frame`.
    Pre,
    /// Boundary after the fill (`A_post`); candidates at or after `scan_hole.end_frame`.
    Post,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorSource {
    ScanRefined,
    EnergyPeak,
    BoolTransition,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnchorCandidate {
    /// Sample index on A: last kept frame before fill (Pre) or first kept after fill (Post).
    pub frame: usize,
    pub side: AnchorSeamSide,
    pub source: AnchorSource,
    /// Peak − local median (energy) or 1.0 for scan fallback.
    pub prominence: f32,
    /// Mean-square RMS in the anchor's bin (A-side only in P0).
    pub rms: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnchorCandidateSet {
    pub pre: Vec<AnchorCandidate>,
    pub post: Vec<AnchorCandidate>,
}

/// One feasible editorial bracket; anchors must contain the scan hole (§8.1 default).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnchorBracket {
    pub refined: RefinedGapFrames,
    pub pre: AnchorCandidate,
    pub post: AnchorCandidate,
    /// `start_delta + end_delta` from scan-refined baseline (for ranking).
    pub move_frames: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct AnchorSeamParams {
    pub context_frames: usize,
    pub max_anchors_per_side: usize,
    pub max_bracket_frames: usize,
    pub min_prominence: f32,
    pub structure: StructureMatchParams,
}
```

### `list_anchor_candidates_a`

```rust
/// Collect ≤K anchor frames per side around a scan hole on A.
///
/// Pre context: `[gap_start - context, gap_start)`.
/// Post context: `[gap_end, gap_end + context)`.
///
/// Always includes scan-refined edges. Merges energy peaks and bool transitions,
/// dedupes by bin, caps at `max_anchors_per_side`, sorts by prominence desc.
pub fn list_anchor_candidates_a(
    samples: &[f32],
    channels: usize,
    scan_hole: RefinedGapFrames,
    params: &AnchorSeamParams,
) -> AnchorCandidateSet;
```

Internal flow:

1. Compute `pre_start` / `pre_end` / `post_start` / `post_end` (same as `build_gap_energy_signature`).
2. Seed `ScanRefined` at `scan_hole.start_frame` / `scan_hole.end_frame`.
3. `energy_bins` on each half → `energy_peak_candidates` (local maxima, prominence filter).
4. `activity_bins` on each half → `bool_transition_candidates` (rising edge Pre, falling Post).
5. `finalize_side_candidates` — dedupe within bin, always keep scan fallback, truncate K.

### Bin → frame mapping

```rust
/// Pre:  gap-facing edge of bin `i` → `origin + (i + 1) * bin_frames`, clamped ≤ scan start.
/// Post: gap-facing edge of bin `i` → `origin + i * bin_frames`, clamped ≥ scan end.
fn bin_to_anchor_frame(
    bin_idx: usize,
    bin_frames: usize,
    origin_frame: usize,
    side: AnchorSeamSide,
    scan_edge: usize,
) -> usize;
```

### `list_feasible_anchor_brackets`

```rust
pub fn list_feasible_anchor_brackets(
    candidates: &AnchorCandidateSet,
    scan_hole: RefinedGapFrames,
    params: &AnchorSeamParams,
) -> Vec<AnchorBracket>;
```

Constraints per pair `(pre, post)`:

- **Contain hole:** `pre.frame ≤ scan_hole.start_frame` and `post.frame ≥ scan_hole.end_frame`.
- **Span:** `post.frame - pre.frame ≤ max_bracket_frames`.
- Sort by `move_frames` asc (stay near scan hole), then prominence desc.

### `anchor_matchable_on_a` (P0)

```rust
/// P0: A-side only — bin has signal above silence floor.
/// P1 adds B-side via `matchability_at_anchor`.
pub fn anchor_matchable_on_a(
    samples: &[f32],
    channels: usize,
    frame: usize,
    side: AnchorSeamSide,
    params: &AnchorSeamParams,
) -> bool;
```

Uses `is_silent_frame` over one bin width adjacent to the anchor frame.

### `matchability_at_anchor` (P1 sketch)

```rust
pub struct AnchorMatchability {
    pub envelope: f64,
    pub pearson: f64,
    pub matchable: bool,
}

/// Score one anchor after unified B placement is known.
pub fn matchability_at_anchor(
    templates: &SeamTemplates<'_>,
    placement: SeamPlacement,
    side: AnchorSeamSide,
    pre_window: usize,
    post_window: usize,
) -> AnchorMatchability;
```

Called from `evaluate_seam_gate_fit_candidate` with the winning `FillAlignment`.

### P0 tests to write first

| Case | Expect |
|------|--------|
| Flat room tone (C1) | Only `ScanRefined` candidates |
| Speech burst ~1 s before throat | Energy peak frame ≠ scan start |
| Bool onset (C3) | `BoolTransition` near onset |
| Bracket span > max | Pair rejected |
| K > 5 peaks | Truncated, scan fallback retained |

### Implementation notes

1. **Promote `activity_bins`** — one-line visibility change; avoids duplicating silence-majority bin rule.
2. **5.1 channels (§8.2)** — P0 uses mono downmix via `energy_bins` (matches structure tier). Optional
   later: `channel_mask` from a coarse `seam_score_channel_indices` pre-pass.
3. **`gap_border_spec`** already keys off `refined.start_frame` / `refined.end_frame` — standoff applies
   relative to the anchor edge once bracket is chosen (§8.3).

---

## 7. Validation

| Case | Expect |
|------|--------|
| **A1** Speech dropout, peaks ±1 s, same master encodes | Anchor at peaks; patch marginal+; listenable splice |
| **A2** C3 speech boundary (asymmetric post) | Anchors near onset; aligns with bool path; no regression vs W3 |
| **A3** Flat room tone (C1) | Fallback to scan edges; behavior ≈ today |
| **A4** F4 decoy / wrong B slide | Residual veto; no anchor_trusted false patch |
| **A5** `baseline_only` profile (energy) | Anchor search runs without requiring `--full` grid |
| **A5b** `baseline_only` + bool signature | Same as A5 under `gap_signature_mode=bool` |

Track: `patch_tier`, `seam_shape`, `anchor_trusted` (new), wall time per gap (candidate count bounded).

---

## 8. Open questions

1. **Bracket vs hole:** Must anchors **contain** the entire scan hole, or may they **shrink** it when
   interior is silence? (Default: contain — don’t leave unfilled scan silence inside bracket.)
2. **5.1 peak picking:** Per-channel envelope max on center-only vs energy-selected channels?
3. **Interaction with `border_standoff_secs`:** Apply standoff relative to **anchor** edge, not scan
   edge.
4. **CLI:** Expose `anchor_seam_mode` or fold into `--full` / new profile `anchor`?

---

## 9. Related reading

| Doc | Contents |
|-----|----------|
| [seam-scoring.md](seam-scoring.md) | Current seam definition, 250 ms throat |
| [gap-repair-guide.md](gap-repair-guide.md) | W5, tiers, vocabulary |
| [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) | Pearson vs residual on quiet seams |
| [archive/energy-signature-plan.md](archive/energy-signature-plan.md) | Structure tier shipped |
