# TEMP — Nominal-first ordering for the `lag_decision` envelope rescue

**Status:** implemented + validated, 2026-08-20. Unit tests (all 5 Exit shapes) and the 12-pair
real-media re-run (`gap-files/2026-08-20-nominal-first-{off,on}-decoupled-unconditioned`) both meet
every Exit bar: 0 mismatches across 505 shoulders (not_consulted/declined), manifest byte-identical,
31/g7–g11 and pair-50 g8–g19 match the stated values exactly. Record-only; S1 production plumbing
(see § Non-goals) is next.
Parent: [archive/TEMP-lag-envelope-recenter-plan.md](archive/TEMP-lag-envelope-recenter-plan.md)
(superseded — its Change shipped the envelope mechanism this plan reorders; its Result sections
are this plan's validation baselines). Sibling:
[TEMP-envelope-adaptive-widen-plan.md](TEMP-envelope-adaptive-widen-plan.md) (window sizing +
rescue-branch guard refinement — re-scoped by this plan; see § Relation to adaptive-widen).

---

## Problem

The recenter plan ran the coarse envelope **first** and, when its guard passed
(`envelope_peak_r ≥ 0.70`, peak interior), ran the fine ±600 ms sweep *only* at the shifted
center. The nominal reading was never taken. Its own smoke join found the consequence: 26/g6 —
whose nominal measurement is a healthy near-nominal lock (pre −41.754 ms `r` 0.997, post
−45.428 ms `r` 0.993) — was silently replaced by a confident, self-consistent, **wrong** lock at
+1880.15 / +1876.47 ms (envelope 1400 ms, `envelope_peak_r` 0.946/0.961, fine `r` 0.927/0.920,
shoulders agreeing within 3.7 ms). The gap's content carries a ~480.5 ms self-similarity comb (the
same comb that once lured production's fill scoring −1.45 s from the same landmark), so
cross-shoulder agreement and a coefficient floor are both defeated by repetition.

The failure is an **ordering** defect, not a guard-threshold defect. The recenter's own population
statement says it owns *shared far offsets where shoulders are dead at nominal*; 26/g6 is alive at
nominal, so the envelope had no business being consulted for movement. No coarse-pass statistic
needs to distinguish a real far peak from a plausible false one on a shoulder whose fine sweep at
nominal already answers the question at `r` 0.997.

## Change

Restructure `lag_at_placement` (`measure.rs:1071`, both the mono and selected-channel `lag_pair`
call sites) from *envelope → fine-at-recentered* into **measure → triage → rescue → accept**:

1. **Measure at nominal.** Run `lag_pair` with zero envelope shift — exactly the current
   `--lag-envelope-max-lag-ms 0` path, including the landmark-gated post chain. No new cost.
2. **Triage per shoulder.** A shoulder is a rescue candidate iff its nominal fine result is
   non-recoverable: `peak_r < 0.5`, `classify_lag`'s existing recoverable line (`measure.rs:418`)
   — i.e. verdict `Decorrelated` or low `Ambiguous`. A shoulder at or above 0.5 keeps its nominal
   summary unconditionally; the envelope may still be *recorded* on it but can never move it.
3. **Rescue only the dead shoulders.** Consult the coarse envelope with the existing guard
   (`envelope_peak_r ≥ 0.70`, ≥1 bin interior; `envelope_recenter_frames`, `measure.rs:1047`) and,
   where it proposes a center, re-run that shoulder's fine sweep there (one extra
   `lag_side_sweep`; the alive shoulder is not re-swept).
4. **Accept only improvement.** Keep the rescued summary iff it is itself recoverable
   (`peak_r ≥ 0.5`, verdict `TimingOffset`); otherwise keep the honest nominal reading with the
   declined candidate stamped in the `envelope_*` fields. Rescue fires only when nominal was
   < 0.5, so acceptance is monotone: the reported measurement can never be worse than the
   nominal-only run.

**Landmark chain stays anchored to the nominal pass.** `lag_pair` centers the post sweep on
`post_nominal + round(L_pre)` only when the pre lock clears `LagSummary::is_landmark`
(`measure.rs:925-938`) — that chain must be computed from the **nominal** pre result, never from a
rescued one. Concretely:

- A rescued pre is never a chain prior. If nominal pre is dead (< 0.5) and gets rescued, the post's
  nominal sweep already ran with `pre_shift = 0`; do not re-run or re-chain the post off the
  rescued pre. The alive-shoulder-untouched property (and the bit-identical exit bar) depends on
  this — a post that was alive and correct at nominal must never move because a *different*
  shoulder got rescued.
- A rescued post retries at `post_nominal + pre_shift + envelope_lag`, keeping whatever chain
  offset the nominal pass already computed (`pre_shift` from the nominal, always-landmark-gated
  pre — rescued or not), not just `b_mapped + envelope_lag` in isolation. This matches today's
  `env_post + pre_shift` composition (`measure.rs:938`) with `env_post` now conditional on triage
  instead of unconditional.

In short: nominal pre computes `pre_shift` once, before any rescue runs; rescue may replace either
shoulder's own *reported* summary, but never becomes a new prior for the other shoulder's search
center.

