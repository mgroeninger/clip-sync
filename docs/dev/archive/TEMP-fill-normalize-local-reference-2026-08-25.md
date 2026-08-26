# TEMP — Fill normalize against a non-local reference: corpus sizing + ear labels (2026-08-25)

**Status:** findings note, not a Change, 2026-08-25. **Archived 2026-08-26** under
[../README.md](../README.md) § *Findings notes* — a note lives in `docs/dev/` only while it has an
open question, and all four of this one's were promoted or closed that day (see § *What survives*).
Sizes the exposure behind [BACKLOG.md](../../../BACKLOG.md) § *Fill normalize boosts against a
non-local reference*, which until now rested on a single ear-confirmed gap (19/g13) and explicitly
forbade designing from n = 1. **This note proposes no change**; it supplies the count that item asked
for, plus four ear labels.

> **Read § *What survives* before quoting the design half.** Results 1–5 are measurement and stand.
> Result 6, the second headline ("the cap has to apply to every boost"), and Q2/Q4 argue for a cap on
> `compute_fill_gain`'s RMS ratio — a fix **nobody is building**. The local reference is no longer
> consulted by default (`normalize_fill` off, 2026-08-26) and its replacement is a paired collar
> residual, not a cap. Do not mine the ~4.5 dB corridor for a threshold.

**Headline.** Of 308 spliced gaps, 199 (65%) are already left at unity by the boost-only policy and
109 are boosted. Of the 8 boosted gaps whose fill reads hotter than +3 dB at the seams, **6 would sit
at or below their own shoulders at unity** — the entire excess is our gain, not a loud donor.
Three of those six are now ear-confirmed audible, a fourth is audible but confounded by an
independent defect. The mechanism is reconstructed end-to-end on 5/g6: the pooled A-border reference
reads 7.6 dB louder than the immediate shoulders, because the window reaches into material that
rises ~15 dB within two seconds of the gap.

**Second headline — the threshold is low.** 45/g5 is audible at a gain of **+5.61 dB**, with a
measured `edge_delta_db` of only +4.89. A fix aimed at the `max_fill_gain_db` ceiling would miss it.
The cap has to apply to every boost, not to the clamped cohort.

Durable behaviour this note reads against: [../../json-output.md](../../json-output.md) § FillLevelCheck
and [../../pipeline.md](../../pipeline.md) § Fill normalization. The boost-only PR that produced the
current gain policy is [TEMP-fill-normalize-boost-only-plan.md](TEMP-fill-normalize-boost-only-plan.md).

---

## Naming convention — read this before quoting any index

**Gaps here are 0-based**, matching dump filenames (`…_g004_full.json`), BACKLOG, and the rest of
`docs/`. The roll-up CSV carries **both** `Gap0Based` and `Gap1Based`, and reading the 1-based column
is an active hazard: during this work "31/g8" was used for dump `g007`, which collided with the
repo's existing **31/g8** (dump `g008`, the Narrow-capture pre miss in BACKLOG § *Fill placement*).
They are different gaps with different defects. Every index below is 0-based; cross-check against a
dump filename before quoting one.

Per [licensed-media-names-never-recorded], gaps are referred to by corpus pair index + gap index
only. All source runs are gitignored (`gap-files/`) and may be deleted; the numbers below are the
durable record — see **Reproduction**.

## Data sources

- **`gap-files/2026-08-24-normalize-gain/`** — full patch path over the 51-pair manifest
  (`gap-files/extended-pairs.csv`), `--patch-only --format json`, current binary, exit 0, no
  warnings. 308 spliced gaps carrying `fill_level`; `normalize_gain` present on 308/308.
- **`gap-files/2026-08-13-repeat-terms/`** — the prior oriented roll-up, used as the drift control.
- **Ear labels, 2026-08-25**, from existing listen dumps: `2026-08-18-one-dead-listen/45`
  (45/g4, 45/g5), `2026-08-18-post-sweep` (31/g7), and
  `archive/mixed-ab/2026-08-12-{nonorm,normalize-boostonly}/19` (19/g13).
- **`gap-files/2026-08-23-complete-corpus/5/…_g006_full.json`** — the fingerprint dump used for the
  5/g6 mechanism reconstruction.

## Method

`scripts/measure/measure-patch-outcomes.ps1` emitted `PeakDeltaDb` and `EdgeDeltaDb` but not the
gain, so the two could not be read together. One column was added — `NormalizeGain`, sourced from
`status.patched.normalize_gain` (**not** a `fill_level` field) — in `Get-FillLevelRows`, the
`Select-Object` list in `Write-FillLevelRollup`, and a `Get-NormalizeGain` helper that returns
`$null` when the property is absent. Absence means the report predates 2026-08-12, **not** unity;
such rows must be held out rather than defaulted to 1.0. The 2026-08-24 run has zero such rows.

