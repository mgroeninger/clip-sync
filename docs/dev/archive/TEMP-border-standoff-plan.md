# `border_standoff_secs`: provenance and the α sweep

**Status:** closed 2026-09-19 on § 7's stop rule, branch 4 (§ 8): no late fill heard as audible at
n = 5, so no default change is motivated. The 0.35 default stays. Tracked as [settled.md](../settled.md) § C → α.

**Why this doc exists.** Production's default of 0.35 s has never had a recorded measurement behind it.
At least one earlier standoff sweep also left no record of why it was run or what it found. A change to
the default made now, without writing down the evidence, would give the next person the same blank
wall. **Nothing may change the default without first adding its evidence to § 4.**

---

## 1. Provenance: what is known

- **Introduced:** commit `a5af846`, 2026-06-09, at 0.35 s. The value has not been changed since.
- **Stated purpose** (the only one on record): "A-side only: skip this much audio immediately adjacent to
  the dropout when building border templates (avoids corrupted seam audio on A)" (`request.rs` doc
  comment). [seam-scoring.md](../../seam-scoring.md) § 1 step 3 adds: "so the seam is not graded against
  the click/fade right at the gap edge".
- **Not on record:** why 0.35 and not some other value, and any measurement of how much corrupted
  audio actually sits next to a dropout. Nobody has checked that the click/fade exists on this corpus,
  or how wide it is.
- **Lost sweep:** `gap-files/2026-08-14-standoff/` was a standoff run. By 2026-08-17 it was already
  "kept but no longer joinable to any question" ([archive/TEMP-unmatchable-shoulder-plan.md](archive/TEMP-unmatchable-shoulder-plan.md)).
  The directory is now deleted and its question and result are unknown. It is the reason
  `PatchSummary.fill_geometry` exists (2026-08-17): every patch report now echoes the standoff it ran with.

**What the standoff feeds.** It feeds the bracket placement search templates, the dual-fit shoulder
templates, the executor's fill reconstruction and, until 2026-09-19, the `registration_residual_ms`
measuring templates (§ 3). The fingerprint probe does **not** read it. It has its own
`probe_border_standoff_secs`, and its recipe test forbids reporting production's 0.35 there.

## 2. The question

α ([archive/TEMP-fill-placement-registration-plan.md](archive/TEMP-fill-placement-registration-plan.md)
Step 5) says bracket fills sit late of the point where both A shoulders register against B. The
suspected mechanism is that the placement search scores standoff-displaced A templates against B
windows that sit flush at the gap edges. If that is right, α should move with `border_standoff_secs`.

With the fixed field (2026-09-18, 52-pair `--patch-only` corpus), the bracket `registration_residual_ms`
has median **+20.5 ms** (IQR +5.9 … +34.9), and 173 of 209 bracket patches are late. The bias is real.
Whether it is *audible* is untested: 47/g14, the one ear-checked candidate, turned out not to be an
instance.

## 3. Sweep 1: 2026-09-18

Three arms of `border_standoff_secs`. Everything else was default, run through
`measure-patch-outcomes.ps1` (`--patch-only --format json`, `he-aac,ac3`) at or after `4792ebf`, the
residual fix. Gaps (1-based `--only-gaps`):

| pair | gaps |
|------|------|
| 50 | 4,6,7,8,9,10,11,15,16,17 |
| 53 | 2,3,4,5,6,7,8,9,10 |
| 3 | 3,8,11,13,14,16,17,18 |

That is 27 gaps, every one a bracket patch at the control. Each arm's `fill_geometry` shows its standoff,
so the arms are identified by the report, not by the directory name.

| arm | median residual | IQR | median min seam r | notes |
|-----|-----------------|-----|-------------------|-------|
| 0.35 (control) | +34.8 ms | +10.6 … +50.1 | 0.387 | matches the corpus run gap for gap |
| 0.175 | +34.9 ms | — | 0.372 | 2 gaps flipped to dual-fit (3 @ 6777.5 s, 50 @ 2627.1 s) |
| 0 | +8.7 ms | −0.0 … +21.7 | 0.553 | |

**How to read it:**

- The residual does **not** scale with the standoff: halving it changes nothing. Removing it entirely
  cuts the bias by about 75%. That is a threshold effect, not the linear one the mechanism predicted.
