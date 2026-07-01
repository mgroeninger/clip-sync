# Pipeline performance redesign — audit + plan

**Purpose.** The detect→gate→fingerprint pipeline grew organically to *explore* gap classification; it was
never reviewed for throughput. This doc (1) **audits** what the pipeline does today — every gate, its minimum
inputs, cost, and overlaps — then (2) proposes a **performant re-assembly** that returns exactly two things:
a **list of gaps to fix** and the **repair-params to fix them**, with everything else deferred.

**Relationship to other docs.** The [status ledger](TEMP-seam-repair-status-ledger.md) is the index (one row
per claim); this is the detail doc for the *perf/pipeline* workstream (D12). It **absorbs** the ledger's
scattered perf notes (D5 FFT, "Perf (before a long rescan)", the "Pipeline (detect → repair)" order) — those
now point here. The [dualfit plan](TEMP-seam-splice-dualfit-plan.md) owns the repair *algorithm*; this owns
the pipeline *assembly*.

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
| **G6** | **Dual-fit detect** *(future, A3)* | `gap_fingerprint.rs` (`splice_dualfit`) | Should dual-fit rescue this skip? | `bracket_exhausted` (B1) ∧ both-sides-recoverable `peak_z` (B3) ∧ donor-continuous (C5) ∧ `splice_dualfit.gate_pass` (C3) | lag sweep + donor + dualfit seam | non-recoverable skips |

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
5. Split production dual-fit path (decision + repair only) from the diagnostic fingerprint dump.

---

## §4 — Validation (prescriptive; TO DECIDE)

*Skeleton.*

- **Behavior-preserving regression harness:** the re-assembled pipeline must emit the **identical fix-list
  and repair-params** as the current one on the corpus (dirs 1–7). Perf work must not change decisions.
- Per-step: assert the affected measurement/decision is bit-or-ε-identical to the pre-refactor baseline.
- Track wall-clock per phase to confirm the wins land where the cost model predicts (§1.3).