Because the gain is a uniform scalar on the fill, `edge_delta_db − gain_db` is what the fill would
have measured at unity. That subtraction is the instrument this note rests on; the durable rule it
applies is already stated in [../../json-output.md](../../json-output.md) § FillLevelCheck ("do not compare
these deltas across gaps without dividing out `normalize_gain` first").

## Result 1 — the exposure, sized

| | count | share of 308 |
|---|---|---|
| At unity — boost declined | 199 | 65% |
| Boosted at all | 109 | 35% |
| Boosted > 6 dB | 18 | 5.8% |
| Clamped at the 12 dB `max_fill_gain_db` ceiling | 7 | 2.3% |

The clamped seven (0-based): 45/g4, 5/g6, 43/g7, 44/g6, 19/g2, 10/g11, 10/g9. Hitting the ceiling
means the pooled reference disagreed with the donor by more than 12 dB. On this corpus that never
once indicated a genuinely quiet donor needing rescue: **all seven back out to ≤ −0.89 dB at unity**,
and two (10/g9, 10/g11) sit 30–44 dB *below* their seams even after 12 dB of boost.

## Result 2 — the run reproduces the 2026-08-13 roll-up exactly

All 308 gaps common to both runs; **max |gain delta| 0.0000 dB, max |edge delta| 0.0000 dB**; no rows
added or dropped. Eleven days of commits changed nothing on this axis. This discharges BACKLOG's
*"Re-run the fill-level roll-up after any gain change"* item, which was owed from the boost-only PR.

## Result 3 — 6 of 8 hot fills are hot only because of us

All 13 gaps measuring `edge_delta_db` > +3 dB, with the gain divided out:

| gap | gain dB | edge measured | edge at unity | attribution |
|---|---|---|---|---|
| 45/g4 | +12.00 | +11.11 | **−0.89** | ours |
| 5/g6 | +12.00 | +10.61 | **−1.39** | ours |
| 24/g46 | +0.00 | +9.83 | +9.83 | donor |
| 19/g13 | +11.78 | +9.11 | **−2.66** | ours |
| 15/g6 | +1.14 | +8.64 | +7.50 | donor |
| 8/g4 | +0.00 | +6.23 | +6.23 | donor |
| 53/g7 | +0.00 | +5.21 | +5.21 | donor |
| 45/g5 | +5.61 | +4.89 | **−0.72** | ours |
| 2/g5 | +0.00 | +4.40 | +4.40 | donor |
| 40/g2 | +0.00 | +4.25 | +4.25 | donor |
| 15/g3 | +10.74 | +4.05 | **−6.68** | ours |
| 31/g7 | +6.56 | +3.56 | **−3.00** | ours |
| 2/g6 | +0.25 | +3.27 | +3.02 | donor |

**The control matters as much as the cohort.** 24/g46 is the correctly-restored gunshot whose +9.83 dB
refuted cutting on `edge_delta_db` alone. Its gain is **0.00 dB** — the boost-only policy declined it.
The statistic that cannot separate correct-loud from incorrect-loud separates them cleanly once the
gain is divided out. Every donor-driven row above is at or near unity; every "ours" row is boosted.

## Result 4 — ear labels: 3 clean confirmations, 1 confounded

| gap | gain | edge at unity | label (2026-08-25) |
|---|---|---|---|
| 45/g4 | +12.00 | −0.89 | **audible** — hole falls in a near-silent passage; patch reads louder than the material either side |
| 45/g5 | +5.61 | −0.72 | **audible** — same description, silent passage |
| 19/g13 | +11.78 | −2.66 | **audible** — noticeably louder than surrounding sounds (re-confirms the 2026-08-12 label) |
| 31/g7 | +6.56 | −3.00 | audible, **confounded** — also stutters |

31/g7's stutter is an independently recorded defect, not a level defect: the gap straddles pair 31's
map step (`step_ms = −1122.2` on the 2026-08-23 dump; see the blocked step-boundary verdict in
[../settled.md](../settled.md) § D and BACKLOG § *Fill placement*). A gap that stutters cannot cleanly
attribute audibility to level, so **the cohort should be counted as n = 3**, with 31/g7 corroborating
only. Note this is dump `g007`, *not* the repo's 31/g8 — see § *Naming convention*.

Independent measurement of the 45/g4 wav against its own `_a_surround.wav` sibling (6-channel
interleaved RMS, 100 ms bins): fill −41.0 dB, shoulders −50.0 / −57.6, **+16.5 dB above the quieter
seam**, where A's own original material in that region was −52.7 dB. The shoulder figures match the
report's `PreShoulderDb`/`PostShoulderDb` (−49.8 / −57.6) to a tenth, confirming the boost is present
in the rendered audio. That +16.5 dB and the report's +11.11 dB are different estimators (whole-fill
vs loudest 100 ms bin; 1 s shoulders vs the quieter seam) — the agreement is directional, and the gap
between the two numbers should not be read as meaningful.

