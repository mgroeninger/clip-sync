# Equivalence divergence — open findings ledger

**Opened:** 2026-07-30. **Status:** both findings OPEN, both **diagnosed in source** (2026-07-30);
neither fixed.

Split out of [archive/TEMP-silence-floor-findings.md](archive/TEMP-silence-floor-findings.md) when
that ledger was archived (2026-07-30). Everything else in it is closed; these two are not, and both
came out of its §5 follow-up rather than its original F1–F12 sweep. Finding IDs **F14/F15** are kept
from the parent ledger so its text still resolves.

Originally recorded from **measurement only**. Source tracing (2026-07-30) confirmed both: F15 is a
**threshold/window-definition** split between the two equivalence front-ends (shared classifier,
incompatible sensors); F14 is a **missing decision wire** — fingerprint `outcome` never includes the
dual-fit rescue production applies after bracket failure. Hypotheses below that were confirmed are
marked as such; one F14 claim about the filename was **partially retracted**.

**Reference audit, 2026-07-30 (second pass).** Every `file:line` in both findings was re-read
against source and all resolve, including the quoted doc comment at `measure.rs:2086` and the
filename retraction at `:2392`. Three things changed as a result, all recorded in place: F15's
"scan gap floor ≈ −82" was an eyeball and is now a **derived bound** (≤ −71.9 dB — the conclusion
holds, the reasoning did not); F15 gained a **third axis of divergence** (donor *window*, not just
predicate and bin size); and F14's `any_ok` paragraph was **arguing with a misreading** of the CLI
help and is now corrected, with the `--repair-preview` pass-1 caveat that limits what the preview
evidence proves.

Media: an uncatalogued licensed 5.1 pair (A ≈ 6900 s, AAC-LC 48 kHz 5.1). Per the media-hygiene
rule the pair is referred to only by these properties; timestamps are numeric, raw logs stay in
gitignored `gap-files/`.

**Verification rule.** Re-read any `file:line` reference before acting on it — the references below
were read 2026-07-30.

## Where the data lives — and why it is inlined here

