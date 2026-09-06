# TEMP — Seam anchor relocation experiment

**Status:** archived 2026-09-06. The funnel did not test its own § 0.
Corpus dump `gap-files/2026-09-05-seam-anchor-relocation`: 967/968
accepted anchors were the first 250 ms stride of a 10 s envelope on the
seam. Successor (the actual off-seam test):
[TEMP-disjoint-offseam-lock-plan.md](../TEMP-disjoint-offseam-lock-plan.md).
Verdict: [settled.md](../settled.md) § A.

**Original status:** draft, revised 2026-09-05 (mechanism in § 2 replaced — see note
below). Experiment, not yet a Change. Diagnostic-only: touches the
fingerprint path (Tier-3), does **not** touch the production gate
(`domain/seam_locks.rs`) until/unless the corpus comparison in § 3
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

## 1. Scope note (carried over, still true — but see the diagnostic-run
   exception below)

This only ever *matters* within dual-fit's post-hoc rescue on an
already-scored-and-failed primary bracket search (`patch_audio/region/dual_fit.rs:133`)
— most gaps resolve via the primary `fill_mode = fit` bracket search and
never reach dual-fit, envelope rescue, grow-post, or this mechanism at all.
That is a statement about when the mechanism would ever be **consulted** if
it were promoted into the production gate.

**It is not a statement about when the Tier-3 diagnostic computation
runs — checked 2026-09-05, this distinction was previously left implicit
and created a real conflict with § 5.** § 5's third comparison bullet
requires computing the anchor-relocated lag on gaps that are *already
confident* under the seam-local Pearson window, specifically to check
whether relocation disagrees with a seam that already patches clean. Gaps
like that never fail the primary bracket search and so never reach
dual-fit rescue at all — if the diagnostic computation only ran where § 1
says the mechanism "applies," § 5's own required comparison would have no
data to compare against. So: for this experiment, `anchor_relocated_lag`
must run on **every** gap that gets a `SpliceDualfit` computed at all in
the `--gap-fingerprints` dump (confirmed this already happens
unconditionally on every dump, § 2), regardless of `gate_pass` — the § 1
scope restriction describes production consultation, not diagnostic
measurement, and the two must not be conflated when implementing the
corpus run.

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
  anchor_relocated_lag(...)                          // new — orchestrates 1→2→3 over the walk
domain/seam_local.rs
  seam_local_peak(...) via lag_correlation_curve_auto // existing, untouched — stage 3 (unchanged confirm), called from probes.rs
```

(`domain/gap_signature.rs` also has a bool-mode sibling,
`bool_bins_have_anchor_contour` — not used here. It's reached today only
through `has_anchor_seam_contour` (`gap_signature.rs:53-61`), not through
`effective_mode`'s `Auto`-mode fallback (`gap_signature.rs:159-179`, which
calls only `energy_envelope_is_flat` to pick Bool vs. Energy mode) — a
different decision than stage 1's per-candidate screen either way. Not
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
   from each other. **This far edge must also be bounded by the nearest
   neighboring gap in the walk direction, not just by
   `ANCHOR_SEARCH_RADIUS_SECS` — see § 4 risk 5**: at pair 50's gap density
   (g10's own cluster), a 15.4 s far edge can reach into a different gap's
   dropout.
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

   **This is the only case that resumes the walk — checked 2026-09-05, an
   earlier draft left the two failure paths ambiguous.** If a candidate
   passes stage 1, stage 2, and this boundary guard, but stage 3 still
   fails to lock (`confirm_failed_to_lock`, step 6), that is **terminal for
   this seam** — do not continue the walk to try a farther candidate.
   Reasoning: a step-boundary rejection means this specific candidate was
   in the wrong regime, and the *next* candidate along the same walk is a
   fresh, independent chance to be in the right one. A confirm-lock
   failure on an otherwise-clean candidate is different — it means the
   nearest genuinely-structured, non-boundary-crossing point still didn't
   produce a confirmable lag, and a farther candidate only weakens the
   negligible-drift assumption this whole mechanism is built on (§ 0)
   further, not less — so retrying does not improve the odds, it only adds
   risk.
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
predecessor. But "add the fields to `schema.rs`" is not, by itself,
sufficient to make them appear in the dump — checked 2026-09-05 against how
the PHAT predecessor's fields actually reach `--gap-fingerprints` JSON, since
that plan's own correction note (top of this file) records that this exact
step was missed once already. The real chain has four touch points, two of
them a struct that this plan's caller diagram omitted entirely:

```
application/gap_fingerprint/measure/probes.rs
  anchor_relocated_lag(...)                 // new — computes the fields (§2)
  splice_dualfit_at(...) (probes.rs:267)    // existing — returns the schema.rs
                                             // struct directly; new anchor fields
                                             // are added here, on SpliceDualfit
