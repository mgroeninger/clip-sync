# Equivalence divergence — open findings ledger

**Opened:** 2026-07-30. **Status:** **F14** border alignment **FIXED and media-validated** (dump A
borders = `mono(refined ± w)` like `try_dual_fit`; `fp_post_F14_fix/` confirms). **F15** still OPEN,
but its donor mechanism is now **measured** on that same run (donor in the band between the two
floors on exactly the divergent gaps) and its noise-floor axis characterized (fine lower on 10/10,
~8 dB) —
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

Artifacts sit under gitignored `gap-files/silence-floor/`: `fp_post_F14_fix/` (**current reference** —
post-F14-fix full-pair run; both floors *and* both noise floors on all ten characterized gaps),
`fp_F15_question/` (the pre-fix run the Answer was first derived from), `fp6/` / `fp/` (earlier
single-gap dumps),
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
**Severity: high → medium (2026-07-30, after the population check). Status: OPEN — floor (a)
decided; silent-core A RMS should follow; context / noise-floor window still open.** Mechanism
**measured** on `fp_post_F14_fix/` (see the post-fix re-run section); population **measured** over
one 17-pair corpus, recipe-invariant across two runs of it — **1.7 % divergent, 0 dangerous**
(see § Population check). Severity drops because no *observed* divergence puts audio at risk: every
action divergence is scan-keeps / fine-drops, the conservative direction. 0/297 bounds the dangerous
rate below ~1 %; it does not establish zero. What is left is a mislabel and wasted search.
Found / diagnosed 2026-07-30.

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
recorded as **0 divergent vs the fine reference**. This gap is divergent.

### Population check — RUN 2026-07-30. It is rare, and never dangerous.

`equivalence-calibration` roll-up over the **17-pair** corpus (`gap-files/anchor-bracket-corpus/`),
media-free, `corpus.json` only:

> **297 gaps compared · 5 divergent (1.7 %) · 0 dangerous · exit 0**

**Reproduced** identically on a second 17-pair set at a different recipe
(`gap-files/fill_length_slack_secs_narrow/`): 297 / 5 / 0.

**Read this as recipe-invariance, not extra media.** Both sets are the *same 17 pairs*; only the
recipe differs. It rules out the divergence being an artifact of one scan recipe — worth having —
but **n = 297, not 594**. Neither set contains the F15 pair, so the pair is corroborated by
independent media; the two corpora do not corroborate *each other*.

**Precision.** 5/297 is a 95 % interval of roughly **0.7 – 3.9 %**. By the rule of three, 0 dangerous
in 297 bounds the dangerous rate at **< ~1 %**, not at zero. The supported claim is "uncommon, and
the dangerous direction is unobserved across 17 pairs" — not "never dangerous".

No re-run was needed for any of this: these are already fingerprint corpora, and both verdicts
(`equivalence`, `scan_equivalence`) sit in each `corpus.json`. Verified that `gaps == cmp` on all 17
pairs — the fine verdict is present even for gaps scan drops, so the dangerous direction is
observable rather than excluded by construction. Extending the *mechanism* to these pairs is the
part that would cost a fingerprint sweep: they predate the floor-provenance fields.

The five, with the coarse→fine class and the input deltas the tool reports:

| pair·gap | scan → fine | `nf` Δ | `aRMS` Δ | `ds` Δ | kind |
|---|---|---|---|---|---|
| 3·#5 | `ambient_quiet` → `shared_silence` | −8.5 | −6.2 | +0.14 | reason only |
| 3·#10 | `dropout` → `ambient_quiet` | −4.7 | **+11.8** | +0.29 | keep → drop |
| 12·#1 | `dropout` → `shared_silence` | −5.3 | −7.0 | +0.71 | keep → drop |
| 13·#32 | `ambient_quiet` → `shared_silence` | −3.0 | −3.9 | +0.51 | reason only |
| 14·#17 | `dropout` → `shared_silence` | −7.2 | −4.0 | +0.37 | keep → drop |

Three read-offs:

