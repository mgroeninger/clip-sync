# Equivalence divergence — findings ledger

> # ARCHIVED 2026-07-30 — closed, do not update
>
> **F14** is fixed and media-validated. **F15**'s three fine-path fixes — silent-core floor + A RMS,
> interleaved reduction, and span → block-confirmed core — are implemented and media-validated by the
> combined re-dump (§ *Combined re-dump*). Sensor convergence, median `|fine − scan|`: A RMS
> **0.101 dB**, floor **0.279**, donor fraction **0.012**, noise floor **2.129** (was one-signed to −19).
>
> Three items were open at archival time and were **split into their own live ledger** rather than
> resolved here: [TEMP-equivalence-instrument-convergence.md](TEMP-equivalence-instrument-convergence.md)
> — equivalence **bin size** (I1), noise-floor **context window** (I2), and the donor predicate's
> missing `b.silent ||` **disjunct** (I3, still unmeasured). All three are the same axis: the two
> front-ends now share corrected sensor *definitions* but sample them with different instruments.
>
> Kept for the rationale, which survives nowhere else: the probe-then-fix method and its results
> (`fp_silent_core_probe/`, `fp_silent_core_floor_probe/`, `..._reduction/`), the Cauchy–Schwarz
> argument that closed the fully-silent residual by *proving* the sample sets differ, the
> reduction-vs-window-vs-span decomposition of the noise-floor axis, and the retracted claims —
> including the § *Probe results* class prediction (*3 divergences → 1*) that the combined re-dump
> **refuted**, and why: it reasoned from a donor's *mean* level where the classifier consumes a
> *per-bin fraction*. Retracted claims are marked in place rather than deleted; the ⚠ callouts are
> part of the record.
>
> Outbound doc links are relative to `docs/dev/archive/`.

**Opened:** 2026-07-30. **Status:** **F14** border alignment **FIXED and media-validated** (dump A
borders = `mono(refined ± w)` like `try_dual_fit`; `fp_post_F14_fix/` confirms). **F15** OPEN but
**fully specified**: its donor mechanism is **measured** on that same run (donor in the band between
the two floors on exactly the divergent gaps), and its noise-floor axis is **decomposed into three
variables** — channel reduction (dominant, confirmed), window/bin (median 2.1 dB), and span. All
**three fine-path fixes** — silent-core floor + A RMS, interleaved reduction, and span → block-confirmed
core — are **implemented and media-validated** (2026-07-30); see § *The three F15 fixes* and
§ *Combined re-dump*. The sensors converged (A RMS to a 0.101 dB median, floor 0.279, donor 0.012), but
the pair still carries **3 class divergences**, all traceable to the one leg left open on purpose:
**window/bin**. What remains is a policy call on that leg — no longer cosmetic, since it is now the sole
source of *action* divergence. Retracted claims are marked in place rather than deleted.

Split out of [TEMP-silence-floor-findings.md](TEMP-silence-floor-findings.md) when
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
**Severity: high → medium (2026-07-30, after the population check). Status: OPEN but SPECIFIED — all
three fine-path fixes (silent-core floor + A RMS, interleaved reduction, span → block-confirmed core)
are specified and none is blocked on a measurement; only the window/bin *policy* leg is undecided,
and it blocks nothing. See § *The three F15 fixes*.** Mechanism
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
`levels.noise_floor_db −64.830` (fine), `levels.bin_ms 0` with an empty `profile_db`.

**Resolved — not a reporting gap.** The empty envelope is deliberate: the corpus writer emits a
*projected* `LevelProfile` that drops the RMS envelope (`project.rs`, `bin_ms: 0`,
`profile_db: Vec::new()`), keeping only the scalars. Tier-3 was on and behaved correctly. The
consequence is real but is a design choice, not a bug: **offline envelope analysis is impossible from
a dump**, which is why the noise-floor question needed a re-run rather than a re-read (see § *Offline
was checked first and is not available*).

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

**Not closed by (B).** (B) means a divergence is now *correctly interpreted*; it does not mean the
sensors are right. The three fine-path defects it left unfixed are now **specified** (silent-core
floor + A RMS, interleaved reduction, span → block-confirmed core) — see § *The three F15 fixes*.
What (B) still does not settle is the window/bin *policy* leg (converge vs accept-and-document), and
that leg blocks none of the three.

**Operational consequence.** `skip_equivalent_gaps` is on by default and consumes the *scan-time*
verdict, so this gap is admitted to the fill plan as a `repairable_dropout`, runs the full bracket
search and dual-fit, and then hard-skips — while the fill-time analysis of the same gap says
`shared_silence, drop: true`. Wasted work, and an operator-facing label that is the opposite of the
truth.

This is why the parent ledger's §0 premise — two signals off the same B audio disagreeing — is
**not fully closed**. F1–F12 fixed the instances then in evidence, not the class.

**Next step.** Floor **(a)** is decided and silent-core A RMS moves with it; the noise-floor axis has
since been decomposed into reduction (dominant), window/bin, and span, leaving only the window/bin
*policy* leg undecided — see § *The three F15 fixes* and *Ready to implement*. Population check
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

> **⚠ Refuted by the combined re-dump (2026-07-30).** The prediction below did not hold: the pair
> still carries **3** divergences (g4, g5, g8), not 1. The probes were right about the *floor* — g4's
> fine floor did collapse 18 dB as predicted — but the class did not follow, because the donor axis is
> a per-bin fraction and the probes reasoned about levels. See § *Combined re-dump, 2026-07-30*. The
> g5 diagnosis below survives intact. Everything else in this subsection is superseded.

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
content-peak contamination and nothing else.

> **Superseded 2026-07-30.** This paragraph originally attributed that residual to "granularity, mono
> downmix, and refined-vs-core span". It is **span**, and the downmix term is signed *against* the
> residual rather than toward it (Cauchy–Schwarz: a downmix can only read *lower*), which makes the
> unexplained budget larger, not smaller. Decomposed in *The fully-silent residual* below.

### Noise-floor probes — 2026-07-30, built and run

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

> **Outcome 2026-07-30.** It did not reproduce scan, a third variable did exist, and it was **not** the
> span guessed at above — it was the **channel reduction**. The anchor is now
> `(2 s, scan_block_ms, Interleaved)`; with the reduction dimension added it reproduces scan on 7/10
> gaps at ≤ ±0.78 dB. The two-variable grid described in this section is therefore the *pre*-reduction
> design; read it as history and see *reduction CONFIRMED* below for the current one.

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
more than the 0.41 dB it has under the floor fix alone). — **All four answered**; the first two by the
run below, the last two by the reduction run after it.

