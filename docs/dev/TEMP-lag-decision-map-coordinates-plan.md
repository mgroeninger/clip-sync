# TEMP — Report `lag_decision` in map coordinates

**Status:** in progress, 2026-08-18. Change landed (`LagSummary::is_landmark`, `lag_pair`
haystack-shift + landmark-gated chain, contract text); Exit re-run against the 14-gap census not
yet done.
Parent: [archive/TEMP-fill-placement-registration-plan.md](archive/TEMP-fill-placement-registration-plan.md)
(Step 5: `border_displacement_ms` is the reader-side correction to `frac_lag_ms`).
Census: `gap-files/2026-08-18-one-dead-listen/wide-sweep/` (gitignored; numbers below are the
durable record). Findings, not a work list, until **Change**.

`lag_decision` is a 1 s waveform sweep at **±600 ms around mapped nominal**. On 12 of the 13
lag-verdict one-dead gaps, that window already contains the true lock. The dump still reports
one shoulder `Decorrelated` because it correlates a **walked-off A template** against a B
haystack still **anchored on the gap edge**, then **chains post by raw `frac_lag`**, which the
schema already says is not the map offset.

This plan makes the **measurement** search and chain in map coordinates. It does not widen
`lag_max_lag_ms`, does not recenter far offsets, and does not decide 31/g7's skip-vs-fill.

> **Verification rule.** A `file:line` reference belongs only in a checklist item that is about
> to be executed. Design sections state the decision and its reason. Re-read `lag_pair` /
> `LagSummary::border_displacement_ms` before coding; the registration plan's Step 5 arithmetic
> is the contract this Change replaces.

---

## Census (measured 2026-08-18)

Predicate (findings §8): one unique (`peak_r ≥ 0.8`, `peak_z ≥ 12`) + one no-peak
(`prominence < 0.02` or `peak_r < 0.3`) → **13** of 308 patched, both shoulders, pairs 6 and 16
dropped. Instrument: throat-adjacent 1 s, **non-chained**, ±2000 ms, lag 0 = nominal `b_mapped`
at each throat (prototype `gap-files/scratch_shoulder_sweep.py`; keep Python out of the repo).
31 #11 (dump g10) added as a 14th control — not in the 13 (`peak_z` 8.6 < 12).

| bucket | n | who |
|---|---|---|
| `shared_near_false_dead` | **12** | 12 #2, 20 #3, 24 #32, 27 #3, 34 #6, 35 #2, 39 #4, 42 #17, 45 #5, 45 #6, 48 #5, 49 #9 |
| `collapsed_bridge` | 1 | 31 #8 (31/g7) — true lags −47 / −1160, `bridge_ms ≈ 0` |
| `shared_far_offset` | 1 | 31 #11 (31/g10) — both ~−1153 |

Hole 2 still has zero members. Dual-fit already rescued five of the 12 (independent ±600 around
nominal, seam-local windows): 12, 20, 24, 35, 39.

**Lag-verdict one-dead is a sensor class, not a media class.** Do not use the 13 as a cell, an
S2b population, or a recenter list.

The 2026-08-18 listen dumps (same decode as the WAVs) record the mechanism on the wire:

| gap | dump “dead” | `border_displacement_ms` | what the dump actually measured |
|---|---|---|---|
| 12 #2 pre | r 0.17 @ +468 | **1005** | template ~1 s before the throat; its match is outside ±600 of the gap-edge anchor |
| 35 #2 pre | r 0.07 @ +596 | **875** | same |
| 39 #4 post | r 0.27 @ +603 | **613** | same, on post; pre was live at +6 |
| 48 #5 post | r 0.13 @ −166 | **1311** | same |
| 27 #3 post | r 0.22 @ +48 | post disp 4 | **chain**: pre r 0.993 @ −549 with disp **568** |

When displacement is still inside the window, dump “live” is the correct match of the wrong 1 s.
Adding the recorded `D` recovers the census (map-true):

