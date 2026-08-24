# Corpus-wide flat-3000 envelope re-fingerprint — sizing the step-boundary population

**Findings note, not a Change.** Nothing here proposes code. It pays BACKLOG § *Fill placement*
order-of-work item 3's sizing dump and retires that item's "a wider-envelope dump might find more"
branch.

**Owned question.** How many gaps in the corpus straddle a pair-level map step, once the ±2000 ms
envelope search radius — the censor the census identified — is removed?

**Answer.** Zero, by the census's own admitted-sequence rule at its 50 ms floor, on 51 pairs / 1073
gaps. 31/g7 remains the only within-gap boundary instance in the corpus, and it is *not* censored by
search radius. The recipe is safe at corpus scale and nothing in this corpus saturates 3000 ms.

The two-pair recipe validation is a different question and stays where it was:
[TEMP-envelope-3000-smoke-2026-08-23.md](TEMP-envelope-3000-smoke-2026-08-23.md).

## Data sources

| Arm | Dump | Recipe |
|-----|------|--------|
| Comparison | `gap-files/2026-08-23-complete-corpus` | `lag_envelope_max_lag_ms` **2000** |
| Subject | `gap-files/2026-08-23-complete-corpus-3000` | `lag_envelope_max_lag_ms` **3000** |

Both arms: 51 pairs, 1073 gaps in the per-pair `corpus.json` inventories, 1049 with full-tier
per-gap records, 2020 populated `lag_decision` shoulder entries. Every other `probe_recipe` field is
identical across the two arms — `lag_envelope_window_secs 2.0`, `lag_max_lag_ms 600`,
`lag_window_secs 1.0`, `border_capture narrow`, `border_standoff_secs 0.0`, `noise_collar_secs 5.0`.
Same media, same gap set, one knob apart, so every difference below is attributable to the radius.

Dumps are gitignored and deletable; the numbers in this note are the durable record.

## Result 1 — the widen is safe at corpus scale

Of **2020 populated shoulders**, 24 differ between the arms. That count is not a regression rate;
broken out it is:

| Change | Shoulders | Gaps |
|--------|-----------|------|
| Dead → rescued (decision-relevant) | 22 | 11 — pair 50 g8–g18 |
| Envelope grid landing moved, lag and verdict unchanged | 2 | 49/g38 pre (−300 → −600, `env_r` 0.9985 both); 50/g19 pre (+1900 → +1800, 0.9929 → 0.9975) — both already `declined`, both still `declined` |

Zero rescues lost, zero alive → dead, zero alive shoulder moved by more than 1 ms, and **zero
outcome changes across all 1073 gaps**. Two `dual_fit_rescue` fields go null → false (50/g13
`waveform_floor`, 50/g17 `structure_floor`), which is a field becoming populated rather than a
decision moving.

This confirms at corpus scale what BACKLOG item 5 predicted from the flag's construction: a flat
widen does not trade against shoulder length (the A template is clamped), it trades against B
lead-in, and capture pads B enough that the predicted casualty class — gaps near the start of B —
produced no members here.

## Result 2 — nothing in this corpus saturates 3000 ms

`envelope_edge_pinned` count: **1 at 2000 ms** (50/g009 post, `envelope_peak_r` 0.473 at +2000),
**0 at 3000 ms**. No shoulder is pressed against the new ceiling.

Stated carefully: this is the absence of saturation, not proof that no step above 3000 ms exists.
The 22 rows that remain dead with both shoulders live are near-nominal weak-`r` cases, not far steps
against the wall — so there is no residual population *pointing* past 3000 either. "Nothing in this
corpus saturates 3000 ms" is the claim this dump supports; "reach beyond 3000 is empty" is stronger
than the evidence.

## Result 3 — the base rate is still zero, now with pair 50 admitted

Census Result 8's rule applied faithfully — Arm A admission, `seam_step_ms()` for the step,
`seam_mid_ms()` for the level, `max(CLEAN_STEP_MS, 6 × MAD)` for "large", **and the
≥2-settled-per-side sequence test**
([archive/TEMP-step-boundary-census-plan.md](TEMP-step-boundary-census-plan.md) §4–§5 is
canonical; do not re-derive it, and do not drop the sequence half — census Question 12):

| Floor | Pairs | Gaps | vs the 2000 ms arm |
|-------|-------|------|--------------------|
| 2 ms | 14 | 24 (2.2–36.2 ms) | 18 / 29 |
| 10 ms | 13 | 15 | 9 / 11 |
| 25 ms | 9 | 15 | 6 / 6 |
| **50 ms** | **0** | **0** | **0 / 0** |
| 100 / 250 ms | 0 | 0 | 0 / 0 |

Same shape, same zero. As in the census, the sub-50 ms rows are millisecond seam wobble on pairs
whose `6 × MAD` collapses to the 2 ms floor — three orders of magnitude below the phenomenon, with
no plateau to pick a threshold from.

