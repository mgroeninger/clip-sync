# TEMP — Seam anchor relocation experiment

**Status:** draft, 2026-09-04. Experiment, not yet a Change. Diagnostic-only:
touches the fingerprint path (Tier-3), does **not** touch the production gate
(`domain/policies/seam_locks.rs`) until/unless the corpus comparison in § 3
says so.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Post-join defects (pairs 31
and 50)* → *50/g10 — echo (post join) + stutter (pre join)*.

**Companions:** [archive/TEMP-phat-seam-scoring-plan.md](archive/TEMP-phat-seam-scoring-plan.md)
(refuted predecessor — rescoring the *same* ±30 ms window with GCC-PHAT
instead of Pearson-r) and
[archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)
Result 4 (the mechanism this experiment is trying to fix).

**Media hygiene:** pair index + gap index only.

---

## 0. Problem (normative)

50/g10's dual-fit decline is not a bad lag — it's an *undecidable* one, at
the seam itself. `post_seam_r = 0.248`, `post_seam_prom = 0.0049`,
`post_seam_z = 5.61`: real whole-curve correlation is present, but no single
lag within the ±30 ms search stands out over its neighbors. Content is quiet
dialogue over a roughly stationary background-noise bed (ear-confirmed) — a
constant noise floor correlates with itself at every offset almost as well
as the true dialogue lag does, so no correlation-domain rescoring of *that
same window* can be expected to help (confirmed for GCC-PHAT, windowed or
not, in the closed predecessor plan).

**The new idea does not try to score the ambiguous window better.** It
tries to avoid scoring it at all: find a confidently-locatable lag at a
point several seconds away from the seam, in content that actually has
structure (not uniform quiet dialogue-over-noise), using coarse/large
buckets so the search stays cheap, then carry that lag back to the seam and
confirm it with the existing ±30 ms Pearson window there — on the
assumption that registration drift over a 3–5 s separation is negligible
compared to the ±30 ms search radius.