### Probe results — RUN 2026-07-30 (`fp_silent_core_floor_probe/`)

**The anchor row does not reproduce scan. There is a third variable, and it is the larger one.**
(Identified in the *next* run as the channel reduction — this section is the evidence that a third
variable exists, not the identification of it.)

| idx | scan `nf` | **(2 s, 100 ms)** anchor | Δ anchor−scan | (2 s, 50 ms) | (3 s, 100 ms) | (3 s, 50 ms) | fine `nf` |
|---|---|---|---|---|---|---|---|
| 1 | −46.74 | −54.70 | **−7.96** | −59.47 | −54.70 | −57.40 | −57.40 |
| 2 | −34.56 | −37.70 | **−3.13** | −38.23 | −37.34 | −37.54 | −37.54 |
| 3 | −49.34 | −55.33 | **−5.98** | −57.15 | −55.33 | −57.15 | −57.15 |
| 4 | −44.86 | −51.81 | **−6.95** | −52.93 | −51.95 | −54.21 | −54.21 |
| 5 | −45.85 | −51.70 | **−5.85** | −59.65 | −62.21 | −64.83 | −64.83 |
| 6 | −56.58 | −62.18 | **−5.60** | −64.37 | −64.36 | −66.89 | −66.89 |
| 7 | −61.58 | −68.91 | **−7.33** | −72.83 | −68.10 | −69.54 | −69.54 |
| 8 | −61.58 | −66.71 | **−5.13** | −66.20 | −64.67 | −64.77 | −64.77 |
| 9 | −46.59 | −51.46 | **−4.87** | −52.61 | −51.46 | −52.41 | −52.41 |
| 10 | −56.63 | −62.19 | **−5.57** | −63.56 | −58.45 | −58.93 | −58.93 |

The probe machinery itself is validated: the `(3 s, 50 ms)` row reproduces `equivalence.noise_floor_db`
to the last digit on **10/10** gaps. So the anchor's 3.1–8.0 dB shortfall is a real third variable, not
probe error, and it is **uniformly signed** — the fine side reads lower on every gap.

**Decomposition of the fine-vs-scan noise-floor spread.** `Δ_reduction` = anchor − scan (what survives
after matching window *and* bin); `Δ_window/bin` = fine − anchor:

| idx | total fine−scan | Δ reduction | Δ window/bin | dominant |
|---|---|---|---|---|
| 1 | −10.66 | −7.96 | −2.70 | reduction |
| 2 | −2.98 | −3.13 | +0.16 | reduction |
| 3 | −7.81 | −5.98 | −1.82 | reduction |
| 4 | −9.35 | −6.95 | −2.40 | reduction |
| **5** | **−18.98** | −5.85 | **−13.13** | **window/bin** |
| 6 | −10.31 | −5.60 | −4.71 | reduction |
| 7 | −7.96 | −7.33 | −0.63 | reduction |
| 8 | −3.19 | −5.13 | +1.94 | reduction |
| 9 | −5.82 | −4.87 | −0.95 | reduction |
| 10 | −2.30 | −5.57 | +3.26 | window/bin |

Median |Δ reduction| ≈ 5.7 dB, median |Δ window/bin| ≈ 2.1 dB. **Reduction dominates on 8 of 10.**
The exception is g5 — the surviving divergence — where window/bin accounts for −13.13 of −18.98
(bin alone −7.95, window alone −10.51; strongly non-additive, so its context is very non-stationary).
So g5 remains the window/bin axis's poster gap; it is just no longer representative.

#### The third variable is almost certainly the multichannel reduction

Not window, not bin, not the excluded span — **how the six channels are collapsed to one number**:

- **scan** — `block_rms_db` → `rms_f32(block)` over *all interleaved samples*: a **power** mean across
  channels. Its doc says so outright: "Downmix-agnostic: the RMS is taken over all interleaved
  samples, matching the scan's own `is_silent` energy."
- **fine** — `mono_rms` averages the channels per frame *then* squares: an **amplitude** mean, i.e. a
  mono downmix. Same for `gap_interior_rms_db` via `interleaved_to_mono`.

The gap between them is set by the **zero-lag cross-correlation between the channel waveforms**. With
equal per-channel power and mean pairwise correlation `ρ̄` over `N` channels:

```
R_fine² / R_scan² = (1 + (N−1)·ρ̄) / N
```

- `ρ̄ = 1` → 0 dB. Requires the channels be *identical waveforms* sample-for-sample (mono duplicated
  into 5.1, or a hard-centred mix), not merely similar.
- `ρ̄ = 0` → `−10·log10(N)` = **−7.78 dB** at the 6 channels this pair carries. The sum grows as `√N`
  against a divisor of `N`.
- `ρ̄ → −1/(N−1)` = −0.2 → −∞ dB, total cancellation.

**7.78 dB is not a ceiling** — it is the `ρ̄ = 0` point, and the curve continues past it. Inverting the
measured penalties for `N = 6` gives the implied `ρ̄`:

| gap | penalty dB | `ρ̄` | | gap | penalty dB | `ρ̄` |
|---|---|---|---|---|---|---|
| g2 | 3.13 | +0.384 | | g5 | 5.85 | +0.112 |
| g9 | 4.87 | +0.191 | | g3 | 5.98 | +0.103 |
| g8 | 5.13 | +0.168 | | g4 | 6.95 | +0.042 |
| g10 | 5.57 | +0.133 | | g7 | 7.33 | +0.022 |
| g6 | 5.60 | +0.131 | | g1 | 7.96 | **−0.008** |

Every value lands in `[−0.01, +0.38]` — mildly-correlated to decorrelated 5.1, with g2 (loudest, most
centre-dominated) the most correlated. That is the signature of a channel-reduction artifact, not of
the audio being quieter.

Two caveats on reading the penalty:

- **The sign is a theorem, not an observation.** By Cauchy–Schwarz `(Σ x_c)² ≤ N·Σ x_c²` pointwise, so
  `R_fine ≤ R_scan` *always*, with equality iff all channels are identical at every sample. The
  "uniformly signed" result is therefore nearly free evidence — it could have falsified the hypothesis
  but its holding confirms little.
- **Decorrelation is not the only route to the full 7.78 dB.** One active channel over `N−1` digitally
  silent ones gives the same `1/N` ratio (`R_scan = r/√N`, `R_fine = r/N`). Centre-only dialogue reads
  identically to fully decorrelated ambience. The penalty measures *coherent-sum gain*, which both
  correlation and power concentration move; only genuine inter-channel identity collapses it to zero.

