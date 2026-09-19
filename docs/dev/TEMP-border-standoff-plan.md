# `border_standoff_secs`: provenance and the α sweep

**Status:** active, 2026-09-19. Tracked as BACKLOG § *Fill placement* → α.

**Why this doc exists.** Production's default of 0.35 s has never had a recorded measurement behind it.
At least one earlier standoff sweep also left no record of why it was run or what it found. A change to
the default made now, without writing down the evidence, would give the next person the same blank
wall. **Nothing may change the default without first adding its evidence to § 4.**

---

## 1. Provenance: what is known

- **Introduced:** commit `a5af846`, 2026-06-09, at 0.35 s. The value has not been changed since.
- **Stated purpose** (the only one on record): "A-side only: skip this much audio immediately adjacent to
  the dropout when building border templates (avoids corrupted seam audio on A)" (`request.rs` doc
  comment). [seam-scoring.md](../seam-scoring.md) § 1 step 3 adds: "so the seam is not graded against
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

## 3. Sweep 1: 2026-09-18 (confounded)

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
- **Confound, which is why this sweep is not evidence.** Until 2026-09-19 the residual's measuring
  templates were built from the same `border_spec` as placement. Each arm therefore changed both the
  placement and the instrument that measures it. At 0 s the instrument's templates run flush into
  whatever sits at the gap edge, which is the very audio the standoff was introduced to avoid.
- The rise in min seam r at 0 s (0.387 → 0.553) is **not** a quality result. It may just be the flush
  templates correlating with gap-edge material.

## 4. Evidence required before the default changes

Each item, with its date and data location, must be written here before a default change lands:

1. **Fixed-instrument re-sweep (§ 5).** α has to move with the placement standoff while the instrument
   is pinned at 0.35. If the 0 arm reads about +35 ms like the control, sweep 1's drop came from the
   instrument: α is not standoff-driven and this item closes.
2. **The original rationale, tested.** Take the gaps whose placement moves most between arms and check
   by ear for a click or fade graded into the seam at 0 s. This is the one claim the 0.35 default rests
   on, and it has never been measured.
3. **Corpus-wide outcome counts.** Patched, skipped and dual-fit counts across all 52 pairs at the
   candidate value. Sweep 1's two dual-fit flips at 0.175 show the standoff changes routing, not only
   placement.
4. **Audibility of α itself.** A 20–35 ms late fill with both shoulders otherwise clean may be
   inaudible: the masking study found 0 audible out of 14. If it is inaudible, no default change is
   motivated, whatever items 1–3 show.

## 5. Next: sweep 2, fixed instrument

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