## Result 5 — the mechanism, reconstructed on 5/g6

5/g6 is not a placement defect. Registration is among the corpus's best and is identical across all
four runs that dumped it: `pre_peak_r` 0.9954 / `post_peak_r` 0.9901, `peak_z` 8.15 / 5.93,
`edge_pinned` false, `splice_dualfit.gate_pass` true, `equivalence_production.class`
`repairable_dropout`. A is at −101.5 dB across all 11 core blocks; B carries −39.1 dB of continuous
material. There is real content to restore and it is correctly registered.

From the A envelope in `equivalence_production.donor_registration.envelopes` (100 ms bins,
interleaved reduction), against `donor_interior_aligned.rms_db` = −45.63 dB:

| reference | level | gain it asks for |
|---|---|---|
| pooled, both sides | −34.6 dB | **+11.0 dB** → clamps at 12 |
| immediate shoulders only (±0.5 s) | −42.2 dB | **+3.7 dB** |

A **7.6 dB spread between the pooled and the local reference on one gap.** Post-gap, A climbs from
~−40 dB at the immediate shoulder to −27 dB two seconds out, with the last bins at −20.9 dB; the
pooled window is dominated by material that is not adjacent to the gap.

**Do not implement a threshold against these decibels.** The diagnostic carries only
`context_secs: 2.0` while `normalize_window_secs` is 5.0, so the real window reaches *further* into
that rising material — +11.0 dB is a floor, consistent with the observed clamp. And neither estimator
here is `compute_fill_gain`'s: it uses the A-border RMS over its own window against the whole
assembled fill, not a 100 ms interleaved envelope against `donor_interior_aligned`. The
reconstruction identifies the mechanism; it does not calibrate a fix.

## Result 6 — what the labels constrain about a fix

45/g5 is audible at **+5.61 dB** of gain and only +4.89 dB of measured edge — the smallest boost in
the cohort. Consequences:

- A fix scoped to the clamped-at-12 cohort catches 3 of 6 and misses 45/g5, 31/g7 and 15/g3. **The
  cap must apply to every boost.**
- The design space is narrow. The two smallest surviving legitimate boosts are 15/g6 (+1.14 dB) and
  2/g6 (+0.25 dB), both donor-driven and both of which a cap must preserve. So the fix must separate
  "+1.14 dB legitimate" from "+5.61 dB audible" — a ~4.5 dB corridor.
- 15/g3 is the informative unlabelled gap: +10.74 dB of gain but only +4.05 dB of measured edge. It
  probes whether a large gain is audible when the fill still is *not* hot at the seams — i.e. whether
  the audible axis is the gain or the resulting edge delta. That distinction picks the fix.

---

## What survives (disposition, 2026-08-26)

Written on archiving. The measurements are durable; the design half is not.