**Why it does not cancel.** Both sides of `a_below_noise` are downmixed, so the penalty looks like it
should subtract out — but it is level-dependent, not constant. Inside a silent gap the channels are at
or near the floor and the penalty is ~0; in the surrounding context it is 3–8 dB. So the penalty
applies almost entirely to the noise floor, lifting `a_below_noise` toward zero and pushing gaps *out*
of `repairable_dropout`. That is precisely the bias direction measured all along, and it means the
noise-floor bias was mostly never about the context window at all.

**This reclassifies the axis.** The window/bin split is a legitimate design difference and the
accept-vs-converge argument stands. A mono downmix used as a *level* measurement on multichannel
material is closer to the `gap_floor_db` situation — a defect, not a choice — because the number it
produces depends on inter-channel correlation rather than on loudness. Downmixing is right for the
correlation work `mono_rms` was built for; it is wrong for a noise floor.

**Confidence: CONFIRMED by direct measurement — see § *Probe results* below.** Two framings written
here before the run were wrong and are retracted. First, an earlier revision flagged g1's 7.96 dB as
"0.2 dB over the predicted ceiling" and withheld confidence on that basis: 7.78 dB is the `ρ̄ = 0`
point, not a bound. Second, and only visible after the run, g1's inferred `ρ̄ = −0.008` was itself an
artifact of the residual — **measured directly it is +0.012, and no gap in the pair is anti-correlated
at all.** Every `ρ̄` in this section is inferred from the fine−scan *difference*; prefer the directly
measured column in the results section below wherever the two disagree.

