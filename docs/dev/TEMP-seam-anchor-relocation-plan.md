# TEMP — Seam anchor relocation experiment

**Status:** draft, revised 2026-09-05 (mechanism in § 2 replaced — see note
below). Experiment, not yet a Change. Diagnostic-only: touches the
fingerprint path (Tier-3), does **not** touch the production gate
(`domain/policies/seam_locks.rs`) until/unless the corpus comparison in § 3
says so.

**2026-09-05 revision note:** the original § 2 re-ran the existing fine
±30 ms Pearson window at every candidate position along the outward walk.
That both (a) contradicts § 0's own framing of the search as "coarse/large
buckets," which is closer to the codebase's existing wide-envelope
confirmer, and (b) doesn't solve the actual problem: a fine window fails on
featureless content regardless of *where* it's centered, so walking it to a
new position without first checking that position has structure just moves
the same failure mode around. § 2 is rewritten below as an explicit
three-stage funnel — cheap single-track feature screen, then coarse
cross-correlation only where the screen passes, then the existing fine
confirm — reusing primitives that already exist in the codebase rather
than inventing new ones.

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
This 3–5 s figure is the *near*-edge distance only — § 2 step 1 works out
the far-edge distance the default constants actually reach (~15 s) and
rechecks the drift budget there.

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

**Three-stage funnel, reusing existing primitives — no new correlation
math, no new domain type for the screen or the coarse pass:**

```
domain/gap_signature.rs
  energy_envelope_is_flat(bins)                    // existing, reused — stage 1
domain/gap_energy.rs
  energy_bins(...)                                  // existing, reused — builds stage 1's input
application/gap_fingerprint/measure/probes.rs
  wide_envelope_side(a_win, b_wave_ctx, sample_rate,
                      env_bin, wide_lag_samples)     // existing, reused (relocated + widened) — stage 2
  seam_local_peak(...) via lag_correlation_curve_auto // existing, untouched — stage 3 (unchanged confirm)
  anchor_relocated_lag(...)                          // new — orchestrates 1→2→3 over the walk
```

(`domain/gap_signature.rs` also has a bool-mode sibling,
`bool_bins_have_anchor_contour` — not used here. It's only reachable today
through `effective_mode`'s own `Auto`-mode fallback, `gap_signature.rs:159-179`,
which is a different decision than stage 1's per-candidate screen. Not
plumbing it in is a deliberate scope cut, not an oversight: if § 5 finds
`GapSignatureMode::Bool` gaps need a stage-1 path too, that's a follow-up,
not a silent gap in this plan.)

Stage 1 is exactly the same check the `auto` gap-signature mode already
uses to decide whether a gap's own pre/post context has "structure an
anchor seam can latch onto" (`GapSignature::has_anchor_seam_contour`,
`gap_signature.rs:53-61`) — it is not being adapted, it is being called at
a different position. `energy_envelope_is_flat` gates a peak-normalized
log-RMS envelope on `(max − min) ≤ 0.05`, touching only the A track; it is
what makes stage 1 cheap enough to run at every stride of the walk, since
it never decodes or correlates against B.

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

Mechanism:

1. From the seam position, walk outward in the single valid direction (see
   above), in `ANCHOR_BUCKET_MS` strides, up to `ANCHOR_SEARCH_RADIUS_SECS`
   — this bounds the candidate's **near edge** (its start offset from the
   seam), not the content it reads: since each candidate's window is
   `ANCHOR_WIDE_WINDOW_SECS` (10 s) long and the radius default (5 s) above
   is *shorter* than that, the far edge of even the first candidate already
   reaches content up to `ANCHOR_SEARCH_RADIUS_SECS + ANCHOR_WIDE_WINDOW_SECS`
   (up to ~15 s) from the seam. § 0's drift estimate (0.15-0.2 ms over 3-5 s)
   must be rechecked against that far-edge distance, not the near-edge one —
   at pair 50's measured `-41 µs/s`, 15 s is still only ~0.6 ms of drift,
   comfortably under the ±30 ms confirm window, but this needs stating
   explicitly rather than quietly relying on the smaller number. If § 5
   shows the far edge is too permissive, shrink `ANCHOR_WIDE_WINDOW_SECS`
   or grow `ANCHOR_SEARCH_RADIUS_SECS` so the two stay consistent — right
   now they're set independently and happen not to conflict, not derived
   from each other.
   At each stride position, take the **same** A-only window of length
   `ANCHOR_WIDE_WINDOW_SECS` that would be handed to stage 2 if this
   candidate qualifies (one segment, not two window concepts), bin it with
   `energy_bins` (`cfg.silence_peak_fraction`, `cfg.absolute_silence_rms` —
   both already fields on `FingerprintConfig`, `config.rs:54-55`, already
   threaded through `probes.rs`; bin width a new `ANCHOR_SCREEN_BIN_MS`
   constant, not reusing the fingerprint-bin-count-sensitive `bin_frames`
   any structure-match caller depends on), and run it through
   `energy_envelope_is_flat`. Flat → reject this candidate *without* ever
   touching B or running a correlation, and continue the walk. Not flat →
   proceed to stage 2 on this same window (no re-slice, no re-decode).
   This ordering is the point of the redesign: the fine ±30 ms window was
   never the thing failing because of *where* it was centered — it fails on
   featureless content regardless of position (§ 0). Stage 1 makes sure
   stage 2 and stage 3 are only ever spent on positions that have a chance.