| Section | Disposition |
|---|---|
| Results 1, 2, 4, 5 | **Stand.** Measurement of runs that are gitignored and deletable; this is their durable record. Result 5's mechanism (pooled 5 s + 5 s reference reading 7.6 dB hotter than the immediate shoulders on 5/g6) is still the diagnosis — the shipped change stops that reference being consulted by default rather than fixing it |
| Result 2's obligation | **Re-incurred.** "Re-run the fill-level roll-up after any gain change" — the 2026-08-26 program-gain change is a gain change, so the discharge claimed here is spent. The 2026-08-25 program-gain census re-discharges it |
| Result 3's instrument | **Stands, and was confirmed.** `edge_delta_db − 20·log10(normalize_gain)` predicted 5/g6 at **−1.39** dB and 2/g6 at **+3.02**; the program-gain census measured **−1.40** and **+2.97** with local off. But `normalize_gain` is now the product `g_program * g_local`, so "the excess is our gain" no longer localizes blame — attributing it to the **local** reference needs `normalize_gain / program_gain` |
| Result 6, and the second headline | **Void.** Both argue that a cap must reach every boost, over a ~4.5 dB corridor between 15/g6 (+1.14 dB) and 45/g5 (+5.61 dB). That corridor was derived from whole-fill RMS against a wide A window, which is the estimator being replaced. No cap is being built |
| **Q1** — are 5/g6 and 15/g3 audible? | **Promoted**, [BACKLOG.md](../../../BACKLOG.md) § *Fill normalize* ("Ear-label 5/g6 and 15/g3"). **Demoted in force**: it was framed as the question that picks between the three candidate fixes, and those are gone. It is now evidence for the collar-residual work, not a gate on shipping |
| **Q2** — which fix? | **Closed.** All three candidates (narrow `normalize_window_secs`, quieter-of-wide-vs-local, cap by the local shoulder ratio) are withdrawn at [BACKLOG.md](../../../BACKLOG.md) § *Fill normalize*, superseded by [../TEMP-local-paired-gain-plan.md](../TEMP-local-paired-gain-plan.md) — a paired B−A collar residual around the hole, not an RMS-window choice |
| **Q3** — default `normalize_fill` false? | **Answered yes**, shipped 2026-08-26 ([../TEMP-program-gain-plan.md](../TEMP-program-gain-plan.md)). This note argued *against* it, on the ground that 15/g6 (+1.14 dB) and 2/g6 (+0.25 dB) are genuine quiet-donor boosts that turning the policy off gives up. That cost is **accepted, not refuted**: both are under 1.2 dB and both are recoverable with `--normalize` |
| **Q4** — the acceptance test | **Split.** "The six 'ours' gaps fall to approximately unity" and "24/g46 stays at 0.00 dB" survive verbatim as [../TEMP-program-gain-plan.md](../TEMP-program-gain-plan.md) § 11 rows 1–2. "15/g6 and 2/g6 must keep their boosts" is **withdrawn** — § 11 replaces it with *may lose under the default, must regain under `--normalize`* |

What replaced the fix this note was sizing: a whole-program A↔B scalar
(`g_program = 10^(−offset_db/20)`) on every B-derived fill with the hole-local knob defaulted off
([../TEMP-program-gain-plan.md](../TEMP-program-gain-plan.md)), and a paired collar residual to
replace `compute_fill_gain` when the local knob is re-enabled
([../TEMP-local-paired-gain-plan.md](../TEMP-local-paired-gain-plan.md)). Neither is a cap on the
RMS ratio. The corpus fact that made the whole-program half possible — every pair's offset inside
±0.26 dB — also means the program scalar does **not** rescue this note's cohort: the six were
boosted 5.6–12 dB by a local reference, and no ~0.2 dB master match touches that. Turning the local
knob off is what removes them.

---

## Open questions as written 2026-08-25 — all four now dispositioned above

**Q1 — Are 5/g6 and 15/g3 audible?** Neither pair has ever had a listen directory; both need a
`--gap-listen` dump. 15/g3 is the discriminating one (see Result 6). Until it is labelled, the
question "is the audible axis gain or edge delta" is open, and it is the question that chooses
between the three candidate fixes.

**Q2 — Which fix?** The three candidates are unchanged: narrow `normalize_window_secs` (5.0), take
the quieter of wide-vs-local, or cap the gain by the local shoulder ratio. This note's evidence
favours the **cap**: 65% of gaps are already at unity and 7 donor-driven boosts must survive, both of
which a cap preserves by construction and a narrowed window does not (it re-references every gap).
Not decided here.

**Q3 — Does `normalize_fill` default to false instead?** The sweep weakens this. The evidence for it
was "one ear case where the boost causes damage, zero where it demonstrably helps"; the first half is
now four cases, but the second half is no longer clean — 15/g6 (+1.14 dB) and 2/g6 (+0.25 dB) are
genuine quiet-donor boosts that turning the policy off would give up. A local reference fixes the six
without touching those two.

**Q4 — What is the acceptance test for any gain change?** Stated here so it is not re-derived: the
six "ours" gaps must fall to approximately unity; **15/g6 and 2/g6 must keep their boosts**; and
24/g46 must stay at 0.00 dB. A fix that flattens the donor-driven rows is worse than the defect.

---

## Reproduction

    pwsh scripts/measure/measure-patch-outcomes.ps1 \
        -Manifest gap-files/extended-pairs.csv \
        -OutDir   gap-files/<dated-dir>

Emits one `<label>.json` per pair plus `fill-level-rollup.csv`, which carries `NormalizeGain`
alongside `EdgeDeltaDb`. `-RollupOnly` re-derives the CSV from existing JSON with no cargo build and
no repair cost. The attribution table (Result 3) is `EdgeDeltaDb − 20·log10(NormalizeGain)` over rows
with `EdgeDeltaDb > 3`; the queries were run ad hoc against the CSV, not from a committed script.

The full patch sweep over 51 pairs is a media run of a few hours. It does **not** pay the per-bracket
anchor-oracle dump cost — that is `--gap-fingerprints`, not `--patch-only`
([gate-refusal-costs-anchor-oracle]).