**The decisive test is one more probe row — BUILT and RUN 2026-07-30; it landed.** `NoiseFloorProbe` now
carries a `reduction: ChannelReduction` field (`Interleaved` = scan's `rms_interleaved` power mean,
`Downmix` = fine's `mono_rms` amplitude mean), and the grid is the full cross product
`{2 s, 3 s} × {scan_block_ms, 50 ms} × {Interleaved, Downmix}` — 8 rows, still provenance-only, still
classified on by nothing. `serde(default)` = `Downmix`, so dumps predating the field deserialize as
what they in fact recorded.

The anchor moves to **`(2 s, scan_block_ms, Interleaved)`**, which matches scan on all three variables
and should reproduce `scan_equivalence.noise_floor_db` to within ~1 dB on all 10 gaps. Its `Downmix`
twin is the old anchor, so the reduction term is now read **directly as the difference between two
adjacent rows** rather than inferred from a residual.

The math is pinned by unit tests rather than left to the run: six identical channels agree to <0.01 dB;
six mutually-orthogonal equal-power channels differ by `10·log10(6)` to <0.05 dB; **one active channel
over five silent ones hits the same 7.78 dB** (the concentration-vs-decorrelation trap); and
`Downmix ≤ Interleaved` holds across a sweep of frequency/amplitude configurations. Deterministic tone
beds at multiples of 20 Hz make the orthogonality exact over a 50 ms bin — no PRNG, no statistical
tolerance.

It landed on 7 of 10 gaps at ≤±0.78 dB, and the three that missed are estimator instability rather
than a fourth variable — see § *Probe results* below. The variable space for the noise-floor axis is
closed at three.

~~**Do not act on the window/bin axis before this is settled.**~~ **Settled 2026-07-30** — reduction
confirmed below; the window/bin leg is now a free *policy* call (median 2.1 dB once reduction is
matched) and does not block the three fixes. The caution still holds historically: before the
reduction run, any accept-vs-converge judgement would have been made against a number that was mostly
a downmix artifact.

### Probe results — RUN 2026-07-30 (`fp_silent_core_floor_probe_reduction/`): reduction **CONFIRMED**

The anchor test landed. `(2 s, scan_block_ms, Interleaved)` vs `scan_equivalence.noise_floor_db`:

| gap | scan NF | **anchor** | err | downmix twin | old err | reduction | measured `ρ̄` |
|---|---|---|---|---|---|---|---|
| g1 | −46.74 | −47.16 | **−0.42** | −54.70 | −7.96 | 7.54 | +0.012 |
| g2 | −34.56 | −34.05 | **+0.51** | −37.70 | −3.13 | 3.65 | +0.318 |
| g3 | −49.34 | −49.74 | **−0.40** | −55.33 | −5.98 | 5.59 | +0.132 |
| g4 | −44.86 | −45.64 | **−0.78** | −51.81 | −6.95 | 6.16 | +0.090 |
| g5 | −45.85 | −43.80 | +2.05 | −51.70 | −5.85 | 7.89 | −0.005 |
| g6 | −56.58 | −54.41 | +2.17 | −62.18 | −5.60 | 7.77 | +0.000 |
| g7 | −61.58 | −61.99 | **−0.42** | −68.91 | −7.33 | 6.91 | +0.044 |
| g8 | −61.58 | −61.25 | **+0.33** | −66.71 | −5.13 | 5.46 | +0.142 |
| g9 | −46.59 | −46.86 | **−0.27** | −51.46 | −4.87 | 4.60 | +0.216 |
| g10 | −56.63 | −54.41 | +2.22 | −62.19 | −5.57 | 7.78 | +0.000 |

**Switching one variable collapsed the bias.** Error goes from a uniform 3.13–7.96 dB (the old downmix
anchor, every gap wrong in the same direction) to **−0.78 … +0.51 on 7 of 10, median +0.03**. Nothing
else changed between the two runs. The multichannel reduction is the dominant term in the noise-floor
divergence, and the axis is no longer "strongly indicated" — it is measured.

**The reduction term is now read directly rather than inferred**, and the direct numbers supersede the
inferred ones: **3.65–7.89 dB** (was 3.13–7.96), `ρ̄` **+0.318 … −0.005** (was +0.384 … −0.008). The
correction is small but tidy — **no gap is actually anti-correlated**. g1, the one that provoked the
"over the ceiling" scare, measures `ρ̄ = +0.012`, not −0.008; its apparent overshoot was residual, not
content. The `ρ̄ = 0` gaps (g5, g6, g10 at 7.77–7.89) are the genuinely decorrelated ones.

**Zero-drift check passes.** The `Downmix` rows and all `silent_core_probes` are byte-identical to
`fp_silent_core_floor_probe/` across all 10 gaps, confirming the new dimension is additive.

#### The three +2.1 dB residuals are estimator instability, not a fourth variable

g5/g6/g10 sit at +2.05/+2.17/+2.22 — a 0.17 dB spread, which looks exactly like a systematic offset. It
is not. A fixed span or exclusion offset would be **stable across window and bin**; these are not:

| gap | 2 s/100 | 3 s/100 | 2 s/50 | 3 s/50 |
|---|---|---|---|---|
| g5 | +2.05 | **−8.57** | −6.57 | **−11.17** |
| g6 | +2.17 | −0.34 | −0.37 | −3.13 |
| g10 | +2.22 | **+5.96** | +0.85 | **+5.44** |

g5's noise floor moves 10.6 dB when the window goes 2 s → 3 s. These three are the **three most
window-unstable contexts in the pair** (Spearman 0.71 between window/bin instability and |anchor
error|; instabilities 13.2 / 5.3 / 5.1 dB against 0.8–4.9 for the seven that pass). Where content
enters and leaves inside the context window, the median is not a stable estimate of anything, and a
±2 dB disagreement between two nearly-identical recipes is the expected behaviour of the estimator —
not evidence of a missing variable.

**This retracts the span-provenance probe recommended in the previous section.** It was the right next
step against the hypothesis that the residual was a fixed exclusion offset; the residual is not fixed,
so span provenance would not explain it. Refinement independently argues the same way: `a_refined_*`
equals `a_start/a_end` on 9 of 10 gaps (g10 differs by 10.7 ms), so refined-vs-raw is ~0 here
regardless. **Do not build it for this axis.** It may still be worth building for the *gap-floor* axis
below, which is a different statistic with a different problem.

### The fully-silent residual — decomposed 2026-07-30, and it is worse than it looked

Raised in review as "granularity + span (+ mono) is doing too much work without a decomposition."
Confirmed, and the decomposition makes the problem **larger**, not smaller. Computed offline from
`fp_silent_core_floor_probe/` — no re-run needed, because `sc@100` is measured at scan's *own* bin size.

The premise the review reasoned from — *"in true silence the downmix penalty should be ~0"* — does not
hold, and the correction cuts the other way. `is_silent_interleaved` admits bins with real low-level
content (`peak < absolute_silence_rms`, here 0.001 ⇒ −59.9 dBFS, or `rms < peak × 0.01`), not digital
silence. A programme's noise floor is the *least* correlated material it has — independent dither and
room noise per channel — so `ρ̄ ≈ 0` and the reduction penalty sits near its 7.78 dB **maximum**, not
near zero. And it is signed **against** the residual: scan reads interleaved, silent-core reads downmix,
so `Downmix ≤ Interleaved` means reduction should push silent-core *below* scan.

`sc@100` differs from `scan_equivalence.gap_floor_db` only in reduction, span (refined vs
block-confirmed core) and bin phase — bin size and predicate are identical. The population splits
cleanly on whether the silent-core filter did anything at all:

| | gaps | `sc@100 − scan` | reduction predicts | verdict |
|---|---|---|---|---|
| **Partial** (filter bites) | g3–g8 | **−7.15 … −0.36** | −7.78 … 0 | every gap inside the band — consistent |
| **Fully silent** (filter is a no-op) | g1, g2, g9, g10 | **+2.78 … +13.20** | −7.78 … 0 | every gap outside it, wrong sign |

On the partial gaps the post-fix residual *is* the reduction term, and the two fixes together predict
near-zero. On the fully-silent gaps span and phase must supply the observed residual **plus** whatever
reduction subtracted — **up to ~21 dB on g2**, against an unexplained budget previously stated as 13.2.

**The floor fix is a provable no-op on these four gaps**, which is why the residual survives it:
`silent_bins == total_bins`, so the filter selects everything and `fine gap_floor_db == sc@50` to
0.00 dB on all four. Any post-fix residual here is *entirely* untouched by F15's decided fix.

**Granularity is measured, not assumed, and it is a phase proxy.** `sc@50 − sc@100` is a pure bin-size
term — same span, same predicate, same reduction:

| gap | granularity | residual | ratio |
|---|---|---|---|
| g1 | 2.88 | 2.78 | 1.0 |
| g9 | 3.18 | 3.37 | 1.1 |
| g2 | 6.65 | 13.20 | 2.0 |
| g10 | 2.96 | 8.65 | 2.9 |

Granularity accounts for essentially all of g1 and g9 and about half of g2. This is also why the
gap-floor axis behaves so differently from the noise-floor axis, where the window/bin term is ~2.1 dB:
`noise_floor_db` is a **median** (robust — halving the bin barely moves it) while `gap_floor_db` is a
**max** (not robust — a finer bin resolves a transient the coarser one averages away, without bound).
Any intuition carried over from the ±2 dB noise-floor term will understate this axis.

**Unexplained and worth its own probe: scan's floor is pinned.** On all four fully-silent gaps scan
reads **−101.34, −101.35, −101.48, −101.27** — a 0.21 dB spread across four unrelated gaps spanning
0.7 s to 170 s, while fine over the same spans varies 13 dB (−95.68 … −81.50). A statistic that
constant is not reading the gap's content; it looks like a fixed artifact floor. That is a hypothesis,
not a finding — it is not the `absolute_silence_rms` cap (−59.9 dBFS) and not `BLOCK_LEVEL_FLOOR_DB`
(−120). Until it is identified, **do not treat the post-fix residual on fully-silent gaps as expected
noise**; the two paths may not be measuring the same quantity there at all.

#### CLOSED offline 2026-07-30 — it is a span delta, and it is provable

Both open questions above were answered from the dumps already in hand. **No re-dump was needed for
either.**

**The residual is *proof* of a span difference, not a hypothesis about one.** `sc@100` is a downmix max
and `scan.gap_floor_db` an interleaved max at the *same* 100 ms bin size. Cauchy–Schwarz makes
`Downmix ≤ Interleaved` on the same samples — the invariant already pinned by
`downmix_never_reads_above_interleaved`. On the six partial gaps the inequality holds (−7.15 … −0.36).
On all four fully-silent gaps it is **violated** (+2.78 … +13.20). A violated theorem means the sample
sets differ. Nothing else can produce it.

**The block counts locate the delta.** On fully-silent gaps every block is silent, so scan's
`a_gap_silent_blocks` × 100 ms *is* its measured span. (Read the **A-side** count, not
`donor_total_blocks` — the two coincide on these four gaps and diverge on six others, so the donor
field will appear to work and then quietly mislead.)

| gap | scan span | fine refined span | delta | residual |
|---|---|---|---|---|
| g1 | 1.70 s (17 blk) | 1.86 s (19 bins) | **200 ms** | +2.78 |
| g2 | 4.10 s (41 blk) | 4.26 s (43 bins) | **200 ms** | +13.20 |
| g9 | 0.60 s (6 blk) | 0.66 s (7 bins) | **100 ms** | +3.37 |
| g10 | 170.20 s (1702 blk) | 170.25 s (1703 bins) | **100 ms** | +8.65 |

One to two 100 ms blocks at the gap edges — where content ramps — move a *max* statistic by 2.78 to
13.20 dB. Scan's span is the narrower one, and it is the **more correct** one for a floor: it excludes
the ramp. Fine's refined span includes it. This is a third F15 defect, distinct from the silent-core
filter and from the reduction, and it is the only one of the three where scan is right on the merits
rather than merely authoritative.

**The −101.3 pin is a decode/dither floor, not an edge block.** Scan's floor is a max; its
`a_gap_rms_db` is the energy mean over the same population. On all four gaps they differ by
**0.13–0.27 dB**, over populations of 6, 17, 41 and 1702 blocks. A max sitting 0.19 dB above the energy
mean of 1702 blocks means every block is at essentially one level. So it is not a single edge or hold
residue — it is a constant floor, at the same value, at four points spread across a 6900 s programme.
That is a decode artifact (AAC-decoded digital silence), and scan is reading it correctly. **Question B
is answered; do not build a probe for it.** A source-level test is only worth writing once the constant
itself is identified — do not golden −101.3.

**The span rule is source-readable, so fix 3 needs no further measurement to specify.** Established
2026-07-30 by reading `derive_gap_equivalence` rather than by probing. Scan's population is

```rust
a_levels.iter().filter(|b| b.silent && block_center(b) >= a_start_secs && block_center(b) < a_end_secs)
```

— *silent* blocks whose **centre** falls in the **raw** gap span, on the scanner's media-absolute block
grid. Fine instead tiles the **refined** span from its own start and keeps every bin including a trailing
partial one. Two concrete differences follow, and they are exactly the 1–2 blocks measured above:

- **grid anchoring.** Scan's bins are phase-locked to the media origin; fine's are phase-locked to the
  gap start. Predicting fine's bin totals with scan's centre-containment rule matches on only 6/10 gaps,
  which is the direct evidence the two grids are out of phase rather than merely offset.
- **the trailing partial bin.** Fine's last bin is short and scan has no equivalent; centre-containment
  discards it by construction.

Both are decided by the predicate above, not by anything a probe could tell us. Note also that
`refined == raw` on 9/10 gaps of this pair, so refined-vs-raw is the *smaller* half of the span delta —
the grid is the larger one. That ordering was assumed backwards earlier in this document.

**Span provenance (item A) is downgraded to optional.** Its job was to decide span-vs-predicate; the
block counts already decided it, and the counts also show the predicate agreed (scan 17/17 silent, fine
19/19 — both call everything silent). It would still confirm that the arg-max lands in the edge blocks,
which is a nice-to-have, not a blocker. **The fix does not wait on it.**

**Pinned in tests, no media** (`application/gap_equivalence.rs`, reviewer item C):
`silent_core_floor_is_set_by_the_span_not_only_the_content` (planting a 20 dB edge two blocks wide
moves the floor ~20 dB), `silent_core_downmix_floor_never_exceeds_interleaved_on_the_same_span` (the
invariant whose violation exposed all of this), and
`silent_core_floor_carries_the_full_downmix_penalty_when_decorrelated` (`10·log10(6)` on a max, not a
median).

### Residual: donor window alignment — opened early, never re-closed

Also from review. The early donor work found the fractions differ on **window × bin × predicate**; the
later work closed the predicate/floor axis and the *window* leg was never re-checked. It is plausibly
non-load-bearing on this pair once the floor fix lands — the band mechanism explains the observed
flips without it — but "plausibly non-load-bearing" is not "closed", and it is not currently listed
anywhere as open. It is now.

#### Measured offline 2026-07-30 — load-bearing on exactly 2 of 10 gaps, and one is the fixture

Done without waiting for the floor fix to land in code (reviewer item E), using
`donor_total_blocks` × 100 ms as scan's window and `silent_core_probes[].floor_db` as the offline
threshold.

**The window delta is one block.** Scan's donor window is 0.05–0.17 s narrower than the gap on all ten
gaps — block-grid truncation, ~1 × `scan_block_ms`. Small, and on most gaps irrelevant.

**But `donor_silence_fraction` is quantized by that same block count, and two gaps sit within one block
of the 0.5 threshold:**

| gap | scan `ds` | as blocks | 1 block | distance to 0.5 | straddles? |
|---|---|---|---|---|---|
| g4 | 0.474 | 9/19 | 0.053 | 0.026 | **yes** |
| g6 | 0.533 | 8/15 | 0.067 | 0.033 | **yes** |
| g5 | 0.100 | 1/10 | 0.100 | 0.400 | no |
| g8 | 0.167 | 2/12 | 0.083 | 0.333 | no |
| g3, g7, g10 | 0.974 / 0.900 / 1.000 | — | — | ≥ 0.4 | no |

So the answer is neither "load-bearing" nor "non-load-bearing" — it is **decision-relevant on g4 and
g6 only**, and for a reason worth stating plainly: with 15–19 donor blocks, one block is 5–7 % of the
fraction, and the threshold is a hard 0.5. A one-block window difference is a class flip on those two.

**g4 is `band_donor.json`.** The committed fixture's donor axis sits 0.026 from the threshold with a
0.053 quantum. That does not invalidate the band mechanism — the floor split there is 21 dB, far
larger than one block of donor — but it does mean the fixture's `keep` verdict is one block of window
alignment away from flipping, independently of the floor. Worth recording on the fixture.

**The floor fix moves all four disagreeing gaps the right way**, checked by floor proximity against the
offline silent-core threshold: g4 (−66.94 vs −80.96), g5 (−54.81 vs −79.89), g6 (−67.57 vs −83.08),
g8 (−67.32 vs −79.32) — every donor is 12–25 dB *above* the silent-core floor, so fine re-reads them
**occupied**, moving its fraction down toward scan's. The predicate/floor axis does the work; the
window is a tie-breaker that only matters at the threshold.

**Recommended close:** do not chase window alignment as a general defect. Instead assert the narrow
thing that is true — same-floor/different-window must not flip the class on g4 — as a unit test, and
leave the axis closed with the quantization noted.

### Two framings to keep straight

Both flagged in review as easy misreads, and both are already stated correctly elsewhere in this
document — repeated here because they are the two places a hurried reader goes wrong.

**The corpus's 5/297 is a class-shape match, not a mechanism match.** Those rows share the keep→drop
direction, the reason-only pattern and the noise-floor sign. They carry **no floor provenance**, so
they are *consistent with* the band-donor mechanism and are not evidence of it. Do not let "matches
F15 exactly" harden into "same cause as `band_donor.json`" — that claim needs floor provenance the
population run does not dump.

**(B) is about authority, not about leaving fine broken.** (B) settled *which front-end is
authoritative* — scan is, and the fine block is read by nothing in the plan/patch path. It did not
settle that fine's measurements are fine as they are. The silent-core and reduction work fixes a
**diagnostic** that misreports, which is orthogonal to (B) and does not reopen it.

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
[gap-vocabulary.md](../gap-vocabulary.md). An early draft that keyed only on eligible failure class +
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
3. **Noise floor** — ±2 s / 100 ms blocks vs ±3 s / 50 ms bins. On `g4`: scan −44.86, fine −54.21
   (**9.4 dB**). On `g5`: scan **−45.85**, fine −64.83. *Since decomposed:* the window/bin difference
   named here is the **smaller** term (median 2.1 dB); the dominant one is the **channel reduction**,
   which this list did not know about. See § *Probe results … reduction CONFIRMED*.

(2) is **largely** the same policy as (1) — "silent core or whole refined span?" — applied to another
output, but see the refutation below; they should still move together. (3) is separate and
**decisive alone**: hold scan's A RMS and swap in only fine's noise floor → not a dropout on both
gaps (`−86.41 < −54.21 − 35` false; `−82.27 < −64.83 − 35` false).

**Net.** Adopt **(a)** for floor and carry it to fine's `a_gap_rms` (silent-core). That closes the
donor axis and part of the A-side split.

> **Updated 2026-07-30.** This paragraph used to end "the **context / noise-floor window** remains
> open — do not claim F15 closed until it is decided". The noise-floor axis is now **decomposed and
> closed at three variables** (reduction ≫ window/bin ≈ span), with reduction confirmed by direct
> measurement. What remains open is not a *measurement* but a *decision on the window/bin leg*
> (converge vs accept-and-document, median 2.1 dB) — and that decision does not block the three fixes
> below, none of which touch it. F15 is still not "closed"; it is now **specified**.

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
that the paths agree. *Since quantified:* `g6` and `g4` are the **only** two gaps of the pair that
straddle 0.5 within one block (quanta 0.067 and 0.053; every other gap sits ≥ 0.4 away), which is
exactly why the donor-window residual is decision-relevant on those two and nowhere else — see
§ *Residual: donor window alignment*.

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

### The three F15 fixes — specified, evidenced, and unblocked (2026-07-30)

All three fine-path sensor defects are now **fully specified**, and **none is waiting on a
measurement**. That claim is load-bearing, so here is what backs each one and — more usefully — what
would have to be true for it to be wrong.

Common to all three: they change a **diagnostic**, not a decision. `(B)` settled that scan is
authoritative and that fine's equivalence block is read by nothing in the plan/patch path, so these
land without a behaviour change to production output. That is *why* they can ship on measurement this
thin.

| # | Fix | Specified as | What specifies it | Blocked on? |
|---|---|---|---|---|
| 1 | **silent-core floor + A RMS** | filter each bin with `is_silent_interleaved`; `gap_floor_db` = max over silent bins, `a_gap_rms` = energy mean over the same; empty set ⇒ `None` | already **implemented and run twice** as `silent_core_probe` | **No** — the code exists and has media results |
| 2 | **interleaved reduction** | replace the amplitude-mean downmix with `rms_interleaved` (interleaved power mean) on the equivalence read | `ChannelReduction::Interleaved`, run side-by-side with its `Downmix` twin | **No** — confirmed by direct measurement |
| 3 | **span → block-confirmed core** | silent blocks whose **centre** lies in the raw `[a_start, a_end)` on the media-absolute block grid; no trailing partial bin | the predicate in `derive_gap_equivalence`, read from source | **No** — it is a source-readable rule, not an empirical one |

**Fix 1 is unblocked because the code already ran.** `silent_core_probe` is not a design; it is a
working implementation that has been through two media runs. It closes the band on **4/4** flagged
gaps (floor drops 21–25 dB, landing within 1.4–5.4 dB of scan), takes divergences **3 → 1**, and
converges A RMS to within 3.2 dB. Adopting it means promoting a probe to the measurement path. The one
thing it does *not* do is close the fully-silent gaps — proved, not assumed, since the filter is a
provable no-op where `silent_bins == total_bins`. That is fix 3's job, which is why they are separable
but should not be *evaluated* separately.

**Fix 2 is unblocked because the anchor now reproduces scan.** The test was pre-registered before the
run: the `(2 s, scan_block_ms, Interleaved)` row matches scan's recipe on all three variables and must
therefore reproduce `scan_equivalence.noise_floor_db`. It does, on **7/10** gaps, error **−0.78 … +0.51
dB, median +0.03** — against a pre-reduction anchor that was wrong by **3.13–7.96 dB on 10/10, always
the same direction**. A systematic bias collapsing to centred sub-dB noise on the addition of one
variable is not a coincidence available to a wrong hypothesis. The three misses (g5/g6/g10, all ≈ +2.1
dB) are **estimator instability, not a fourth variable**: a fixed offset would be stable across window
and bin, and these swing (g5 goes +2.05 → −8.57 between 2 s/100 ms and 3 s/100 ms). They are the pair's
three most window-unstable contexts, and instability correlates with |anchor error| at Spearman 0.71.
The underlying algebra is pinned media-free by six unit tests, so the fix cannot be adopted on a
misremembered version of the maths.

**Fix 3 is unblocked because the rule is in the source, not in the data.** This is the one that looks
like it should need another dump, and does not. Its *mechanism* was closed offline by a theorem
violation: `sc@100` is a downmix max and `scan.gap_floor_db` an interleaved max at the same bin size,
Cauchy–Schwarz forbids `Downmix > Interleaved` on the same samples, and on all four fully-silent gaps
the inequality is violated (+2.78 … +13.20 dB). A violated theorem admits exactly one explanation — the
sample sets differ — so "is there a span delta?" was answered without a probe. Its *size* is then
located by block counts (100–200 ms at the gap edges), and its *rule* is read directly off
`derive_gap_equivalence`'s filter. Both differences that rule implies — grid phase and the trailing
partial bin — are decided by the predicate, not by anything a measurement could adjudicate. Three
synthetic tests pin the behaviour with no media.

**What is deliberately still open, and why it does not block.** The window/bin leg of the noise-floor
axis (±2 s/100 ms vs ±3 s/50 ms, median 2.1 dB) is a *policy* choice between converging and
accept-and-document. None of the three fixes touches it. Two probe-derived nice-to-haves are likewise
optional and explicitly not blockers: span-provenance arg-max confirmation, and identifying the −101.3
dB constant (its *character* is settled — max−mean = 0.13–0.27 dB over 6–1702 blocks ⇒ a constant
decode floor — and a source test should wait until the constant is named rather than goldening the
number).

### Implemented 2026-07-30 — and a fourth defect the implementation exposed

All three landed in `application/gap_equivalence.rs`, which now measures its **own** A levels and donor
occupancy from PCM instead of reusing `fp.levels.*` and `donor_interior_nominal`. 488 lib tests green,
clippy clean. Four things worth recording, because three of them were nearly shipped wrong:

**A fourth sensor was on the old floor: the donor.** The first cut moved A's floor, reduction and span
and left `donor_interior_nominal.silence_fraction` as the donor input — which is a **mono downmix of B
thresholded against `levels.gap_floor_db`**, the unfiltered whole-span peak. That leaves fix 1
*half-applied*: A's floor moves, but the predicate that actually reaches the class still tests against
the number the fix exists to replace, and on the band mechanism **that predicate is the class flip**.
Caught in review before the re-dump. The donor is now passed as **PCM** (`DonorSpan`) so
`measure_gap_equivalence` thresholds it against the floor it just measured — the coupling is structural,
not a convention a later refactor can quietly break.

**The donor's reduction had to move with it.** Not in the original three-fix spec, and it is forced:
thresholding a *mono* donor against an *interleaved* floor reintroduces up to `10·log10(N)` of bias in
the **dangerous** direction (the donor reads spuriously silent ⇒ `shared_silence` ⇒ drop). Both sides of
a comparison must share a reduction; fix 2 is a property of the comparison, not of the A side.

**No fallbacks that silently un-apply a fix.** An unmeasurable noise floor now yields `None` ⇒
`NotEvaluated` ⇒ keep, rather than substituting `levels.noise_floor_db` — which is a downmix, so the
fallback would have un-applied fix 2 on exactly the gaps too thin to measure. Same rule for an absent
donor floor.

**`band_donor.json` cannot be the acceptance signal, and stayed green.** It re-derives a class from
*recorded* numbers, so it can never observe a change to the measurement path that produced them. Its
README said the fix "will fail" these tests; that is only true of a **re-harvested** fixture, and is now
corrected in place. The executable acceptance signal is
`band_donor_mechanism_now_classifies_as_repairable`, which rebuilds the band shape from synthetic PCM and
asserts the class flip, paired with a test that the two floor definitions genuinely disagree on that
donor so it cannot pass for an unrelated reason.

**Still deliberately open**, and now stated in the module docs rather than only here: the noise-floor
context window and bin size (the policy leg), and the donor predicate's missing `b.silent ||` disjunct —
scan's donor test is a disjunction with the scanner's own silence bit, the fine path's is the floor
comparison alone. That axis is unmeasured and was not part of the three fixes.

**The one thing that is not measured.** Nobody has run all three together. Each is measured in
isolation, and the residuals are known to interact in sign — fix 1 raises nothing on fully-silent gaps,
fix 2 raises fine's floor (interleaved ≥ downmix), fix 3 lowers it (narrower span, but a *max*, so the
direction is not guaranteed per-gap). Expect the combined result to need one confirming re-dump
**after** implementation. That is a validation step, not a specification gap — the distinction the
heading rests on.

