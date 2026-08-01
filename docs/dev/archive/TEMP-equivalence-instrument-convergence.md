# Equivalence instrument convergence — ledger

> # ARCHIVED 2026-07-31 — closed, do not update
>
> All three instrument axes are resolved and the follow-up work is done: `band_donor.json` re-harvested
> as a regression fixture (`tests/equivalence_divergence.rs`), and the probe scaffolding decided —
> `noise_floor_probes` **retained** for I2 attribution, `silent_core_probes` vestigial and to be removed
> opportunistically when that file is next touched.
>
> **Current behaviour lives in the code and in
> [../gap-fingerprint.md](../gap-fingerprint.md) § *`equivalence` vs `scan_equivalence`*, which carries
> the converged/residual table.** Read this file for the *rationale* — in particular I3, where a null
> measurement was correctly overturned by asking why the effect was absent.
>
> **One thing this ledger delegated forward:** I3's null result (`0 dangerous / 297 gaps`) is
> uninterpretable while the dump records no codec or losslessness, and the probe scaffolding named
> above cannot be deleted until something permanent replaces it. Both are specified in
> [TEMP-fingerprint-provenance-plan.md](TEMP-fingerprint-provenance-plan.md) (**opened
> 2026-07-31**), whose §2 blocks the next large fingerprint run.

**Opened:** 2026-07-30. **Status:** **all three closed.** **I1** done + media-validated; **I2** decided
(accept-and-document); **I3** measured, found a latent defect, **fixed 2026-07-31** — the fine donor's
missing silence disjunct was *not* vestigial; it was unexercised by this corpus (lossy media never
reaches the −120 digital-silence floor that triggers it) and misclassified in the **dangerous**
direction when it was reached.

After I1, the two front-ends agree **exactly** on the A side — `gap_floor_db` and `a_gap_rms_db` are
0.00 apart on all ten gaps of the F15 pair — and the pair carries **one** class divergence (g5), on
the accepted context-window axis, in the safe direction. After I3 the donor predicate matches scan's
disjunction as well, so the only surviving instrument difference is I2's 2.0 s / 3.0 s window.