What changed underneath the zero is admission: Arm A admits **949** rows (vs 938 on the same-media
2000 ms arm; the census's 935 was the older `2026-08-19-decision-map-oriented` dump). Arm B pinned
0, dead-shoulder 22 (was 33), no-lag-pair 102 (unchanged). The admission delta is **exactly 11 gaps,
all pair 50 g8–g18, dead → admitted**.

So the zero is no longer a property of an instrument with a known blind spot. The blind spot was
opened, 11 gaps walked through it, and the count did not move.

## Result 4 — 31/g7 is the only within-gap boundary, and radius is not what censors it

Pair 50's admitted levels are g1–g7 ≈ −7…−33 ms and g8–g18 ≈ +2761…+2812 ms, with a largest
*within-gap* step of 43.8 ms. The step falls **between** g7 and g8. The widen therefore supplies the
sequence test's flanks — ≥2 settled gaps on each side — and still no straddling gap on that pair,
exactly as the smoke predicted.

A between-gap level-jump diagnostic (>100 ms; explicitly **not** the rule, and not a substitute for
it) hits exactly twice corpus-wide: pair 31 g6 → g9 (−5.4 → −1144.7) and pair 50 g7 → g8 (−32.6 →
+2819.0). Both were already known.

31/g7 stays in Arm B at 3000 ms, and the reason matters:

| Shoulder | `frac_lag_ms` | `peak_r` |
|----------|---------------|----------|
| 31/g7 pre | −38.07 | 0.8287 |
| 31/g7 post | −1160.25 | 0.7820 |

**Both lags are the true ones** and both shoulders carry `verdict: timing_offset` — the boundary is
already on the wire, at 2000 ms and at 3000 ms alike. What fails is the census's admission floor,
`SPLICE_MIN_PEAK_R = 0.85`, missed by 0.021 and 0.068. No search radius reaches that; widening can
never admit this gap.

Do not fold 31/g8 into this story. 31/g8 pre is a different mechanism — `peak_r` 0.152 at +550.8,
envelope +800 / 0.816, `declined`, `verdict: decorrelated`. That is the `BorderCapture::Narrow` pre
miss BACKLOG already tracks as its own row, not an admission-floor near-miss. And pair 31's map step
itself was never radius-censored on this media: g9–g11 read −1142.6 … −1158.4 at `peak_r`
0.965–0.995 in **both** arms.

## Result 5 — a lead for the pair-50 note, scoped

At the corrected +2.8 s registration, `donor_interior_aligned.continuous` flips on four pair-50
skips:

| Gap | 2000 ms aligned | 3000 ms aligned | Nominal |
|-----|-----------------|-----------------|---------|
| 50/g11 | `true`, silence 0.00, −45.4 dB | `false`, silence 0.917, −83.8 dB | `true` |
| 50/g12 | `true`, silence 0.00, −45.6 dB | `false`, silence 1.000, −83.8 dB | `true` |
| 50/g13 | *(no aligned donor)* | `false`, silence 0.944, −83.2 dB | `true` |
| 50/g17 | *(no aligned donor)* | `false`, silence 0.909, −80.3 dB | `true` |

The flanks that patch — g10 and g14 — stay continuous in both arms. Nominal stays `true`
throughout, which is the point: the nominal reading was never looking at the right place.

Read this as a candidate answer to the pair-50 note's Question 2 — *why do g11–g13 skip between
correctly-resolved flanks?* — namely that at the correct registration B does not bridge the hole.
It is a lead on that pair, not a corpus-wide donor finding: the strict corpus-wide `true → false`
count is **two** (50/g11, 50/g12), with 50/g13 and 50/g17 going from *absent* to `false`.

## Result 6 — report-level movement, for provenance

Mostly pair 50 becoming legible: verdicts decorrelated 12 → 2, ambiguous 2 → 1, timing_offset 983 →
994; dead-shoulder 33 → 22; splice 555 → 563; both-sides-recoverable 555 → 563; registration
716/255 clean/stepped → 721/250; |offset| max 1158.4 → 2812.4 ms; seam offset max 1146.72 → 2811.74
ms; skipped |step| max 1040.3 → 62.6 ms. 50/g11–g13 and g17 go from garbage steps (1040.3, 433.3,
−931.5, −799.0 ms) to `step 0.0` at r ≈ 0.99 on both shoulders — those "steps" were artifacts of a
search that could not reach.

Reproduce with `cargo run --features calibration --bin gap-fingerprint-stats -- <root>`; redirect
all output to a file rather than piping through `Select-Object`, which kills the process before
`GAP_FP_CSV=1` writes `target/gap_fingerprint_corpus.csv`.

## What this settles

- Item 3's wider-envelope branch is **spent**. It was the cheap route to a base rate; it ran, and
  the base rate is zero.
- **n = 1 is a measured property of this corpus**, not an artifact of not having looked. Any
  step-boundary classifier would be designed from a single visible instance that the Arm A rule
  will not count.
- The 2000 ms default is **measured safe to raise and unmotivated to raise**: zero outcome changes
  across 1073 gaps means no demonstrated production benefit, against a corpus-wide re-baseline of
  every `lag_decision` field.

## What this does not settle

- It does not unpark the classifier, and it does not weaken the parking reason.
- It says nothing about the `env_frames == 0` `Declined` conflation, 31/g8's Narrow-capture miss, or
  the rescue-branch envelope z/prominence diagnostic. None of those were this dump's job.
- It is not a `wide_envelope` provenance instance. These JSONs were dumped at defaults and carry no
  `wide_envelope` block; the `WIDE_ENV_MAX_LAG_MS = 400` vs rescue-at-3000 split is still real in
  code, but a `--fingerprint-diagnostics` dump would be needed to exhibit it.