### Combined re-dump, 2026-07-30 — sensors converged, classes did not

Corpus `silence-floor/fp_band_donor_mechanism_now_classifies_as_repairable_check`, same pair, 10 gaps,
all three fixes live. This is the confirming run the paragraph above called for.

**The sensors closed.** Median `|fine − scan|`, pre-fix → post-fix:

| sensor | pre-fix spread | post median | post max |
|---|---|---|---|
| `a_gap_rms_db` | to **21.62** dB | **0.101** | 0.824 |
| `gap_floor_db` | to **25.89** dB | **0.279** | 17.13 |
| `donor_silence_fraction` | to **0.83** | **0.012** | 0.410 |
| `noise_floor_db` | −2.3 … −19.0, **10/10 same sign** | **2.129** | 11.17 |

The noise-floor column is the cleanest confirmation of fix 2: a systematic one-signed bias consistent
with `10·log10(6)` became a mixed-sign 2.13 dB median, which is the window/bin residual this document
predicted and did not fix.

**The classes did not.** The § *Probe results* prediction — *pair-level 3 divergences → 1* — is **refuted**.
Still 3: g4, g5, g8. But the causes have changed, and each is now attributable:

| gap | why it still diverges | action-relevant? |
|---|---|---|
| g4 | floor residual 2.84 dB + donor granularity ⇒ `ds` 0.474 → 0.610, straddling 0.5 | keep vs drop |
| g5 | `is_dropout` splits on an 11.17 dB noise-floor gap — the open context-window leg | keep vs drop |
| g8 | floors agree to 0.18 dB; `ds` 0.167 → 0.577 on bin granularity alone | no — both drop |