- 24 post: −56 − 338 = **−394 ms**
- 20 post: −64 − 226 = **−290 ms**
- 27 pre: −549 + 568 = **+19 ms**

Pair 27 is the chain failure in one line. Pre is a real lock of a 568 ms-walked template, so
`frac_lag` is −549. True map offset is +19. Post is searched around `b_mapped_end − 549 ms`.
True post (~+58) sits at fine lag ≈ 607 ms — past the ±600 edge — and is called `Decorrelated`.

The schema already states the conversion (`LagSummary::border_displacement_ms`):

- pre true ≈ `frac_lag_ms + border_displacement_ms`
- post true ≈ `frac_lag_ms − border_displacement_ms`

`lag_pair` never applies it to the search or to the sequential shift. It chains
`round(frac_lag_samples)` as `pre_shift`.

31/g7 and 31/g10 are **not** this defect. Both have tiny `D` (0.8 / 14 ms). Their true lags
really are outside ±600 of nominal.

---

## Goal

Keep the ±600 ms / 1 s waveform sweep. Change the **coordinate system** it searches and reports
in, so `frac_lag_ms` is the A↔B map offset at the throat, not the lock of a walked-off template
against a gap-edge haystack.

At true registration, `frac_lag_ms` reads the map offset (~0 when the pair is locked).
`border_displacement_ms` stays on the wire as a diagnostic of how far the template sat from the
throat; it is no longer a reader-side correction to `frac_lag_ms`.

**Non-goals**

- Do not widen `lag_max_lag_ms` as the primary fix, and do not run a corpus fingerprint dump to
  do it. The 12 are already in reach of a throat-adjacent ±600.
