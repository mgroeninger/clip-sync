# TEMP — Seam step vs masking (ear-label study)

**Status:** measurement plan, 2026-09-16. **Not a Change** — nothing ships
unless § 3's first branch hits. No code, no flag, no report field is added
by this plan; § 0 defines numbers computed by a throwaway script.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *31/g10 — post-join step,
exposed rather than exceptional*.

**Question.** Every patched seam carries a waveform step (the 2026-09-16
four-gap measurement: 0.0048 … 0.0273, implied timing error 0.10 … 0.42 ms,
all far above sample-grid rounding). Step size does **not** order
audibility — 50/g18 has the largest step of the four and is clean by ear,
31/g10 has a middling one and is not. The surviving hypothesis is
**masking**: a step is heard when the program around it does not cover it.
This plan tests that hypothesis and nothing else.

**Why pre-register.** The score below is cheap to redefine after the fact
until it separates. The stop rule in § 3 is only meaningful if § 0 and § 1
are fixed **before** any clip is heard. The likely outcome is a negative
that closes the item; a post-hoc score cannot deliver one.

**Media hygiene:** pair index and gap index only. No filenames, no titles,
no `gap-files/` paths in this document. Run dirs are provenance for the
person at this machine; every number that matters is inlined here.

---

## 0. The score (normative, fixed 2026-09-16)

All three are measured on the **downmix to mono** of the patched clip's
channels, at the gap's post join `t = a_end_secs`. Sample rate is the
source's own.

**Masking, post-seam only** — measured on `[t, t + 100 ms]`:

- `hf_fraction` — energy above **3 kHz** as a fraction of total energy, on
  a Hann-windowed rFFT of the 100 ms window.
- `rise_db` — the largest ratio between consecutive **10 ms** RMS blocks
  within `[t, t + 50 ms]`, in dB.
- `post_rms_db` — RMS of the 100 ms window, for context only. Not a band
  variable.

Post-seam only is deliberate: in the unpatched A file the pre-seam side is
the gap's own silence, so a two-sided window would score the fill rather
than the program. Post-only is therefore computable from **A alone**,
which is what makes § 2 stage 1 cheap. It also preserves the four-gap
ordering — measured both ways, 31/g10 stays the lowest-HF gap
(two-sided 0.0029, post-only 0.0031, next-lowest 0.0115 / 0.0116).

**Step** (dependent variable, needs the patched clip) — `max |dx|` over
`t ± 1 ms` on the mono downmix, reported three ways: absolute, in dB
relative to the 100 ms post-seam RMS, and as an implied timing error
(`step / median |dx|` over `t ± 30 ms`, in samples).

**Band assignment** uses `hf_fraction` and `rise_db` only:

| Band | Rule |
|------|------|
| exposed | `hf_fraction` < 0.005 **and** `rise_db` < +3.0 |
| covered | `hf_fraction` ≥ 0.020 **or** `rise_db` ≥ +5.0 |
| middling | everything else |

Thresholds are set from the four anchors (below) and are frozen. If stage
1 finds a band with fewer than 4 members, **widen nothing** — record the
shortfall and sample what exists.

### Anchor labels (already collected, 2026-09-16)

Heard on the 09-16 build. 31/g10's "blip" label is a re-listen of audio
byte-identical to its 08-20 vintage apart from a flat −0.014 dB.

| Gap | hf_fraction | rise_db | post_rms_db | step | step vs RMS | implied err | band | ear |
|-----|------------|---------|-------------|------|-------------|-------------|------|-----|
| 31/g10 | 0.0031 | +2.2 | −35.7 | 0.0097 | −4.0 dB | 0.22 ms | exposed | **blip** |
| 31/g11 | 0.0116 | +2.3 | −39.8 | 0.0048 | −5.5 dB | 0.10 ms | middling | clean |
| 50/g8 | 0.0483 | +1.6 | −39.0 | 0.0122 | +0.3 dB | 0.15 ms | covered | clean |
| 50/g18 | 0.0149 | +5.0 | −35.7 | 0.0273 | +3.7 dB | 0.42 ms | covered | clean |