`divergence_is_never_in_the_dangerous_direction` **holds**: g4 and g5 are both scan-keep / fine-drop.

**What the run newly establishes.** The residual is no longer a sensor-definition problem; it is
almost entirely the one leg left open on purpose, and it has two distinct signatures:

- *max-statistic granularity.* Fine's floor is a max over 50 ms bins, scan's over 100 ms blocks, so
  fine's can only be **≥** scan's — and was, on **10/10 gaps, 0 negatives**. Magnitude tracks how
  peaky the silence is, not gap length alone: g1/g3/g9 sit at 0.08 dB while g2 (7.18) and g10 (17.13)
  are near-digital-silence gaps whose isolated ticks the 100 ms blocks average away. g10's apparent
  "regression" (11.61 → 17.13 dB) is this effect, not a defect; both paths still classify it the same.
- *donor-fraction granularity.* Fine's `donor_silence_fraction` ran **higher** on 5 of 6 gaps with a
  donor (+0.136, +0.154, +0.410, +0.030, +0.013, −0.011): finer bins dip below the floor more often.
  This biases fine toward `drop` and is now the **largest remaining term** — bigger than anything the
  three fixes left behind, and the direct cause of g4 and g8.

**A prediction-method error to carry forward.** The `band_donor` README predicted g4 would converge
because the donor's `donor_interior_nominal.rms_db` is −66.94, ~10 dB above the new floor. That
reasoned from a **mean**; the classifier consumes the **per-bin fraction**, and 61 % of that donor's
bins fall below −76.66 despite the mean. Never predict a donor verdict from a mean level on
non-stationary content. Corrected in place in that README.

