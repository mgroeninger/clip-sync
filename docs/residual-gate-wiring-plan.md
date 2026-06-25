# Residual / floor gate — end-to-end wiring design

Status: **P1 + P2 + P4 shipped** (default `residual_gate = veto`). Implemented: unified lag
radius, `SeamResidualVerdict.informative`, fit-mode measurement, `apply_residual_to_confidence`,
config/CLI, `ResidualHeadroomExceeded` skip reason, residual scalars on `Patched`, `residual_band`
per-gap tag, `donor_relation` run diagnostic, real-codec gate oracle (AAC + Vorbis 128k). Not yet:
`veto_rescue` as default; MP3 calibration (M4 deferred).

Builds on the report-only prototype (`policies::seam_residual_diagnostics`,
`policies::seam_floor_probe`) and the corpus experiments in `tests/seam_residual_corpus.rs`.

Companions: [seam-scoring.md](seam-scoring.md) (seam mechanics), [gap-fill-modes.md](gap-fill-modes.md)
(fit/gate, tiers), [nway-donor-alignment-plan.md](nway-donor-alignment-plan.md) (the floor's other use).

---

## 1. What this wires, in one sentence

Promote the residual-cancellation **headroom** (seam residual minus a per-gap *measured* noise
floor) from a debug log into an **anti-echo veto** and optional **false-skip rescue** on the fit
waveform tier — active only where the floor proves the pair is same-master and well-aligned, and
otherwise abstaining so nothing changes.

## 2. Empirical constraints this design must honor

From steps 1–2 and the alignment sweep (`tests/seam_residual_corpus.rs`):

1. **Gate on headroom, not absolute residual.** Truth residual swings with conditions
   (−120 clean → −44.8 codec noise → −18.3 with a 3.4-sample delay) but headroom stays ≈ 0 at
   truth and 18–120 dB at decoys. The floor normalizes content/codec/alignment.
2. **Headroom is robust to sub-sample misalignment** (the common same-master case): floor absorbs
   the identical integer-lag penalty, so headroom ≈ 0 even when absolute residual degrades.
3. **The seam and floor lag radii must be equal.** With seam ±64 and floor ±512, offsets in
   (64, 512] made a *true* fill's headroom blow up to ~40 dB (false reject). **Resolved:** a single
   configurable `max_lag_frames` (from `residual_lag_secs`, 10 ms default) serves both probes, so the
   mismatch band is gone. Within reach, lag-centered chosen measurement keeps headroom meaningful
   (**M5**). Beyond the lag radius, headroom is undefined — the gate **abstains** (not veto).
4. **Headroom ≈ 0 is necessary but not sufficient.** Beyond the lag radius, *if the floor itself also
   fails to cancel* (gross donor misalignment), headroom collapses to ~0 spuriously (false accept).
   **Also require the floor itself to be low (`floor_db ≤ FLOOR_OK`)** — shipped as the verdict's
   `informative` flag.
5. **The floor-informative check is the regime gate.** Two-mic / different-capture pairs cannot
   cancel → `floor_db` high → uninformative → the gate abstains automatically. `donor_relation` is
   therefore *derived from* the floor, not an independent input.
6. **F4 is the value case:** decoy `seam_pre` 0.84 (Pearson nearly accepts) vs residual headroom
   108 dB (residual rejects). The veto fires exactly here.

**Why the two signals disagree (the root invariant).** Pearson endpoint identification and the
residual gate measure *different things on different template bases*, and that is precisely why they
diverge on broadband (noise-like) seams:

- **Pearson** (`fill_seam_correlations` → `seam_pearson`) scores **waveform-shape similarity** on the
  **trimmed** border template, peak-normalized. On broadband content there is no distinctive shape to
  match — both sides are noise that differs sample-to-sample even between two encodes of the same
  master — so Pearson reads ~0 *even at the correct placement* (H2-B: 0.099/0.100, below
  `fill_absolute_floor`).
- **Residual** (`seam_chosen_and_floor`) scores **whether A can actually be subtracted from B** on the
  **raw** window. A same-master broadband seam cancels → residual ≈ floor → headroom ≈ 0.

So on a *correct* same-master broadband fill, Pearson says reject (dead zone) while residual says
accept (headroom ≈ 0). Two consequences flow from this single fact: the **rescue** path exists to
recover those Pearson false-skips (constraint 4b dead-zone row), and the **veto** path exists for the
mirror case where Pearson accepts a decoy that residual rejects (constraint 6 / F4). Unifying the
residual side onto the *raw* window was H1; the trimmed-vs-raw asymmetry between the two systems is
permanent by design, not a bug. See [residual-gate-findings.md](residual-gate-findings.md) H1/H2-B.

## 3. Current integration points (exact)

| # | Location | Role today | Change |
|---|----------|------------|--------|
| A | `application/patch_region.rs::evaluate_seam_gate_fit_candidate` (≈365–562) | computes structure gate + `classify_fill_waveform_confidence`; already builds `templates`, `cache.b_mono`, `offset_nominal_start`, the seam windows, and (in the DEBUG block) `seam_residual_diagnostics` + `seam_floor_probe` | **primary site**: compute residual/floor unconditionally (not just under DEBUG), apply the gate, populate new outcome fields |
| B | `domain/gap_fill_fit.rs::classify_fill_waveform_confidence` (47–70) | maps `min(pre,post)` Pearson → High/Marginal/Err | add a sibling that composes the Pearson tier with the residual verdict (pure fn, unit-testable) |
| C | `domain/policies.rs` (`seam_residual_diagnostics`, `seam_floor_probe`, `SEAM_RESIDUAL_MAX_LAG`, `SEAM_FLOOR_MAX_LAG`) | report-only primitives | unify lag radius (constraint 3); add one combined evaluator returning per-side `residual_db`, `floor_db`, `headroom`, `floor_source`, `informative` |
| D | `application/patch_region.rs::SeamGateParams` (49–89) + `SeamGateOutcome` (30–47) + `SeamGateFailure` (91–103) | gate inputs/outputs | add residual config to params; add residual fields to outcome; add a `ResidualHeadroomExceeded` failure |
| E | `application/patch_audio.rs` (≈1431 `SeamGateParams { … }`) | builds params from `RepairConfig` | thread new config fields + computed lag-radius frames |
| F | `infrastructure/config.rs` (`RepairConfig`, `default_*`, validation, CLI mirror) | config surface | add knobs (§6) |
| G | `domain/patch_result.rs::GapPatchStatus::Patched` / `GapPatchSkipReason` | report/JSON outcome | add residual fields to `Patched`; reuse or extend skip reason |
| H | `domain/gap_tags.rs` (`GapTags`, derivation) | vocabulary | add `residual_band` + `donor_relation` axes (§7) |

No change to: scan, fill-plan, structure match, splice/crossfade/mux, or `fill_mode = gate`.

## 4. Design

### 4a. Domain primitive (point C)

Unify the lag radii and add one evaluator that returns everything the gate needs:

As shipped (`policies.rs`):

```rust
pub struct SeamResidualVerdict {
    pub chosen_pre_db: f64,            // chosen-placement residual (per side)
    pub chosen_post_db: f64,
    pub floor_pre_db: f64,             // nominal floor (per side)
    pub floor_post_db: f64,
    pub floor_source_pre: SeamFloorSource,  // Border | Walked | None
    pub floor_source_post: SeamFloorSource,
    pub informative: bool,             // every measured side: floor_db ≤ FLOOR_OK
    pub placement_slide_frames: u64,   // |chosen_delta − nominal_delta|; drives reach abstention
    pub max_lag_frames: i64,           // unified lag radius for this verdict (0 = reach check off)
}
impl SeamResidualVerdict {
    pub fn worst_headroom_db(&self) -> f64; // max over sides of (chosen − floor), non-finite filtered
    pub fn worst_chosen_db(&self) -> f64;   // worst-side chosen residual (higher = less cancellation)
    pub fn beyond_lag_reach(&self) -> bool; // slide > max_lag_frames → gate abstains (M5)
}
```

`placement_slide_frames` + `max_lag_frames` are what power the reach-abstention row of the §4b table:
`beyond_lag_reach()` is `max_lag_frames > 0 && placement_slide_frames > max_lag_frames`. They are
populated by `from_parts_with_placement`; the harness/`from_parts` constructors leave them `0`
(reach check disabled), which is why both serialize with `skip_serializing_if`.

- **Lag radius unification (constraint 3) — shipped:** one `max_lag_frames` on `SeamFloorParams`,
  computed by `residual_gate::residual_max_lag_frames(rate, residual_lag_secs)` (10 ms default), used
  by both the chosen-placement and floor probes. The old `SEAM_RESIDUAL_MAX_LAG` /
  `SEAM_FLOOR_MAX_LAG` constants are gone.
- **Floor anchor in production = the alignment nominal**, not "true" (the harness used true because
  its fixtures have pathological nominals). Anchor at `offset_nominal_start` with the unified lag,
  plus the existing outward-walk + `None` fallback.

### 4b. Gate composition (point B)

The residual gate **augments** the existing Pearson tier; it never runs alone. Pure function:

As shipped (`gap_fill_fit.rs`): the veto/rescue/no-opinion outcomes are encoded in the return rather
than a separate enum.

```rust
pub enum ResidualGateError {
    PearsonBelowFloor(f64),                       // skip (carries the Pearson min score)
    HeadroomExceeded { headroom_db: f64, margin_db: f64 }, // veto
}

pub fn apply_residual_to_confidence(
    pearson: Result<FillConfidence, f64>,   // from classify_fill_waveform_confidence
    verdict: &SeamResidualVerdict,
    margin_db: f64,                          // HEADROOM_MARGIN
    rescue_enabled: bool,
) -> Result<FillConfidence, ResidualGateError>
// !informative → returns `pearson` unchanged (no-opinion); rescue → Ok(Marginal).
```

Rules (constraints 1, 4, 5, 6):

| Floor | Headroom | Slide vs reach | Pearson tier | Result |
|-------|----------|----------------|--------------|--------|
| uninformative (`!informative`) | — | — | any | **unchanged** (abstain → today's behavior; protects two-mic) |
| informative | — | `slide > max_lag` | any | **unchanged** (abstain — headroom undefined beyond unified lag; **M5**) |
| informative | ≤ `margin` | ≤ `max_lag` | High/Marginal | **unchanged** (agree → patch) |
| informative | > `margin` | ≤ `max_lag` | High/Marginal | **veto → skip** (`ResidualHeadroomExceeded`) — the F4/echo catch |
| informative | ≤ `margin` | ≤ `max_lag` | dead_zone (Err) | **rescue → Marginal** *iff* `rescue_enabled` — the W5/false-skip catch |
| informative | > `margin` | ≤ `max_lag` | dead_zone (Err) | unchanged (skip) |

Within reach, the chosen probe's lag search is centered on `floor.best_lag + nominal_delta − chosen_delta` so headroom reflects content mismatch, not independent lag picks (**M5**).

The veto is the high-confidence win and should ship first; rescue is opt-in (it widens what
patches, so it needs the disagreement table before becoming default).

### 4c. Site wiring (point A)

In `evaluate_seam_gate_fit_candidate`, after `classify_fill_waveform_confidence`:

1. Compute `SeamResidualVerdict` (anchor = `offset_nominal_start`, unified lag).
2. `confidence = apply_residual_to_confidence(pearson_result, &verdict, margin, rescue)?`
   (the `?` turns a veto into `Err → WaveformBelowThreshold`/`ResidualHeadroomExceeded`).
3. Populate `SeamGateOutcome` residual fields for reporting.

This sits inside the joint-grid candidate loop, so a vetoed baseline candidate naturally drives the
grid search exactly as a Pearson failure does today (`record_fit_joint_candidate`) — no control-flow
redesign. Cost: one residual + one floor probe per evaluated candidate; gate it behind
`residual_gate != off` so default runs that disable it pay nothing.

### 4d. `donor_relation` (derived, not required)

Per constraint 5, `floor informative` already restricts the gate to same-master+aligned content.
A run-level `donor_relation ∈ {same_master, mixed, diff_capture}` is a **diagnostic** inferred from
the fraction of gaps with informative floors (e.g. ≥ 70 % informative → `same_master`). It does not
gate; it explains and can later tune thresholds. Compute in the summary pass over per-gap verdicts.

### 4e. Thresholds (starting values — must be calibrated on real media)

| Symbol | Meaning | Start | Basis / caveat |
|--------|---------|-------|----------------|
| `FLOOR_OK` | `floor_db` below which the floor is "established" | **−15 dB** | sweep: established ≈ −44, failed ≈ −4. Synthetic floor is optimistic; real codecs are noisier → calibrate, keep conservative |
| `HEADROOM_MARGIN` | max headroom to still call it a match | **6 dB** | truth ≈ 0, decoys ≥ 18; 6 dB tolerates noise once radii unified |
| `residual_lag` (`residual_lag_secs`) | unified seam/floor lag radius | **10 ms** | Time-based → rate-independent but fewer frames at low rates (≈160 @16k, ≈480 @48k). Defines the recovery reach: a correct fill offset beyond it false-rejects. Must exceed residual alignment error after the aligner; larger = O(lag·window) cost. |

All config-overridable; none hard-coded in the gate.

## 5. Behavior-safety invariants

- `fill_mode = gate`: untouched.
- `residual_gate = off`: zero behavior change, zero added cost (opt out with `--residual-gate off`).
- Default is **`veto`** (P4): residual measured on every fit-mode candidate; use `off` for regression baselines.
- Two-mic / diff-capture pairs: floors uninformative → `NoOpinion` → unchanged.
- Veto can only **remove** patches; rescue (opt-in) can only **add** them. Ship veto first.

## 6. Config (point F)

```toml
[repair]
residual_gate = "veto"         # off | veto | veto_rescue   (CLI: --residual-gate)
residual_floor_ok_db = -15.0   # FLOOR_OK
residual_headroom_margin_db = 6.0
residual_lag_secs = 0.010      # unified seam+floor lag radius
```

`patch_audio.rs` converts `residual_lag_secs → frames`, passes all four into `SeamGateParams`.
Validation: `residual_lag_secs > 0`, `headroom_margin ≥ 0`. Default is **`veto`** after AAC/Vorbis
calibration and disagreement-table validation; use `off` for byte-identical regression baselines.

## 7. Reporting & vocabulary (points G, H)

- `GapPatchStatus::Patched`: add `residual_db`, `floor_db`, `headroom_db` (Option, `skip_serializing_if`).
- New skip reason — **as shipped**, `SeamGateFailure::ResidualHeadroomExceeded { pre, post, residual,
  margin_db }` (`patch_region.rs`), where `residual` is the full `SeamResidualVerdict` (carries
  `chosen_*`/`floor_*`/headroom, so `headroom_db` and `floor_db` are derived from it rather than stored
  flat); surfaced in `cli-output.md` skip strings + `json-output.md`.
- `gap_tags.rs`: new axes —
  - `residual_band ∈ {cancels, correlates_only, no_floor}` from headroom vs margin + informative;
  - `donor_relation ∈ {same_master, mixed, diff_capture}` (run-level, derived).
  Add to `GapTags`, `format_gap_tags_verbose_line`, JSON. Update `gap-repair-guide.md` §Vocabulary
  with the two axes (the "missing source-relationship axis" noted earlier).

## 8. Phasing

- **P0 (done):** debug diagnostics; corpus harness; alignment sweep.
- **P1 (shipped):** unified lag radii; residual on outcomes; `residual_band` tag; scalar fields on `Patched`; report when gate active or `measure_residual`.
- **P2 (shipped, default):** veto (`residual_gate = veto` default); F4 corpus test; integration `f4_decoy_residual_gate_vetoes_bool` (ignored).
- **P3 — rescue (`veto_rescue`):** false-skip rescue; validate it doesn't over-patch.
- **P4 (shipped):** default `veto`; `donor_relation` run diagnostic; `floor_oracle_residual_gate_real_codec` extended to Vorbis 128k speech/ambient.

## 9. Test plan

- **Domain unit:** `apply_residual_to_confidence` truth table (each row of 4b); lag-radius unify
  regression (sweep offset 100 no longer false-rejects); `informative` boundary at `FLOOR_OK`.
- **Corpus (extend `tests/seam_residual_corpus.rs`):** the **disagreement table** — per fixture/
  variant/placement, does the gate decision match the oracle, and where does it flip the Pearson
  decision correctly (F4 veto) vs wrongly. Add a **two-mic-like** fixture to assert `NoOpinion`.
- **Integration:** a same-master A/B pair (broadband) through `PatchAudio::execute` with
  `residual_gate = veto` asserting the echo gap skips and the true gap patches; and with `off`
  asserting byte-identical output to today.
- **Regression:** existing energy-corpus tests unchanged with `residual_gate = off`.

## 10. Open questions / risks

- **Real-codec floor calibration.** Synthetic floor ≈ −44 dB is optimistic; lossy codecs sit
  higher. `FLOOR_OK = −15` is a guess — P1's real-media numbers set it. If real floors routinely
  exceed `FLOOR_OK`, the gate abstains too often (safe but useless) → may need per-gap-relative
  floor instead of absolute. **Validated:** AAC, Vorbis, music (`source_gap_oracle_floor_csv`).
  **MP3 unvalidated (M4):** excluded from calibration; rides the codec-agnostic gate without
  codec-specific code; acting-branch behavior and libmp3lame same-encoder determinism floor
  unverified — see [residual-gate-findings.md](residual-gate-findings.md) M4. Parked behind
  punch-after-encode oracle + `veto_rescue`-on-MP3 run.
- **Fractional-delay ceiling.** Integer-only lag caps absolute cancellation (−16 dB at 0.5 sample);
  headroom hides it within reach (lag-centered chosen probe, **M5**); beyond reach the gate abstains.
  Mitigation: parabolic/fractional resample before subtraction (deferred).
- **Cost.** Unified lag at 10 ms × seam window × candidates × gaps. Keep behind the off-by-default
  flag; profile in P1.
- **Interaction with `anchored_retry`.** Residual verdict could feed anchor eligibility (a
  headroom-clean patch is a stronger anchor) — out of scope here, noted for later.

---

## Related
- [seam-scoring.md](seam-scoring.md) — seam pre/post mechanics the gate composes with
- [gap-fill-modes.md](gap-fill-modes.md) — fit tiers (`classify_fill_waveform_confidence`)
- [gap-repair-guide.md](gap-repair-guide.md) — vocabulary to extend (`residual_band`, `donor_relation`)
- [nway-donor-alignment-plan.md](nway-donor-alignment-plan.md) — the floor's multi-donor use
- [residual-gate-findings.md](residual-gate-findings.md) — bug/gap/smell ledger (H1/M1/M5 fixed; M4 deferred; L1–L12 open)
- `tests/seam_residual_corpus.rs` — the experiments grounding §2 and §4e