n = 4 is consistent with the hypothesis and proves nothing: one member per
band, and the only audible gap is also the only exposed one.

## 1. Selection (normative)

Universe: gaps with `status: patched` in the 2026-09-16 `--patch-only`
corpus reports.

Exclusions, applied in order:

1. **Duplicate media** — drop pair labels 6 and 16 (≡ 22 and 28).
2. **Gain confound** — drop any gap where `|20·log10(normalize_gain)|` or
   `|20·log10(local_gain)|` exceeds **0.5 dB**. Fill normalize against a
   non-local reference is a separate ear-confirmed defect; a gap carrying
   both tests neither.
3. **Anchors** — the four above are carried into the study as labelled
   members, not re-drawn.

Sampling: **4 per band**, 12 new gaps, under two constraints —

- at most **3 gaps per pair**, across **≥ 5 distinct pairs**, so pair
  content cannot masquerade as the score;
- at least **3 marginal-tier** gaps in total, so the tier with the worst
  seams is represented.

Within a band, order candidates by `hf_fraction` and take a spread across
the band's range rather than its extremes — the question is where the
boundary is, not whether the extremes differ.

## 2. Procedure

**Stage 1 — score the universe (A files only, minutes).** For every gap
surviving § 1, compute § 0's masking pair from the A media at
`a_end_secs`. No repair run, no patched audio. Output: one CSV, plus the
per-band counts. Stage 1 is what decides whether the exposed band can be
filled at all — if it holds fewer than 4 gaps corpus-wide, say so in § 4
and sample what exists.

**Stage 2 — patched clips for the 12 (`--gap-listen`, ~3 min/gap).**
Narrow each pair's row with `--fingerprint-gap` (**1-based**: flag =
gap index + 1; the emitted filenames are 0-based). Compute § 0's step on
each resulting patched clip.

**Stage 3 — blind labels.** Clips are presented under neutral names, in
shuffled order, carrying no gap id, no band and no score. Labels are
`audible` / `clean` / `unsure`, one pass, no going back. **2 clips are
repeated** in different positions.

**Stage 4 — unblind and record** in § 4: every label, the repeat
agreement, and the band split.

## 3. Stop rule (normative, fixed before listening)

Read in this order; the first matching branch decides.

1. **Repeats disagree** (either repeated clip gets different labels) →
   **stop**. The ear cannot carry this question at the resolution it
   needs. Close to [settled.md](settled.md) § E as a measurement caution,
   citing the repeat pair. This branch is live evidence, not a formality:
   31/g10 was labelled "audible blip" (2026-08) and "hardly noticeable"
   (2026-09) on audio that differs by −0.014 dB.
2. **Audible set spans bands** — any `covered` gap labelled audible, or
   the `exposed` band no more audible than the others → **stop,
   hypothesis refuted**. Close to [settled.md](settled.md) § A. Seam steps
   are inherent (§ separate encodes, below) and audibility is not
   predictable from the report.
3. **Audible set is confined to `exposed`**, with ≥ 3 of 4 exposed gaps
   audible and 0 of 8 elsewhere → **a real predictor**. Next deliverable
   (not this plan): carry `hf_fraction` / `rise_db` into `fill_level` and
   propose a warning, never an automatic skip — a skip trades a silent
   gap for an unfilled one.
4. **Anything else** (partial separation, `unsure` labels carrying the
   result) → **stop at n = 16**, record the split, and do not enlarge the
   study. A predictor that needs more than 16 gaps to show itself is not
   one that can gate a patch decision.

## 4. Results

### Stage 1 — scored 2026-09-16

**327** patched gaps across **51** pairs survive § 1's exclusions (pairs 6
and 16 dropped as duplicate media; gaps over 0.5 dB of gain dropped as the
normalize confound). Band counts:

