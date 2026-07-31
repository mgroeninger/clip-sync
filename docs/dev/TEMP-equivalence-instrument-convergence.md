# Equivalence instrument convergence — open ledger

**Opened:** 2026-07-30. **Status:** three open items, all one axis. The two equivalence front-ends
now share corrected sensor *definitions*; what remains is that they sample those definitions with
different instruments — bin size, context window, and one missing predicate disjunct.

Split out of [archive/TEMP-equivalence-divergence-findings.md](archive/TEMP-equivalence-divergence-findings.md)
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

> **Stale justification to fix when I1 lands.** That module header still argues the drop-bias from
> "fine reads lower on 10/10 gaps, ~3–19 dB" — that was the **channel-reduction** term, which is now
> fixed. Post-fix the noise-floor delta is mixed-sign, median 2.13 dB. The drop-bias survives, but
> **granularity** carries it now, not reduction. The conclusion holds; the stated reason does not.

---

## I1 — Equivalence bin size: 50 ms (fine) vs 100 ms (scan)

**Status: open, specified, recommended. Not implemented.**

### What was measured (parent § *Combined re-dump*)

After the three F15 fixes, this is the **only** source of *action* divergence left. Two signatures:

- **Max-statistic granularity.** `gap_floor_db` is a max. A max over 50 ms bins can only be **≥** a
  max over 100 ms blocks — and was, on **10/10 gaps, zero negatives**. Magnitude tracks how peaky the
  silence is, not gap length: g1/g3/g9 sit at 0.08 dB while g2 (7.18) and g10 (17.13) are
  near-digital-silence gaps whose isolated ticks the 100 ms blocks average away.
- **Donor-fraction granularity.** `donor_silence_fraction` ran **higher** on 5 of the 6 gaps with a
  donor (+0.136, +0.154, +0.410, +0.030, +0.013, −0.011): finer bins dip below the floor more often,
  so fine reads donors as more silent. This is the larger term and the direct cause of g4 and g8.

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

**Cost:** a few lines at one call site, plus the re-dump.

---

## I2 — Noise-floor context window: 2.0 s (scan) vs 3.0 s (fine)

**Status: measured, unblocked, undecided.**

Worth a **median 2.13 dB** of the fine−scan noise-floor spread once the channel reduction is matched
(pre-fix this axis was masked by the reduction's 3.65–7.89 dB). g5 is the poster gap: an 11.17 dB
noise-floor split that flips `is_dropout` and produces a keep/drop divergence on its own.

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

**Blocked on:** the I1 re-dump. Not on any new measurement of its own.

---

## I3 — Donor predicate: fine is missing scan's `b.silent ||` disjunct

**Status: UNMEASURED. The only item here with no data.**

Scan's donor test is a **disjunction** with the scanner's own silence bit; the fine path's is the
floor comparison alone. So a donor block the scanner flagged silent, but whose level sits above the
floor, reads silent to scan and occupied to fine.

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

1. **I1** — converge the equivalence bin size (narrow change, one call site).
2. **Re-dump** the F15 pair. Confirm g4/g8 close and g5 survives.
3. **I3** — run the dead-disjunct count against the same dump (no extra media run needed).
4. **I2** — decide accept-vs-converge with g5 isolated as a single-variable case.
5. Fix the stale reduction-based justification in `equivalence_calibration.rs`'s header (see above).
6. Re-harvest `band_donor.json` under the fixed path and convert it to a **regression** fixture, per
   the standing instructions in its README.

## Also carried over, lower priority

- **Probe scaffolding is now redundant.** `SilentCoreProbe` and `NoiseFloorProbe` existed to predict
  the three F15 fixes before they landed. The combined re-dump exists; both probe sets can be deleted.
- **`band_donor.json` is a pre-fix artifact.** Its assertions still pass because they re-derive from
  recorded numbers and never execute the measurement path — see the ⚠ sections in its README. Green
  there currently means "the pre-fix numbers still say what they said", not "the fix works".

## Reproducing

Per the parent ledger's § *Reproducing these runs*. Real-media `--gap-fingerprints` runs need
`--features calibration,he-aac` and `--silence-hold-ms 500` pinned explicitly (the manifest recipe
omits it); ~15 GB RSS, one pair at a time.