1. **The shape matches the F15 pair exactly** — 3 action divergences (all keep → drop, the *safe*
   direction) and 2 reason-only, which is `g4`/`g5` + `g8`. F15 is not a property of one pair.
2. **The noise-floor bias replicates on independent media** — `nf` Δ is negative on 5/5 here, as on
   10/10 there. That is now a two-corpus result, and it is the axis with no decided answer.
3. **The 8-pair validation was not wrong, just under-powered.** At 1.7 %, a 121-gap set expects ~2
   divergences and can easily see zero. Nothing changed after it; the shape was never sampled. That
   closes the "did something regress?" question in the negative.

**Bearing on severity.** Across **307 gaps** (297 + this pair's 10) the dangerous direction —
scan drops a gap the fine path would keep — occurs **zero** times. No audio is at risk from this
finding on any media measured. What remains is a wrong operator-facing label on ~2 % of gaps and
wasted bracket-search work on the ones scan keeps.

**Direction matters for severity.** `equivalence-calibration` gates CI on the *dangerous* direction
only — scan drops while fine keeps (a false drop / unrepaired hole). Every observed divergence is the
**safe** direction: scan keeps, fine drops. So the tool exits 0, and no audio is lost. Severity is
now **medium**, carried by the wrong operator-facing label and the wasted search, not by data loss.

### (B) applied — 2026-07-30

The fork is closed on **(B): keep the sensors different, fix the interpretation.** Rationale: the
divergence rate is ~2 %, always safe-direction, and both known input differences bias the *fine* side
toward `drop` — so fine is the more aggressive path, and converging on it would import that bias into
the gate that actually drops gaps. Converging on scan would be a rewrite of the fine path with no
measured benefit.

**Nothing in the plan or patch path reads `equivalence`** (verified: only the fingerprint dump writes
it; only `equivalence-calibration` and the fixture tests read it). So (B) is a documentation and
authority change, with **zero behaviour change**:

- `equivalence` is documented as **diagnostic, not a reference**. The phrase "the fine reference" is
  retired from `schema.rs`, `application/gap_equivalence.rs`, `equivalence-calibration`'s header and
  `--help`, and `gap-fingerprint.md`. `scan_equivalence` is stated as authoritative.
- Both biases are recorded as **measured, same-direction** offsets, so a future divergence is read as
  "which known bias is this?" rather than "the scan gate is inaccurate".
- **One real bug fell out of the re-reading:** `tests/gap_cell_fixtures.rs::assert_equivalence_class`
  asserted each fixture's declared cell against the **fine** block, though `GapCellType`'s own docs
  define the cells as *scan-time*. It passes today only because the three equivalence fixtures happen
  to agree on both paths. Now reads `scan_equivalence`. This is exactly the class of error (B) exists
  to prevent — the wrong sensor silently promoted to authority.
- `equivalence-calibration`'s dangerous-direction exit-1 gate is **kept and re-justified**: it is the
  one direction neither known bias produces, so a hit there is signal rather than a known offset.

**Step 4 — the class is now pinned media-free.**
`tests/gap_corpus/fingerprints/equivalence_divergence/band_donor.json` is `g4` of this pair, harvested
as a committed single-gap fingerprint (non-identifying: hashed ids, numbers, enum names only —
audited). It deliberately sits *outside* `curated/`: a cell is a property of a gap, a divergence is a
property of the pair of front-ends, so folding it into the cell manifest would need a fake
`GapCellType` and drag in unrelated per-cell assertions. `tests/equivalence_divergence.rs` asserts:

1. both paths reclassify **live** to opposite dispositions (scan `repairable_dropout` / keep, fine
   `shared_silence` / drop) — and each matches the class stored in the artifact;
2. the **band premise**: `scan_floor (−79.50) < donor_rms (−66.94) < fine_floor (−58.39)`;
3. the donor fractions **straddle** the 0.5 threshold (0.474 vs 1.000) — which is what turns a floor
   difference into a class difference;
4. **attribution**: scan's A-side signals + fine's donor fraction reproduces fine's class, so the
   donor axis alone accounts for this flip;
5. fine's noise floor reads **lower** (the second bias, sign-pinned);
6. the divergence is **not** in the dangerous direction.

Assertions 1 and 4 are the ones with teeth: a refactor that quietly converged the two sensors would
fail them loudly instead of silently discarding a deliberate difference.

**Not closed by (B).** The two open axes are unchanged — fine's silent-core A RMS (decided in
direction, needs a silence predicate for 50 ms bins) and the noise-floor context window. (B) means a
divergence is now *correctly interpreted*; it does not mean the sensors are right.

