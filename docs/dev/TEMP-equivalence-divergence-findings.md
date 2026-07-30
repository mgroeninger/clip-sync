# Equivalence divergence — open findings ledger

**Opened:** 2026-07-30. **Status:** **F14** border alignment **FIXED and media-validated** (dump A
borders = `mono(refined ± w)` like `try_dual_fit`; `fp_post_F14_fix/` confirms). **F15** still OPEN —
floor **(a)** decided; silent-core A RMS should follow; noise-floor / context window still open.
Retracted claims are marked in place rather than deleted.

Split out of [archive/TEMP-silence-floor-findings.md](archive/TEMP-silence-floor-findings.md) when
that ledger was archived (2026-07-30). Everything else in it is closed; these two are not, and both
came out of its §5 follow-up rather than its original F1–F12 sweep. Finding IDs **F14/F15** are kept
from the parent ledger so its text still resolves.

Originally recorded from **measurement only**. Source tracing confirmed both: F15 is a
**threshold/window-definition** split between the two equivalence front-ends (shared classifier,
incompatible sensors); F14 began as a **missing decision wire** and is now a **shared-predicate /
divergent-sensor** problem after the wire landed. Hypotheses below that were confirmed are marked as
such; one F14 claim about the filename was **partially retracted**.

**Reference audit, 2026-07-30 (second pass).** `file:line` refs were re-read that day; they drift —
re-verify before acting. Notable corrections from that pass (kept in place below): F15's early
"scan gap floor ≈ −82" eyeball became a derived bound, then a **measured** floor on the
`fp_F15_question/` re-run; F15 gained a third divergence axis (donor *window*); F14's `any_ok` /
CLI-help paragraph was corrected.

Media: an uncatalogued licensed 5.1 pair (A ≈ 6900 s, AAC-LC 48 kHz 5.1). Per the media-hygiene
rule the pair is referred to only by these properties; timestamps are numeric, raw logs stay in
gitignored `gap-files/`.

**Verification rule.** Re-read any `file:line` reference before acting on it — the references below
were read 2026-07-30 and some have already moved.

## Where the data lives — and why it is inlined here