**What this changes downstream.** The window/bin leg can no longer be deferred as cosmetic — it is the
sole remaining source of *action* divergence (g4, g5). Converging `gap_signature_bin_ms` onto
`scan_block_ms` for the equivalence path specifically is now the obvious next lever, and unlike the
three fixes it is a policy call rather than a defect repair.

### Ready to implement

| Work | Status | Notes |
|---|---|---|
| **F14:** `splice_dualfit_at` A borders → raw `mono(refined ± w)` like `try_dual_fit` | **Done + media-validated** | `fp_post_F14_fix/`: `g1` flag `true`, `trim_frames −9` = production. |
| **F14 residual:** `bridge_frames > 0` + NaN-aware step-real/`gate_pass` (no `finite_corr` on dual-fit scores) | **Done** | Fixed from source, not measurement — neither edge fired on this pair. |
| **F15:** fine `gap_floor_db` + A RMS → silent-core **(a)**, interleaved reduction, block-confirmed span | **Done + media-validated (2026-07-30)** | All three shipped together in `application/gap_equivalence.rs`. Combined re-dump: A RMS median 0.101 dB, floor 0.279, donor 0.012, NF 2.129 (was one-signed to −19). Band mechanism closed — g4's fine floor −58.39 → −76.66. **But 3 class divergences remain** (g4/g5/g8), all on the window/bin leg; safety invariant holds. See § *Combined re-dump*. |
| **F15 follow-on:** converge `gap_signature_bin_ms` → `scan_block_ms` for equivalence | **Open — policy call, now the only action-relevant axis** | Post-fix, every remaining *action* divergence (g4, g5) traces here. Two signatures: max-statistic granularity (fine floor ≥ scan on 10/10) and donor-fraction granularity (fine higher on 5/6). Not a defect repair — decide converge vs accept-and-document. |
| **F15:** noise-floor / context window (±2 s/100 ms vs ±3 s/50 ms) | **Measured; demoted to the smaller term; accept-vs-converge open** | Median 2.1 dB of the fine−scan spread once reduction is matched. Reduction confirmed (row below), so this decision is unblocked. g5 remains the poster gap (−13.13 dB window/bin of −18.98 total pre-fix). |
| **F15 (new):** multichannel reduction — downmix (fine `mono_rms`) vs interleaved power (scan `block_rms_db`) | **CONFIRMED 2026-07-30 — ready to fix** | Anchor `(2 s, scan_block_ms, Interleaved)` reproduces scan NF on **7/10** gaps (err −0.78…+0.51, median +0.03); direct reduction **3.65–7.89 dB**, measured `ρ̄` **+0.318…−0.005** (none anti-correlated). The 3 misses (g5/g6/g10, all ~+2.1 dB) are **estimator instability** on the pair's most window-unstable contexts — not a fourth variable; span-provenance retired for this axis. Level-dependent, so it does **not** cancel between `a_rms` and `nf`. Defect, not a design choice. See § *Probe results* (`fp_silent_core_floor_probe_reduction/`). |
| **F15:** population check via `equivalence-calibration` | **Done 2026-07-30** | 17-pair corpus **297 gaps / 5 divergent / 0 dangerous**, reproduced on a second 17-pair set at another recipe. Sets the severity and unblocks the fork below. |
| **F15 fork:** (A) converge the sensors vs (B) keep them different and fix the *interpretation* | **(B) DONE 2026-07-30** | See § *(B) applied* below. No behaviour change — the fine block was already read by nothing in the plan/patch path. |
| **F15:** curated fixture with a band-donor gap | **DONE 2026-07-30** | `tests/gap_corpus/fingerprints/equivalence_divergence/band_donor.json` + `tests/equivalence_divergence.rs` (4 tests). |
| **F15 (new):** fully-silent residual — fine measures `gap_floor_db` over the **refined** span, scan over the block-confirmed **core** | **MECHANISM CLOSED offline 2026-07-30 — ready to fix** | Proved, not inferred: `sc@100` (downmix) reads *above* scan (interleaved) at the same bin size on all four fully-silent gaps, which Cauchy–Schwarz forbids on the same samples — so the sample sets differ. Block counts locate it at a **100–200 ms span delta** at the gap edges (scan 1.70/4.10/0.60/170.20 s vs fine 1.86/4.26/0.66/170.25 s), worth 2.78–13.20 dB on a *max*. Scan's narrower span is the more correct one. The −101.3 pin is resolved: max−mean = 0.13–0.27 dB over 6–1702 blocks ⇒ a constant decode floor, not an edge block. Span-provenance probe **downgraded to optional**; the fix does not wait on it. See § *The fully-silent residual*. |
| **F15 (new):** donor **window** alignment | **Measured offline 2026-07-30 — decision-relevant on g4 + g6 only; close with a test, do not chase** | Window delta is ~1 × `scan_block_ms` on all ten gaps. Irrelevant except where `donor_silence_fraction` sits within one block of the 0.5 threshold — **g4 (0.474, quantum 0.053) and g6 (0.533, quantum 0.067)**. g4 is `band_donor.json`. The floor fix moves all four disagreeing donors the right way on its own (each is 12–25 dB above the silent-core floor). Close with a same-floor/different-window unit test on g4, not a general fix. |
| Reading the corpus 5/297 as the **same mechanism** as `band_donor.json` | **Do not** | Class-shape match only (keep→drop, reason-only, NF sign). Those rows carry no floor provenance, so they are *consistent with* the band mechanism, not evidence of it. |
| Reading the silent-core / reduction work as reopening **(B)** | **Do not** | (B) settled *authority* (scan is authoritative; the fine block is read by nothing in plan/patch). These fixes repair a **diagnostic** that misreports — orthogonal to (B). |
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
  in [TEMP-scan-recipe-plan.md](../TEMP-scan-recipe-plan.md)).
- `--fingerprint-gap N` is **1-based** on the gap-table `#`, and emits **0-based** filenames.
- Use `RUST_LOG=debug`, not `RUST_LOG=clip_sync_repair=debug`, when the question might involve the
  `clip_sync` crate (alignment, seek, decode) — the narrower filter hides those errors.

Both findings above are reproducible from one command per finding: `--fingerprint-gap 6` for F15,
`--fingerprint-gap 2` plus a `--repair-preview` run for F14.