- Do not envelope-recenter these 12. Shared far offsets stay
  [TEMP-lag-envelope-recenter-plan.md](TEMP-lag-envelope-recenter-plan.md) (31 #11).
- Do not change dual-fit’s `SEAM_LOCAL_SEARCH_MS`, fill search radius, or `fill_offset_mode`.
- Do not put the result in a gate. Record-only, same as S1.
- Do not decide 31/g7 skip-vs-one-seam. Collapsed-bridge detection is a separate predicate
  (`|gap + post_lag − pre_lag| ≈ 0`) after this sensor is honest.
- Do not add a dump ±2000 field as the fix for these twelve. That field is for lags that are
  actually out of reach.

---

## Why map coordinates, not a bigger window

Walk-off + 12 % trim is correct for **production seam templates** (hold the splice off the
dropout). It is the wrong 1 s for **registration**. The B haystack stays on the gap edge, so a
template whose gap-facing edge sits `D` away reads `−D` / `+D` at truth — and if `D > 600 ms`,
reads `Decorrelated` even though the throat-adjacent 1 s locks inside ±600.

Sequential recenter (ledger A2) is built for a shared map offset plus a small bridge mismatch.
It assumes `frac_lag` *is* that offset. Chaining the uncorrected value slides post by `D_pre`.
Landmark-only chaining without the coordinate correction does not save pair 27: that pre already
clears `peak_r ≥ 0.8`, `peak_z ≥ 12`.

Dual-fit never chains and uses seam-local windows on the actual seam. That is why it rescued
five of the 12, and why the fix is to make `lag_decision` do the same kind of thing — not to
teach dual-fit to trust this dump.

---

## Change

In `lag_pair` / `lag_at_placement` (fingerprint path now; production path when S1 is re-scoped):

1. **Search in map coordinates.** Shift each shoulder’s B haystack so lag 0 is true
   registration of *that* template: pre anchor at `gap_start − D_pre`, post (before chain) at
   `gap_end + D_post`. Equivalent: keep the haystack on the gap edge and use a throat-adjacent
   1 s (cap walk-off so `D ≪ 600 ms`). Prefer the haystack shift if production still wants
   walk-off templates for the *seam*; either way, reported `frac_lag_ms` is the map offset.
2. **Chain post by the map-true pre lag**, not raw template-coordinate `frac_lag`. After (1)
   those are the same number. Today they differ by `D_pre` (pair 27: −549 vs +19).
3. **Chain only if that corrected pre is a landmark** — reuse the findings §8 unique bar
   (`peak_r ≥ 0.8`, `peak_z ≥ 12`). Otherwise search post at nominal (fail open). Weak-pre
   chaining is how 31/g10’s post reached −1152 under a 600 ms max lag; after this Change that
   post is *supposed* to look dead at ±600-of-nominal until envelope-recenter.

Contract `_contract.placement` must say so: “fine sweep in map coordinates (B haystack follows
template-edge displacement); post chained by map-true `L_pre` only when pre is a unique
landmark.” No consumer of `frac_lag_ms` / `border_displacement_ms` applies the old `± D`
correction in code (checked: `project.rs`, `gap_repair_spec.rs`'s `RegistrationTags`, the harness
corpus analysis/check modules, `route2-consensus.ps1`, `seam_local.rs`'s FFT-vs-naive test) — the
correction lived only in plan-doc prose and manual verification, so this Change needs no
backward-compatibility flag. Old fingerprint dumps remain readable; only their `frac_lag_ms`
values predate the coordinate fix.

The unit test that currently locks “true registration reads `−D` / `+D`”
(`lag_at_placement_reads_border_displacement_at_true_registration`) becomes the proof of the
new contract: true registration reads ~0, `border_displacement_ms` still equals `D`.

S1, when unblocked, plumbs **this** `frac_lag_ms`. Residual `applied − frac_lag` then compares
two map quantities; today it compares applied map to template coordinates and absorbs `D`.

---

## Order vs envelope-recenter

This plan **orders ahead of** [TEMP-lag-envelope-recenter-plan.md](TEMP-lag-envelope-recenter-plan.md).
Recenter’s fine sweep is existing `lag_pair`. Shipping recenter first would still chain
uncorrected `frac_lag` on every post, and its smoke-join exit (“median `frac_lag_ms` must not
move”; “do not spend the ~22 ms bias”) assumes the old coordinates. After this Change:

- Recenter’s fine sweep inherits map-true search and landmark-only chain.
- Recenter’s smoke join must expect the probe-`D` half of the −21.9 ms bias to **leave**
  `frac_lag_ms` (it remains on `border_displacement_ms`). The production-α ~25 ms late term is
  unchanged.
- 31/g10’s post looking dead between this Change and recenter is expected, not a regression.

Independent populations, shared function. Do not merge the plans.

---

## Exit

Same binary, map-coordinate search on vs off. Not a corpus fingerprint pass. Re-run the
gitignored listen dumps through the existing prototype (or the patched `lag_pair`) on the 14.

**The 12** (`shared_near_false_dead`): both shoulders live under the dump’s own unique bar;
`frac_lag_ms` agrees with the census throat-adjacent lag within ~10 ms; `|D|` may still be
large (walk-off is allowed) but no longer moves the reported lag. Pair 27 post is live near
+58, not `Decorrelated` at +48.

**Must not move**

| gap | expect |
|---|---|
| 31/g7 (31 #8) | pre stays ≈ −47 ms, unique; post still out of ±600 (true −1160); `bridge_ms` still ≈ 0 |
| 31/g10 (31 #11) | pre still weak / non-landmark; **post no longer chained to −1152**; dead at nominal ±600 until recenter |
| 26/g6 | stays near-nominal (≈ −50 ms), not the −1.45 s slide |

**Smoke join** on the oriented load-bearing cell `{2, 3, 13, 14, 18, 22, 32, 47, 51}`: every
shoulder that was unique stays unique. Median `frac_lag_ms` **may** move by ~`D` (the probe
half of Step 5’s −21.9 ms). That move is the Change, not a tolerance. Fail if a previously
unique near-zero lock becomes `Decorrelated`, or if a lock jumps by hundreds of ms.

If any of the 12 remain one-dead after map-true search *and* landmark-only chain, stop: that
row is a new question, not this plan.