application/gap_fingerprint/schema.rs
  SpliceDualfit (schema.rs:1441)            // new Tier-3 fields land here —
                                             // this IS the measurement's own
                                             // return type, not a separate DTO
domain/gap_repair_spec.rs
  SeamLocalTags (gap_repair_spec.rs:93)     // separate fixed-field struct —
                                             // new fields must ALSO be added
                                             // here or they are silently
                                             // dropped before the dump, same
                                             // as the PHAT predecessor's
                                             // near-miss
application/gap_fingerprint/project.rs
  tags_from_fields (project.rs:503-530)     // wiring: SpliceDualfit → SeamLocalTags
  spec_to_fingerprint_summary (project.rs:184-219)
                                             // wiring: SeamLocalTags → SpliceDualfit
                                             // (dump side) — BOTH directions must
                                             // be updated, not just one
```

Concretely: `splice_dualfit_at` (`probes.rs:267`) returns
`Option<SpliceDualfit>` — the schema.rs struct is already the measurement's
native output type, so new anchor fields land there directly, no new DTO
needed. But the value only survives into a `GapRepairSpec` via
`tags_from_fields` copying it field-by-field onto `SeamLocalTags`
(`project.rs:503-530`, e.g. `pre_seam_phat_r: d.pre_seam_phat_r`), and only
comes back out into the dumped JSON via `spec_to_fingerprint_summary`
copying `SeamLocalTags` back onto a `SpliceDualfit` (`project.rs:184-219`,
e.g. `pre_seam_phat_r: sl.pre_seam_phat_r`). A field added only to the
`schema.rs` struct and forgotten on `SeamLocalTags` compiles cleanly and
produces `null` in every dump, silently — there is no error, just an absent
field, exactly the failure mode the PHAT plan caught late. §3 lists 8 field
names *per seam* — implementation must add all **16** new fields (8 × {pre,
post}, not 8 total; this is the same undercount the predecessor plan itself
warns about, see the top of this file) in both structs and both copy sites
in the same change.

**Production stays diagnostic-only — required, not automatic.**
`application/patch_audio/region/dual_fit.rs:409-441` constructs a real
`SeamLocalTags` literal on every actual repair (`try_dual_fit`'s rescue-
success arm) — this is live, on the production patch path, not diagnostic
code. It already carries the precedent this plan must follow: the PHAT
fields are explicitly hardcoded to `None` there
(`dual_fit.rs:421-428`) under the comment *"GCC-PHAT experiment
(`TEMP-phat-seam-scoring-plan.md`): diagnostic-only, never computed on the
production patch path,"* and `SeamLocalTags`'s own doc comment
(`gap_repair_spec.rs:105-108`) states the general invariant: `None` on
every real patch, `Some` only on the fingerprint scan's own measurement.
Because `SeamLocalTags` is a plain struct literal, Rust will refuse to
compile `dual_fit.rs:409-441` once the new fields exist until they are
explicitly set — so there is no risk of silently *omitting* this, only of
setting them without the matching "diagnostic-only" comment. Add all
**16** new anchor fields to that literal as explicit `None`s with the same
annotation the PHAT fields carry.

`domain/seam_locks.rs` is out of scope for this plan. Promoting
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
  exists (§ 2 step 6); a named reason, not a bare `None`, following this
  codebase's convention of a dedicated reason enum per mechanism (e.g.
  `NotEvaluatedReason`, `LocalGainReason`, `GapFillSkipReason`,
  `ProgramGainRefusal` — no mechanism in `domain/` shares its "why no
  measurement" enum with another; `ResidualUninformative`
  (`domain/policies/seam_residual.rs:136`) was considered as a reuse
  candidate and rejected: its variants and the exhaustive matches over it
  (`seam_residual.rs:749-764`, `1020-1039`) are shaped around floor-probe
  semantics specifically, and none of this plan's four outcomes maps onto
  them without inventing a meaningless arm). Four values, matching § 2
  step 6 exactly: `stage1_no_contour` (every stride within
  `ANCHOR_SEARCH_RADIUS_SECS` was flat); `stage2_no_correlation` (at least
  one candidate passed stage 1 but none cleared the stage-2 r floor);
  `step_boundary_rejected` (a stage-2 qualifier existed but every one
  failed the post-stage-3 step-boundary guard); `confirm_failed_to_lock`
  (the best candidate passed stages 1, 2, and the boundary guard, but
  stage 3 did not lock). **Must be a plain, unit-variant-only enum
  deriving `Copy`** — `SpliceDualfit` itself derives
  `Copy` (`schema.rs:1440`), and every field on a `Copy` struct must
  itself be `Copy`; a `String` reason (or any variant carrying owned/boxed
  data) will not compile here.
- `pre_seam_anchor_lag_ms`, `pre_seam_anchor_distance_ms`,
  `pre_seam_anchor_r`, `pre_seam_anchor_prom`, `pre_seam_anchor_screened_count`,
  `pre_seam_anchor_confirm_r`, `pre_seam_anchor_confirm_prom`,
  `pre_seam_anchor_reason` — identical set, pre-seam side (searches
  backward, per § 2).

Schema doc comments must say plainly that no gate reads these fields yet.

**Decode cost, checked 2026-09-05, re-verified 2026-09-05 — resolved, not a
blocker.** Both tracks are already decoded whole per pair before any
seam-local probe runs: `A` as `a_samples` (`probes.rs:271`, destructured
from `SpliceDualfitInput.a_samples`), which is `a_pcm.samples` by reference
all the way back to `CharacterizeAbPcm.a_pcm` (`characterize.rs:76,107,215`)
— not a copy, not a narrowed subset — mirroring `dual_fit.rs`'s equivalent
full-decode field (`DualFitRepairInput.a_samples`, "A's full decoded PCM");
see the comment at `probes.rs:310`, "A is decoded whole here." `B` is
`b_samples_full` (`characterize.rs:108`). What's narrow today is only the
**B-side slice** handed down to the seam probes — `characterize.rs:152-159`
computes `pad_lead`/`pad_tail` from `gap_signature_context_secs +
fill_border_search_secs + fill_align_margin_secs` (defaults 3.0 + 10.0 +
1.0 s), bounding `lo`/`hi` into `b_samples_full` (`characterize.rs:184-189`)
as a zero-copy subslice reference, not a copy or re-decode. A is never
sliced by `pad_lead`/`pad_tail` at all — the full array is passed
everywhere unconditionally, so it needs no widening. So there is no new
decode and no new RSS cost (the ~15 GB corpus-fingerprint-run-protocol
budget, dominated by whole-track decodes already resident, is unaffected by
a slice-bound change).

**But the widening target is not `ANCHOR_SEARCH_RADIUS_SECS` (5 s) —
checked 2026-09-05, this was wrong in an earlier draft of this section.**
The B-side content a candidate actually needs reaches
`ANCHOR_SEARCH_RADIUS_SECS + ANCHOR_WIDE_WINDOW_SECS + ANCHOR_WIDE_LAG_MS`
= 5 + 10 + 0.4 = **15.4 s** from the seam (§ 2 step 1 already derives this
far-edge distance for the drift check; the pad must reach the same
far edge, not just the near-edge radius). Computed from the defaults above:
`pad_lead` (governs the *pre*-seam, backward direction) defaults to
3.0 + 10.0 + 1.0 = **14.0 s** — short of the 15.4 s needed, so **pre-seam
anchors clip** under today's default and must have `pad_lead` explicitly
widened, not just left alone. `pad_tail` (post-seam, forward direction)
additionally includes `fill_extract_tail_slack_secs.max(fill_align_margin_secs)`
(default 5.0 s) and so defaults to 3.0 + 5.0 + 10.0 + 1.0 = **19.0 s** —
already past 15.4 s, so post-seam anchors need no widening by default.
Implementation must widen both to at least
`radius + window + lag_reach_secs`, computed from the actual constants in
use, not hardcode 15.4 s or assume the existing defaults already suffice on
either side.

Note also: `characterize.rs` computes this same `pad_lead`/`pad_tail`
formula **twice** — once inside `characterize_gaps` (~line 620, feeding the
summary-fingerprint sub-step) and again at lines 152-159 (inside
`characterize_gaps_from_decode`, which calls `characterize_gaps` internally
and then layers the full bracket/lag detail the anchor walk needs). Both
are part of the same `--gap-fingerprints` diagnostic path, not a
production-vs-diagnostic split, and both currently compute identical
values — but they are two separate literal copies of the same formula, not
one shared function, so a widening fix applied to only one site will not
propagate to the other. Confirm whether the summary sub-step actually needs
the wider pad (it may not, if the anchor walk only runs in the full-detail
step); either way, name both sites in the implementation, don't rely on
"characterize.rs's pad computation" as if there were only one.

The only new allocations from widening are the derived owned buffers built
from the widened slice per gap-iteration (`b_mono`/`b_ch`,
`overlay.rs:274-275`, and any new anchor-walk scratch) — transient, freed
each iteration, sized by the widened slice length, not by corpus size.

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
4. **Confirmed 2026-09-05: production has already exhausted the exact lag
   band stage 3's confirm will re-check at 50/g10's post seam, using the
   same underlying primitive — a "success" there would be surprising, not
   merely hoped-for, and needs a specific explanation if it happens.**
   `grow_post_lock` (`domain/seam_locks.rs:442`) is called from the real
   repair path (`patch_audio/region/dual_fit.rs:283`) with
   `max_lag_frames = 0.6 * sample_rate` (`dual_fit.rs:109`) — a **±600 ms**
   lag search — and internally calls `seam_local_peak`
   (`domain/seam_local.rs:297`, imported at `seam_locks.rs:20`) — **the
   same function** this plan's stage 3 confirm uses (§ 2's tree, "via
   `lag_correlation_curve_auto`") — at five window widths
   (`POST_GROW_LADDER_MS`: 375/500/625/750/1000 ms). Per
   `archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md` Result 4, g10's
   post seam ran this full ladder and **no rung cleared `POST_GROW_ADMIT_R`
   (0.5)** — the doc's own conclusion: *"A locally-ambiguous seam does not
   sharpen with a wider window — it just samples more of the same
   ambiguous content."*

   This means: a real peak-lag search, using the identical primitive
   stage 3 will use, has already covered every lag within ±600 ms of
   nominal at g10's post seam (at several window widths) and found nothing
   above 0.5 (the best known number being § 0's `post_seam_r = 0.248`).
   § 0's own drift argument is that the anchor's carried-back lag should
   sit within roughly a millisecond of nominal for a well-behaved case —
   i.e., **comfortably inside the ±600 ms band grow-post already
   exhausted.** So for 50/g10 specifically, under the plan's own stated
   assumptions, stage 3's confirm is very likely to re-probe already-known
   territory and reconfirm the existing non-lock, not discover something
   new. Two things follow, and § 5/§ 6 should say both explicitly rather
   than treat "does confirm_r lock" as a clean success signal on its own:
   - **A failed lock at g10 is not informative by itself** — it was close
     to the predicted outcome going in, given grow-post's prior result, and
     doesn't distinguish "the anchor mechanism doesn't work" from "the
     anchor mechanism works exactly as designed and correctly landed near
     nominal, where grow-post already showed there is nothing to find."
   - **A locked confirm at g10 would be the surprising result and needs a
     specific explanation before being taken as validation** — most likely
     either (a) the carried-back lag actually landed *outside* ±600 ms of
     nominal, meaning the mechanism accidentally found a step-boundary-
     scale correction rather than a drift correction (contradicts § 0's
     safety premise, and should be checked against risk 1's guard rather
     than celebrated), or (b) stage 3's specific window/instrument
     configuration differs from grow-post's in some way that matters and
     hasn't been identified. Either way, § 5 must report the accepted
     candidate's `distance_ms`/lag delta from nominal alongside the
     confirm result for g10 specifically, not just the confirm pass/fail.

5. **The walk can cross into a *different* gap's own dropout, and nothing
   currently stops it (added 2026-09-05).** § 2 step 1's far-edge distance
   is not a few seconds — it is `ANCHOR_SEARCH_RADIUS_SECS +
   ANCHOR_WIDE_WINDOW_SECS + ANCHOR_WIDE_LAG_MS` (5 + 10 + 0.4 = 15.4 s by
   default, see § 3's decode-cost paragraph). Pair 50 is not sparse at that
   scale: g10 itself sits in a documented cluster (g8–g10, g14–g16, risk 1
   above) — gaps close enough together that a 15.4 s walk from one seam has
   a real chance of reading into, or past, a neighboring gap's own hole
   rather than clean A content.

   This was not checked against actual gap-to-gap spacing before this
   revision. If a candidate window overlaps another gap's `[video_a_start_secs,
   video_a_end_secs)` span, stage 1's flatness screen does not protect
   against it — a neighboring hole is not necessarily *flat* (it can carry
   donor fill, residual noise, or an unpatched dropout with its own
   spurious energy), so it can pass stage 1 and stage 2 as if it were real,
   locatable A content, producing a confidently "found" anchor lag that is
   actually measuring correlation against — or across — a different
   defect entirely. This is a distinct failure mode from risk 1 (a map
   step): it doesn't require any A↔B misregistration, just two gaps close
   enough together in the same track.

   **The data needed for a guard already exists, unthreaded.** Gap
   detection is a single forward pass over `a_runs`
   (`application/scan_gaps.rs:296-314`) that appends to `gaps: Vec<Gap>` in
   increasing-`pos` order — so `report.gaps` (the full `GapReport`, already
   in scope throughout the per-gap loop in
   `application/gap_fingerprint/measure/characterize.rs:162-516`,
   `let gap = &report.gaps[i];` at line 165) is timeline-ordered by
   construction, and `report.gaps[i-1]` / `report.gaps[i+1]` are genuinely
   the previous/next gap in A. `domain::gap::Gap`
   (`domain/gap.rs:23-48`) carries exactly the fields a bound needs —
   `video_a_start_secs`/`video_a_end_secs`, plain `f64` seconds directly
   comparable to a walk position. But none of this reaches the walk as
   currently placed: `probes.rs`'s per-seam input structs
   (`SpliceDualfitInput`, `SeamProbeAtPlacementInput`,
   `WideEnvelopeAtPlacementInput`) take only this gap's own `refined`
   frame span, never `report` or a neighbor reference, and there is no
   existing "distance to nearest gap" concept anywhere in `domain/` or
   `application/gap_fingerprint/` to borrow (checked 2026-09-05 — every
   "neighbor"/"adjacent" hit in this codebase means the adjacent *lag
   sample* in a correlation curve, not an adjacent *gap*).

   **Required guard:** before starting the walk for a given seam,
   `characterize.rs` must resolve the neighboring gap in the walk's
   direction (`report.gaps[i-1]` for a pre-seam walk, `report.gaps[i+1]`
   for a post-seam walk) and pass its near boundary down as an explicit
   `max_walk_secs`-style bound into the new `anchor_relocated_lag(...)`
   input, clamping the walk (and rejecting any candidate whose window
   would cross that boundary) short of `ANCHOR_SEARCH_RADIUS_SECS` when the
   neighbor is closer. § 3's dump fields should record when this bound —
   rather than the flatness screen or the fixed radius — is the reason a
   walk stopped early, so § 5 doesn't misread "found nothing" as a stage-1
   failure when it was actually neighbor-bounded. § 5's corpus run must
   also report actual gap-to-gap spacing for g10's cluster (and any other
   corpus member with a similarly tight cluster) so it's known whether
   this guard actually engages for the plan's own motivating case, or only
   matters for other pairs.

## 4a. Funnel unit test (required before the corpus run — added 2026-09-05)

Nothing in this plan currently proves the three-stage funnel's own logic is
correct independent of what it happens to report on real media. A corpus
run alone cannot distinguish "the mechanism is broken" from "there is
genuinely no rescuable structure nearby" — both produce the same observed
outcome (no qualifying anchor, or an unlocked confirm). Before spending the
corpus run on real conclusions, add a fixture-based test (synthetic PCM,
`clip-sync-repair-fixtures` or inline) that exercises each stage against a
known ground truth:

- A flat/silent synthetic region must be rejected by stage 1
  (`energy_envelope_is_flat` returns true) without ever calling stage 2 —
  assert stage 2's correlation function is not invoked (or its call count
  is zero) on that candidate.
- A synthetic region with real structure at a known, injected lag must
  clear stage 2 and produce a `post_seam_anchor_lag_ms`/`_r` matching that
  injected lag within a small tolerance.
- Stage 3's confirm must be shown to actually re-center on the stage-2 lag
  — construct a case where the seam's own nominal lag and the stage-2 lag
  differ by more than the ±30 ms confirm window, so a confirm "centered on
  nominal" and a confirm "centered on the stage-2 lag" would disagree, and
  assert the result matches the stage-2-centered expectation.
- A synthetic step discontinuity planted between the seam and a
  stage-1/2-qualifying candidate must produce exactly `step_boundary_rejected`
  (§ 2 step 5), not a locked-but-wrong-regime confirm.

This is the only way to know the funnel's stages are individually correct
before trusting what they report on 50/g10 or the wider corpus — the
corpus run in § 5 tests whether the *idea* helps, this test proves the
*mechanism* does what § 2 says it does.

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
  the un-relocated fine window at the nominal seam did not? **See § 4
  risk 4 before treating this as a clean pass/fail signal for g10
  specifically** — production has already run a real peak search over
  ±600 ms at this exact seam with the same underlying primitive and found
  nothing above r = 0.5, so an unlocked confirm here is expected, not
  diagnostic, unless the carried-back lag can be shown to fall outside
  that already-searched band.
- **Anchor distance distribution — caveat added 2026-09-05, this metric is
  weaker than it looks.** How far from the seam did a qualifying anchor
  have to be found, corpus-wide? Read this distribution with care: the walk
  starts its first candidate at near-edge distance **0** (right at the
  seam), and that first candidate's window already reads out to
  `ANCHOR_WIDE_WINDOW_SECS` (10 s, § 2 step 1). If that immediate 10 s
  window contains *any* non-flat content — plausible on most real gaps,
  since "flat for a full 10 s" is a stronger condition than "ambiguous at
  the seam's own ±30 ms window" (§ 0's g10 content is quiet dialogue-over-
  noise, not silence, so it likely already passes stage 1's flatness test
  at distance 0) — then `post_seam_anchor_distance_ms` and
  `post_seam_anchor_screened_count` will both read **0** on the very first
  try, every time. A distribution clustered near 0 is therefore not
  automatically a success signal for "relocation found something a wider
  window at the seam couldn't" — it may just mean the walk never had to
  relocate at all, which makes this instrument functionally
  indistinguishable from **grow-post's own already-tried lever** (widening
  the window while staying centered near the seam, § 0's "why this is
  different" argument) rather than a genuine relocation to more distant,
  richer content. § 5 must report the distance distribution **conditioned
  on `screened_count`** (candidates actually rejected before one qualified)
  rather than reading `distance_ms` alone as the success metric, and should
  separately flag cases where the accepted candidate is the walk's very
  first try — those are not evidence the relocation idea (as opposed to
  "a wider window near the seam") is doing anything new. A distribution
  concentrated at a genuinely nonzero few seconds, with `screened_count`
  &gt; 0, supports the plan; one dominated by zero-distance, zero-screened
  hits does not, even though it would look like "success" on `distance_ms`
  alone.
- **Cases already confident under the seam-local Pearson window**: does the
  anchor-confirmed lag roughly agree with the seam's own lock, or does
  relocation introduce disagreement on gaps that already patch clean? (An
  anchor mechanism that disagrees broadly is not trustworthy just because it
  helps g10.)

## 6. Decision point

- **Anchor relocation locates a real, confirmable peak at 50/g10's post seam
  *with an identified reason consistent with § 4 risk 4* (i.e., the
  carried-back lag is shown to fall outside the ±600 ms band grow-post
  already exhausted, or some other concrete explanation for why this
  succeeds where grow-post's identical-primitive search did not), and does
  not destabilize already-confident cases** → open a real Change to
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

§ 4a's funnel unit test passes first — it is a prerequisite for trusting
any corpus-run result, not an optional nicety. After that, either a working
gate Change is scoped and BACKLOG'd, or this plan is archived with a
refuted verdict in `settled.md` § A and BACKLOG's 50/g10 row is rewritten
around the manual-review-flag alternative.