**Operational consequence.** `skip_equivalent_gaps` is on by default and consumes the *scan-time*
verdict, so this gap is admitted to the fill plan as a `repairable_dropout`, runs the full bracket
search and dual-fit, and then hard-skips — while the fill-time analysis of the same gap says
`shared_silence, drop: true`. Wasted work, and an operator-facing label that is the opposite of the
truth.

This is why the parent ledger's §0 premise — two signals off the same B audio disagreeing — is
**not fully closed**. F1–F12 fixed the instances then in evidence, not the class.

**Next step.** Floor **(a)** is decided; silent-core A RMS should move with it; context / noise-floor
window is still open — see *What must be resolved first* and *Ready to implement*. Population check
**run 2026-07-30**: a class, but a rare and safe-direction one — 5/297 on each of two 17-pair
corpora, 0 dangerous. See § *Population check* under F15.

### Silent-core probes — 2026-07-30, measurement before code

The floor question is answered in *direction* **(a)** but not in *magnitude*, and the gaps it has to
work on (4/5/6/8 — the mixed ones, part quiet part content) are exactly the gaps with the thinnest
silent-bin populations. That is where a silent-core floor's behaviour is least predictable, so the
candidate is now **measured before it is adopted**, not after.

`equivalence.silent_core_probes` — additive, `Option`/`Vec`-shaped so old corpora still parse,
**never read by the classifier**. Two probes per gap (`gap_signature_bin_ms` = 50 ms, and the scan
recipe's `scan_block_ms`), each carrying `floor_db` (max over silent bins — the candidate
`gap_floor_db`), `a_rms_db` (energy mean over the same bins — the candidate A signal, the *other*
open axis, free in the same pass), and `silent_bins` / `total_bins`.

