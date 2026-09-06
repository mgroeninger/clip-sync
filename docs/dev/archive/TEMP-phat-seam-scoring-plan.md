# TEMP — Phase-transform (GCC-PHAT) seam scoring experiment

**Status:** closed 2026-09-04, refuted. See [../settled.md](../settled.md) § A
("GCC-PHAT (phase-transform) seam scoring, windowed or not, as a fix for
50/g10's post seam"). Real-buffer scan on 50/g10 (`gap-files/2026-09-04-g010-phat-win/`):
unwindowed PHAT pins its post-seam peak to exactly the ±30 ms search boundary
(edge-pinning artifact, not a located seam); Hann-windowing (added mid-plan to
test spectral leakage as the cause) moves the peak off the boundary but
collapses prominence ~5000–7000× to noise-flat, revealing no real peak
underneath. `domain/policies/seam_locks.rs` was never touched, per this
plan's own scope fence in § 1. Successor: `TEMP-disjoint-offseam-lock-plan.md`
(1 s Pearson on A disjoint from the seam template). The intermediate
`TEMP-seam-anchor-relocation-plan.md` did not run that test.

**Status (original, superseded above):** draft, 2026-09-03. Experiment, not yet a Change. Diagnostic-only:
touches the fingerprint path (Tier-3), does **not** touch the production gate
(`seam_locks.rs`) until/unless the corpus comparison in § 3 says so.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Post-join defects (pairs 31
and 50)* → *50/g10 — echo (post join) + stutter (pre join)*.

**Companions:** `docs/dev/settled.md` § A ("50/g10 fix: sub-frame
waveform-lag refinement" — refuted, unrelated mechanism) and
[archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)
Result 4 (the mechanism this experiment is trying to fix).

**Media hygiene:** pair index + gap index only.

---

## 0. Problem (normative)

50/g10's dual-fit decline is not a bad lag — it's an *undecidable* one.
`post_seam_r = 0.248`, `post_seam_prom = 0.0049`, `post_seam_z = 5.61`: real
whole-curve correlation is present, but no single lag stands out over its
neighbors (near-zero prominence). Content is quiet dialogue over a roughly
stationary background-noise bed (ear-confirmed) — the shape where time-domain
Pearson-r is expected to be flat across many candidate lags, because a
constant noise floor correlates with itself at every offset almost as well
as the true dialogue lag does.

**Hypothesis:** phase-transform whitening (GCC-PHAT) sharpens the true-lag
peak in exactly this shape of signal, because it discounts each frequency's
correlation contribution by its own magnitude before combining — so a loud,
broadband, stationary noise bed stops dominating the score, and whatever
phase-coherent structure exists in the quiet dialogue gets to compete on
equal footing.

This is unverified. The point of this plan is to find out cheaply, without
touching anything that gates a real patch decision.

## 1. Placement (normative)

The new curve is a domain **scoring primitive**, a sibling of
`lag_correlation_curve_auto` in `domain/seam_local.rs` — reuse its existing
FFT branch/threshold machinery, add phase whitening as a second curve
function rather than a mode flag on the first (keeps the well-tested
Pearson path untouched).

```
domain/seam_local.rs
  lag_correlation_curve_auto(a, b_ctx, max_lag)      // existing, untouched
  phat_lag_correlation_curve(a, b_ctx, max_lag)       // new, added here
```

The **only** caller in this plan is the fingerprint path:

```
application/gap_fingerprint/measure/probes.rs  →  calls phat_lag_correlation_curve
application/gap_fingerprint/schema.rs          →  new Tier-3 fields
```

`domain/policies/seam_locks.rs` (the production gate: `min_fill_correlation`,
`fill_absolute_floor`, `grow_post_lock`) is explicitly **out of scope** for
this plan. Promoting PHAT into the gate is a separate, later Change with its
own threshold calibration — not part of this experiment.

**Correction (2026-09-03):** the wire `GapFingerprint`'s `splice_dualfit` on
`tier: full` is not built directly from `probes.rs`'s live measurement —
`spec_to_fingerprint_summary` (`project.rs`) always rebuilds it from the
spec's persisted `SeamLocalTags` (single-source rule, A7: "anything not
stored here never reaches the wire, however carefully the probe measured
it" — `domain/gap_repair_spec.rs`). So the six PHAT fields also had to be
added to `SeamLocalTags` (not just `SpliceDualfit`) and threaded through
`tags_from_fields`, or every value computed in `probes.rs` was silently
discarded before serialization. Production's own `SeamLocalTags` builder
(`patch_audio/region/dual_fit.rs`) still sets all six to `None` explicitly —
production never computes PHAT, so this stays diagnostic-only; a from-decode
replay of a real patch reads `None` here for the same reason, not as a
special case.

**Addendum (2026-09-04):** prominence alone can't answer § 3's "does PHAT
disagree with already-confident Pearson calls" question — the two curves
aren't on a comparable scale, so a prominence-magnitude ratio conflates
"different scale" with "different lag". Added `pre_seam_lag_ms`/
`post_seam_lag_ms` (Pearson) and `pre_seam_phat_lag_ms`/`post_seam_phat_lag_ms`
(PHAT) to `SpliceDualfit`/`SeamLocalTags`, each curve's own fractional peak
lag (ms) over the same ±30 ms seam-uniqueness window already used for
prominence — so a same-lag-agreement check is now possible directly. Same
plumbing as the correction above (`tags_from_fields` → `SeamLocalTags` →
`spec_to_fingerprint_summary`); production's `dual_fit.rs` sets all four to
`None`. The 2026-09-03 corpus run (51 pairs, `gap-files/2026-09-03-phat-seam-fingerprints/`)
predates these fields and cannot answer the agreement question — needs a
rescan.

## 2. New fields (Tier-3, nothing gates on these)

Mirror the existing `pre_seam_r`/`pre_seam_prom`/`pre_seam_z` triplet:

- `pre_seam_phat_r`, `pre_seam_phat_prom`, `pre_seam_phat_z`
- `post_seam_phat_r`, `post_seam_phat_prom`, `post_seam_phat_z`

Same peak/prominence/z-score extraction logic already used for the Pearson
curve, just fed the PHAT curve instead — no new statistics vocabulary.

Schema doc comments must say plainly (as `seam_probe`'s do) that no gate
reads these fields yet.

## 3. Corpus comparison (the actual experiment)

One `--gap-fingerprints --features calibration,he-aac` run over the existing
corpus (per `docs/dev/corpus-fingerprint-run-protocol` — one pair at a time,
~15 GB RSS) produces both curves' fields on every gap, old data untouched.

Compare:

- **50/g10's post seam specifically**: does `post_seam_phat_prom` show a real
  peak where `post_seam_prom` (0.0049) does not?
- **Cases already confident under Pearson-r**: does PHAT roughly agree, or
  does it introduce disagreement on gaps that already patch clean? (A PHAT
  score that disagrees everywhere is not trustworthy just because it helps
  g10.)
- **Other low-prominence-despite-real-z cases in the corpus**, if any exist
  beyond 50/g10 and 31/g10 — this determines whether PHAT is worth a gate
  Change at all, or just explains one gap.

## 4. Decision point

- **PHAT sharpens g10's post seam and does not destabilize already-confident
  cases** → open a real Change to wire it into `seam_locks.rs`'s dual-fit
  gate (own threshold calibration, own corpus validation pass).
- **PHAT does not sharpen g10** (quiet dialogue may simply carry too little
  signal to localize by any correlation-based method) → close this plan,
  record refuted in `settled.md` § A, and the BACKLOG next action becomes an
  explicit decision to flag this gap shape for manual review instead of
  auto-patching.
- **PHAT disagrees broadly with Pearson-r on already-good gaps** → refuted as
  a drop-in gate signal regardless of g10; same close-out as above.

## 5. Done when

Either a working gate Change is scoped and BACKLOG'd, or this plan is
archived with a refuted verdict in `settled.md` § A and BACKLOG's 50/g10 row
is rewritten around the manual-review-flag alternative.