- Per-gap values are noisy. For example, 53 @ 7250.6 s goes from +76 to −63.
- **Suspected confound, refuted by sweep 2 (§ 6).** The residual's measuring templates were built
  from the same `border_spec` as placement, so each arm moved the instrument as well as the placement.
  Sweep 2 pinned the instrument at 0.35 and reproduced every sweep 1 value to within one frame. The
  instrument does not depend on its template standoff, so sweep 1's numbers stand as placement
  measurements.
- The rise in min seam r at 0 s (0.387 → 0.553) is **not** a quality result. It may just be the flush
  templates correlating with gap-edge material.

## 4. Evidence required before the default changes

Each item, with its date and data location, must be written here before a default change lands:

1. **Fixed-instrument re-sweep (§ 5). DONE 2026-09-19 (§ 6):** with the instrument pinned, the 0 s
   arm still moves placement, by a median −16.9 ms. The standoff does drive part of α. It is not a
   linear effect, though: 0.175 s moves individual gaps but leaves the median where it was.
2. **The original rationale, tested.** Take the gaps whose placement moves most between arms and check
   by ear for a click or fade graded into the seam at 0 s. This is the one claim the 0.35 default rests
   on, and it has never been measured.
3. **Corpus-wide outcome counts.** Patched, skipped and dual-fit counts across all 52 pairs at the
   candidate value. Sweep 1's two dual-fit flips at 0.175 show the standoff changes routing, not only
   placement.
4. **Audibility of α itself.** A 20–35 ms late fill with both shoulders otherwise clean may be
   inaudible: the masking study found 0 audible out of 14. If it is inaudible, no default change is
   motivated, whatever items 1–3 show. Design fixed 2026-09-19 in § 7; results in § 8.

## 5. Sweep 2 recipe: fixed instrument (run 2026-09-19)

`--residual-border-standoff-secs` (2026-09-19) builds the residual's measuring templates at their own
standoff. Placement never reads it, and the report echoes it as `fill_geometry.residual_border_standoff_secs`.
It reaches the **bracket** residual only. A dual-fit residual comes from `try_dual_fit`'s own lags,
which is one more reason gaps that change routing leave the paired read.
Use the same manifest and gaps as sweep 1, with the instrument pinned at 0.35 in **every** arm:

```powershell
foreach ($s in '0.35', '0.175', '0') {
    ./scripts/measure/measure-patch-outcomes.ps1 -Manifest gap-files/2026-09-18-standoff-sweep.csv `
        -OutDir "gap-files/2026-09-19-standoff-fixed-$s" `
        -RepairArgs "--border-standoff-secs $s --residual-border-standoff-secs 0.35"
}
./scripts/measure/census-registration-residual.ps1 -DumpDir gap-files/2026-09-19-standoff-fixed-0 `
    -BaselineDir gap-files/2026-09-19-standoff-fixed-0.35