Remaining work is cleanup, not measurement: see [Also carried over](#also-carried-over-lower-priority).

Split out of [TEMP-equivalence-divergence-findings.md](TEMP-equivalence-divergence-findings.md)
when that ledger was archived. Everything else in it is closed (**F14** fixed and media-validated;
**F15**'s three fine-path fixes implemented and media-validated). These three were open at archival
time and are carried here unchanged. Read the parent for the forensics — the measurements below are
summarised, not reproduced.

Media hygiene: the pair is referred to by properties only (uncatalogued licensed 5.1, A ≈ 6900 s,
AAC-LC 48 kHz 5.1); gap indices are numeric; raw logs stay in gitignored `gap-files/`.

## The setting, in one paragraph

Two front-ends feed the same `classify_gap_equivalence`:

| | authoritative | diagnostic |
|---|---|---|
| fn | `domain::gap_equivalence::derive_gap_equivalence` | `application::gap_equivalence::measure_gap_equivalence` |
| caller | `application/scan_gaps.rs:289` — **gates repair** | `gap_fingerprint/measure.rs:2434` — **fingerprint dump only** |
| grain | `scan_block_ms` = 100 ms | `gap_signature_bin_ms` = 50 ms |
| NF context | 2.0 s | 3.0 s |

**Verified 2026-07-30:** `measure_gap_equivalence` has exactly one caller. No repair decision reads
it. Every item below is therefore a **diagnostic-fidelity** problem, not a production-safety one —
with the one important exception in I1's "why it matters".

## Why fidelity still matters here — the CI gate

`bin/equivalence_calibration.rs` diffs the two and **exits 1 only on the dangerous direction**
(scan drops, fine keeps). Its own header justifies that asymmetry: every known difference biases the
fine side toward `drop`, so that direction is the one fine's biases cannot manufacture.

The instrument differences below bias the same way. That means they **cannot cause false alarms** —
but they **can mask true ones**: a gap where scan genuinely false-drops and fine would have kept it
gets pushed to `drop` by an instrument artifact, and the gate stays silent. The failure mode is lost
sensitivity in a safety gate, which is the worse of the two.

> **Stale justification — CORRECTED 2026-07-31.** That module header argued the drop-bias from "fine
> reads lower on 10/10 gaps, ~3–19 dB" (the **channel-reduction** term, fixed in F15) and from an
> unfiltered fingerprint floor (also fixed in F15). Granularity carried the argument afterwards, and I1
> removed that too. The header now states the converged/residual split per input, names the two accepted
> residuals with their measured sizes, and flags that the `0 dangerous` population result predates I1/I3
> and was measured on all-lossy media — the corpus that structurally cannot trigger I3. The `PairVerdict`
> variant docs were corrected to match.

---

## I1 — Equivalence bin size: 50 ms (fine) vs 100 ms (scan)

**Status: DONE — implemented and media-validated 2026-07-30.**

### Validated (`fp_i1_bin_convergence/`, same pair, 10 gaps)

Every pre-registered prediction held. **3 divergences → 1**, survivor **g5**, on the predicted axis.

| | pre-I1 | post-I1 |
|---|---|---|
| divergences | g4, g5, g8 | **g5 only** |
| `gap_floor_db` median (max) | 0.279 (17.13) | **0.000 (0.000)** |
| `a_gap_rms_db` median (max) | 0.101 (0.824) | **0.000 (0.000)** |
| `donor_silence_fraction` | 0.012 (0.410) | **0.008 (0.067)** |
| `noise_floor_db` | 2.129 (11.17) | **0.606 (8.574)** |

- **g4 closed** — `ds` 0.610 → 0.476 vs scan 0.474; class returns to `repairable_dropout`.
- **g8 closed** — `ds` 0.577 → 0.154 vs scan 0.167.
- **g5 survived** — 8.57 dB noise-floor gap denies `is_dropout`. Scan keeps, fine drops: safe direction.
  This is I2, and g5 is now a **single-variable case** on it.
- **g2/g10 floor residuals collapsed** (7.18 → 0.00, 17.13 → 0.00), confirming they were pure
  bin-granularity artifacts of a max statistic, not a defect.

**Stronger than predicted:** `gap_floor_db` and `a_gap_rms_db` are **exactly 0.00 on all ten gaps**.
The A-side sensors are not approximating each other — the two front-ends now compute the same numbers,
given the same silent-core filter, reduction, span rule, and grid.

**Two secondary findings:**

1. **Bin size was the larger term on the noise floor as well.** With only the context window still
   split, the NF median fell 2.129 → 0.606 dB. The archived ledger attributed that median to window/bin
   *jointly*; it was mostly bin. **This shrinks I2's measured size considerably** — see I2.
2. **`ds` is no longer exactly zero** (median 0.008, max 0.067) though the donor floor now matches
   exactly. That is the donor *window* residual of ~1 × `scan_block_ms` recorded in the archived
   ledger — independent of bin size, as that ledger predicted.

The whole equivalence overlay now bins at `report.scan_block_ms` via a single `equiv_bin_ms` local at
`gap_fingerprint/measure.rs` — both the silent-core bins (`SilentCoreConfig::bin_frames`) and the
noise-floor probe, so the overlay cannot bin two ways internally. `gap_signature_bin_ms` is untouched
and keeps its production consumers. The context window is deliberately *not* converged — that is I2.

**Not yet confirmed on media.** Prediction: g4 and g8 close, **g5 survives** on I2. Treat a surviving
g5 as the expected result, not a failed fix. At the time, nothing in the committed test suite could
observe this change (fixtures re-derive from recorded numbers) — the re-dump *was* the acceptance
signal. `band_donor.json` has since been re-harvested from that dump as a regression fixture.

The rest of this section is the reasoning as it stood when the change was made.

### What was measured (parent § *Combined re-dump*)

After the three F15 fixes, this is the **only** source of *action* divergence left. Two signatures:

- **Max-statistic granularity.** `gap_floor_db` is a max. A max over 50 ms bins can only be **≥** a
  max over 100 ms blocks — and was, on **10/10 gaps, zero negatives**. Magnitude tracks how peaky the
  silence is, not gap length: g1/g3/g9 sit at 0.08 dB while g2 (7.18) and g10 (17.13) are
  near-digital-silence gaps whose isolated ticks the 100 ms blocks average away.
- **Donor-fraction granularity.** `donor_silence_fraction` ran **higher** on 5 of the 6 gaps with a
  donor (+0.136, +0.154, +0.410, +0.030, +0.013, −0.011): finer bins dip below the floor more often,
  so fine reads donors as more silent. This is the larger term and the direct cause of g4 and g8.

### Where the 50 ms came from — inherited, not chosen

Traced 2026-07-30. `gap_signature_bin_ms` is documented at `infrastructure/config.rs:114` as *"Bin
width (milliseconds) for active/silent **structure signatures**"*, and all three production call sites
(`gap_fingerprint/measure.rs:1475`, `patch_audio/geometry.rs:36`, `patch_audio/region.rs:1512`) pair it
with `context_frames` to build exactly that: a binary active/silent pattern matched across seams and
gated by `min_structure_match_score` (0.55) / `strong_structure_trust`.

**50 ms is well chosen for that job.** A binary pattern match wants fine bins — 50 ms resolves
syllable-scale on/off structure, where 100 ms would smear speech gaps and blunt discrimination. (Cf.
the `lag_window_secs` note at `measure.rs:2424`, frozen at 1 s for the mirror-image reason.) Scan's
100 ms answers a different question: level estimation across a whole-file scan, trading resolution
against cost and stability.

Equivalence is a **third** job — level and threshold estimation for comparison against scan — and it
inherited 50 ms by proximity, because the value was already in `FingerprintConfig` when the overlay was
written. There is no evidence anyone selected 50 ms *for equivalence*.

**The desirable property inverts between the jobs.** For a binary pattern match, finer bins are more
discriminative. For a **max** statistic (`gap_floor_db`) and a **threshold-crossing fraction**
(`donor_silence_fraction`), finer bins are upward-biased and noisier — which is precisely what the
re-dump measured (max ≥ coarser on 10/10; donor fraction up on 5/6). So this is not merely an
inconsistency between two paths: the fine path is applying a value tuned for *discrimination* to two
statistics where discrimination is not the goal and granularity is a bias source.

### Recommendation — **converge, narrowly**

Give the equivalence overlay its **own** bin size, defaulted to `scan_block_ms`.

**Do not** change `gap_signature_bin_ms` globally. It is shared with `patch_audio/geometry.rs:36` and
`patch_audio/region.rs:1512` — real fill geometry. Coarsening it would alter fill placement, which has
nothing to do with this and carries a far larger blast radius.

The change is already separable: `measure.rs:2441` builds its own
`SilentCoreConfig { bin_frames, .. }` and merely *happens* to derive it from `gap_signature_bin_ms`.
Pass `report.scan_block_ms` instead. The struct exists precisely because this is a separable knob.
Signature, geometry, and fill path keep 50 ms untouched.

**Rationale.** Where the two paths are *supposed* to agree, they should compare like-for-like; a
diagnostic that disagrees with the thing it audits for instrument reasons is not auditing it. The
alternative (accept-and-document) leaves a known one-sided bias inside a CI gate whose entire value is
sensitivity in that direction.

**Expected outcome:** g4 and g8 close; **g5 survives** (it splits on I2, a different parameter). That
would isolate g5 to one axis, which is itself informative — so a partial result here is a success, not
a failure. Needs one media re-dump to confirm.

*Outcome: all of the above held. See § Validated at the top of this section.*

**Cost:** a few lines at one call site, plus the re-dump.

---

## I2 — Noise-floor context window: 2.0 s (scan) vs 3.0 s (fine)

**Status: isolated, re-measured smaller, undecided. Now the only remaining axis.**

**Re-measured after I1 (2026-07-30) — and it shrank.** The 2.13 dB median previously attributed to
this axis was mostly **bin size**, not window. With bin converged and the window still split at
2.0 s vs 3.0 s, the noise-floor residual is a **median 0.606 dB**. g5 remains the poster gap at
**8.57 dB** (was 11.17), and is now the **only** divergence in the pair — a clean single-variable case,
which is exactly what sequencing I2 after I1 was meant to produce.

It is also the only gap where this axis changes an action: scan keeps, fine drops. Safe direction.

### Recommendation — **decide after I1, and expect to accept it**

Two reasons to sequence it second rather than bundle it:

1. **Attribution.** With I1 landed, g5 should be the *only* surviving divergence. That is a clean
   single-variable experiment on this axis — worth far more than changing both at once and inferring.
2. **The windows are not obviously reconcilable.** Unlike bin size, the context window encodes a real
   judgement about how much surrounding material defines "the noise floor here". 2.0 s is
   `EQUIVALENCE_CONTEXT_SECS`; 3.0 s is `gap_signature_context_secs`, a *configurable* fingerprint
   parameter with other consumers. Converging means one of them stops meaning what it was set to mean.

Lean **accept-and-document**, with the residual stated in the calibration tool's header as a known
term rather than silently absorbed — *unless* the post-I1 re-dump shows this axis flipping classes on
more than the one gap. One gap out of ten, in the safe direction, does not justify perturbing a
configurable parameter with unrelated consumers.

### Resolved test, 2026-07-30 — **accept-and-document**

The condition above was pre-registered and the re-dump answered it: this axis flips **one** gap (g5),
in the safe direction, and its median contribution re-measured at **0.606 dB** rather than the 2.13 dB
it was charged with before bin size was separated out. Both facts point the same way.

Recommendation stands, now on measured rather than anticipated grounds: **do not converge the context
window.** Instead:

- state the residual in `bin/equivalence_calibration.rs`'s header as a known, bounded term (median
  0.606 dB, one gap of ten, conservative direction) — folded into the header correction already queued
  in § *Why fidelity still matters here*;
- keep g5 as the named single-variable case, so anyone re-opening this has a worked example.

**Re-open if:** a broader corpus shows this axis flipping gaps in the **dangerous** direction (scan
drops, fine keeps), or flipping more than a small minority. `equivalence-calibration` already exits 1
on the former, so that trigger is automatic.

Note the honest limit: 0.606 dB is a median over **one pair, ten gaps**. It bounds the axis on this
pair, not on the corpus.

---

## I3 — Donor predicate: fine is missing scan's `b.silent ||` disjunct

**Status: MEASURED 2026-07-30 — and the finding inverts the original recommendation. Fix, don't retire.**

### Measured: no effect on this pair, because this pair cannot trigger it

Over `fp_i1_bin_convergence/`, every donor delta is within **±1 block** (max exactly 1.00) with mixed
signs (4 negative, 3 positive, 3 zero). The disjunct can only ever *add* silent blocks to scan, so a
real contribution would drive `fine − scan` systematically negative and could exceed one block. It
doesn't. All residual donor disagreement is **window alignment**, as the archived ledger predicted.

### But the triggering condition is absent here, and that is the finding

`BLOCK_LEVEL_FLOOR_DB = −120.0` (`domain/policies/silence.rs:35`) and `block_rms_db` clamps a
digitally-silent block to exactly that. `domain/gap_equivalence.rs:418` states the mechanism outright:
*"digitally silent blocks sit at BLOCK_LEVEL_FLOOR_DB and `rms < gap_floor` is false when both are
−120."*

This pair's floors bottom out at **−101.48** — near-silence, not digital silence, as expected from
lossy AAC-LC. **The condition the disjunct exists for never occurs in this corpus.** Absence of effect
here is therefore *not* evidence the disjunct is vestigial; it is evidence the corpus lacks the case.

`application/gap_equivalence.rs:297` implements the fine donor as `db < floor` with **no silence
predicate**. So on a gap whose silent core is digitally silent, with a digitally-silent donor:

| | donor reads | `b_occupied` | class | action |
|---|---|---|---|---|
| scan (`b.silent ‖ rms < floor`) | silent | false | `SharedSilence` | **drop** |
| fine (`rms < floor` only) | **occupied** (`−120 < −120` is false) | true | `RepairableDropout` | **keep** |

**Scan drops, fine keeps — the dangerous direction**, and exactly the condition
`bin/equivalence_calibration.rs` exits 1 on. This is the only defect in this family that trips that
gate, and it would trip it *spuriously*, on the material where the correct answer is least ambiguous.

Derived from source semantics, **not observed on media** — no dump in hand contains a −120 floor.

It also explains the population result (5/297 divergent, **0 dangerous**): every pair in that corpus is
lossy and never reaches exact digital silence. A lossless or genuinely muted source would.

### Recommendation — **add the disjunct to the fine path**

Reverses the original "measure before fixing; do not add speculatively" guidance, and the reason that
guidance existed is now spent: it warned that adding the disjunct would push fine's donor further
toward *silent*, compounding I1's granularity bias in the same threshold region. **I1 is done** and the
donor axis agrees to within one block, so there is nothing left to compound.

Apply the same silence predicate the A-side silent core already uses (`is_silent_interleaved` per bin,
via the existing `silent_core_levels` machinery), OR'd with the floor comparison, inside
`donor_silence_fraction_at_floor`. That makes the two donors structurally identical rather than
coincidentally agreeing.

**Acceptance signal:** a synthetic-PCM unit test — digitally-silent gap core, digitally-silent donor —
asserting fine now reads the donor silent and classifies `SharedSilence`. That test **fails today** and
is the real signal, since no media dump on hand contains the case. Do not wait on a re-dump: the
corpus cannot produce this gap, which is the whole point.

Scan's donor test is a **disjunction** with the scanner's own silence bit; the fine path's is the
floor comparison alone. So a donor block the scanner flagged silent, but whose level sits above the
floor, reads silent to scan and occupied to fine.

### Outcome — **fixed 2026-07-31**

`donor_silence_fraction_at_floor` now takes the whole `SilentCoreConfig` and counts a bin silent on
`is_silent_interleaved(...) || below_floor`, structurally matching scan. Two consequences beyond the
planned one:

- **The `floor_db?` early return is gone.** Scan evaluates the donor from the silence bit whether or
  not A's gap produced a floor, so fine now does too. The change is inert at the classifier —
  `gap_floor_db` and `a_gap_rms_db` come from the same silent set, so a gap with no floor also has no
  A RMS and classifies `NotEvaluated` regardless of the donor — but it removes a second structural
  difference rather than leaving it to be rediscovered.
- **Two existing unit tests failed, and they were right to.** `silent_gap_occupied_donor_is_repairable`
  and `band_donor_mechanism_now_classifies_as_repairable` built "occupied" donors at 1e-4 and 2e-4 —
  *below* the run recipe's `absolute_silence_rms` of 0.001. Those donors are scanner-silent, so the
  tests were asserting the I3 divergence itself: occupied to fine, silent to scan, class `keep` against
  scan's `drop`. Both donors were raised to 2e-3, which preserves each test's actual subject (a donor
  above A's silent-core floor; in `band_donor`'s case still strictly inside the band between floor and
  content peak) while being occupied on both paths. This is a second, independent sighting of the
  defect — the first from source, this one from fixtures written before the disjunction was understood.

The acceptance test asserts the mechanism inline before asserting the class, so it cannot pass for an
unrelated reason: `bin_level_db` on a digitally-silent bin returns exactly `SILENCE_FLOOR_DB`, and
`!(bin < SILENCE_FLOOR_DB)` — the floor-only predicate reading digital silence as occupied — is
asserted as an executed fact. Suite: **504 passed, 0 failed**, clippy clean.

### Superseded — the original recommendation, kept for the record

> **Retained because the reversal is the useful part.** This was the guidance before the measurement
> ran: it predicted the disjunct would prove to be dead code and warned against fixing from source. The
> measurement did find no effect on media — and that null result, taken at face value, would have closed
> this as a no-op. What changed the conclusion was asking *why* the effect was absent: the corpus is
> all-lossy and structurally cannot reach the −120 floor the disjunct exists for. A null measurement over
> a corpus that cannot produce the condition is not evidence about the code.

### Recommendation — **measure before deciding; do not fix from source**

This is the one item where the direction of the bias is not established. Note it points the
**opposite** way to everything else in this ledger: it makes *scan* read more silent, i.e. more
drop-prone, where I1/I2 make *fine* more drop-prone. Two opposing biases of unknown relative size are
exactly the situation where fixing one from source alone can make the net worse.

Cheapest measurement: the fingerprint dump already carries both verdicts per gap. Count donor blocks
where `b.silent` is true but `rms_db >= gap_floor_db` — if that set is empty across the corpus, the
disjunct is dead code on real media and this closes as a no-op with a documenting test. That is the
likely outcome, and it is cheap enough to be worth confirming rather than assuming.

**Do not** add the disjunct to the fine path speculatively. It would move the fine donor toward
*silent* — compounding I1's bias in the same direction, in the same threshold region, before I1 is
fixed.

---

## Suggested order

1. ~~**I1** — converge the equivalence bin size~~ **DONE + media-validated 2026-07-30.**
2. ~~**Re-dump** the F15 pair~~ **DONE** (`fp_i1_bin_convergence/`): 3 divergences → 1, all predictions held.
3. ~~**I2** — decide accept-vs-converge~~ **DECIDED: accept-and-document.** Residual re-measured at
   0.606 dB median, one gap, safe direction.
4. ~~**I3** — run the dead-disjunct count~~ **DONE**: no effect on this pair, but the pair cannot
   trigger it. **Not** dead code — a latent fine-path defect in the *dangerous* direction.
5. ~~**I3 fix** — add the silence predicate to `donor_silence_fraction_at_floor`~~ **DONE 2026-07-31.**
   Acceptance was the synthetic-PCM test, as pre-registered; two pre-existing tests also had to be
   corrected because they encoded the defect. See [Outcome](#outcome--fixed-2026-07-31).
6. **Fix the stale reduction-based justification in `equivalence_calibration.rs`'s header** ← **next**
   (see above). It argues fine's drop-bias from the channel-reduction term, which is fixed; granularity
   carried it afterwards, and post-I1 even that is nearly gone. Fold in I2's accepted residual
   (0.606 dB median, one gap, safe direction).
7. ~~**Re-harvest `band_donor.json`** under the fixed path and convert it to a **regression** fixture~~
   **DONE 2026-07-31.** Re-harvested from `fp_i1_bin_convergence/` g4 (post-I1; I3 is a no-op on this
   pair). Both paths now `repairable_dropout`; floors match exactly. Pre-fix band arithmetic retained
   as constants in `tests/equivalence_divergence.rs`. See the fixture README.

## Also carried over — probes retained, not deleted

The F15 probe fields (`silent_core_probes` / `noise_floor_probes`) were scheduled for deletion once the
fixes landed. **Demoted 2026-07-31: do not delete as an archive chore.** They are dump-schema
provenance (builder + `measure.rs` emit + committed corpus JSON), not dead locals — removing them
changes what new dumps emit. Nothing classifies on them; the cost is dump size / measure time only.

| | Keep? | Why |
|---|---|---|
| `noise_floor_probes` | **Yes** | I2 was accept-and-document with a re-open trigger. The window×bin×reduction grid is the cheapest offline attribution for NF residuals, and the only dump-native record of the reduction proof. |
| `silent_core_probes` | Vestigial | Live `gap_floor_db` / `a_gap_rms_db` *are* the silent-core path at `scan_block_ms`. The dual-bin rows answered I1. Drop these first if dumps ever feel fat; leave the NF grid. |

Soft-retire later via empty `skip_serializing_if` if needed — old corpora keep parsing. Do not strip
committed fixtures (including `band_donor.json`) just to tidy the schema.

## Reproducing

Per the parent ledger's § *Reproducing these runs*. Real-media `--gap-fingerprints` runs need
`--features calibration,he-aac` and `--silence-hold-ms 500` pinned explicitly (the manifest recipe
omits it); ~15 GB RSS, one pair at a time.