Artifacts sit under gitignored `gap-files/silence-floor/`: `fp_F15_question/` (full-pair re-run with
both floors recorded — the Answer's source), `fp6/` / `fp/` (earlier single-gap dumps),
`scan-postfix.json`, `preview-debug.log`, plus a `*-scan.json` and `*.log` per run.
**`gap-files/` is ephemeral and deletable** — it is licensed-media-derived and not a durable
reference. Every number these findings depend on is therefore quoted inline below, so the findings
survive the directory being cleared. Re-deriving them costs one ~15 GB run per finding (see the
reproduction section).

The pair is the same one throughout: gap indices are **0-based in filenames, 1-based in the gap
table and in `--fingerprint-gap`**. F15's gap is table `#6` / file `g005`; F14's is table `#2` /
file `g001`.

---

## F15 — Scan-time and fill-time equivalence disagree on the same gap, post-fix
**Severity: high. Status: OPEN — floor (a) decided; silent-core A RMS should follow; context /
noise-floor window still open.** Found / diagnosed 2026-07-30.

The gap at **2585.11–2586.25 s** carries both verdicts in one fingerprint file, and they are
opposites:

| | `scan_equivalence` | `equivalence` (fill-time) |
|---|---|---|
| class | `repairable_dropout` | **`shared_silence`** |
| `drop` | false | **true** |
| `donor_silence_fraction` | **0.10** | **0.8696** |
| `a_gap_rms_db` | −82.27 | −60.68 |
| `noise_floor_db` | −45.85 | −64.83 |

**Correction to how this was first written.** The fill-time fraction is **not** independently
corroborated by `donor_interior_nominal` — it *is* that number. `measure.rs:2269-2271` passes
`fp.donor_interior_nominal.silence_fraction` straight into `measure_gap_equivalence` as the donor
fraction, so `equivalence.donor_silence_fraction == donor_interior_nominal.silence_fraction`
(0.8696) by construction. Treating the match as agreement was double-counting one measurement.

**Second correction (2026-07-30, deeper) — there is no independent corroboration at all.** The two
remaining "independent" measurements are the same function with the same floor, at different spans:

- `donor_interior` (anchored span): `silence_fraction: 0.9`, `longest_silence_ms: 900`,
  `rms_db: −55.87` — `domain/donor.rs:donor_interior_at`, floor = `gap_floor_db`.
- dual-fit's aligned bridge: `silence_fraction: 0.5833`, `longest_silence_ms: 350` —
  `domain/dual_fit.rs:191` and `:219` call **`donor_interior_at` with `p.a_gap_floor_db`**.

So 0.58 / 0.87 / 0.90 are **one predicate against one floor over three spans**, not three
measurements. The whole fine side reduces to a single sensor. The earlier framing — "three
measurements agree, scan-time's 0.10 is the outlier" — does not survive: the true count is **one
fine reading vs one coarse reading**, and the tally never favoured either.

### Which floor is right — **answered (a)**; see *What must be resolved first*

The two floors are not two granularities of one definition. They are different statistics:

| | coarse `gap_floor_db` | fine `gap_floor_db` |
|---|---|---|
| source | `derive_gap_equivalence` (`domain/gap_equivalence.rs`, silent-block max) | `level_profile` (`gap_fingerprint/measure.rs`, all-bins max) |
| set | max RMS of A's **silent** in-gap blocks | max RMS over **all** bins in the gap span |
| silence filter | **yes** (the F2/R1 fix) | **none** |
| value on g5 (`fp_F15_question/`) | **−74.53** (measured) | −51.03 |

The fine floor is computed with **no silence filter**, so it is inflated by exactly the
edge-refinement and hold-bridged content that the parent ledger's **F2** identified and fixed — on
the coarse path only. A higher floor makes more of B count as silent, pushing toward
`shared_silence` and `drop: true`, which is the **dangerous** direction.

That inverted the working assumption. This document (and `equivalence-calibration`) had been treating
fine as ground truth; on the one axis where the parent ledger already settled the semantics, the
**coarse** path carries the fixed definition and the fine path carries the pre-F2 shape. The donor
here sits at −55.87 dB: below A's *unfiltered* gap content (−51.03) but ~16 dB above A's *silent*
floor. Both readings are internally consistent; they answer different questions. **Decision: (a)** —
details and what (a) does *not* close are under *What must be resolved first*.

### The full input set

The class disagreement is **overdetermined**: all three inputs to `classify_gap_equivalence`
differ between the two paths, and the A-side pair alone would flip the class without the donor
fraction being involved at all.

| input | scan-time (coarse) | fill-time (fine) | Δ |
|---|---|---|---|
| `a_gap_rms_db` | −82.27 | −60.68 | 21.6 dB |
| `noise_floor_db` | −45.85 | −64.83 | 19.0 dB |
| `a_below_noise_db` | −36.42 | **+4.15** | sign flip |
| `donor_silence_fraction` | 0.10 | 0.8696 | 8.7× |

Supporting geometry and levels from the same file: A span `2585.1105–2586.2542` (1.1437 s),
B mapped `2583.4889–2584.6326`, `fill_offset_secs −1.62155`; `levels.gap_floor_db −51.026`,
`levels.noise_floor_db −64.830` (fine), `levels.bin_ms 0` with an empty `profile_db` (Tier-3
`--fingerprint-diagnostics` was on, but the level profile did not populate — worth a glance, it may
be a separate reporting gap).

### The two code paths are different functions, not one function at two granularities

They do **not** share a donor computation; they share only the final classifier.

- **Coarse / `scan_equivalence`:** `derive_gap_equivalence` (`domain/gap_equivalence.rs`) — A RMS
  from **scanner-silent blocks only** over the **core** interval; noise floor = median of scan
  blocks in **±2 s** (`EQUIVALENCE_CONTEXT_SECS`); donor fraction from scanner `BlockLevel`s,
  counting a block silent when `b.silent || rms_db < scan_gap_floor`.
- **Fine / `equivalence`:** `measure_gap_equivalence` (`application/gap_equivalence.rs`) — A RMS =
  full refined PCM span (`gap_interior_rms_db`); noise floor = `fp.levels.noise_floor_db` (median of
  **50 ms** bins in **±3 s**, `gap_signature_context_secs`); donor fraction is **precomputed** from
  `donor_interior_nominal` (`domain/donor.rs:donor_interior_at` — pure `rms < fine_gap_floor`, no
  `silent` bit).
- The join: `characterize` / `measure.rs`, where `equivalence` is computed and `scan_equivalence` is
  copied in by positional index.

Scan deliberately uses the silent **core** (not the hold-bridged refined `[start, end]`) so
sub-block edge refine cannot inflate A RMS and flip a dropout to ambient — `scan_gaps.rs:243-248`.
Fill-time uses the opposite window (refined interior). That asymmetry is intentional for each path
in isolation; together it feeds the same classifier incompatible sensors.

**The donor windows differ too — not just the predicate.** The same comment states that B's
donor-silence window is "the same core offset-mapped so it matches A", i.e. coarse measures the
donor over the **core-mapped** span while fine measures `donor_interior_nominal` over the
**refined nominal** span. So the two donor fractions differ on *three* axes — window, bin size, and
predicate — and only the predicate is diagnosed above. Any fix that aligns predicates without
aligning windows will narrow this divergence without closing it.

**Bin arithmetic, consistent with both.** Fine: `0.8696 = 20/23` — 23 bins × 50 ms = 1.15 s, matching
the 1.1437 s span. Coarse: `0.10 = 1/10` — 10 block centers at 100 ms. So the coarse path saw
**1 silent block in 10** where the fine path saw **20 silent bins in 23**. A grid difference cannot
produce that; the *predicate* resolves differently. Note the block count is **inferred from the
0.10** — a core-mapped 100 ms window over a 1.1437 s gap could hold 10 or 11 centers, and the JSON
records the fraction, not the counts. `1/10` is the reading that fits; `1.1/11` is not a thing.

### Source diagnosis (confirmed 2026-07-30)

**Not a copy/index bug and not an arithmetic defect.** Both paths call
`classify_gap_equivalence`; the three inputs are differently defined measurements. Either the
A-side pair or the donor fraction alone is enough to flip the class on this gap.

**Donor threshold-band — confirmed, but read the bound below.** Fine gap floor is −51.03; anchored
donor RMS is −55.87 — below it, so `donor_interior_at`'s `rms < floor` calls the donor silent
(0.87). The scanner's `silent` bit is peak-domain against the absolute floor
(0.001007 ≈ −59.9 dBFS); −55.87 sits **above** it. Content in the band between the abs floor and
the fine gap floor is silent to one rule and occupied to the other — exactly the 0.10 vs 0.87
split, with no arithmetic bug.

The third leg — that `rms_db < scan_gap_floor` also fails — was first established by a bound, because
early artifacts had no scan floor field. Provenance later landed (`gap_floor_db` on
`GapEquivalenceVerdict`); the `fp_F15_question/` re-run **measures** scan floor on g5 as **−74.53 dB**.
The donor at −55.87 dB is ~19 dB above it, so `rms_db < scan_gap_floor` cannot fire. (The earlier
bound was ≤ −71.9 from `max ≤ mean_db + 10·log₁₀(N)` with `a_gap_rms_db = −82.27` and `N ≤ 11` — the
conclusion held; the measured value is tighter.)

**A-side alone also flips dropout.**

| | scan | fine |
|---|---|---|
| `a_gap_rms` vs `noise_floor` | −82.27 vs −45.85 → **36 dB below** | −60.68 vs −64.83 → **above** floor |
| `is_dropout` (margin 35) | true | false |
| class with observed donor | `repairable_dropout` (dropout ∧ occupied) | `shared_silence` (¬occupied) |

Drivers of the A-side Δ: silent-blocks-only aggregate over the **core** (scan) vs full-span RMS
over the **refined** interval (fine); context median over **±2 s / 100 ms blocks** vs
**±3 s / 50 ms bins**.

**Ruled out already:** an index misalignment between `fp.index` and `report.gap_equivalence`
(positional lookup in `measure.rs`). Scan report entry `[5]` reads
`repairable_dropout / 0.1 / −82.27 / −45.85` — exactly the values in the fingerprint's
`scan_equivalence`. The arrays are index-parallel and the copy is correct.

~~**Note the stale comment** at `measure.rs` naming "250 ms" for the coarse join.~~ **Fixed
2026-07-30** — that comment (and its twin) now name the `scan_block_ms` recipe knob.

### Prior validation says this should not exist

`skip_equivalent_gaps` shipped on-by-default (2026-07-20) after an 8-pair / 121-gap validation
recorded as **0 divergent vs the fine reference**. This gap is divergent. Either that corpus did not
contain this shape, or something changed after it — resolving which is still useful population
context: `equivalence-calibration` (`src/bin/equivalence_calibration.rs`) diffs these two verdicts
per gap from `corpus.json` alone.

**Direction matters for severity.** That tool gates CI on the *dangerous* direction only — scan
drops while the reference keeps (a false drop / unrepaired hole). This gap is the **safe**
direction: scan keeps, reference drops. So `equivalence-calibration` would exit 0 on it, and no
audio is lost. Severity stays high on the strength of the 8× measurement disagreement and the
wrong operator-facing label, not on data loss.

**Operational consequence.** `skip_equivalent_gaps` is on by default and consumes the *scan-time*
verdict, so this gap is admitted to the fill plan as a `repairable_dropout`, runs the full bracket
search and dual-fit, and then hard-skips — while the fill-time analysis of the same gap says
`shared_silence, drop: true`. Wasted work, and an operator-facing label that is the opposite of the
truth.

This is why the parent ledger's §0 premise — two signals off the same B audio disagreeing — is
**not fully closed**. F1–F12 fixed the instances then in evidence, not the class.

**Next step.** Floor **(a)** is decided; silent-core A RMS should move with it; context / noise-floor
window is still open — see *What must be resolved first* and *Ready to implement*. Population check
(`equivalence-calibration` over multi-pair corpora) is still useful for "one gap or a class".

### The whole scan verdict set for this pair

Preserved because it is the population context, and because the source file is deletable. All 11
gaps, from `scan-postfix.json` / `fp6-scan.json` `gap_equivalence[]` (index-parallel to `gaps[]`):

| idx | class | `drop` | donor frac | `a_gap_rms_db` | `noise_floor_db` |
|---|---|---|---|---|---|
| 0 | `not_evaluated` | false | — | −100.78 | −43.28 |
| 1 | `repairable_dropout` | false | 0.0 | −101.49 | −46.74 |
| 2 | `repairable_dropout` | false | 0.0 | −101.47 | −34.56 |
| 3 | `shared_silence` | true | 0.9744 | −80.10 | −49.34 |
| 4 | `repairable_dropout` | false | 0.4737 | −86.41 | −44.86 |
| **5** | **`repairable_dropout`** | **false** | **0.10** | **−82.27** | **−45.85** |
| 6 | `shared_silence` | true | 0.5333 | −89.16 | −56.58 |
| 7 | `shared_silence` | true | 0.9 | −75.80 | −61.58 |
| 8 | `ambient_quiet` | true | 0.1667 | −80.79 | −61.58 |
| 9 | `repairable_dropout` | false | 0.0 | −101.75 | −46.59 |
| 10 | `shared_silence` | true | 1.0 | −101.47 | −56.63 |

Two things to read off it. The coarse `noise_floor_db` swings 27 dB across one recording
(−34.56 to −61.58), which is a lot for "the recording's noise floor" and may be the same
measurement weakness from the other side. And index 5 is not an obvious outlier *within* this
table — nothing about the scan-side row looks wrong until the fine verdict is put beside it, which
is why this only surfaced on a fingerprint run.

---

## F14 — Fingerprint `outcome` records `skip` where production patches (dual-fit rescues invisible)
**Severity: medium-high (calibration-oracle integrity). Status: FIXED AND MEDIA-VALIDATED
2026-07-30** — field wired, then flag refuted on media (A-border skew), then `splice_dualfit_at`
aligned to production's raw `mono(refined ± w)`, then **confirmed on a post-fix run of the same pair**
(`gap-files/silence-floor/fp_post_F14_fix/`): the flag flips to `true` on the rescued gap and
`trim_frames` lands on production's value exactly. Found 2026-07-30.

The gap at **1050.82 s**, fingerprinted and previewed from the **same binary with the same flags**:

| | fingerprint corpus | production (`--repair-preview`) |
|---|---|---|
| decision | `outcome.tier: skip` | `patched` |
| reason | `skip_reason: correlation_below_threshold` | `dual_fit_used: true`, `patch_tier: high`, `confidence: high` |
| seams | `splice_dualfit` 0.9972 / 0.9821, `gate_pass: true` | `pre 0.9947 / post 0.9821` |
| filename | `..._g001_full_timing_offset.json` | — |

As first recorded: seams look alike (`gate_pass: true` in the same file whose `outcome` said
`skip`), so the bug read as a **missing decision wire**. That wire is now in; the re-run shows the
remaining disagreement is **measurement** (step-real) — see below.

Two reasons this is not cosmetic:

1. The corpus is the **oracle** for calibration sweeps (17-pair fingerprint runs, gap-vocabulary
   analysis, the pre-gate work). A `skip` recorded where production patches biases every roll-up
   computed over it, and the effect is concentrated on exactly the dual-fit-rescued gaps that
   recent work targets.
2. Directory listings that treat the filename as a patch/skip tag can mislead — see the filename
   correction below.

**On the `any_ok` note.** Earlier write-ups treated the `--repair-preview` help as claiming the
fingerprint path is *more permissive*, then argued with that. It claims no such thing. The exact
text (`infrastructure/cli/args.rs:131-133`) is: "Characterize planned gaps with the production patch
gate … **Not the same as `--gap-fingerprints` (fingerprint `any_ok`)**." It is a *distinguishing*
note — the two paths use different gates — not an ordering. Nothing in it needs explaining away.

**But that same help text limits the evidence.** `--repair-preview` is documented as
"**pass-1 only; no anchored retry**". So preview is ground truth for pass-1 production, not for
production overall — a gap that preview reports as `patched` is patched in pass 1, and a gap it
reports as skipped might still be rescued on a retry preview never runs. For F14 this cuts the
harmless way (preview *patched*, corpus *skipped*), so the finding stands. Do not reuse preview as a
general production oracle without re-reading that caveat.

### Source diagnosis (2026-07-30) — why the wire was needed

**Original read: missing decision wire.** Fingerprint `outcome.tier` is set solely from bracket-gate
`any_ok` inside `compute_region_measurements`. After all brackets fail → `tier: "skip"`.
`splice_dualfit_at` runs after that; its `gate_pass` was published and not consulted for `outcome`.
The from-decode dump does **not** run the production patch gate (`characterize_gaps_from_decode`:
"Keeps fingerprint semantics — does NOT run the production patch gate"). Production calls
`skip_or_dual_fit` → may `try_dual_fit` → `patched` with `dual_fit_used: true`.

`dual_fit_eligible` excludes only `StructureAlignmentFailed`. Wiring an additive
`dual_fit_rescue` beside `tier` (not mutating `tier`) was the right shape — see *Implemented*.
**Post-wire:** the flag can still disagree with production when the dump's dual-fit *inputs* differ;
that is the re-run finding, not a reason to re-tier.

**Filename correction.** `entry_verdict` prefers the **lag** verdict over `outcome.tier`. The
`..._full_timing_offset.json` tag is `LagVerdict::TimingOffset`, not a skip claim. Retract the
earlier claim that the filename “encodes the same wrong verdict.”

**Design constraint — additive, not a `tier` mutation.** Do **not** rewrite `outcome.tier` to say
`patch` when dual-fit would rescue. `tier` is contractually the `any_ok` bracket-gate result
(`characterize_gaps_from_decode`: "Keeps fingerprint semantics — does NOT run the production patch
gate"), the corpus goldens are built on it, and overloading it would destroy the one axis that
currently means something precise. `golden_baseline` keys on `outcome.tier` as Tier-1 exact-compare;
three committed tests break on any `tier` change (`curated_golden_baseline_invariance`,
`projection_preserves_curated_golden_baseline`, `decode_path_projection`).

### Implemented 2026-07-30 — additive field + full `try_dual_fit` conjunction

Shipped: `GateOutcome.dual_fit_rescue`, roll-ups via `GapRow::production_patched()`, carve-out in
[gap-vocabulary.md](gap-vocabulary.md). An early draft that keyed only on eligible failure class +
`gate_pass` was **wrong and dangerous** — it reported `Some(true)` for `04_program_quiet` (seams
~0.998, dead donor), the cell production declines. The shipped `dual_fit_rescue_flag`
(`gap_fingerprint/schema.rs`) models all of `try_dual_fit`'s accept conditions:

1. dual-fit-eligible failure class (`brackets_dual_fit_eligible` — the per-bracket analogue of
   production's `StructureAlignmentFailed` carve-out);
2. `splice_dualfit.gate_pass`;
3. the step is **real** — `post_seam_r − post_seam_global_r ≥ DUALFIT_STEP_REAL_MARGIN` (0.15), so a
   rigid single-lag map doesn't already explain the seam;
4. the **aligned** donor is `continuous`;
5. the **nominal** donor is not program-quiet (`silence_fraction < PROGRAM_QUIET_SILENCE_FRAC`, 0.5).

Same conjunction `gap_repair_spec::classify_bracket_exhausted_skip` uses for the `SilenceSplice`
cell. `None` means *no claim* (already patched, or an input missing), never a defaulted `false`.
Pinned by unit tests in `schema.rs` and by `04_program_quiet`'s curated golden row reading `false`.

**Fixture note.** `dual_fit_rescue` is *derived*, so projection emits it while pre-field fixtures
read `null`. Handled by `tests/curated_fixture_backfill.rs` (`CURATED_FIXTURE_BACKFILL=1`) then
`CURATED_GOLDEN_REGEN=1` — any future derived fingerprint field should use that path, not a golden
exclusion.

**Where wired.** `measure.rs` (from-decode) and `project.rs` (projection) share one helper; carried
on `GapRow.dual_fit_rescue`; Tier-1 in `golden_baseline`; `GapRow::production_patched()` =
`patched() || dual_fit_rescue == Some(true)` (`patched()` stays the `any_ok` axis). Corpus report
counts rescues as their own term (with a pre-flag fallback line).

**Still predictive, not observed.** Assumes `--dual-fit` is on; models the decision from
fingerprint-side measurements.

### Re-run 2026-07-30 — the flag is **wrong on its own ground-truth gap**

A second full-pair fingerprint run over the same pair was checked against the same
`--repair-preview` log. **The flag reads `false` on the 1050.82 s gap that production rescues.** So
the "one real-media confirmation" claimed above is retracted: it is a refutation.

| gap | flag | production | which condition fails |
|---|---|---|---|
| 1050.82 s (`g1`) | `false` | **rescued** (`trim=-9`, `confidence=High`) | **step-real only** — 0.9821 − 0.8646 = 0.1175 < 0.15 |
| `g3`, `g5`, `g6` | `false` | declined | aligned donor not `continuous` |

Conditions 1, 2, 4 and 5 hold on `g1`; the seam gate passes. Only the step-real term (3) misses, and
it misses by 0.025. Every *correct* `false` in the run is carried by donor continuity — a robust
bit — so on this evidence step-real is the sole fragile term. **n = 2 comparisons, 1 wrong.**

**Cause — and a retraction.** The first analysis blamed the anchor: "the dump re-anchors on nominal
`b_mapped` and production doesn't." That is **wrong**. `dual_fit.rs:99` says *"Fit each shoulder at
its seam-local lag around the **NOMINAL** b_mapped"* — `pre_start = b_mapped_start.checked_sub(w)`,
`b_post_nominal = b_mapped_start + gap_frames`. Both paths re-anchor on nominal, identically.

The real skew is the **A-border construction**:

| | A borders |
|---|---|
| production (`patch_audio/region.rs:476-477`) | `mono(refined.start_frame − w, refined.start_frame)` — raw |
| dump (`gap_fingerprint/measure.rs:1079`) | `border_templates_for_gap` → silence skip + `trim_low_energy_suffix`/`_prefix` (`policies/gap_borders.rs:236-237`) |

The evidence signature fits that and little else: `post_r` agrees to five decimals (0.9821396 vs
0.9821346 — the post trim did not bite) while `pre_r` differs in the third (0.99716 vs 0.99473) and
lands ~256 frames away (`trim_frames=247` vs preview `trim=-9`). Same post template, different pre
template ⇒ different pre lag ⇒ different `post_seam_global_r` ⇒ the step-real difference flips.

**This also kills the obvious patch.** "Score the global term at production's shoulder" is circular:
the shoulder *is* `b_mapped_start + pre_lag`, and `pre_lag` comes from correlating the A pre-border.
The shoulder cannot be matched without first matching the border. The two live options are

- ~~**align the dump's dual-fit A borders with `try_dual_fit`'s raw `mono(refined ± w)`**~~ **DONE
  2026-07-30** — `splice_dualfit_at` now uses the same range guard and `interleaved_to_mono` slices
  as `build_dual_fit_input`; the `border_templates_for_gap` path is gone from this call site only
  (other fingerprint seam probes still use templates).
- **return `None` near the step-real margin** — not taken; honesty patch, weaker, left unused.

**Claim after the fix.** The flag applies production's rule to **matching A-border construction**.

### Post-fix re-run 2026-07-30 — validated, and the skew was pair-wide

Same pair, post-fix binary (`gap-files/silence-floor/fp_post_F14_fix/`), against the same
`--repair-preview` numbers. The prediction held on `g1`:

| | pre-fix dump | post-fix dump | production preview |
|---|---|---|---|
| `trim_frames` | 247 | **−9** | **−9** |
| `pre_seam_r` | 0.99716 | **0.994735** | 0.9947 |
| `post_seam_r` | 0.98214 | 0.982135 | 0.9821 |
| `post_seam_global_r` | 0.86460 | **0.829133** | — |
| step-real | 0.1175 ✗ | **0.1530 ✓** | accepts |
| `dual_fit_rescue` | `false` | **`true`** | rescued |

`trim_frames` matching `−9` **exactly** is the load-bearing evidence: that is a frame-count, not a
correlation that happens to land close. The seam correlations agreeing to four decimals is
corroboration, not the proof — `post_r` already agreed pre-fix.

**The pre-fix skew was pair-wide, not specific to `g1`.** Every gap carrying a `splice_dualfit` block
moved:

| idx | `post_seam_global_r` | `trim_frames` |
|---|---|---|
| 3 | −0.069 → **0.993** | 2561 → **−1** |
| 4 | −0.014 → **0.936** | 7013 → **−1** |
| 6 | −0.096 → **0.995** | 571 → **−1** |
| 7 | −0.033 → **0.997** | 8102 → **−1** |
| 8 | 0.008 → **0.874** | 3130 → **−1** |

Pre-fix the trimmed pre-template mis-placed the pre shoulder on essentially the whole pair, yielding
phantom bridges up to 15462 frames (≈322 ms of step that is not there) and a near-zero global. The
post-fix rows read `trim = −1` with the global ≈ the own-lag post — internally consistent, since a
rigid single-lag map means post@pre ≈ post@own. **This makes the earlier framing an understatement:
the A-border skew was not a marginal effect on one near-threshold gap, it was a systematic placement
error that only *surfaced* as a flag flip on `g1`.**

**Two errors were cancelling on `g3` and `g6`.** Pre-fix the flag read `false` there for the wrong
reason: step-real was spuriously **true** (post_r − ≈0 ≈ 0.99) and only the donor-continuity term
produced the correct verdict. Post-fix both read step ≈ 0.001 — correctly *not* a step — and still
decline on the donor. The `false`s are now right on their merits. Anyone reading a pre-fix corpus
should treat `splice_dualfit.post_seam_global_r` and `trim_frames` as **unusable**, not merely noisy.

**No regression from the new range guard.** `g0` and `g10` carry no `splice_dualfit` in *either* run,
so the early `return None` did not newly drop a measurement.

**One caveat to carry.** `g1` clears step-real by **0.0030** (0.15300 vs the 0.15 margin). The
agreement with production is structural rather than lucky — both now compute the same quantity from
the same borders, so they move together — but the gap sits close enough to the margin that a
re-encode could flip both at once.

**Two further parity edges — found in review, fixed 2026-07-30.** Neither fired on this pair (all
`bridge_frames > 0`, no non-finite globals), so the media run does not validate them; they were fixed
by reading `try_dual_fit` rather than by measurement.

1. **Shoulder crossing.** `try_dual_fit` declines when `b_post_seam <= b_pre_seam`
   (`dual_fit.rs:157-164`); the flag did not model it. Now `df.bridge_frames > 0`.
2. **NaN inversion.** `finite_corr` mapped non-finite → `0.0`, inverting production **both** ways: on
   step-real production's `partial_cmp` returns `None` and declines while the flag accepted
   (`post_r − 0.0 ≥ 0.15`), and on `gate_pass` production passes (`NaN < floor` is false) while the
   flag failed. Dual-fit correlations are now kept **raw** in `splice_dualfit_at`, `gate_pass` uses
   production's `!(smin < floor)` form, and `step_real` uses the same `partial_cmp`/`is_some_and`
   shape. `SpliceDualfit`'s three correlation fields carry `ser_nan_as_null`/`de_null_as_nan` so a NaN
   round-trips through the corpus instead of failing or defaulting.

`normalized_correlation` returns NaN on a zero-variance window — digital silence — which is exactly
the neighbourhood these gaps occupy, so (2) was reachable in principle even though this pair never hit
it.

**What is *not* yet recorded here.** The `outcome` block's other fields for this gap (beyond `tier`
and `skip_reason`) were not transcribed, and the corresponding `preview-debug.log` lines were read
but not quoted. If `gap-files/` is cleared before a fix is validated, re-running
`--fingerprint-gap 2` plus a `--repair-preview` restores both — the two commands in the
reproduction section. The claim that the two runs used the same binary and flags rests on that
being done in one session; a fresh validation should re-establish it rather than assume it.

---

## What must be resolved first

### Floor question — ANSWERED 2026-07-30: **(a)**

Should `gap_floor_db` — the threshold "is the donor quieter than the hole we're filling?" — be

- **(a)** the loudest **silent** block in A's gap (coarse today; F2/R1; immune to hold-bridge /
  edge-refinement contamination), or
- **(b)** the loudest content anywhere in A's gap span (fine today; no silence filter)?

**(a).** Direction (b) biases toward `shared_silence` / `drop` — the dangerous direction the CI gate
exists to catch. Evidence from `fp_F15_question/` (ten characterized gaps, both floors recorded):

*Self-consistent.* (b) takes the max over **all** bins — by construction a non-silent number — and
uses it as a silence threshold. (a) takes the max over A's *silent* blocks, which is a floor.

*Circularity resolves for (a).* On `g4`, anchored `di_rms = −67.16`: silent under fine's floor
(−58.39), **not** silent under scan's (−79.50). The "patch fills silence — drop it" reading was an
artifact of (b); dropping `g4` would lose a real repair.

**But (a) alone does not close F15.** `gap_floor_db` is not an input to `classify_gap_equivalence` —
it reaches the verdict only through `donor_silence_fraction`. Fixing it moves fine from
`shared_silence` to `ambient_quiet`; both are `drop = true`, against scan's `repairable_dropout` /
keep:

| gap | scan A vs NF | fine A vs NF | fine `is_dropout` (margin 35) |
|---|---|---|---|
| `g4` | −86.41 vs **−44.86** → dropout | −66.88 vs −54.21 | `−66.88 < −54.21 − 35`? **no** |
| `g5` | −82.27 vs **−45.85** → dropout | −60.68 vs −64.83 | `−60.68 < −64.83 − 35`? **no** |

**Three sensor mismatches, each sufficient alone to flip `g4`/`g5`:**

1. **`gap_floor_db`** — silent-blocks max vs whole-span max. Drives the donor axis. **Answered: (a).**
2. **A gap RMS** — scan aggregates A's **silent blocks** over the core; fine takes
   `gap_interior_rms_db` over the whole refined span, unfiltered.
3. **Noise floor / context window** — ±2 s / 100 ms blocks vs ±3 s / 50 ms bins. On `g4`: scan
   −44.86, fine −54.21 (**9.4 dB**). On `g5`: scan **−45.85**, fine −64.83.

(2) is the same policy as (1) — "silent core or whole refined span?" — applied to another output.
They should move together. (3) is separate and **decisive alone**: hold scan's A RMS and swap in
only fine's noise floor → not a dropout on both gaps
(`−86.41 < −54.21 − 35` false; `−82.27 < −64.83 − 35` false).

**Net.** Adopt **(a)** for floor and carry it to fine's `a_gap_rms` (silent-core). That closes the
donor axis and part of the A-side split. The **context / noise-floor window** remains open — do not
claim F15 closed until it is decided (or deliberately accepted).

**Corroborating separation.** Scanner abs silence threshold −59.94 dBFS. Fine floor exceeds it on
exactly `g4`, `g5`, `g6`, `g8` (divergent / near-divergent); the other six agree. Class splits:
`g4`/`g5` keep vs drop, `g8` reason only. Control `g3`: floors differ 2.7 dB, donors agree to 0.001.

### Already done (2026-07-30)

1. ~~Emit `gap_floor_db` + silent/total block counts on `GapEquivalenceVerdict`.~~ Provenance via
   `with_scan_provenance` / `with_gap_floor_db`; `Default` on the verdict types.
2. ~~`equivalence_calibration.rs` premise.~~ Fine is a **second opinion, not an oracle**.
3. ~~Stale "250 ms" join comments.~~ Name `scan_block_ms`; `default_scan_block_ms` note demoted from
   invariant.
4. ~~F14 additive wiring.~~ `dual_fit_rescue` + `production_patched()` + vocabulary carve-out +
   fixture backfill (see F14 *Implemented*), then the A-border alignment that makes the flag
   correct on media (see F14 *Post-fix re-run*).

**`outcome.tier` blast radius:** Tier-1 exact-compare in `golden_baseline`; do not re-tier — stay
additive.

### Ready to implement

| Work | Status | Notes |
|---|---|---|
| **F14:** `splice_dualfit_at` A borders → raw `mono(refined ± w)` like `try_dual_fit` | **Done + media-validated** | `fp_post_F14_fix/`: `g1` flag `true`, `trim_frames −9` = production. |
| **F14 residual:** `bridge_frames > 0` + NaN-aware step-real/`gate_pass` (no `finite_corr` on dual-fit scores) | **Done** | Fixed from source, not measurement — neither edge fired on this pair. |
| **F15:** fine `gap_floor_db` + A RMS → silent-core **(a)** | **Decided; needs design** | Fine has no `BlockLevel.silent` — pick a silence filter (abs-floor / peak test on bins, or reuse scan signals) before coding. Move floor and A RMS together. |
| **F15:** noise-floor / context window (±2 s/100 ms vs ±3 s/50 ms) | **Open** | Decisive alone on g4/g5. Do not claim divergence closed until decided or accepted. |
| Naive "coarse adopts fine's floor" | **Do not** | Re-imports pre-F2 contamination into the path that makes plan decisions. |

---

## Reproducing these runs

Cost two failed runs to rediscover, so it is recorded here as well as in the run-protocol note.

- Build with **`--features calibration,he-aac`**. `calibration` gates the `--gap-fingerprints` /
  `--fingerprint-gap` / `--fingerprint-diagnostics` flags (otherwise
  `unexpected argument '--gap-fingerprints'`). `he-aac` is required **even for plain AAC-LC media**:
  `codec_registry.rs:34-47` registers *any* AAC decoder only under `he-aac`/`ac3`, so a `default =
  []` build fails with `alignment failed: no decodable audio tracks` — presenting as exit 0, an
  empty scan JSON, and zero fingerprints.
- **One pair at a time.** Peak RSS is ~15 GB for a ~2.3 h 5.1/48 kHz pair (characterization
  materializes the whole B track: `secs × rate × channels × 4 B`). Two concurrent runs OOM with
  `memory allocation of N bytes failed`.
- Recipe knobs need no flags — the defaults (`min_gap` 500 ms, block 100 ms, rms 33/32767, hold
  500 ms) already equal the 2026-07-26 reference `scan_recipe`. Pin `--silence-hold-ms 500`
  explicitly anyway, because the manifest's `scan_recipe` does not record it (that is F11, tracked
  in [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md)).
- `--fingerprint-gap N` is **1-based** on the gap-table `#`, and emits **0-based** filenames.
- Use `RUST_LOG=debug`, not `RUST_LOG=clip_sync_repair=debug`, when the question might involve the
  `clip_sync` crate (alignment, seek, decode) — the narrower filter hides those errors.

Both findings above are reproducible from one command per finding: `--fingerprint-gap 6` for F15,
`--fingerprint-gap 2` plus a `--repair-preview` run for F14.