Silence filter: `is_silent_interleaved`, the scanner's own predicate. It is pure, and all four of its
inputs (`samples`, `channels`, `silence_peak_fraction`, `absolute_silence_rms`) already exist at the
fingerprint call site — there was no predicate to design. What it could **not** do is land in
`levels.gap_floor_db`: that closure is mono-RMS-only with no silence notion, and its other consumers
(`snr_db`, dual-fit's `a_gap_floor_db`) would move with it. The equivalence overlay computes its own.

Empty-set fallback (`floor_db: None` when no bin is silent) currently mirrors the scan path's
`NEG_INFINITY` fold → `None` → `NotEvaluated` → keep. That is deliberately **not** a decision yet;
the run below is what says whether the case is real.

**The run, and the three questions it must answer.** One `--gap-fingerprints` re-dump of the F15 pair
(pair index per the corpus manifest; build and recipe per § *Reproducing these runs*):

1. **Does silent-core close the band?** Compare each candidate `floor_db` against the donor's
   `rms_db` on gaps 4/5/6/8. Falsifiable: g4's floor should land near the scan path's −79.50, putting
   the −66.94 donor *above* it and flipping fine's donor read to occupied.
2. **Does bin size matter?** The two probes side by side with their silent-bin counts. If 50 ms and
   `scan_block_ms` agree, the fine path can keep its own binning and only the *filter* changes.
3. **Is the empty-set fallback hypothetical or real?** Any gap with `silent_bins == 0` makes it
   load-bearing and forces a decision before coding. None, and it stays a defensive unit test.

If (1) holds, `tests/equivalence_divergence.rs`'s two `band_donor_*` tests go red **by design** —
that is the acceptance signal, and the response is spelled out in the fixture README.

### Probe results — RUN 2026-07-30 (`fp_silent_core_probe/`, 11 gaps)

All three questions answered. **Q1 yes, Q2 no, Q3 hypothetical** — and a fourth result says the floor
must not ship alone.

| idx | scan floor | fine floor | **p50** | **p100** | silent 50 ms | silent 100 ms | donor `rms_db` | scan ds |
|---|---|---|---|---|---|---|---|---|
| 1 | −101.34 | −95.68 | −95.68 | −98.56 | 38/38 | 19/19 | −51.60 | 0.000 |
| 2 | −101.35 | −81.50 | −81.50 | −88.14 | 86/86 | 43/43 | −39.89 | 0.000 |
| 3 | −72.40 | −69.74 | −75.64 | −75.65 | 80/82 | 39/41 | −61.23 | 0.974 |
| **4** | **−79.50** | **−58.39** | **−80.96** | **−81.99** | 30/41 | 10/21 | **−66.94** | 0.474 |
| **5** | **−74.53** | **−51.03** | **−79.89** | **−80.20** | 15/23 | 8/12 | −54.81 | 0.100 |
| **6** | **−84.51** | **−58.61** | **−83.08** | **−84.87** | 18/32 | 2/16 | −67.57 | 0.533 |
| 7 | −74.01 | −78.91 | −78.91 | −79.39 | 16/23 | 8/12 | −81.10 | 0.900 |
| **8** | **−74.86** | **−58.79** | **−79.32** | **−82.01** | 16/26 | 3/13 | −67.32 | 0.167 |
| 9 | −101.48 | −94.93 | −94.93 | −98.11 | 14/14 | 7/7 | −43.77 | 0.000 |
| 10 | −101.27 | −89.66 | −89.66 | −92.62 | 3406/3406 | 1703/1703 | −109.12 | 1.000 |

(Gap 0 is `summary_na` — no B, no equivalence block, no probes.)

**Q1 — the band closes. Prediction confirmed, magnitudes included.** On the four band gaps the floor
drops **21–25 dB** (g4 −58.39 → −80.96; g5 −51.03 → −79.89; g6 −58.61 → −83.08; g8 −58.79 → −79.32),
landing within **1.4–5.4 dB** of the scan floor instead of 21–25 dB above it. The fixture's specific
prediction for g4 — "falls from −58.39 toward −79.50" — measured **−80.96**, 1.5 dB past the target.

The donor consequence follows from monotonicity rather than a second measurement: the fine donor
predicate is `bin_rms < gap_floor` (`domain/donor.rs`), so lowering the floor can only *lower* the
silent fraction. Reading each new fine floor against the scan floor whose `ds` is already known:

| idx | new fine floor vs scan floor | ⇒ fine `ds` vs scan `ds` | predicted fine class | scan class | |
|---|---|---|---|---|---|
| 3 | 3.2 dB lower | ≲ 0.974 ⇒ still silent | `shared_silence` | `shared_silence` | agrees |
| 4 | 1.5 dB lower | ≲ 0.474 ⇒ occupied | `repairable_dropout` | `repairable_dropout` | **fixed** |
| 5 | 5.4 dB lower | ≲ 0.100 ⇒ occupied | `ambient_quiet` | `repairable_dropout` | **still diverges** |
| 6 | 1.4 dB **higher** | ≳ 0.533 ⇒ still silent | `shared_silence` | `shared_silence` | agrees |
| 7 | 4.9 dB lower | ≲ 0.900 | `shared_silence` | `shared_silence` | agrees |
| 8 | 4.5 dB lower | ≲ 0.167 ⇒ occupied | `ambient_quiet` | `ambient_quiet` | **fixed** |

Pair-level: **3 divergences → 1**. The survivor is g5, and it is no longer a floor problem — its
floor is fixed and its donor now reads occupied; it diverges because fine's noise floor reads
**−64.83 against scan's −45.85**, a 19 dB spread (the widest in the table), which drags
`a_below_noise` to −22.68 and denies the dropout. g5 is the noise-floor axis's poster gap.

These are *predictions from the probes*, not post-fix verdicts. The direction is a monotonicity
argument and is sound; the exact fractions depend on donor bin shape and need the fix to land.

**Q2 — bin size does not matter for any verdict; prefer 50 ms for population.** The 100 ms floor is
uniformly ≤ the 50 ms floor (0.01–6.64 dB, median ≈ 2 dB — larger bins average more, so the max over
them sits lower). On the band gaps the two differ by 0.3–2.7 dB while the decision distance is
12–25 dB, so nothing flips either way. The tiebreak is the *population*, not the value: at 100 ms g6
keeps **2 of 16** bins and g8 **3 of 13**, versus 18/32 and 16/26 at 50 ms. Keep the fingerprint's own
`gap_signature_bin_ms`; the filter is what changes, not the binning.

**Q3 — the empty set never occurred.** Minimum silent-bin count at 50 ms is 14 (g9, fully silent);
the thinnest *fraction* is g6 at 18/32. The `None → NotEvaluated → keep` fallback stays as written and
stays a defensive unit test. It is not load-bearing and does not need designing.

**Bonus — the A-RMS axis is validated by the same data.** Fine's `a_gap_rms_db` on the band gaps is
13–23 dB too *high* (g4: −66.88 vs scan's −86.41) because it averages the whole refined span. The
silent-core mean lands at **−89.62**, within 3.2 dB of scan. Moving A RMS with the floor is confirmed,
not just assumed.

**The result that changes the plan: do not ship the floor alone.** g4 converges on a margin of
**0.41 dB** — silent-core `a_rms` −89.62 against fine's noise floor −54.21 is −35.41, and the dropout
threshold is −35.0. That convergence is real but fragile, and it is fragile *because* the noise-floor
axis is still unfixed: against scan's −44.86 floor the same gap sits 44.8 dB down, nowhere near the
threshold. The two open axes are complementary — fixing the floor moves the donor, fixing the noise
floor moves the A side — and fixing only the first leaves g4 balanced on 0.41 dB and g5 divergent.

**One expectation to correct.** Silent-core does **not** make the two floors equal. On the fully-silent
gaps (1, 2, 9, 10 — `silent_bins == total_bins`) the silent-core floor is *identical* to the current
fine floor to the last digit, and still sits **5.7–19.9 dB** above scan's. The silence filter removes
content-peak contamination and nothing else; the residual is granularity, mono downmix, and refined-
vs-core span. Anyone expecting the floors to converge after the fix will read that residual as a bug.

### Noise-floor probes — 2026-07-30, built; run pending

Same method as the silent-core probes, for the axis that is now binding. Unlike the floor, this
difference is **not a defect**: both paths take the median of context bins outside the gap
(`measure.rs` `level_profile` and `domain/gap_equivalence.rs` `derive_gap_equivalence` agree
structurally, gap excluded on both sides). They differ only in **±2 s / 100 ms** vs **±3 s / 50 ms**.
Two variables, one observed difference, so neither can be blamed yet:

- **bin size** — 50 ms bins resolve short quiet troughs (between words, between notes) that 100 ms
  blocks average over, fattening the low tail and dragging the median down. If this dominates, fine is
  measuring something real that scan cannot see, and *accept-and-document* gets much stronger.
- **window** — the extra second beside a dropout is often quieter. If this dominates, it is closer to
  an artifact and converging is easier to justify.

`equivalence.noise_floor_probes`: the cross product of `{EQUIVALENCE_CONTEXT_SECS,
gap_signature_context_secs}` × `{scan_block_ms, gap_signature_bin_ms}`, deduped, each row carrying
`floor_db` and `context_bins`. Provenance only, `Vec`-shaped, classified on by nothing.

**The anchor row.** `(2 s, scan_block_ms)` is scan's own definition and should **reproduce**
`scan_equivalence.noise_floor_db`. If it does, the variable space is closed at two and the crosses
separate them. If it does not, a third variable exists — most likely the excluded span, since fine
excludes the *refined* gap and scan the block-confirmed *core* — and it surfaces before any conclusion
rests on it. That check is the highest-value row in the grid and costs nothing extra.

Built by calling `level_profile` itself rather than re-deriving the bin walk, so a probe cannot drift
from what it characterizes. `level_profile` now returns its context-bin count as a second tuple
element; deliberately *not* a `LevelProfile` field, because that type is serialized into every dumped
gap and this is scaffolding.

**Offline was checked first and is not available.** All four combinations are derivable from a single
50 ms envelope — coarser bins recompose exactly (100 ms RMS = √(mean of the two 50 ms squares)) and
narrower windows are a subset — but the corpus carries no envelope: the writer emits a *projected*
`LevelProfile` that drops it (`project.rs`, "the RMS envelope is X (unread)"), leaving `profile_db`
empty and `bin_ms` 0. Only the two scalars survive. So the probes need A PCM and the answer needs one
re-dump of the same pair. Emitting the envelope instead would make this and every future floor
question answerable offline, at the cost of thousands of floats per gap in every dump, forever, to
save one re-run of a scaffold that is scheduled for deletion — declined.

**What the run must answer.** Which variable drives the offset; whether the anchor row reproduces
scan; whether adopting scan's definition converges g5; and how much margin g4 actually gains (it needs
more than the 0.41 dB it has under the floor fix alone).

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

(2) is **largely** the same policy as (1) — "silent core or whole refined span?" — applied to another
output, but see the refutation below; they should still move together. (3) is separate and
**decisive alone**: hold scan's A RMS and swap in only fine's noise floor → not a dropout on both
gaps (`−86.41 < −54.21 − 35` false; `−82.27 < −64.83 − 35` false).

**Net.** Adopt **(a)** for floor and carry it to fine's `a_gap_rms` (silent-core). That closes the
donor axis and part of the A-side split. The **context / noise-floor window** remains open — do not
claim F15 closed until it is decided (or deliberately accepted).

### Post-F14-fix re-run 2026-07-30 — the mechanism is now measured, and one framing is refuted

`gap-files/silence-floor/fp_post_F14_fix/` is the first run carrying **both** floors and both noise
floors on all ten characterized gaps (the "safe to do now" provenance item). Four results.

**1. The band hypothesis is confirmed directly, not inferred.** Testing the nominal donor's `rms_db`
against each path's floor:

| idx | `di_rms` | scan floor | fine floor | silent to scan | silent to fine | verdicts agree |
|---|---|---|---|---|---|---|
| 1 / 2 / 9 | −51.6 / −39.9 / −43.8 | ≈ −101 | −95.7 / −81.5 / −94.9 | no | no | ✓ |
| 3 | −61.2 | −72.4 | −69.7 | no | no | ✓ |
| **4** | −66.9 | −79.5 | −58.4 | no | **yes** | **divergent** |
| **5** | −54.8 | −74.5 | −51.0 | no | **yes** | **divergent** |
| **6** | −67.6 | −84.5 | −58.6 | no | **yes** | ✓ (fragile — see 4) |
| **8** | −67.3 | −74.9 | −58.8 | no | **yes** | ✓ (reason differs) |
| 7 / 10 | −81.1 / −109.1 | −74.0 / −101.3 | −78.9 / −89.7 | yes | yes | ✓ |

The donor sits **between** the two floors on exactly the four flagged gaps and nowhere else. Where it
is loud to both or silent to both, the paths agree. That is the entire donor axis of F15, on measured
floors. Note also that `g5`'s scan floor measures **−74.53** against the earlier derived bound of
≤ −71.9 — the bound held, and was tight.

**2. The noise-floor split is a systematic bias, not per-gap noise.** Fine's noise floor is lower than
scan's on **10 of 10** gaps: −2.3, −3.0, −3.2, −5.8, −7.8, −8.0, −9.4, −10.3, −10.7, −19.0 dB. So axis
(3) is a characterizable ~8 dB offset rather than an open unknown. Direction matters: since
`is_dropout = a < nf − 35`, a lower noise floor makes fine *less* likely to call a dropout ⇒
`ambient_quiet` / `shared_silence` ⇒ **drop**. **Both** fine-side sensor differences therefore push the
same way — toward dropping repairable gaps, the direction the CI gate exists to catch. "Fine is a
second opinion, not an oracle" now rests on ten gaps and two axes rather than one inference.

*Corroborated on two further sets, one of them permanent.* The 5 divergences of the 17-pair corpus
carry a negative `nf` delta 5/5 (§ Population check). And the three **committed curated fixtures**
that carry an equivalence verdict — harvested from a *different* corpus again — show the same sign:
−6.1, −9.2, −4.4 dB. That third set is in the repo, so the bias is now pinned by artifacts that
outlive `gap-files/`, and `tests/equivalence_divergence.rs` asserts the sign directly.

**3. Refuted — "the floor split and the A-RMS split are one policy question" was too clean.** That
model (scan filters to silent blocks, fine does not) predicts fine ≥ scan on `a_gap_rms_db`
universally. It does not hold:

- mixed gaps (4, 5, 6, 8): fine reads **13–20 dB higher** — the filter effect, as predicted;
- pure dropouts (1, 2, 9, 10): fine reads **5–8 dB lower** (e.g. −107.10 vs −101.49).

When every block is silent the filter is a no-op, and what remains is a second, **opposite** effect —
span and bin granularity (fine's refined span / 50 ms bins vs scan's core / 100 ms blocks) reading
deeper into true silence. So (1) and (2) share a *dominant* cause, not a single one. This does not
change the recommendation — the filter effect is what moves the divergent gaps — but "adopt
silent-core and the A axis aligns" overstates it, and any implementation should expect the pure-dropout
rows to move by several dB in the other direction.

**4. `g6` is one block from being a third divergence.** Its scan donor fraction is **0.533** against
the 0.5 threshold. A single block's movement makes it `repairable_dropout` against fine's
`shared_silence`. The present agreement there is luck, not robustness — do not count `g6` as evidence
that the paths agree.

**Convergence check, and its limit.** Under scan's floor, `g4`'s donor (−66.9 > −79.5) and `g5`'s
(−54.8 > −74.5) both read **occupied**, so fine would classify `repairable_dropout` — matching scan.
Adopting (a) + silent-core A RMS + scan's context does converge them. But that is close to
tautological: it converges because it makes fine equal scan. The reason to prefer scan remains what it
was — it carries the F2-fixed definition and errs safe — not this arithmetic.

**Superseded.** The earlier "corroborating separation" note (fine floor vs the −59.94 dBFS scanner
absolute threshold on `g4`/`g5`/`g6`/`g8`) was a proxy for the band test now measured directly in (1).
Kept only as the record of how the split was first spotted.

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
| **F15:** fine `gap_floor_db` + A RMS → silent-core **(a)** | **Measured and validated; ship with the noise-floor fix, not before** | Filter = `is_silent_interleaved` per bin at `gap_signature_bin_ms`. Probes measured 2026-07-30: closes the band on 4/4 gaps (floor −21 to −25 dB, within 1.4–5.4 dB of scan), 3 divergences → 1, A RMS converges to within 3.2 dB. **But** g4 then converges on a 0.41 dB margin — see § *Probe results*. |
| **F15:** noise-floor / context window (±2 s/100 ms vs ±3 s/50 ms) | **Binding axis; probes shipped, awaiting measurement** | Sole cause of the one surviving divergence (g5: fine −64.83 vs scan −45.85, 19 dB), and what makes g4's post-floor-fix margin 0.41 dB instead of ~9.8. The floor fix is not worth shipping without it. `equivalence.noise_floor_probes` now emits the `{window} × {bin}` grid — see § *Noise-floor probes*. |
| **F15:** population check via `equivalence-calibration` | **Done 2026-07-30** | 17-pair corpus **297 gaps / 5 divergent / 0 dangerous**, reproduced on a second 17-pair set at another recipe. Sets the severity and unblocks the fork below. |
| **F15 fork:** (A) converge the sensors vs (B) keep them different and fix the *interpretation* | **(B) DONE 2026-07-30** | See § *(B) applied* below. No behaviour change — the fine block was already read by nothing in the plan/patch path. |
| **F15:** curated fixture with a band-donor gap | **DONE 2026-07-30** | `tests/gap_corpus/fingerprints/equivalence_divergence/band_donor.json` + `tests/equivalence_divergence.rs` (4 tests). |
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