Record which branch ran — a per-shoulder `envelope_rescue` marker distinguishing *alive, never
consulted* / *rescued* / *rescue declined* — and update `_contract.placement` to: "fine sweep at
`b_mapped`; a non-recoverable shoulder (`peak_r` < 0.5) retries at `b_mapped + envelope_lag` when
that shoulder's coarse peak is interior and ≥ 0.70; the retry is kept only if it is itself
recoverable." Old corpora stay valid (`envelope_*` absent ⇒ nominal center; recenter-vintage
dumps are identified by the recenter plan's contract string).

**The `is_landmark` trap (do not step in it).** The triage predicate must be the `peak_r < 0.5`
recoverability line, **not** `LagSummary::is_landmark` (`peak_r ≥ 0.8` **and** `peak_z ≥ 12`,
`schema.rs:1026`). 26/g6's *correct* nominal locks have `peak_z` only 6.13/6.45 at `r`
0.997/0.993 — under a landmark-based triage the gap would count as dead, get rescued, and land
right back on the +1.88 s comb tooth. The z ≥ 12 bar answers "strong enough to chain the other
shoulder off", a different question; pair 26 is direct evidence that healthy gaps live below it.

## Non-goals

- Do not remove the envelope mechanism. It is the only instrument that makes pair 31's shared-far
  class measurable (31/g7 post detection at −1160.25 ms; g8–g11 at −1137.7..−1158.4 ms `r`
  0.96–1.00; g10's both-shoulder rescue). This plan demotes it to a rescue path; `0` still
  disables rescue entirely.
- Do not tune any threshold. Both numbers this plan uses (0.5 recoverable, 0.70 envelope guard)
  are existing, recorded values; changing them is
  [TEMP-envelope-adaptive-widen-plan.md](TEMP-envelope-adaptive-widen-plan.md)'s business, now
  confined to the rescue branch.
- Do not address the alive-but-wrong-at-nominal case (a repetition alias inside ±600 with
  `r ≥ 0.5` while the true peak is far). Nominal-first cannot see it, envelope-first could not fix
  it, and production has the same blindness today; it is a fine-sweep
  prominence/`top2_spacing_ms` question, out of scope.
- Do not gate or change patch/skip. Record-only, same as the parent; S1 production plumbing is a
  separate step and inherits *this* ordering when it lands.
- Do not re-decode in production. When S1 plumbs this measurement, the sweep must feed from the
  shoulder PCM production already holds for gate search — the per-gap cost budget (two FFTs per
  shoulder, double on rescued shoulders) assumes no new decode.
- Do not let a rescued shoulder become a chain prior for its sibling. See § Change's landmark-chain
  note — the chain always reads the nominal pre, never a rescued one.

## Exit

**Unit tests** (split the expectations of `lag_at_placement_envelope_recenter_is_per_shoulder`
and `lag_at_placement_envelope_shared_far_unlocks_both`; add the missing shapes):

- 26/g6 shape: strong true peak near 0 **plus** a periodic repeat inside the envelope window —
  alive at nominal, result bit-identical to the envelope-off run.
- 31/g7 shape: alive pre + dead post — pre untouched (nominal-centered), post rescued.
- pair-50 shape: dead at nominal, rescue lands < 0.5 — nominal `decorrelated` kept, candidate
  stamped declined.
- the trap test: alive shoulder with `r ≥ 0.5` but `peak_z < 12` must not be triaged as dead.
- **chain-isolation shape:** dead pre + alive post (post already recoverable at nominal). Assert
  the post's reported summary and search center are unaffected by whatever the pre's rescue does
  — bit-identical to a run where the pre's rescue is disabled. Distinguishes this from 31/g7's
  shape (alive pre + dead post) by chaining the other direction.

**Real-media re-runs** (same protocol, `--features calibration,he-aac`): pairs 31 + 50 and the
9-pair cell `{2,3,13,14,18,22,32,47,51}` + pair 26, on vs off, diffed against
`gap-files/2026-08-19-recenter-*`. The bars **tighten** relative to the parent's:

| check | bar |
|-------|-----|
| 26/g6 and every alive shoulder (9-pair cell, 31 g1–g6, 50 g1–g7) | **bit-identical** to the off-run — not "within re-quantization noise"; nominal-first never re-centers an alive shoulder, so the parent's few-ms tolerance and the two `peak_z` gate-line wobbles (3/7 pre, 18/39 post) must disappear |
| 31/g7 | pre −38.07 ms from a nominal-centered sweep; post rescued at −1160.25 ms |
| 31/g8–g11 | rescued at their recenter-on values (−1137.7..−1158.4; g10 both ~−1153) |
| pair 50 g8–g17 | verdicts identical to off-run; g12/g18/g19 rescues attempted and **declined** by the acceptance rule |
| `manifest.json` | patch/skip byte-identical everywhere — still record-only |

No ear pass needed: no audio output changes while record-only.

## Relation to adaptive-widen

This plan lands first and does not wait on
[TEMP-envelope-adaptive-widen-plan.md](TEMP-envelope-adaptive-widen-plan.md)'s diagnostic dump —
it introduces no new threshold. Adaptive-widen is re-scoped by it: the widen ladder and any
`envelope_peak_z` / `envelope_prominence` guard refinement live **inside the rescue branch**,
where a wrong guard decision costs a wasted fine sweep or an honestly-kept `decorrelated` row,
never a corrupted healthy measurement. 26/g6's fixture obligation transfers as "must never
*enter* the rescue branch" — enforced by ordering, tested by the bit-identical bar.