Artifacts sit under gitignored `gap-files/silence-floor/`: `fp6/` (F15's gap, one fingerprint),
`fp/` (F14's, two fingerprints), `scan-postfix.json`, `preview-debug.log`, plus a `*-scan.json` and
`*.log` per run. **`gap-files/` is ephemeral and deletable** — it is licensed-media-derived and not
a durable reference. Every number these findings depend on is therefore quoted inline below, so the
findings survive the directory being cleared. Re-deriving them costs one ~15 GB run per finding (see
the reproduction section).

The pair is the same one throughout: gap indices are **0-based in filenames, 1-based in the gap
table and in `--fingerprint-gap`**. F15's gap is table `#6` / file `g005`; F14's is table `#2` /
file `g001`.

---

## F15 — Scan-time and fill-time equivalence disagree on the same gap, post-fix
**Severity: high. Status: OPEN (diagnosed), found 2026-07-30; source diagnosis 2026-07-30.**

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

### Which floor is right is now the open question — and the answer may be the opposite

The two floors are not two granularities of one definition. They are different statistics:

| | coarse `gap_floor_db` | fine `gap_floor_db` |
|---|---|---|
| source | `domain/gap_equivalence.rs:234-237` | `gap_fingerprint/measure.rs:59-68` |
| set | max RMS of A's **silent** in-gap blocks | max RMS over **all** bins in the gap span |
| silence filter | **yes** (the F2/R1 fix) | **none** |
| value here | ≤ −71.9 (bounded, below) | −51.03 |

The fine floor is computed with **no silence filter**, so it is inflated by exactly the
edge-refinement and hold-bridged content that the parent ledger's **F2** identified and fixed — on
the coarse path only. A higher floor makes more of B count as silent, pushing toward
`shared_silence` and `drop: true`, which is the **dangerous** direction.

That inverts the working assumption. This document (and `equivalence-calibration`) has been treating
fine as ground truth; on the one axis where the parent ledger already settled the semantics, the
**coarse** path carries the fixed definition and the fine path carries the pre-F2 shape. The donor
here sits at −55.87 dB: below A's *unfiltered* gap content (−51.03) but ~16 dB above A's *silent*
floor. Both readings are internally consistent; they answer different questions.

**Nothing below should be actioned until that is resolved** — see "What must be resolved first".

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

- **Coarse / `scan_equivalence`:** `domain/gap_equivalence.rs:218-275` (`derive_gap_equivalence`)
  — A RMS from **scanner-silent blocks only** over the **core** interval; noise floor = median of
  scan blocks in **±2 s** (`EQUIVALENCE_CONTEXT_SECS`); donor fraction from scanner `BlockLevel`s,
  counting a block silent when `b.silent || rms_db < scan_gap_floor` (`:256-271`).
- **Fine / `equivalence`:** `application/gap_equivalence.rs:43-53` (`measure_gap_equivalence`) —
  A RMS = full refined PCM span (`gap_interior_rms_db`); noise floor = `fp.levels.noise_floor_db`
  (median of **50 ms** bins in **±3 s**, `gap_signature_context_secs`); donor fraction is
  **precomputed** from `donor_interior_nominal` (`domain/donor.rs:donor_interior_at` — pure
  `rms < fine_gap_floor`, no `silent` bit).
- The join: `measure.rs:2263-2280`, where `equivalence` is computed and `scan_equivalence` is copied
  in by positional index.

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

The third leg — that `rms_db < scan_gap_floor` also fails — needs care, because **the scan gap
floor is not recorded anywhere.** `scan_equivalence` carries no floor field, so it cannot be read
off the artifacts; it has to be bounded from what is recorded:

> `gap_floor_db` is the **max** RMS of A's silent in-gap blocks
> (`domain/gap_equivalence.rs:234-237`) and `a_gap_rms_db` is the **energy mean** of that same set
> (`aggregate_rms_db`, `:177-192`). For `N` blocks, `max ≤ mean_db + 10·log₁₀(N)`. The gap is
> 1.1437 s at 100 ms blocks ⇒ `N ≤ 11` ⇒ **scan gap floor ≤ −82.27 + 10.4 = −71.9 dB**.
> The donor at −55.87 dB is ≥ 16 dB above that ceiling, so `rms_db < scan_gap_floor` cannot fire.

The conclusion survives, and now on an arithmetic bound rather than an eyeball. Anyone re-deriving
this should keep the distinction: the fine floor (−51.03) is measured, the scan floor is bounded.

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
(`measure.rs:2280` is a positional lookup). Scan report entry `[5]` reads
`repairable_dropout / 0.1 / −82.27 / −45.85` — exactly the values in the fingerprint's
`scan_equivalence`. The arrays are index-parallel and the copy is correct.

**Note the stale comment** at `measure.rs:2278`: "the coarse **250 ms** scan-block verdict". This run
used `scan_block_ms: 100`. Probably just rot, but it is in the one comment describing this join.

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

**Next step.** Decide which sensor definition the plan-time gate should use (align scan donor /
A-side predicates with the fine reference, or accept the divergence and stop treating fine as
ground truth for `skip_equivalent_gaps`). Cheap population check first:
`equivalence-calibration` over existing multi-pair corpora — "one gap or a population".

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
**Severity: medium-high (calibration-oracle integrity). Status: OPEN (diagnosed), found 2026-07-30;
source diagnosis 2026-07-30.**

The gap at **1050.82 s**, fingerprinted and previewed from the **same binary with the same flags**:

| | fingerprint corpus | production (`--repair-preview`) |
|---|---|---|
| decision | `outcome.tier: skip` | `patched` |
| reason | `skip_reason: correlation_below_threshold` | `dual_fit_used: true`, `patch_tier: high`, `confidence: high` |
| seams | `splice_dualfit` 0.9972 / 0.9821, `gate_pass: true` | `pre 0.9947 / post 0.9821` |
| filename | `..._g001_full_timing_offset.json` | — |

The measurements agree; only the **recorded decision** disagrees. The corpus even carries
`splice_dualfit.gate_pass: true` in the same file whose `outcome` says `skip`, so the dual-fit
rescue was measured and then not reflected in the outcome axis.

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

### Source diagnosis (confirmed 2026-07-30)

**Missing decision wire, not a measurement disagreement.** Fingerprint `outcome.tier` is set solely
from bracket-gate `any_ok` inside `compute_region_measurements`
(`gap_fingerprint/measure.rs:1834-1904`). After all brackets fail, `patched = false` →
`tier: "skip"` / `skip_reason: "gate skipped"`. `splice_dualfit_at` runs **after** that
(`:1992-2000`); its `gate_pass` is published on the fingerprint and never consulted for `outcome`.

The from-decode dump then projects that skip into a placeholder
`GapPatchSkipReason::CorrelationBelowThreshold` (`measure.rs:2204-2213`) and explicitly does **not**
run the production patch gate (`characterize_gaps_from_decode` doc at `:2084-2086` — "Keeps
fingerprint semantics — does NOT run the production patch gate").

Production, after the same class of bracket/seam failure, calls `skip_or_dual_fit`
(`patch_audio/region.rs:595-607`), which may `try_dual_fit` and emit `patched` with
`dual_fit_used: true` (`finalize_dual_fit` / SilenceSplice arm).

**The eligibility predicate is trivial, and that makes the fix concrete.**
`dual_fit_eligible` (`region.rs:532-534`) is the whole rule:

```rust
request_dual_fit && !matches!(fail, SeamGateFailure::StructureAlignmentFailed)
```

Every scored-but-failed seam-gate variant qualifies; only `StructureAlignmentFailed` — where no
bracket ever scored, so there is nothing to rescue — does not (pinned by
`dual_fit_eligible_excludes_structure_alignment_failed`, `region.rs:2527-2547`). The fingerprint
path already knows which failure class it hit and already computes `splice_dualfit.gate_pass`, so
predicting the production disposition needs **no new measurement** — it is a wiring change over
values the dump has in hand. That moves "wire dual-fit into the decision axis" from aspiration to a
bounded edit, and it is the strongest argument for fixing rather than documenting the carve-out.

So for this gap: brackets fail → fingerprint records skip; dual-fit still clears
(`gate_pass: true`); production rescues. Same class of intentional fingerprint limitation already
documented in [gap-vocabulary.md](gap-vocabulary.md) (outcome from seam scoring only;
residual-veto and related paths “not fingerprint-representable”). Dual-fit rescue is another
production disposition the dump does not represent on the `outcome` axis.

The comment at `measure.rs:584-588` that dual-fit validators are **published rather than acted on**
is exactly this design, applied to the diagnostic fields — and it extends to `outcome` itself.

**Filename correction.** `entry_verdict` (`measure.rs:2390-2398`) prefers the **lag** verdict over
`outcome.tier`. The `..._full_timing_offset.json` tag is `LagVerdict::TimingOffset`, not a claim
that the gap was skipped. The misleading recorded decision is `outcome` (and any roll-up that keys
off it); the lag tag in the filename is a separate, accurate axis. Retract the earlier claim that
the filename “encodes the same wrong verdict.”

**Recommended change — additive, not a `tier` mutation.** Do **not** rewrite `outcome.tier` to say
`patch` when dual-fit would rescue. `tier` is contractually the `any_ok` bracket-gate result
(`measure.rs:2084-2086`, "Keeps fingerprint semantics — does NOT run the production patch gate
(pre-flip review Finding 1)"), the corpus goldens are built on it, and overloading it would destroy
the one axis that currently means something precise. Instead:

1. Add a **separate field** to `GateOutcome` — e.g. `dual_fit_rescue: Option<bool>` — set after
   `splice_dualfit_at` (`measure.rs:1992`) when `!any_ok`, the failure class is dual-fit-eligible,
   and `splice_dualfit.gate_pass` is true. No new measurement: all three values are already in hand
   at that point, and `dual_fit_eligible` (`region.rs:532-534`) is a one-line predicate over the
   failure class.
2. Point the **roll-ups** that ask "did production patch this?" at `tier || dual_fit_rescue` rather
   than `tier` alone. This is where the oracle bias actually lives.
3. Add the carve-out to [gap-vocabulary.md](gap-vocabulary.md) beside the existing residual-veto
   "not fingerprint-representable" note, so the axis's limits are stated in one place.

**Verified 2026-07-30 (this was flagged "check before implementing"):** `golden_baseline` **does**
key on `outcome.tier`, as a Tier-1 *exact-compare* axis — `golden_baseline.rs:27` / `:76` /
`:236 tier1!(tier)`, sourced from `analysis.rs:507` and frozen in
`crates/clip-sync-repair-harness/golden/curated.golden.json`. The curated fixtures carry
`outcome.tier` and reach that comparison via `curated_gap_cell_rows()`; `check.rs:281-409`
separately cross-checks `tier` against the filename and the gate-Ok bracket count. Three committed
tests fail on any `tier` change: `curated_golden_baseline_invariance`,
`projection_preserves_curated_golden_baseline`, `decode_path_projection`. **This settles the
additive-vs-mutation question in favour of additive** — re-tiering is not a judgement call here, it
breaks the frozen decision contract.

Still unverified: whether adding a field to `GateOutcome` requires regenerating the committed
`tests/gap_corpus/fingerprints/curated/*.json`. The `gap_floor_db` work above is the precedent —
four additive `Option` fields with `skip_serializing_if`/`default` required **no** fixture
regeneration and left all four golden/fixture tests green — but that was on `GapEquivalenceVerdict`,
not `GateOutcome`, so treat it as a strong prior rather than a result.

**What is *not* yet recorded here.** The `outcome` block's other fields for this gap (beyond `tier`
and `skip_reason`) were not transcribed, and the corresponding `preview-debug.log` lines were read
but not quoted. If `gap-files/` is cleared before a fix is validated, re-running
`--fingerprint-gap 2` plus a `--repair-preview` restores both — the two commands in the
reproduction section. The claim that the two runs used the same binary and flags rests on that
being done in one session; a fresh validation should re-establish it rather than assume it.

---

## What must be resolved first

**One blocking question, and it is not answerable from the artifacts:** should `gap_floor_db` — the
threshold "is the donor quieter than the hole we're filling?" is measured against — be

- **(a)** the loudest **silent** block in A's gap (coarse today; the F2/R1 definition, immune to
  hold-bridge and edge-refinement contamination), or
- **(b)** the loudest content anywhere in A's gap span (fine today; no silence filter)?

They differ by ~20 dB on this gap and that difference alone flips the verdict. It is a semantic
choice about what the gate is asking, not a bug to be found by more reading. Deciding it wrong in
direction (b) biases toward `shared_silence` / `drop` — dropping repairable gaps, the direction the
CI gate exists to catch.

Everything substantive in both findings sits downstream of it: aligning donor predicates, aligning
windows or bins, changing what `skip_equivalent_gaps` consumes, and any recalibration of
`equivalence-calibration`'s reference. Do not start those until (a)/(b) is settled.

### Safe to do now — independent of that decision

1. ~~**Emit `gap_floor_db` (and the silent/total block counts) in `scan_equivalence`.**~~ **DONE
   2026-07-30.** `GapEquivalenceVerdict` gained four `Option` provenance fields — `gap_floor_db`,
   `a_gap_silent_blocks`, `donor_silent_blocks`, `donor_total_blocks` — all
   `skip_serializing_if = "Option::is_none"`, so existing corpora deserialize unchanged. The coarse
   path fills them via `with_scan_provenance` in `derive_gap_equivalence`; the fine path records its
   own (differently-defined) floor via `with_gap_floor_db` in `measure.rs`. **The next fingerprint
   run reads the floors instead of bounding them** — the arithmetic bound above becomes checkable.
   `GapEquivalenceVerdict`/`GapEquivalenceClass` now derive `Default` (`NotEvaluated`, the only
   variant that asserts nothing about the audio) so further provenance fields don't break callers.
2. ~~**Fix the false premise in `src/bin/equivalence_calibration.rs:1-6`.**~~ **DONE 2026-07-30.**
   Replaced with the five-row input table (A RMS / noise floor / donor window / donor predicate /
   `gap_floor`) and the conclusion that **fine is a second opinion, not an oracle** — a divergence is
   not by itself proof the scan path is wrong.
3. ~~**Fix the stale comment at `measure.rs:2278`**~~ **DONE 2026-07-30.** Both stale "250 ms"
   comments now name the `scan_block_ms` recipe knob rather than any literal, since the value is
   configurable and the literal is what went stale. `default_scan_block_ms` in `config.rs` is
   annotated that its "coarse and fine agree" note held for that corpus, not as an invariant.
4. **F14's wiring** (below) — it is orthogonal to the floor question and can proceed. **Not yet
   done.**

**Blast-radius check for anything that later touches `outcome.tier` (verified 2026-07-30):
`tier` *is* a Tier-1 exact-compare axis** — `golden_baseline.rs:27` / `:76` / `:236 tier1!(tier)`,
fed from `analysis.rs:507` (`gap.outcome.tier`) and frozen in
`crates/clip-sync-repair-harness/golden/curated.golden.json`. Three committed tests break on any
`tier` change: `curated_golden_baseline_invariance`, `projection_preserves_curated_golden_baseline`,
`decode_path_projection`. The curated fixtures themselves carry `outcome.tier` (e.g.
`07_shared_silence.json`) and reach that comparison through `curated_gap_cell_rows()`; separately
`gap_fingerprint_corpus/check.rs:281-409` cross-checks `tier` against filename and gate-Ok bracket
counts. This is the concrete reason F14's fix must stay **additive** rather than re-tiering.

### Deliberately not recommended yet

Aligning the two donor predicates, which is the change this document would have recommended before
the floor asymmetry surfaced. Done naively — coarse adopts fine's floor — it would import the
pre-F2 contamination into the path that actually makes decisions, re-opening a finding the parent
ledger closed.

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