| Band | n | pairs | marginal |
|------|---|-------|----------|
| exposed | 52 | 31 | 9 |
| middling | 89 | 43 | 19 |
| covered | 186 | 48 | 47 |

No band is short, so § 0's shortfall clause does not fire and the
thresholds stand as frozen. The **exposed** band is 16% of patched gaps —
not a corner case, which is what makes the question worth an ear pass:
if masking predicts, it predicts for one patched gap in six.

### Stage 2 sample — drawn 2026-09-16

Drawn deterministically: within a band, candidates sorted by
`hf_fraction`, targets at the 12.5 / 37.5 / 62.5 / 87.5 percentiles,
walking outward to the nearest admissible candidate on a clash. The draw
came in at **1 gap per pair over 12 distinct pairs**, comfortably inside
§ 1's "≤ 3 per pair, ≥ 5 pairs", and hit the 3-marginal floor exactly.

| Gap | band | hf_fraction | rise_db | post_rms_db | tier |
|-----|------|------------|---------|-------------|------|
| 41/g5 | exposed | 0.0002 | +1.3 | −54.4 | high |
| 44/g7 | exposed | 0.0011 | +2.2 | −26.7 | high |
| 43/g7 | exposed | 0.0025 | +0.4 | −38.4 | marginal |
| 53/g9 | exposed | 0.0044 | +0.4 | −28.2 | high |
| 50/g3 | middling | 0.0042 | +3.4 | −42.6 | high |
| 52/g9 | middling | 0.0081 | +2.2 | −27.3 | high |
| 2/g20 | middling | 0.0118 | −0.1 | −46.7 | high |
| 12/g4 | middling | 0.0161 | −0.8 | −28.5 | high |
| 23/g15 | covered | 0.0204 | +2.8 | −20.2 | high |
| 34/g6 | covered | 0.0446 | +5.1 | −19.3 | marginal |
| 26/g4 | covered | 0.0847 | +1.1 | −25.0 | marginal |
| 18/g41 | covered | 0.2297 | +1.1 | −26.7 | high |

Plus the four anchors (31/g10, 31/g11, 50/g8, 50/g18) as labelled members:
**16 gaps, 12 to label blind.**

Note `post_rms_db` is not the band variable and does not track it — the
sample spans −54.4 … −19.3 dB, and the quietest gap (41/g5) and the
loudest (34/g6) sit in different bands for reasons of spectrum and
transient, not level. If the labels turn out to follow `post_rms_db`
instead, that is a § 3.4 outcome (partial separation), not a win.

### Stages 2–4 — run and labelled 2026-09-16/17

All 12 gaps patched under `--gap-listen`, one gap per pair, 313 … 512 s per
pair. Blind set: 14 clips, neutral ids, shuffled, 2 repeats, key withheld
until the labels were in. Labels were given in one pass.

**Repeat control (§ 3.1) — passes.** Both repeated gaps drew the same
label twice: 41/g5 (exposed) clean/clean, 34/g6 (covered) clean/clean. The
ear is stable at this resolution on this material, which is what 31/g10's
2026-08 vs 2026-09 label drift put in doubt.

**Labels: 0 audible, 1 unsure, 13 clean.**