**Numerical support for the assumption (pair 50's own measured drift):**
`offset_drift_secs = -0.337` over `duration_secs = 8280.066` in the pair's
alignment metadata ⇒ ~-41 µs/s ⇒ ~0.15–0.2 ms of expected drift over a 3–5 s
anchor-to-seam separation. That is roughly 1/150th of the ±30 ms window —
negligible if drift is actually smooth at that timescale **and no step
boundary sits between the anchor and the seam** (see § 4 risks 1 and 2).

**Why this is a genuinely different mechanism, not a rename of one that
exists:** the existing envelope rescue (`domain/lag_envelope.rs`) widens the
*lag reach* (`lag_envelope_max_lag_ms`, default 2000 ms) and the *template
window* (`lag_envelope_window_secs`, default 2.0 s) but keeps the anchor
**centered on the same nominal seam point** — it searches harder at the
ambiguous location, it does not relocate the search to a different, more
informative point in time. Grow-post (shipped, archived) widens the same
seam's window up to 1000 ms, same limitation. Neither one leaves the
undecidable window.

## 1. Scope note (carried over, still true)

This only ever applies within dual-fit's post-hoc rescue on an
already-scored-and-failed primary bracket search (`patch_audio/region/dual_fit.rs:133`)
— most gaps resolve via the primary `fill_mode = fit` bracket search and
never reach dual-fit, envelope rescue, grow-post, or this mechanism at all.

## 2. Placement (normative)

New domain primitive, sibling of the existing seam-local curves:

```
domain/seam_local.rs
  lag_correlation_curve_auto(a, b_ctx, max_lag)   // existing, untouched
  ...
  anchor_relocated_lag(a_track, b_track, seam_pos, search_radius_secs,
                        bucket_ms, confirm_window_ms)   // new
```

**Search direction is one-sided, not outward in both directions.** A seam
sits at the boundary between real A content and the hole. For the *post*
seam, searching backward from it walks into the hole itself, where A has no
real audio to correlate against — that is exactly the content being
repaired, so a "candidate" there is meaningless, not just low-quality.
Symmetrically, a *pre*-seam anchor can only search backward (earlier),
never forward into the hole. So: **post-seam anchors search strictly
forward** (deeper into real, already-resumed A content); **pre-seam
anchors search strictly backward** (further before the gap). Each seam
gets its own one-directional scan; there is no tie-break between two
directions because only one direction is ever valid.

Per-bucket measurement: `bucket_ms` is the **stride** between successive
candidate positions during the outward walk, not a correlation window
width. At each candidate position, run the same reference-segment length
already used by the existing pre/post seam windows in `seam_local.rs`
(no new segment-length parameter) through `lag_correlation_curve_auto`
with the existing ±30 ms local search, and read its prominence/z off the
existing `seam_local_peak` extraction — the same statistic used for the
final confirmation in step 4, just evaluated at a relocated position
first. This needs confirming against the actual segment-length parameter
`seam_local.rs` exposes today before implementation; flagging here rather
than inventing a second, incompatible window concept.

Mechanism:

1. From the seam position, walk outward in the single valid direction (see
   above), in `bucket_ms` strides, up to `search_radius_secs`, evaluating
   each candidate as described above, until one clears a **qualification
   floor**: reuse `min_fill_correlation` (default 0.35) as the r floor — the
   same floor the production gate already uses to mean "this is a real
   lock, not noise." No corpus-validated prominence floor exists yet to
   borrow (the closest analog, `SPLICE_MIN_PEAK_R = 0.85`, is a different
   instrument's admission floor, not a drop-in). Treat the prominence floor
   as an explicit output of § 5's corpus run, not a guessed constant —
   record the r-qualifying candidates' prominence distribution there before
   picking one.
2. Take the first (nearest) qualifying candidate, record its lag and its
   distance from the seam (`*_anchor_distance_ms`, always positive — "how
   far away from the gap," since direction is fixed by construction and
   carries no separate sign).
3. **Step-boundary guard (§ 4 risk 1):** between the anchor and the seam,
   apply the existing step-boundary signature test (`settled.md` § D:
   shoulders on either side of a candidate boundary disagreeing by ≈ a
   constant, sign-agnostic). If a boundary is detected, reject this
   candidate and resume the walk from just past it (do not fall through to
   step 4 with a rejected candidate, and do not simply stop — a step
   boundary invalidates one candidate, not the whole remaining radius).
4. Apply the existing ±30 ms `lag_correlation_curve_auto` window **at the
   seam**, but centered on the anchor's lag instead of the nominal map lag,
   and confirm it locks there (existing `min_fill_correlation` /
   `fill_absolute_floor`-shaped acceptance, diagnostic-only for now — see
   § 5).
5. **No qualifying result** (nothing clears the floor within
   `search_radius_secs`, every candidate found is step-boundary-rejected, or
   the step-4 confirmation fails to lock) is a normal, named outcome, not an
   error — see § 3's `*_anchor_reason` field. It must be distinguishable
   *which* of those three happened, since they imply different next steps
   (widen the radius vs. treat the content as genuinely unlocatable vs.
   suspect the drift assumption itself).

The **only** caller in this plan is the fingerprint path, same fence as the
predecessor:

```
application/gap_fingerprint/measure/probes.rs  →  calls anchor_relocated_lag
application/gap_fingerprint/schema.rs          →  new Tier-3 fields
```

`domain/policies/seam_locks.rs` is out of scope for this plan. Promoting
this into the gate is a separate, later Change with its own threshold
calibration.

## 3. New fields (Tier-3, nothing gates on these)

Both sides, spelled out (no "mirror the other side" left implicit — that
is how fields silently went unbuilt in the predecessor plan until its own
correction note caught it):

- `post_seam_anchor_lag_ms`, `post_seam_anchor_distance_ms`,
  `post_seam_anchor_r`, `post_seam_anchor_prom` — the relocated anchor's own
  measurement, at its own location. `distance_ms` is always positive (see
  § 2 — direction is fixed by construction, so there is nothing to sign).
- `post_seam_anchor_confirm_r`, `post_seam_anchor_confirm_prom` — the
  seam-local ±30 ms confirmation, centered on the anchor's lag.
- `post_seam_anchor_reason` — populated only when no accepted anchor
  exists (§ 2 step 5); a named reason, not a bare `None`, following the
  existing abstention-reason convention (`SeamResidualVerdict`'s
  `beyond_lag_reach`/`no_reference_window`/etc.). Candidate values: no
  candidate cleared the r floor within `search_radius_secs`; every
  candidate that cleared it was step-boundary-rejected; the best candidate
  cleared the floor and the boundary guard but its seam-local confirmation
  (step 4) failed to lock.
- `pre_seam_anchor_lag_ms`, `pre_seam_anchor_distance_ms`,
  `pre_seam_anchor_r`, `pre_seam_anchor_prom`, `pre_seam_anchor_confirm_r`,
  `pre_seam_anchor_confirm_prom`, `pre_seam_anchor_reason` — identical set,
  pre-seam side (searches backward, per § 2).

Schema doc comments must say plainly that no gate reads these fields yet.

**Decode cost, open before implementation.** Reaching several seconds away
from the seam likely needs a wider PCM read of A and/or B than the shoulder
buffers `probes.rs` currently decodes for the existing pre/post seam
windows (which are sized to the seam's own local window, not a multi-second
reach). Check what's already decoded for this gap before assuming a new
read is needed — the predecessor plan's own § 1 correction shows how easily
a field gets silently starved of the data it needs when this isn't checked
up front. If a new/wider decode is required, size it against the same
corpus-fingerprint-run-protocol RSS budget (~15 GB) the other Tier-3 probes
already share, not as a separate cost.

## 4. Open risks (flag explicitly, do not paper over)

1. **An undocumented step boundary could sit inside the search radius, and
   pair 50 specifically has already shown this failure mode is real.**
   "Map step" here means a genuine, content-level discontinuity in the
   A↔B correspondence: instead of a single near-constant offset (plus the
   smooth linear drift `offset_drift_secs` already captures), the true
   offset jumps by a fixed amount at one point in the timeline — most
   likely because the two releases differ in edited content by about that
   much somewhere earlier in the runtime. This is not a decode/storage
   artifact; it was measured directly on decoded PCM (wide ±3000 ms
   sliding-window cross-correlation, `archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md`
   Result 1) and corroborated independently across several other
   investigations, all converging on the same ~+2.789 s value for pair 50.
   If the outward scan in § 2 crosses a boundary like this, the "anchor"
   it finds is confidently locked to the *wrong* regime — not a small
   drift error, but an error on the order of the step itself (seconds, not
   milliseconds), and it will look exactly as confident as a correct
   anchor.

   **What is and isn't established for 50/g10 itself:** pair 50's own
   documented +2.789 s transition falls between gap indices g7 and g8
   (`settled.md` § D), and g10's whole cluster (g8–g10, g14–g16) already
   sits uniformly inside the *post*-step regime, corrected upstream by
   production's anchor-seam bracket search before any of this seam-local
   work runs — so the specific, already-measured boundary is not
   necessarily within a "several seconds" search radius of g10. **This has
   not been checked against actual gap-to-gap timing**, so do not assume
   either way. The generalizable risk is that pair 50 has now demonstrated
   this class of boundary exists somewhere in its timeline at least once,
   which is reason enough to check for one near *any* candidate anchor
   rather than assume none exists.

   **Required guard, not just a caveat:** before accepting a relocated
   anchor, apply the existing step-boundary signature test (`settled.md`
   § D: pre- and post-anchor shoulders disagreeing by ≈ a constant amount,
   sign-agnostic) between the anchor and the seam. An anchor that fails
   this check must be rejected, not merely flagged — it does not degrade
   gracefully into "worse than nominal by a little," it fails by seconds.

2. **Local drift smoothness (short of a full step) is unverified.** Even
   with risk 1's guard in place, the 0.15–0.2 ms estimate is a *global
   average* drift rate; it says nothing about whether drift is smooth on
   the anchor-to-seam scale specifically. Sub-step irregularities (e.g. a
   variable-frame-rate segment) could still be missed by a boundary check
   tuned to catch second-scale steps.
3. **A richer anchor point is not guaranteed to exist nearby.** If a whole
   scene is uniformly quiet dialogue-over-noise (plausible — that is g10's
   own content), `search_radius_secs` may need to grow past "a few
   seconds" before finding real structure, at which point risk 1 becomes
   more likely, not less (a wider search is more likely to cross a
   boundary). The plan must record how far g10 (and any other corpus
   member in this shape) actually had to search.

## 5. Corpus comparison (the actual experiment)

One `--gap-fingerprints --features calibration,he-aac` run over the existing
corpus (per `docs/dev/corpus-fingerprint-run-protocol` — one pair at a time,
~15 GB RSS) produces the new fields on every gap, old data untouched.

Compare:

- **50/g10's post seam specifically**: does the relocated anchor produce a
  real, prominent peak (`post_seam_anchor_prom` well above the ~0.005 floor
  measured at the seam itself), and does the seam-local confirmation
  (`post_seam_anchor_confirm_r`/`_prom`) lock there?
- **Anchor distance distribution**: how far from the seam did a qualifying
  anchor have to be found, corpus-wide? A distribution concentrated at a
  few seconds supports the plan; one that regularly needs tens of seconds
  undercuts the "negligible drift" assumption this plan is built on.
- **Cases already confident under the seam-local Pearson window**: does the
  anchor-confirmed lag roughly agree with the seam's own lock, or does
  relocation introduce disagreement on gaps that already patch clean? (An
  anchor mechanism that disagrees broadly is not trustworthy just because it
  helps g10.)

## 6. Decision point

- **Anchor relocation locates a real, confirmable peak at 50/g10's post seam,
  and does not destabilize already-confident cases** → open a real Change to
  wire it into `seam_locks.rs`'s dual-fit gate as a fallback when the
  in-place window is prominence-flat (own threshold calibration, own corpus
  validation pass).
- **Anchor relocation cannot find a qualifying anchor near g10 within a
  reasonable radius, or the confirmation still fails to lock** → close this
  plan, record refuted in `settled.md` § A, and the BACKLOG next action
  becomes an explicit decision to flag this gap shape for manual review
  instead of auto-patching (the fallback already named in the predecessor
  plan).
- **Anchor relocation disagrees broadly with the seam-local Pearson window
  on already-good gaps** → refuted as a drop-in gate mechanism regardless of
  g10; same close-out as above.

## 7. Done when

Either a working gate Change is scoped and BACKLOG'd, or this plan is
archived with a refuted verdict in `settled.md` § A and BACKLOG's 50/g10 row
is rewritten around the manual-review-flag alternative.