```

**Control check.** The 0.35 arm must reproduce sweep 1's 0.35 arm gap for gap, since the override
equals the placement value there. If it does not, the binary has drifted and the other arms answer
nothing.

**Read:** the paired per-gap delta against the fixed-0.35 control, not the aggregate medians
(settled.md § E, *Comparing two census arms on aggregate medians*). Gaps that change routing
(bracket ↔ dual-fit) are excluded from the paired delta and listed separately.

## 6. Sweep 2 results (2026-09-19)

Data: `gap-files/2026-09-19-standoff-fixed-{0.35,0.175,0}/`. Every report's `fill_geometry` shows the
arm's `border_standoff_secs` and `residual_border_standoff_secs: 0.35`.

**Control check passed.** The fixed-0.35 arm reproduces sweep 1's 0.35 arm exactly on all 51 gap
records: routing, residual and `align_adjustment_secs`.

**The instrument does not depend on its standoff.** On 50 of the 52 paired bracket gaps (0.175 and 0
arms), sweep 2's residual is bit-identical to sweep 1's. The other two are pair 3 @ 71.3 s in both arms,
which differs by one frame (47.604 → 47.583 ms), so the override did reach the measurement. This is
what the 2026-09-18 anchor-displacement fix predicts: a correctly anchored shoulder reads the same lag
whatever its template standoff. The unit test
`residual_border_standoff_override_moves_the_instrument_not_the_placement` shows the same thing.

**Placement moves with the standoff.** Paired against the fixed-0.35 control, bracket only:

| arm | n | Δ residual median (IQR) | placement move median (IQR) | corr(Δr, move) | residual median | late |
|-----|---|------------------------|-----------------------------|----------------|-----------------|------|
| 0.175 | 25 | −0.0 (−10.9 … +9.9) | +0.3 (−10.9 … +17.0) | 0.86 | +34.9 | 24/25 |
| 0 | 27 | −13.1 (−43.1 … −2.0) | −16.9 (−43.1 … −2.0) | 0.81 | +8.7 | 20/27 |

Routing changes are excluded from the table: at 0.175, 3 @ 6777.5 s and 50 @ 2627.1 s went from
bracket to dual-fit, the same two as sweep 1. At 0 there are none.

**How to read it:**

- At 0 s, all 27 placements move and the residual falls by roughly the same amount. The standoff is a
  real input to α.
- Halving the standoff moves individual gaps by ±10–17 ms in both directions and leaves the median
  unchanged. The mechanism in § 2 predicted a monotone scaling, and that is not what happens. Whatever
  0 s changes, it is something about the gap-edge audio itself, and this sweep does not identify it.
- Zero standoff is **not** simply better. Some gaps overshoot to early: 53 @ 7250.6 s moves −140 ms
  to −63.4, and 50 @ 242.5 s reads −30.7. The median shifts, but the per-gap spread does not narrow.
- The next items are § 4 items 2–4. Item 4, audibility, is the cheapest and could make the others moot.

## 7. Listen test: design (fixed 2026-09-19, before any listening)

Answers § 4 item 4: is a late bracket fill audible? Everything in this section was written before any
clip was rendered or heard. Results go in § 8, and nothing here may be edited after listening starts.

**Reused from the masking study** ([archive/TEMP-seam-step-masking-plan.md](archive/TEMP-seam-step-masking-plan.md)):
the protocol (one-pass labels, repeats as a consistency control, a stop rule fixed first, unblind and
record), the blinding (seeded shuffle, neutral clip names, key in a file that is never printed) and
the seam-step readout (`max |dx|` within ±1 ms of each join, relative to the 100 ms post-seam RMS).
That study's blinding script was never committed and its key is gone. This one is
`scripts/measure/build-standoff-listen-set.ps1`, which carries the seed and the gap list, so the set can
be rebuilt exactly.

**Not reused:** the clips. They came from `--gap-listen`, whose placement disagreed with production by
10 ms on 47/g14 (+224.3 vs +214.3 ms). That is too much error for a test of 40–140 ms placement
differences, so every clip here is cut from a real repair's output (below).

### Selection (normative)

Universe: sweep 2's bracket gaps (§ 6), arms 0.35 and 0. A gap is eligible when:

1. **Placement is the only difference.** Both arms are bracket patches with the same
   `gap_start_adjust_frames` / `gap_end_adjust_frames`. When the anchor-seam extension differs, the two
   clips replace different stretches of A (up to 2.7 s more), and a label could not be put down to
   placement.
2. **The arms move the fill by at least 30 ms** (difference in `align_adjustment_secs`).
3. **Gain is not a confound:** `|20·log10(normalize_gain)|` ≤ 0.5 dB, as in the masking study. All
   sweep gaps pass (0.02–0.03 dB).
4. **At most 3 gaps per pair.**

Rule 1 removed four of the eight gaps first proposed (3 @ 1453.7, 3 @ 7656.8, 50 @ 2105.4, 53 @ 2247.7);
each has an anchor extension in one arm only. Rule 2 then leaves exactly five gaps, so rule 4 is not
binding:

| pair | gap (0-based) | `--only-gaps` | A start (s) | residual @ 0.35 | residual @ 0 | move 0 − 0.35 |
|------|---------------|---------------|-------------|-----------------|--------------|---------------|
| 3 | 10 | 11 | 3193.845 | +125.1 | +13.5 | −111.6 |
| 50 | 3 | 4 | 242.503 | +38.8 | −30.7 | −77.5 |
| 53 | 2 | 3 | 790.880 | +106.5 | +50.0 | −56.5 |
| 53 | 6 | 7 | 4085.749 | +50.5 | +7.4 | −43.1 |
| 53 | 8 | 9 | 7250.594 | +76.4 | −63.4 | −139.9 |

All values are in ms, from `gap-files/2026-09-19-standoff-fixed-{0.35,0}/`. Every 0.35 clip is late by
38.8–125.1 ms, the upper part of the corpus distribution (median +20.5): if these are clean, the typical
α is too. Two gaps swing to early at 0 (50 @ 242.5, 53 @ 7250.6), so the 0 arm also samples early
overshoot.

**Anchor (known audible):** 26/g6 (0-based; `--only-gaps 7`), A start 2151.688 s, at default settings.
On the 2026-09-18 corpus run it reads `align_adjustment_secs` −1.450 s and residual −900.8 ms. It is
one of the corpus's two ear-confirmed wrong patches. The render must reproduce both numbers.

**Repeats:** 2 clips, one drawn from each arm with the seeded RNG, placed at other positions in the
shuffle. **Total: 10 + 1 + 2 = 13 clips.**

### Rendering and checks

Seven repair runs: pairs 3, 50 and 53 at `--border-standoff-secs 0.35` and `0`, both with
`--residual-border-standoff-secs 0.35`, plus pair 26 at the defaults. Each uses `--only-gaps`,
`--format json` and `--mux` to `.mkv`, through a config with `audio_codec = "flac"` and
`mux_audio_bitrate = "default"`. The mux is used because classic WAV cannot hold these files: pairs 3
and 53 run past 7650 s of 6-channel 48 kHz audio, which is over 4 GiB at 16 bit. FLAC keeps the output
lossless. Each clip is cut at [A start − 4 s, A end + 4 s] to 16-bit WAV with every channel kept.

The build refuses to produce a set unless every check passes:

1. **Report match.** Each run's report matches the sweep report for that gap: routing, adjust frames,
   `align_adjustment_secs` (to 0.1 ms) and `registration_residual_ms` (to 0.1 ms). The anchor must
   match its 2026-09-18 values in the same way. This is what ties the audio to the measured placement.
2. **Same cut.** A gap's two clips are sample-identical outside [A start − 50 ms, A end + 50 ms]. This
   shows both clips were cut at the same point and that the fill is the only difference.
3. **Placement readback.** Each arm's fill interior (with 50 ms trimmed off each end, mono downmix) is
   located in B, using one decode of B per gap so seek error cancels. The two B positions must differ by
   the move column to within 1 ms, and each fill must correlate at r ≥ 0.80 or it counts as not found.
   This confirms, per gap, that the audio moved by the amount the report claims.

   *Amended 2026-09-19, before any clip was heard.* As first written, check 3 cross-correlated the two
   arms against each other. It passed 4 gaps exactly (to 0.0 ms) and failed 53 @ 7250.6: 16.6 ms read
   against 139.9 reported. That failure came from the check, not the placement. At a 140 ms shift about
   a quarter of the window falls outside the other arm's fill, and this fill repeats about every 30 ms
   (autocorrelation 0.86 at 30 ms), so a comb tooth outscored the true lag (r 0.92 vs 0.63). Located in
   B, the two fills sit 139.88 ms apart (r 0.996 / 0.993). A trial with separate B decodes for each arm
   agreed with the reports to within 0.9 ms on all five gaps. With the shared decode, all five match to
   the sample (3 @ 3193.8 111.60, 50 @ 242.5 77.50, 53 @ 790.9 56.52, 53 @ 4085.7 43.13 and
   53 @ 7250.6 139.88 ms; r 0.94–0.996). Side observation, not investigated: each fill sits in B about 70 ms earlier than
   `A time + recommended_offset_secs + align_adjustment_secs`, the same for every arm and pair.
   Relative placement is unaffected.

### Presentation

- The 13 clips are named `clip-01.wav` … `clip-13.wav` in seeded shuffled order and carry no gap ID,
  arm or residual.
- `labels.csv` lists the clip ids with an empty label column.
- The key (`key.csv`: clip → pair, gap, arm, residual, move, seam step, repeat or anchor flag) is
  written beside them. The builder never prints it, and it is not opened until every label is in.
- Labels are `audible` / `clean` / `unsure`, given in one pass with no going back. An `audible` label
  may add a free-text note on what was heard (blip, doubling, dropout, level).
- Listen on headphones, the same setup as the masking study.

### Stop rule (normative, fixed before listening)

Read in order; the first branch that matches decides.

1. **The repeats disagree, or the anchor is labelled clean** → stop. The ear cannot carry this
   question. Record it in [settled.md](settled.md) § E as a measurement caution.
2. **All five 0.35 clips are clean** → α is inaudible even at 2–6× its corpus median. Close α in
   [settled.md](settled.md) (safe but unmotivated) and keep the 0.35 default: § 4 items 2–3 are no
   longer needed. Record the 0-arm labels anyway, since an audible early overshoot is evidence against
   0 as a default.
3. **At least 3 of 5 0.35 clips are audible, with no reversal** → α is audible. A *reversal* is a gap
   where the clip with the smaller |residual| is audible and the larger one is clean. Go on to § 4
   items 2–3.
4. **Anything else** (for example, both arms audible, a reversal, or `unsure` labels deciding the
   outcome) → audibility is not tracking placement at this n. Record the split and stop at five gaps;
   do not enlarge the study.

**Known limits.** Five gaps from three pairs, three of them from pair 53. Two gaps (50 @ 242.5 and
53 @ 7250.6) have nearly equal |residual| in both arms and opposite signs, so they barely test the
reversal rule; they compare late with early. Both arms of each gap are in the same session, so the
listener may recognize content and compare clips. That is accepted: each label is still given
per clip, in a single pass.

## 8. Listen test: results (2026-09-19)

Set: `gap-files/2026-09-19-standoff-listen/` (13 clips, seed 20260919, key in `key.csv`). Headphones,
one pass, labelled before the key was opened. The listener reported the task as hard. Clips have
4 s of context, so the pre join is at 4.0 s and the post join is at 4.0 s plus the gap length. Step
columns are § 7's readout (dB relative to the 100 ms program-side RMS).

| gap | arm | residual (ms) | label | listener note | step pre / post (dB) |
|-----|-----|---------------|-------|---------------|----------------------|
| 3 @ 3193.8 | 0.35 | +125.1 | clean | | −10.6 / +1.4 |
| 3 @ 3193.8 | 0 | +13.5 | unsure | "possibly small gaps at patch" | −9.0 / −0.3 |
| 50 @ 242.5 | 0.35 | +38.8 | clean | | −15.4 / −4.8 |
| 50 @ 242.5 | 0 | −30.7 | clean | | −11.7 / −2.9 |
| 53 @ 790.9 | 0.35 | +106.5 | clean | | −21.3 / +3.6 |
| 53 @ 790.9 | 0 | +50.0 | clean | | −19.3 / +2.1 |
| 53 @ 4085.7 | 0.35 | +50.5 | unsure ×2 | "click/blip in a spoken word" (repeat) | 0.0 / +4.4 |
| 53 @ 4085.7 | 0 | +7.4 | clean ×2 | | −11.7 / −0.5 |
| 53 @ 7250.6 | 0.35 | +76.4 | unsure | "slight click around 4.5 s" (near the post join); "can't tell if … the soundtrack" | −28.3 / −0.9 |
| 53 @ 7250.6 | 0 | −63.4 | **audible** | "slight click around 4 s" | −26.5 / +6.7 |
| 26/g6 anchor | default | −900.8 | **audible** | "alignment/echo issue?" | −28.3 / +9.0 |

**Reading the stop rule in order:**

1. **Controls pass.** Both repeats agree: 53 @ 4085.7 at 0.35 was unsure both times, and at 0 was
   clean both times. The anchor was heard as audible, and as the right kind of defect (echo or
   misalignment, which is what a −1.45 s slide produces).
2. **Does not fire.** Three of the five 0.35 clips are clean, including the two latest fills
   (+125.1 and +106.5 ms), but two are `unsure`. The rule asks for all five clean.
3. **Does not fire.** No 0.35 clip was labelled audible, against the ≥ 3 of 5 required.
4. **Decides the outcome.** The `unsure` labels carry the result, so record the split and stop at
   n = 5. Do not enlarge the study.

**What the split says (read under branch 4, not a new rule):**

- **No late fill was heard as a defect.** The two latest fills in the set (+125.1 and +106.5 ms) were
  both clean. The evidence that α is audible is nil, so it does not motivate a default
  change. Branch 4 does not license declaring it inaudible either, because two late clips are unsure.
- **The one audible test clip is from the 0 arm.** It is 53 @ 7250.6 at −63.4 ms, the early
  overshoot § 6 flagged, and it carries the largest post-join step in the test set (+6.7 dB). That is
  n = 1, but it is the only ear evidence either way about standoff 0, and it points against 0 as a
  default.
- **Closest thing to placement-tracking:** 53 @ 4085.7 is unsure ×2 at +50.5 ms and clean ×2 at
  +7.4 ms, and its 0.35 post-join step is 4.9 dB larger than the 0 arm's. One gap, carried by unsure
  labels.
- The reversal clause is untestable as labelled: no gap has one arm audible and the other clean.

**Consequence for § 4:** item 4 is answered as far as this protocol allows, and items 2–3 (reached
only through branch 3) are not motivated. The 0.35 default stays, still without a measurement of its
original click/fade rationale. That gap is now recorded rather than silent.
