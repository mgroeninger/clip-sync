# TEMP — Envelope rescue-branch peak statistics

**Status:** draft, not started. **One deliverable:** stamp `envelope_peak_z`,
`envelope_prominence`, and `envelope_curve_n` on rescue-branch shoulders (`rescued` /
`declined`). Diagnostic first — do not retune `min_envelope_r` in this Change.

Split out of [archive/TEMP-envelope-adaptive-widen-plan.md](archive/TEMP-envelope-adaptive-widen-plan.md)
on 2026-08-23. That draft mixed three jobs (sizing, floor, widen-trigger); the ladder is
**refuted** (measurement 1: pair 50's only edge-pin is g009 post, r 0.47 at +2000 ms). That
measurement was taken *at* the 2000 ms cap it was judging; the same shoulder locks at +2800 once
the cap moves, so do not cite it as "nothing lives past 2000" (caveat on the archived plan's
banner). Sizing is a flat `--lag-envelope-max-lag-ms` dump, not a Change
([BACKLOG.md](../../BACKLOG.md) § *Fill placement*). What remains is the floor's cheap remaining
job: declining doomed rescue candidates early so they do not pay a wasted fine sweep.

Parent: [archive/TEMP-lag-nominal-first-plan.md](archive/TEMP-lag-nominal-first-plan.md)
(the rescue branch this field lives in). Durable `lag_decision` behaviour:
[gap-fingerprint.md](gap-fingerprint.md) § *`lag_decision`*.

## Problem

`min_envelope_r = 0.70` is an uncalibrated absolute floor inside the rescue branch. The coarse
pass (`envelope_peak_at`) already builds the same `lag_correlation_curve` the fine sweep
summarizes as `peak_z` / `prominence`, but keeps only `max_by` — `peak_r` and `edge_pinned`.
Recording the missing statistics lets a later dump decide whether 0.70 tracks a high-z split or
gates a different population. That decision is **not** this plan.

Under nominal-first the populated set is only shoulders dead at `peak_r` < 0.5. Expect a small n;
do not describe the dump as a base rate. 26/g6 is `EnvelopeRescue::NotConsulted` and stamps **no**
`envelope_*` fields — it is not a calibration point. 31/g8 is a Narrow-capture miss, not a floor
instance (BACKLOG).

**Envelope z is not fine-pass z.** The envelope curve is ~41 points (±2000 ms at 100 ms bins);
`z_max = sqrt(n − 1)` ≈ **6.32**. Fine-sweep z (~57.6k points, ceiling ~240) is incomparable.
Record `envelope_curve_n` beside the z so a reader cannot carry a fine-scale number across, and
so a later width change that moves n is legible.

## Change

1. Extend `EnvelopePeak` with `peak_z`, `prominence`, and curve length. Reuse `secondary_peak`
   — it already takes `&[(i64, f64)]`, the type `envelope_peak_at` builds. No envelope-scale copy.
2. Serialize `envelope_peak_z` / `envelope_prominence` / `envelope_curve_n` with the same
   presence rule as `envelope_lag_ms`: set on `rescued` / `declined`, absent on `not_consulted`
   and when recenter is off.
3. Re-dump the 12-pair cell (`{2,3,13,14,18,22,32,47,51}` + 26 + 31 + 50) at default envelope
   knobs, no `--fingerprint-diagnostics`. Inspect whether 0.70 on `envelope_peak_r` tracks a
   sensible z/prominence split. Named read: 50/g009 post (edge-pinned, r 0.47) should look like
   low-z noise if the statistic is doing any work.

## Non-goals

- Do not implement an adaptive widen ladder. Refuted; a flat `--lag-envelope-max-lag-ms`
  re-fingerprint is the sizing dump (BACKLOG).
- Do not retune `min_envelope_r` or pick an `envelope_peak_z` threshold in this Change. The
  deliverable is the field, not the number.
- Do not raise the 2000 ms default.
- Do not scale the window by gap duration.
- Do not flip `--border-capture` to `wide`.
- Do not treat 26/g6 as an in-branch calibration point, or 31/g8 as a floor instance.
- Do not unify the Tier-3 `wide_envelope` diagnostic onto these knobs (BACKLOG *Envelope width
  knobs move only the rescue path*).

## Exit

- The three fields are populated on rescued/declined shoulders of the 12-pair dump and absent on
  `not_consulted`.
- `envelope_curve_n` is present wherever `envelope_peak_z` is, and is ~41 at the 2000 ms default.
- A written read of whether 0.70 tracks z/prominence on that dump, including 50/g009 post.
  **No threshold change in this plan**, whether the split looks clean or not.

Guard revision (if any) is a follow-up TEMP after this dump is read.