2. **Stage 2 — coarse correlation, on stage-1-qualifying candidates only.**
   Run `wide_envelope_side` — reused verbatim, not reimplemented — against
   the corresponding B slice, with its window widened from the existing
   `WIDE_ENV_WINDOW_SECS` (2.0 s, today's nominal-placement confirmer) to a
   new `ANCHOR_WIDE_WINDOW_SECS` and its lag reach set by a new
   `ANCHOR_WIDE_LAG_MS` (starting point: reuse `WIDE_ENV_MAX_LAG_MS`'s
   400 ms unless § 5's corpus run shows drift beyond that at this
   distance from the seam — this is generous already, since the plan's own
   drift estimate is 0.15–0.2 ms over a few seconds, see § 0). The envelope
   bin itself (`WIDE_ENV_BIN_MS`, ~10 ms) is untouched — only the window and
   lag reach change; cost is linear in window/bin (not quadratic), so
   widening the window ~5× over the nominal confirmer costs proportionally
   more per call, offset by stage 1 having already screened out most
   candidates before any call happens at all.
   A candidate **qualifies** here when `wide_envelope_side`'s `peak_r`
   clears `min_fill_correlation` (default 0.35) — the same floor the
   production gate already uses to mean "this is a real lock, not noise."
   No corpus-validated prominence floor exists yet to borrow (the closest
   analog, `SPLICE_MIN_PEAK_R = 0.85`, is a different instrument's
   admission floor, not a drop-in) — treat it as an explicit output of § 5,
   recording the r-qualifying candidates' prominence distribution before
   picking one.
3. Take the first (nearest) stage-1-and-2-qualifying candidate, record its
   stage-2 lag and its distance from the seam (`*_anchor_distance_ms`,
   always positive — "how far away from the gap," since direction is fixed
   by construction and carries no separate sign).
4. **Stage 3 — the existing ±30 ms `lag_correlation_curve_auto` confirm,
   unchanged, at the seam**, centered on the candidate's stage-2 lag
   instead of the nominal map lag (existing `min_fill_correlation` /
   `fill_absolute_floor`-shaped acceptance, diagnostic-only for now — see
   § 5).
5. **Step-boundary guard (§ 4 risk 1) — adapted, not reused; see § 4 risk 1
   for why — and evaluated *after* stage 3, not on the stage-2 lag.**
   Stage 2's lag is quantized to `WIDE_ENV_BIN_MS` (~10 ms); the guard's
   fixed floor (`|step| > 2.0 ms` — the same value as
   `clip-sync-repair-harness`'s `CLEAN_STEP_MS`, `report.rs`, but
   re-declared locally: `probes.rs` cannot depend on the harness crate,
   which depends on `clip-sync-repair`, not the other way around) would be
   dominated by that quantization noise if applied to the coarse lag
   directly. So: run stage 3's confirm first, then apply the guard to the
   difference between the confirm's precise lag and the seam's own nominal
   lag. If it trips, reject this candidate (do not accept a locked-but-
   wrong-regime confirm) and resume the walk from just past it — a step
   boundary invalidates one candidate, not the whole remaining radius. This
   is deliberately a weaker guard than `settled.md` § D's full rule — see
   § 4 risk 1 for the gap and the alternative.
6. **No qualifying result** is a normal, named outcome, not an error — see
   § 3's `*_anchor_reason` field, now with four distinguishable cases
   (stage 1 never passed within the radius; stage 1 passed somewhere but
   stage 2 never cleared the r floor; a stage-2 qualifier existed but every
   one was step-boundary-rejected; the best candidate passed 1–2 and the
   boundary check but stage 3 failed to lock). They imply different next
   steps — widen the radius, treat the content as genuinely unlocatable, or
   suspect the drift assumption itself — so collapsing them loses exactly
   the information § 5 needs.

**Tunable placement (checked against the codebase, 2026-09-05).**
`ANCHOR_SEARCH_RADIUS_SECS`, `ANCHOR_BUCKET_MS`, `ANCHOR_SCREEN_BIN_MS`,
`ANCHOR_WIDE_WINDOW_SECS`, `ANCHOR_WIDE_LAG_MS`, and
`ANCHOR_CONFIRM_WINDOW_MS` are plain crate-local constants beside
`DUALFIT_SEAM_UNIQ_LAG_MS` (`probes.rs:22`) and `WIDE_ENV_WINDOW_SECS`
(`pcm.rs:260`) — both existing precedent for a Tier-3-only knob that nothing
gates on:

```rust
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_SEARCH_RADIUS_SECS: f64 = 5.0;
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_BUCKET_MS: f64 = 250.0;
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_SCREEN_BIN_MS: f64 = 100.0;
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_WIDE_WINDOW_SECS: f64 = 10.0;
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_WIDE_LAG_MS: f64 = 400.0;
pub(in crate::application::gap_fingerprint::measure) const ANCHOR_CONFIRM_WINDOW_MS: f64 = 30.0;
```

`ANCHOR_SCREEN_BIN_MS` is a placeholder pending § 5 — it only needs to be
fine enough that a real feature (e.g. a spoken syllable) isn't averaged away
by the gating bin; it does not need to match `WIDE_ENV_BIN_MS` since stage 1
never feeds a lag estimate anywhere, only a bool.

Deliberately **not** `infrastructure/config.rs` (where `fill_seam_search_secs`
and `lag_envelope_*` live): those get TOML/CLI surface and validation because
they are real gate-facing tunables someone needs to adjust in production.
This mechanism is diagnostic-only per this plan's own header — giving it
config/CLI surface now would be building plumbing for a Change that may
never happen (§6 may refute this entirely). If § 6 promotes this into the
gate, that is the point to add real config wiring, with its own calibration
— not before.

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
  `post_seam_anchor_r`, `post_seam_anchor_prom` — the accepted candidate's
  **stage 2** measurement, at its own location. `distance_ms` is always
  positive (see § 2 — direction is fixed by construction, so there is
  nothing to sign).
- `post_seam_anchor_screened_count` — how many stride positions stage 1
  rejected as flat before this candidate (or before giving up), corpus-wide
  input to § 5's "how far did anchor have to search" question independent
  of `distance_ms` alone, since a long flat run and a short one that simply
  needed one extra stride both produce small `distance_ms` values but very
  different screened counts.
- `post_seam_anchor_confirm_r`, `post_seam_anchor_confirm_prom` — the
  **stage 3** seam-local ±30 ms confirmation, centered on the stage-2 lag.
- `post_seam_anchor_reason` — populated only when no accepted anchor
  exists (§ 2 step 6); a named reason, not a bare `None`, following the
  existing abstention-reason convention (`SeamResidualVerdict`'s
  `beyond_lag_reach`/`no_reference_window`/etc.). Four values, matching § 2
  step 6 exactly: `stage1_no_contour` (every stride within
  `ANCHOR_SEARCH_RADIUS_SECS` was flat); `stage2_no_correlation` (at least
  one candidate passed stage 1 but none cleared the stage-2 r floor);
  `step_boundary_rejected` (a stage-2 qualifier existed but every one
  failed the post-stage-3 step-boundary guard); `confirm_failed_to_lock`
  (the best candidate passed stages 1, 2, and the boundary guard, but
  stage 3 did not lock).
- `pre_seam_anchor_lag_ms`, `pre_seam_anchor_distance_ms`,
  `pre_seam_anchor_r`, `pre_seam_anchor_prom`, `pre_seam_anchor_screened_count`,
  `pre_seam_anchor_confirm_r`, `pre_seam_anchor_confirm_prom`,
  `pre_seam_anchor_reason` — identical set, pre-seam side (searches
  backward, per § 2).

Schema doc comments must say plainly that no gate reads these fields yet.

**Decode cost, checked 2026-09-05 — resolved, not a blocker.** Both tracks
are already decoded whole per pair before any seam-local probe runs: `A` as
`a_pcm.samples` (full decode; see the `dual_fit.rs`-mirroring comment at
`probes.rs:310`, "A is decoded whole here") and `B` as `b_samples_full`
(`characterize.rs`). What's narrow today is only the **slice** handed down
to the seam probes — `characterize.rs:152-159` computes `pad_lead`/`pad_tail`
from `gap_signature_context_secs + fill_border_search_secs +
fill_align_margin_secs`, and that sum is not guaranteed to reach
`ANCHOR_SEARCH_RADIUS_SECS` (5 s default above). So there is no new decode
and no new RSS cost (the ~15 GB corpus-fingerprint-run-protocol budget is
unaffected) — implementation just needs to widen `pad_lead`/`pad_tail` (or
thread a second, wider slice specifically for the anchor walk) so the
already-decoded PCM is actually reachable at `search_radius_secs`.

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

   **Required guard, not just a caveat — but "the existing test" overstated
   what's actually reusable (checked 2026-09-05).** `settled.md` § D's rule
   is not a callable function anywhere in `clip-sync-repair` — it is a
   corpus-wide statistical census, coded only in
   `clip-sync-repair-harness`'s `report.rs`/`gap-fingerprint-stats` binary,
   operating post-hoc on already-dumped JSON across *all* of a pair's gaps.
   Per `archive/TEMP-step-boundary-census-plan.md` § 4, its rule has two
   pieces the plan itself calls "load-bearing together": (1) an *adaptive*
   step threshold, `|step| > max(CLEAN_STEP_MS, 6 × MAD(pair's admitted
   steps))`, where the MAD comes from the pair's *other* gaps, not the two
   points being compared; and (2) a *sequence* test — ≥2 neighbouring
   admitted gaps settling at a new level on each side of a candidate
   boundary, which is what separates a real boundary from a one-gap
   outlier excursion. Neither ingredient exists in the shape this plan
   needs: an anchor candidate is an arbitrary point from an outward walk,
   not one of the pair's catalogued gaps, so it has no "neighbouring gaps"
   to run a settling test on, and no per-pair step distribution is
   naturally available inside a single-gap fingerprinting pass to compute
   MAD from.

   So step 5's guard is a **deliberately weaker, fixed-floor adaptation**
   (`|step| > 2.0 ms`, no MAD, no sequence/settling check), not a reuse of
   § D's rule. That is an accepted, named simplification for this
   diagnostic-only experiment, not an oversight — but it means the guard
   can still be fooled by a sub-2 ms-detectable boundary that a full census
   would catch, and § 5's corpus run should report how often the fixed
   floor and the (separately re-run, offline) harness census disagree on
   the same pairs before this is ever considered for promotion into the
   gate. An anchor that fails the fixed-floor check must still be
   rejected, not merely flagged — it does not degrade gracefully into
   "worse than nominal by a little," it fails by seconds.

2. **Local drift smoothness (short of a full step) is unverified.** Even
   with risk 1's guard in place, the 0.15–0.2 ms estimate is a *global
   average* drift rate; it says nothing about whether drift is smooth on
   the anchor-to-seam scale specifically. Sub-step irregularities (e.g. a
   variable-frame-rate segment) could still be missed by a boundary check
   tuned to catch second-scale steps.
3. **A richer anchor point is not guaranteed to exist nearby.** If a whole
   scene is uniformly quiet dialogue-over-noise (plausible — that is g10's
   own content), `ANCHOR_SEARCH_RADIUS_SECS` may need to grow past "a few
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
  real, prominent peak? `post_seam_anchor_prom` is a **different
  instrument** than the `post_seam_prom = 0.0049` measured at the seam
  itself (§ 0) — that number came from the fine ±30 ms Pearson curve, while
  `post_seam_anchor_prom` comes from stage 2's coarse envelope-lag curve —
  so it is not a valid before/after comparison on the same scale. Judge it
  against the r-qualifying-candidate prominence distribution § 5 collects
  corpus-wide (§ 2 step 2), not against the old fine-window number. The
  actual before/after comparison on a shared scale is
  `post_seam_anchor_confirm_r`/`_prom` (stage 3, same fine ±30 ms
  instrument as `post_seam_r`/`post_seam_prom`) — does *that* lock where
  the un-relocated fine window at the nominal seam did not?
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