| Gap | band | hf_fraction | step | step vs post RMS | step / median \|dx\| | label |
|-----|------|------------|------|------------------|---------------------|-------|
| 41/g5 | exposed | 0.0002 | 0.00068 | **+8.0 dB** | 33.5 | clean ×2 |
| 44/g7 | exposed | 0.0011 | 0.00817 | +0.6 dB | 43.4 | clean |
| 43/g7 | exposed | 0.0025 | 0.00090 | −9.7 dB | 3.5 | clean |
| 53/g9 | exposed | 0.0044 | **0.02647** | **+7.0 dB** | **57.2** | **unsure** |
| 50/g3 | middling | 0.0042 | 0.00096 | −4.8 dB | 15.7 | clean |
| 52/g9 | middling | 0.0081 | 0.00294 | −10.8 dB | 2.7 | clean |
| 2/g20 | middling | 0.0118 | 0.00090 | −1.5 dB | 13.5 | clean |
| 12/g4 | middling | 0.0161 | 0.00082 | −21.5 dB | 9.5 | clean |
| 23/g15 | covered | 0.0204 | 0.01113 | −3.5 dB | 12.2 | clean |
| 34/g6 | covered | 0.0446 | 0.00836 | −8.9 dB | 4.6 | clean ×2 |
| 26/g4 | covered | 0.0847 | 0.01619 | +2.5 dB | 11.4 | clean |
| 18/g41 | covered | 0.2297 | 0.00228 | −14.6 dB | 2.7 | clean |

**Verdict: § 3.2 — the hypothesis is refuted.** The exposed band is not
more audible than the others; it is indistinguishable from them. The
decisive member is **41/g5**: the most exposed gap drawn (HF fraction
0.0002, 14× below the next), carrying a step **8 dB above** its local RMS
at 33× the local median, labelled clean on **both** presentations. Under
the hypothesis that is the set's strongest predicted positive. Two exposed
gaps with large steps (41/g5, 44/g7 at 43× median) both read clean, so the
failure is not a shortage of exposed material or of step size.

**What the null does and does not establish.** With zero audible labels
there is no positive class, so this is evidence that audible seams are
**rarer than 1 in 12 patched gaps**, not a discrimination test that
masking lost on its merits. What it *does* refute, on positive evidence,
is the predictive claim the score was built for: gaps scoring maximally
exposed, with steps at the top of the observed range, are not audible.
A score whose extreme predicts nothing is not a gate, whatever a larger
sample might say about its middle.

**53/g9, the one unsure** ("pretty sure I hear something, but it sounds
like a gain increase"): not a gain and not a level step. `normalize_gain`
0.9976 (−0.02 dB), `local_gain` off, `edge_delta_db` +0.02, and the
measured 200 ms RMS runs −38 … −42 dB across the fill with no step at the
seam. It carries the set's largest step — 0.0265, +7.0 dB over post-seam
RMS, **57×** the local median, twice 31/g10's implied timing error. So the
one non-clean label in the study sits on the one seam that is a clear
outlier by step, in an exposed context, and even that is `unsure` rather
than audible. This is the residue, and § 3.4 forbids enlarging the study
to chase it.

**Not carried forward.** `post_rms_db` did not order the labels either
(clean at −54.4 dB and at −19.3 dB alike), so the § 4 caveat about level
masquerading as the score does not apply — nothing separated.

### Closed

Closed 2026-09-17 on § 3.2. Verdict row:
[settled.md](settled.md) § A. The BACKLOG item *31/g10 — post-join step*
is deleted, not re-scoped: its question ("is g10 defective?") was answered
by the four-gap measurement, and its successor question ("does masking
decide?") is answered here.

---

## Background: why every seam steps

From the 2026-09-16 four-gap measurement, recorded here so it is not
re-derived:

- The implied timing error at a seam is **5–20 samples** (0.10 … 0.42 ms),
  far above the half-sample that sample-grid rounding could explain. This
  is sub-millisecond registration, below what the alignment resolves.
- A and B are **separate AAC encodes**. The fill correlates **0.968** with
  its B source; the audio past the seam correlates **0.506** with B. Fine
  waveform detail cannot agree across the join however good the alignment
  is.

So a step at every seam is inherent, not a bug with a fix. That is why
this plan asks only which steps are *heard*.

Also refuted as orderings of audibility, same four gaps: step size
(50/g18 largest, clean), post-seam repeat (31/g10 r = 0.51 blip vs 50/g18
r = 0.53 clean), and registration residual (−295 … −341 ms across all
four, both labels).
