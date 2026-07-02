# Pipeline performance redesign — audit + plan

**Purpose.** The detect→gate→fingerprint pipeline grew organically to *explore* gap classification; it was
never reviewed for throughput. This doc (1) **audits** what the pipeline does today — every gate, its minimum
inputs, cost, and overlaps — then (2) proposes a **performant re-assembly** that returns exactly two things:
a **list of gaps to fix** and the **repair-params to fix them**, with everything else deferred.

**Relationship to other docs.** The [status ledger](TEMP-seam-repair-status-ledger.md) is the index (one row
per claim); this is the detail doc for the *perf/pipeline* workstream (D12). It **absorbs** the ledger's
scattered perf notes (D5 FFT, "Perf (before a long rescan)", the "Pipeline (detect → repair)" order) — those
now point here. The [ledger §4 wire spec](TEMP-seam-repair-status-ledger.md#4-dual-fit-repair--wire-spec-a3-unbuilt)
owns the repair *algorithm*; this owns the pipeline *assembly*.

**Hard wall.** §1 is **descriptive** ("what the pipeline does today") and stays factually true regardless of
plan revisions. §2–§4 are **prescriptive** ("what it should become"). Keep them separate so the audit can
graduate to a permanent `docs/pipeline-architecture.md` later if useful.

**Status:** §1 audit populated from code (2026-07-01, audit v1). §2–§4 scaffolded, not yet decided.

---

## §1 — Audit (what the pipeline does today)

### §1.0 There are two paths, and that's the core finding

- **Production repair (`PatchAudio`):** detect → gate (structure → waveform → residual) → fill. Relatively
  lean; computes only what the fill/skip decision needs.
- **Diagnostic fingerprint (`characterize_gaps_with_gate` / `dump_gap_fingerprints`):** runs the *same gate*
  **plus every measurement** (`baseline_lag`, `seam_probe`, `donor_interior`, `wide_envelope`,
  `splice_dualfit`, diagnostic `lag`, …) on **every matched gap**, for analysis.

The perf problem is that the **future dual-fit-capable production pipeline** needs *some* of the fingerprint
measurements (registration, step, donor, program-quiet) but **not** the diagnostic-only ones — yet today
they're computed together, unconditionally, per gap. The audit's job is to split the fingerprint set into
**decision / repair / diagnostic** so the production path carries only the first two.

### §1.1 Phase / gate inventory

Ordered as data flows. "Rejects" = what this gate filters out.

| # | Gate | Where | Decides | Minimum decision inputs | Cost class | Rejects |
|---|------|-------|---------|-------------------------|-----------|---------|
| G0 | **Silence-run detect** | `scan_gaps.rs` (`ScanGaps`) | Is there a gap? | A block-RMS vs `silence_peak_fraction`/`absolute_silence_rms`; `silence_hold_blocks`; `min_gap_secs` | `O(N)` decode + RMS | non-silent regions |
| G0b | **Fillable coverage** | `scan_gaps.rs` (`limit_fill_to_mapped_region`) | Is the gap inside mapped clip coverage? | gap time vs alignment coverage | scalar | out-of-coverage (tails / P6) |
| G0c | **B cross-check** (opt) | `scan_gaps.rs` (`scan_both`, `cross_check`) | Does B agree / have energy there? | B silence scan; `gap_offset_agreement`; `b_has_energy_in_range` | `O(N)` (B decode) | offset-disagreeing / B-empty |
| G1 | **Structure align** | `patch_region.rs` (`gate_structure_align`) | Is there *a* B placement? | A border templates + gap signature (energy/bool) + B haystack; unified search over `search_radius` | `O(N·radius)` **per bracket** | no placement → `StructureAlignmentFailed` |
| G2 | **Structure threshold** | `patch_region.rs` (`structure_passes_gate`) | Is the envelope match strong enough? | `structure_pre/post` ≥ `min_structure_match_score` (short-gap mean adj.) | scalar (from G1) | weak envelope → `StructureBelowThreshold` |
| G3 | **Waveform seam** | `patch_region.rs` (`classify_fill_waveform_confidence`) | Do the seams match at sample level? | pre/post Pearson @ placement vs `min_fill_correlation` / `fill_marginal_margin` / `fill_absolute_floor` | `O(seam)` | dead seam → `waveform_below_threshold` |
| G4 | **Residual** | `patch_region.rs` (`finalize_fit_outcome_residual`) | Same-source confirm for a *marginal* seam | least-squares cancellation @ throat vs floor | `O(seam)`, **at selection only** | veto marginal; rescue marginal |
| R | **Bracket ranking** | `patch_region.rs` (`fit_candidate_ranking_score`) | Which *passing* bracket wins | `min(pre,post)`, `boundary_move` | scalar | — (selection, not reject) |
| **G5** | **Program-quiet** *(new, D11 — not yet a gate)* | `gap_fingerprint.rs` (`donor_interior_nominal`) | Is this a real dropout, or quiet in both masters? | nominal-span B `silence_fraction`; A `gap_floor` vs `noise_floor` | `O(N)` | B-silent → non-dropout (don't fill, don't count as miss) |
| **G6** | **Dual-fit detect** *(A3, = `dualfit_target()`)* | prod dual-fit path (port from `gap_fingerprint.rs`) | Should dual-fit rescue this skip? | `skip` ∧ `bracket_exhausted` (B1) ∧ `splice_dualfit.gate_pass` ∧ **step-real** (`post_own − post@pre ≥ 0.15`) ∧ `donor_interior.continuous` ∧ ¬program-quiet (`donor_interior_nominal`). **NOT** uniqueness/`peak_z` — retired (mispredicts seam viability; ledger A3). | nominal-anchored ±600 ms seam search (`seam_local_peak`) + donor | donor-BROKEN / program-quiet / step-spurious skips |

### §1.2 Measurement → gate map (decision / repair / diagnostic)

Every fingerprint field, what produces it, its cost, which gate consumes it, and its **label**. Label =
**D**ecision (a gate branches on it) · **R**epair (needed to build the fix, survivors only) · **X** = diagnostic
(no gate reads it — report/exploration only).

| Measurement | Produced by | Cost | Consumed by | Label |
|-------------|-------------|------|-------------|-------|
| A block-RMS / silence runs | `scan_gaps` | `O(N)` | G0 | **D** |
| alignment / `gap_offset` | aligner | (align) | G0b, geometry | **D** |
| border templates (`a_pre`/`a_post` + per-ch) | `border_templates_for_gap` | `O(border)` | G1, G3 **(+ lag, seam_probe, wide_env, dualfit)** | **D** (shared — see §1.4) |
| gap signature (energy/bool) | `build_gap_signature` | `O(context)` | G1 | **D** |
| unified placement (`structure_pre/post`, `start_frame`, `pre/post_correlation`) | `match_gap_fill_unified` | `O(N·radius)`/bracket | G1, G2, G3 | **D** |
| `levels` (A `LevelProfile`) | `build_gap_fingerprint` | `O(N)` | `gap_floor_db`/`noise_floor_db` → **G5**; `speech_peak`/`profile_db` → none | **D** (2 fields) / **X** (rest) |
| `silence` / `contour` / `anchors` | `build_gap_fingerprint` | `O(N)` | **gate recomputes its own** (`build_gap_signature`) — these fields feed no gate | **X** (records; but anchor *detection* is a shared computation — §1.4) |
| `brackets[]` (seam + `failure_stage`) | `oracle_score_fit_candidate` ×N_br | N_br × G1–G3 | G3, dual-fit `bracket_exhausted` | **D** |
| `baseline_lag` (lag sweep, **mono only** — `selected: None`) | `lag_at_placement` | **`O(n·L)`** (1 sweep) | G6, uniqueness (`peak_z`/`prom`), `splice` | **R** (+ X today) |
| `residual` | `oracle_measure_residual` | `O(seam)` | G4 | **D** |
| `seam_probe` (R2/R4/spectrum/env/recov) | `seam_probe_at_placement` | `O(seam)` + FFT | **none** | **X** |
| `donor_interior` (aligned span) | `donor_interior_at` | `O(N)` | G6 | **R** |
| `donor_interior_nominal` (nominal span) | `donor_interior_at` | `O(N)` | **G5** | **D** |
| `b_levels` (symmetric B profile) | `level_profile` (B) | `O(N)` | validation only | **X** |
| `splice` (step + peaks) | derived from `baseline_lag` | ~free | G6, repair | **R** |
| `wide_envelope` | `wide_envelope_at_placement` | `O(env sweep)` | **none** | **X** |
| `splice_dualfit` (+ validators) | `splice_dualfit_at` | `O(seam)` + small sweeps | **G6** | **R**/D |
| diagnostic `lag` (best-energy bracket) | `place_on_b` + `lag_at_placement` (**mono + selected**) | `O(N·radius)` + `O(n·L)`×2 | **none** | **X** |
| `outcome` (tier / skip_reason) | classification | scalar | (output) | **D** |

### §1.3 Cost hierarchy (what actually dominates)

1. **Structure search per bracket** — `N_brackets × O(N·radius)` unified fill search (G1). The single
   largest per-gap cost; the fingerprint re-runs the oracle for *every* bracket to get per-bracket
   `failure_stage`, even when production only needs the winner.
2. **Lag sweep** — `O(n·L)` mono, naive time-domain (`n≈48k`, `L≈57.6k`). One sweep for `baseline_lag`
   (feeds `peak_z`/`prom`/`splice`); the diagnostic `lag` adds **two more** (mono re-sweep + selected, both
   X). **FFT target** (§3, was D5) — ~50–150×; dropping diagnostic `lag` removes the two extra sweeps.
3. **Decode** — A and B (once); `dump_gap_fingerprints` re-decodes after repair.
4. Cheap (`O(N)` RMS or `O(seam)`): `levels`/`silence`/`contour`, `donor_*`, `b_levels`, `seam`,
   `residual`, `splice_dualfit` seams.

### §1.4 Overlaps & redundancy (share once / defer)

- **Border templates** are rebuilt in G1, `lag_at_placement`, `seam_probe`, `wide_envelope`, and
  `splice_dualfit`. → **Hoist one extract per gap** and pass down. (Perf note already flagged this.)
- **`place_on_b` runs ≥3×** — summary pre-gate, gate-throat overlay, diagnostic `lag`. → **Dedup to one**
  (the gate throat is authoritative; F1).
- **RMS binning happens ~7×** over decoded audio — A `levels`, `silence`, `contour`, A `gap_floor`,
  `donor_interior`, `donor_interior_nominal`, `b_levels`. → **One binned-RMS pass per side**, all consumers
  index in.
- **Lag sweep — `baseline_lag` is one mono sweep** (`selected: None`) feeding 4 consumers (`baseline_lag`,
  `peak_z`, `prominence`, `splice.step`). The **diagnostic `lag`** re-sweeps at a *different* placement and
  adds a **selected-channel** sweep → **2 extra `O(n·L)` sweeps, both X**. Dropping diagnostic `lag` from the
  production path removes them entirely. *(Confirmed 2026-07-01: audit open item (a).)*
- **Gate is independent of the fingerprint** — `patch_region.rs` never reads `GapFingerprint`; it recomputes
  `build_gap_signature` (structure) and its own anchor brackets. So the fingerprint's `levels`/`silence`/
  `contour`/`anchors` fields are **diagnostic records**, *not* gate inputs. But both sides compute
  overlapping things (RMS profile, anchors, signature) → a **shared-computation** opportunity if the
  production dual-fit path and the gate are unified. *(Confirmed 2026-07-01: audit open item (b).)*
- **Diagnostic-only, computed unconditionally per gap:** `seam_probe`, `wide_envelope`, diagnostic `lag`
  (incl. its 2 sweeps), `b_levels`, per-bracket structure. → **Defer behind a flag / compute lazily** (only
  near-threshold gaps).

### §1.5 The two outputs, mapped to the audit

- **Fix-list (detect):** produced by G0→G0b→(G5)→G1→G2→G3→G4, plus (future) G6 for dual-fit skips.
  Needs: silence runs, coverage, nominal-donor, structure placement, seam, residual.
- **Repair-params (fix, survivors only):** placement `start_frame` (from G1); for dual-fit — `baseline_lag`
  per-shoulder, `splice.step`, trim (`splice_dualfit.trim_frames`), donor span.
- **Everything labeled X** (`seam_probe`, `wide_envelope`, diagnostic `lag`, `b_levels`) is **not** on either
  output path — it exists to *explain* decisions, and belongs behind a diagnostics flag.

> **Audit open items:**
> - **(a) RESOLVED (2026-07-01)** — selected-channel lag sweep is **diagnostic-only**. `baseline_lag`
>   (both sites) passes `selected: None` ⇒ mono sweep only; the selected sweep runs only in the diagnostic
>   `lag` (`gap_fingerprint.rs:2340`, `p.selected_channels.first()`). So the production/repair path has **one**
>   lag sweep; diagnostic `lag` adds two more (mono re-sweep at a different placement + selected).
> - **(b) RESOLVED (2026-07-01)** — the gate (`patch_region.rs`) **does not read `GapFingerprint`**; it
>   recomputes `build_gap_signature` and its own anchors. Fingerprint `levels`/`silence`/`contour`/`anchors`
>   are diagnostic records. Only `levels.gap_floor_db`/`noise_floor_db` are decision-relevant (→ G5). Anchor
>   detection is decision-relevant but done *inside* the gate — a shared-computation opportunity, not a
>   fingerprint dependency.
> - **(c) RESOLVED (2026-07-01)** — G0c cross-check is **on by default**: `scan_both: default_true()`
>   (`config.rs:420`).
> - **(d) PENDING** — measure per-gate reject rates on the corpus for the §2 ordering. Needs the fresh scan
>   (`donor_interior_nominal` for G5's rate); not code-confirmable.

---

## §2 — Target architecture (prescriptive; TO DECIDE)

*Skeleton — fill after §1 open items resolve.*

- **Two-pass:** cheap **detect** pass emits the fix-list; expensive **repair** pass runs only for survivors.
- **Cheap-first short-circuit ordering:** G0 → G0b (drop tails) → **G5 drop program-quiet** (`O(N)`) →
  G1/G2 structure → G3/G4 seam/residual (expensive, survivors only). Order gates by *selectivity ÷ cost*.
- **Compute-once-share:** one decode/side · one binned-RMS/side · one border extract/gap · one lag curve/shoulder.
- **Lazy diagnostics:** X-labeled measurements behind a flag or only for near-threshold/ambiguous gaps.
- **FFT lag sweep** (§3) at its sequenced point.

*(Target gate ordering table + data-flow DAG go here.)*

---

## §3 — Migration steps (prescriptive; TO DECIDE)

*Skeleton. Each step behavior-preserving, landed behind the §4 regression harness.*

1. Hoist shared subexpressions (border extract, binned-RMS, dedup `place_on_b`).
2. Gate diagnostics (X-set) behind a flag.
3. Insert cheap early-reject gates (G0b tails, G5 program-quiet) *before* the structure search / lag sweep.
4. **FFT lag sweep** — numerator via FFT, denominator via prefix sums; naive fallback for small `L`; gate on
   `fft_curve ≈ naive_curve` test. *(Absorbs ledger D5 — the full spec lives in the ledger's
   "FFT lag sweep" block; move it here when this step is started.)*
5. Split production dual-fit path (decision + repair only) from the diagnostic fingerprint dump. **This is
   A3 — built FIRST, not last** (it *creates* the production dual-fit path; steps 1–4 then optimize the whole
   pipeline including it). Full build plan in **§5**.

> **Sequencing — optimize the fingerprint scan LAST (2026-07-01 decision).** The diagnostic scan and the
> production path **share code** (the scan's oracle wraps the production gate; the lag sweep is shared with
> `baseline_lag`). Optimizing the scan *before* the production path is built and split (step 5) tunes code
> that the redesign then restructures — pure rework. The scan's cost (~1.7 h/pair) is a **dev/calibration
> cost, not a product cost**, so it is the lowest-priority optimization. Order: **build the A3 repair →
> redesign/tune the production path (steps 1–5, the split falls out here) → then port the FFT lag sweep and
> hoist shared subexpressions into the now-stable diagnostic scan.** The FFT sweep (step 4) is likewise
> deferred past calibration so a stable naive baseline exists to write the `fft ≈ naive` equivalence test
> against.

---

## §4 — Validation: the decision-invariance harness

Every perf step is behavior-preserving, so the harness's job is narrow and strong: **prove a refactor did
not change any repair decision or repair parameter** on the corpus (dirs 1–7), while *allowing* the drift
perf actually needs (FFT ≈ naive at ~1e-10; diagnostics recomputed differently or skipped). The harness
structure **is the gap vocabulary** — it snapshots the decision/repair (D/R) axes, not the whole
fingerprint, so a failure is meaningful ("`donor-nominal` silence changed on 2·g7") rather than "some JSON
field differs." See [TEMP-gap-vocabulary-redesign-plan.md](TEMP-gap-vocabulary-redesign-plan.md) §2 for the
axes and [status ledger](TEMP-seam-repair-status-ledger.md) §1.2-map for the D/R/X labels.

### §4.0 Prerequisites (do NOT capture the golden baseline before these)

The harness is a **characterization** harness — it pins whatever the pipeline currently emits. So the
pipeline must first be in a state we are willing to call *the correct baseline*. Two gates:

1. **Golden baseline captured post-scan.** Capture only **after** the `seam-local-fix` scan completes and
   **validates** the fix (`2·g1`/`1·g22` recover as targets; `splice_dualfit.pre_seam_r` for `2·g1` ≈ 0.98,
   not −0.008) **and** donor-continuity as a gate is confirmed (A4/C5). Capturing now would pin pre-fix
   numbers we already expect to change (regression-testing a moving target). Until then, §4 defines the
   **schema**, not the values.
2. **P2 orthogonality gate.** Before freezing the golden-record schema, run vocab **P2** (cluster the fresh
   corpus on the D/R axes — now unblocked, registration placement is fixed). Confirm the axes are
   **independent** (no two always co-vary), **populated** (cells that never occur aren't axes), and
   **non-redundant**. Collapse/rename any axis the data refutes *before* the schema is frozen — otherwise the
   harness locks in a mis-factored structure.

### §4.1 Golden record — capture the D/R axes, each tagged with its placement

Per gap, snapshot **only** the decision/repair-bearing coordinates, each stamped with the **placement** it is
measured at (placement is part of the key — the two bugs we hit were placement conflations, so a refactor
must not silently move a measurement across placements):

| Field(s) | Placement | Tier (see §4.2) |
|----------|-----------|-----------------|
| `outcome.tier`, `dualfit_target()`, `program_quiet_skip()`, `bracket_exhausted()` | derived | **1 — exact** |
| `splice_dualfit.gate_pass`; `donor_interior.continuous`; `donor_interior_nominal.continuous`; `edge_pinned` | seam-local / aligned / nominal | **1 — exact** |
| `brackets_total`, `brackets_passing` | gate throat | **1 — exact (ints)** |
| `baseline_lag` pre/post `peak_r`, `frac_lag_ms`; `splice.step_ms` | **gross** `b_mapped` (1 s) | **2 — ε** |
| `splice_dualfit` `pre/post_seam_r`, `post_seam_global_r`, `trim_frames` | **seam-local** (250 ms ±`SEAM_LOCAL_REFINE_MS`) | **2 — ε** |
| `donor_interior_nominal.silence_fraction` | **nominal** `b_mapped` span | **2 — ε** |
| `donor_interior.silence_fraction`, `rms_db` | **aligned** bridge span | **2 — ε** |
| `residual` chosen/floor dB | gate throat | **2 — ε** |
| `levels.gap_floor_db`, `noise_floor_db`; `structure_min` | gate throat / A | **2 — ε** |
| `peak_z`, `prominence` | gross `b_mapped` (1 s) | **2 — ε** (feed the descriptive classifier; the FFT step's equivalence test pins these specifically) |
| `seam_probe`, `wide_envelope`, `b_levels`, diagnostic `fp.lag` | (various) | **3 — ignore / presence-only** (X — not on the fix-list or repair-params) |

**Fix-list** = gaps where `dualfit_target()` ∨ patched. **Repair-params** = per-shoulder seam-local lags →
`b_pre`/`b_post`, `trim_frames`, donor span. Both are functions of Tier-1/2 fields only.

### §4.2 Two-tier assertion (the FFT is what forces this)

- **Tier 1 — derived verdicts + booleans + integer counts: bit-exact.** A perf refactor must **never flip a
  decision.** `tier`, `gate_pass`, `dualfit_target`, `program_quiet`, `bracket_exhausted`, `continuous`,
  `edge_pinned`, bracket counts — identical, no tolerance.
- **Tier 2 — continuous D/R inputs: within ε.** Pick ε so a real regression trips but f64-FFT / reassociation
  noise (~1e-10 relative) does not. This tolerance is *exactly what lets the FFT lag sweep (step 4) land* —
  it moves `baseline_lag`/`peak_z` at 1e-10 but must not move a Tier-1 verdict. (Layered guarantee:
  continuous inputs may drift within ε; the booleans they derive must not flip. If an ε drift *does* flip a
  verdict, the gap sits on a threshold — flag it, don't widen ε.)
- **Tier 3 — diagnostics (X): not asserted** (or presence-only). They may be recomputed differently, skipped
  behind the lazy-diagnostics flag (step 2), or dropped entirely without failing the harness.

### §4.3 Placement-provenance guard

Because the golden record keys on `(field, placement, value)`, the harness fails if a refactor changes the
**placement** a field is measured at — even if the value looks plausible. This directly guards the planned
"share one border extract at the throat" hoist (step 1) from dragging a **seam-local** score onto the
**gross/throat** placement (the exact class of bug behind `2·g1`).

### §4.4 Footgun characterization tests (pin the post-fix behavior — values TBD post-scan)

Lock in the two fixes so no refactor silently reverts them. Concrete expected values get pinned once the scan
confirms them (§4.0); the *assertions* are defined now:

- **Seam-local placement** — a gap whose gross and seam-local lags diverge (`2·g1`) keeps `pre_seam_r` at its
  seam-local peak (~0.98), **not** the gross-placed dead value (−0.008); `gate_pass` = true.
- **Donor placement split** — a large-step gap that is silent at the **nominal** span but occupied at the
  **aligned** span classifies `program_quiet` (nominal wins) — guards D11's registration-independence.
- **Donor gate necessity** — a gap with `gate_pass` = true but `donor_interior` BROKEN (`1·g19`: seams 0.998,
  interior silent) yields `dualfit_target()` = false.
- **Edge-pin validity** — an edge-pinned shoulder flags its step GIGO and is excluded (0/55 today, so this is
  a guard against a future regression, not a live case).

### §4.5 Per-axis localization + wall-clock

- Diff **per axis**, not per gap-blob: the orthogonal axes let a failure name the responsible axis + placement.
- Track **wall-clock per phase** to confirm wins land where the cost model predicts (§1.3) — a step that
  passes the invariance harness but doesn't move the predicted phase cost is suspect.

### §4.6 What the harness deliberately does NOT cover

- **Decoy / wrong-placement safety (D8)** — the corpus has no genuine negatives; the harness proves
  *invariance*, not *correctness of the fill*. D8 (audible/decoy validation) is separate, post-A3.
- **The `different`/`ambiguous` shared-source regime** — untested; not a live axis (shared-source collapsed
  to a constant on this corpus, B2/C1).

---

## §5 — A3 production build plan (= §3 step 5, built FIRST)

Wire the dual-fit repair into **production `PatchAudio`**, flag-gated, as a fallback for gaps the existing
gate skips. Viability is proven (9 targets, golden baseline frozen); this section is the **production wiring**.
Algorithm = ledger **§4 wire-spec**; scope predicate = `dualfit_target()` (= `G6`). Build lean per §1.5/§2:
the production path carries only the **D/R** set for the survivors, never the diagnostic **X-set**.

### §5.1 Where it plugs in
- **Entry:** `application/patch_audio.rs::prepare_region_patch`. Today: build gate params → `evaluate_seam_gate`
  → on success build `RegionPatch` (fill), on `SeamGateFailure` → `skipped_patch(reason)` → `splice_into_a`
  writes nothing. **Change:** when the gate skips **and** the gap is bracket-exhausted **and** the dual-fit
  flag is on, fall through to a **dual-fit branch** instead of returning the skip. Everything else unchanged
  (D6: flag off ⇒ byte-identical — the §4 harness enforces this).
- **Shared primitive:** extract `seam_local_peak` (nominal-anchored ±600 ms seam search) from
  `application/gap_fingerprint.rs` to a shared home (e.g. `domain/`) so production and the diagnostic scan
  call the **same** function (no second implementation to drift — the bug class we just fixed twice).

### §5.2 The dual-fit branch (per gap, only reached on a bracket-exhausted skip)
1. **Detect (`G6` = `dualfit_target`)** — on the already-decoded A/B window:
   - per-shoulder seam-local lag via `seam_local_peak` at nominal `b_mapped` → `pre_lag`/`post_lag`;
   - score the two 250 ms seams (reuse `policies::fill_splice_seam_correlations_interleaved`) → `gate_pass`
     (`min ≥ min_fill_correlation ∧ ≥ fill_absolute_floor`);
   - `post@pre` (post seam at the pre lag) → **step-real** (`post_own − post@pre ≥ 0.15`);
   - `donor_interior` (aligned) `.continuous` **and** `donor_interior_nominal` `.silence_fraction < 0.5`.
   - All hold ⇒ dual-fit target; else return the skip as today. *(D/R measurements only — no `seam_probe`,
     `wide_envelope`, `b_levels`; those stay in the scan.)*
2. **Fit + reconcile (§4 steps 2–3)** — `b_pre = b_mapped_start + pre_lag`, `b_post = b_mapped_start +
   gap_frames + post_lag`; bridge `= B[b_pre..b_post]`; `trim = bridge − gap`. **New primitive
   `trim_at_lowest_energy_interior`**: find the min-RMS interior frame of the bridge and trim/pad `|trim|`
   there with a short crossfade — the smallest audible splice. *(Existing `fit_fill_to_gap_frames` /
   `pick_fill_length_anchor` trim only at head/tail; this is the one genuinely new audio op.)*
3. **Validate with the UNCHANGED gate (§4 step 4)** — score the assembled fill's pre/post seams with
   `fill_splice_seam_correlations_interleaved` + `classify_fill_waveform_confidence` at the existing floors.
   Accept iff `min(pre,post) ≥ floors`; else skip. **No loosening** — the fix earns the current validator.
4. **Splice** — wrap into `RegionPatch { b_samples, gain, a_start_frame, a_end_frame, crossfade }`; existing
   `splice_into_a` crossfades it into A. Unchanged.

### §5.3 Reuse vs. new
- **Reuse (production, unchanged):** `fill_splice_seam_correlations_interleaved`, `classify_fill_waveform_
  confidence` + floors, border templates, the decode window from `prepare_region_patch`, `splice_into_a`.
- **New:** the dual-fit branch in `prepare_region_patch`; `trim_at_lowest_energy_interior`; the `G6`/
  `dualfit_target` detect logic in production (port the predicate; do **not** re-derive uniqueness).
- **Move (share):** `seam_local_peak` → shared module.

### §5.4 Validation (before this ships)
- **§4 golden-baseline harness (regression guard):** with the flag **off**, the 23 existing patches are
  byte-identical (D6); with it **on**, production's dual-fit target set must equal the frozen **9**
  (`golden/re-anchor-dual-fit-on-nominal.golden.json`). This is A3's correctness cross-check.
- **D7 (the real test):** run on the media, **listen** to the 9 fills — gate-pass is necessary, not
  sufficient; the interior trim point must sound clean. First bad fill = the first labeled negative (→ D8).

### §5.5 Open decisions
- **Detect wiring:** (a) self-contained — production recomputes detection on-demand *(recommended)*; vs
  (b) scan-fed — read targets from a prior fingerprint. The *repair* is production either way.
- **Interior trim crossfade length** — audibility knob (D7); start with the existing `crossfade_secs`.
- **Flag surface** — `FillMode::DualFit` vs a `--dual-fit` bool on the request. Keep off by default until D7.
