# Fill placement axis — measure the end search before changing it

> **Status (2026-07-26): ARCHIVED.** Phase A **COMPLETE** (placement observable armed). Phase B
> slack-use exit **MET** on the 17-pair fingerprint corpus (denominator = bracket span, not original
> gap) → **Phase C NO-GO.** Optional structure-vs-production second placement was never required for
> that exit and is deferred outside this plan (only if a weight A/B is planned later). Perf residue —
> narrowing `fill_length_slack_secs` — lives in [repair-perf.md §5 #3](../repair-perf.md) and
> [BACKLOG.md](../../../BACKLOG.md), not here.
>
> The placement is emitted, carried to `GapRow`, recorded in `GoldenRecord`, and the
> golden is armed end-to-end: fixtures **11** (`bracket_patch_donor_broken`) and **12**
> (`bracket_patch_clean`), harvested from `gap-files/fill-placement-arm`, carry non-null
> `fill_start_frame` / `fill_frames`. Legacy fixtures 01–10 remain null (provenance media
> gone) and untouched. Lever 1b(c) (end-search repeat hoist) was byte-identical and
> already landed independently of this plan.

**Problem:** the unified fill search picks `end` (and therefore fill length) over a ±5 s slack using
**structure evidence only** — every waveform term in `unified_search_best_fill_end` is loop-constant
(see [repair-perf.md §1a](../repair-perf.md)). Whether that is right is an open question. We cannot
answer it, because **no harness records the chosen fill placement.** A change that moved every fill
length on the corpus would leave the golden diff green.

**Goal:** make fill placement an observable, then use it to A/B the end-search scoring on media.

**Non-goals (of this plan):** changing the end search's objective in Phase A or B — those are pure
measurement. Re-tuning `fill_length_slack_secs` (now a separate perf candidate — see
[repair-perf.md §5 #3](../repair-perf.md)). Touching the seam gate or the dual-fit path.

---

## Why the harness was blind (historical — closed in code, open in fixtures)

| Layer | What was wrong | Status now |
|-------|----------------|------------|
| Fingerprint `place_on_b` | `waveform_weight: 0.0` — structure-only; end-search waveform terms invisible by construction | Unchanged on purpose (protects `classify_bracket_stage`); not the dump's placement source |
| Fingerprint dump | `fill_frames` never left the search / `BracketInfo` | **Fixed:** `compute_region_measurements` projects production-weights `FillAlignment` onto each passing bracket |
| Golden baseline | No `fill_*` fields | **Fixed:** Tier-1/2 present and **armed** on fixtures 11/12; Tier-1 null on legacy 01–10 (dead provenance) |
| Projection differential | Would demand tags carry placement | **Scoped:** `GoldenBaseline::without_placement()` |
| Unit tripwire | Nothing caught a 1-frame fill move | **Fixed:** `diff_catches_tier1_flip` |

**The observable already exists in the type system.** `GapRepairStrategy::Bracket { alignment, .. }`
carries `FillAlignment { start_frame, fill_frames, pre_correlation, post_correlation }` — exactly the
four numbers an end-search change moves. Phase A is a projection, not new machinery.

**But placement is an output, not a decision — do not confuse the strategy with the tags.**
`GapRepairStrategy` is the *executor's input* and carries the alignment. `GapRepairTags`
(`{ registration, seam_local, donor_nominal, donor_aligned, gate, levels }`) is the *decision*
carrier, and deliberately does not: no gate, tier, or verdict reads a frame index. So the media-free
`gap_repair_spec_diff` differential — whose contract is *tags are a complete decision carrier* —
must **not** assert on `fill_*`, and widening `GapRepairTags` to make it able to would be inverting
the test's purpose. The placement tripwire belongs on **committed fixtures that were measured with
the current dump** (legacy 01–10 cannot provide that; fixtures 11/12 do — *How it was armed*).
`GoldenBaseline::without_placement()` scopes the projection path accordingly.

---

## Should the fingerprint get a production-weights mode?

**Yes — as an additional placement, not as a flag that flips `waveform_weight` in place.**

### The corpus is already multi-placement, and seam-chosen placement is already in it

`splice_dualfit_at` (`measure.rs:985`) places **each shoulder independently at the lag maximizing its
own seam** (`seam_local_peak` over ±`SEAM_LOCAL_SEARCH_MS`), and derives
`bridge_frames = b_post_seam - b_pre_seam`, `trim_frames = bridge_frames - gap_frames` — a
*discovered* gap length, deliberately unequal to the original. It runs unconditionally for every gap
(`measure.rs:1809`). So "the best seam we could find anywhere in the search radius" is not a
contaminant the fingerprint avoids; it is an axis the fingerprint already ships, because for gaps
whose surroundings are silent there is no structurally-correct placement to measure against.

**The estimator-bias concern is handled by publishing validators, not by abstaining.** Directly below
that argmax: `pre_seam_z` / `post_seam_z` (whole-curve z of each seam's peak over the ±
`SEAM_LOCAL_SEARCH_MS` search — the **primary** alias guard: a search that locked onto a far
*periodic* rival reads low), `pre_seam_prom` / `post_seam_prom` (the ±30 ms single-rival margin —
**secondary**, and it false-flags correct-but-periodic content, e.g. 5·g6), and `post_seam_global_r`
(*"is the step necessary?"*). z is the guard against max-of-noise inflation; prominence is a
tiebreaker that must not be read alone. Any seam-chosen placement axis must ship both — see Phase B.

### What the `waveform_weight: 0.0` is actually protecting

One narrow thing: `brackets[].seam_pre` / `seam_post` feed `classify_bracket_stage`
(`measure.rs:1427`), and **those two fields carry no prominence or z companion.** Measured at a
structure-chosen placement they mean "structure found a placement; does the waveform corroborate?" —
which is what makes `waveform_floor` a meaningful failure stage. Overwrite them with an unguarded
argmax and that distinction is gone. That is an argument about two specific fields, not about
seam-chosen placement in general, and it is satisfied by adding fields rather than replacing them.

The `0.0` rationale is documented in `gap-fingerprint.md` (*Bracket placement*) and as a comment at
the `UnifiedFitWeights` literal in `measure.rs` — **Phase A**.

### The two objections that survive

1. **Golden churn.** If the *existing* placement became a function of `fill_fit_waveform_weight`,
   `fill_repeat_penalty_weight`, and the FFT flag, every retune would churn the committed goldens —
   the durable reference. Additive fields avoid this entirely.
2. **Cost.** `place_on_b` runs **per bracket** in the `Full` tier (`measure.rs:1409`) — 4.3–5.0
   s/bracket over ~5061 brackets. `wf > 0.0` activates the per-candidate repeat penalty, 72% of
   production repair time. Lever 1b(c) removes the end-side half; the start-side half remains. Hence
   opt-in, paid once per A/B.

**Conclusion:** record `placement_structure` (existing, unchanged) *and* `placement_production` (new,
opt-in, own fields, own validators), alongside the `splice_dualfit` seam-placed axes that are already
there. Three placement models, three repair models, each measured on its own terms.

---

## Phase A — project the placement (no behavior change) — **code complete 2026-07-25**

- [x] Add `fill_frames` to `PlacementScores`; carry `matched.alignment.fill_frames` through
      `place_on_b`. (`end_frame` dropped as redundant — it is `start_frame + fill_frames`.)
- [x] Emit per-bracket `start_frame` / `fill_frames` in the fingerprint JSON alongside `seam_pre` /
      `seam_post` (`BracketInfo`, `Option`, `skip_serializing_if` ⇒ old dumps still parse).
- [x] Carry them to `GapRow` as `best_bracket_{start_frame,fill_frames,seam_pre,seam_post}`, selected
      by the **same highest-min-seam rule** `best_bracket_seam` and `closest_failure_stage` already
      use (`best_seam_bracket`), so "the chosen bracket" means one thing across the analyzer.
- [x] Add to `GoldenRecord`: `fill_start_frame`, `fill_frames` (**Tier-1**, bit-exact — frame indices
      are integers, so there is no ε to hide in) and `fill_pre_r`, `fill_post_r` (**Tier-2**).
      `schema` string updated.
- [x] Document the `waveform_weight: 0.0` rationale in `gap-fingerprint.md` (new *Bracket placement*
      section) and as a comment at the `UnifiedFitWeights` literal in `measure.rs`.
- [x] Tripwire test: `diff_catches_tier1_flip` now also asserts a **one-frame** change to
      `fill_frames` or `fill_start_frame` produces exactly one diff error.
- [x] **Emit the *production-weights* placement, not the structure-only one.** There are two dump
      paths. `characterize_gap_region` (`measure.rs:1447`) places via `place_on_b`, which runs at
      `waveform_weight: 0.0` — blind to end-search scoring changes by construction (the very blindness
      this plan exists to fix), so a placement recorded there guards nothing. The from-decode path
      `compute_region_measurements` scores each bracket with `oracle_score_fit_candidate`, i.e. the
      **production gate**, and its `SeamGateOutcome.alignment` already carries `start_frame` /
      `fill_frames`. `oracle_score_fit_candidate` now returns `OracleFitScores` (named struct — the
      5-tuple was already at its limit) including that alignment, and `compute_region_measurements`
      writes it onto each `BracketInfo`. Cost: zero — same search, one field stops being discarded.
      `None` on gate failure, where there is no chosen placement.
- [x] Scope the projection differential with `GoldenBaseline::without_placement()` — placement is a
      search output, not a tag-carried decision (see above).
- [x] **Re-baseline the golden** (`CURATED_GOLDEN_REGEN=1`). Purely additive: 40 new keys plus the
      `schema` line, **zero pre-existing values changed** — property 1 below holds exactly, because the
      fixtures were not touched. Populated: `fill_pre_r` / `fill_post_r` on the 6 gaps with a bracket
      carrying a complete seam pair (they read the pre-existing `seam_pre` / `seam_post`, so this is a
      re-read of committed data through a new selector, not a new measurement). Null: both Tier-1
      placement axes, everywhere.
- [x] Assert the null column rather than leaving it to be discovered — a null Tier-1 column is
      indistinguishable from a passing one. (Interim; superseded by the positive assert below.)
- [x] **Arm the Tier-1 tripwire with fill-bearing fixtures from media we still have.** Done — see
      *How it was armed*. Did **not** require regenerating the original 10 cell fixtures.

**Exit — MET.** A golden diff turns red when a fill length changes, proven both at unit level
(`diff_catches_tier1_flip`) and end-to-end: fixtures 11/12 carry non-null `fill_start_frame` /
`fill_frames` in `curated.golden.json` as Tier-1 bit-exact axes.

### How it was armed (additive; media we still have)

**The original 10 fixtures' provenance was never re-run.** Those gaps came from
`re-anchor-dual-fit-on-nominal` and `equiv-coarse-vs-fine` (`curated/manifest.json`). Both corpora are
ephemeral and **gone** from `gap-files/`; they are **not** the same media as
`gap-files/anchor-bracket-corpus` (17 pairs). Re-extracting 01–10 from dead sources is impossible.
Fixture `10_decorrelated` stays synthetic (`derived-from` 03) — no media run.

01–10 remain the classification / footgun set, bit-identical, Tier-1 `fill_*` null. Two fixtures were
**added** from a dedicated `--gap-fingerprints` run (`gap-files/fill-placement-arm`), chosen because
their **best-by-min-seam** bracket passes the gate:

| Fixture | Cell | Brackets | `fill_start_frame` / `fill_frames` | Why this one |
|---|---|---|---|---|
| `11_bracket_patch_donor_broken_placement` | `bracket_patch_donor_broken` (dNom 0.958) | 20 = 15 pass + 5 `waveform_floor` | 599115 / 147285 | **Mixed pass/fail in one gap** — the 5 failures carry seams with null placement, pinning the `Ok`-branch-only rule |
| `12_bracket_patch_clean_placement` | `bracket_patch_clean` (dNom 0.0) | 22, all pass | 675558 / 53169 | All-pass counterpart; covers the clean cell |

These duplicate the cells of 01/02 **on purpose** — `representable_cells_have_a_fixture_and_others_do_not`
asserts cell *membership*, not uniqueness, so duplicates are legal. The manifest `note` records why.

Golden re-baseline was purely additive: **79 insertions, 1 deletion**, the deletion being
`"gap_count": 10`. Property 1 held exactly.

**Two findings worth keeping.** (a) `--fingerprint-gap` is **0-based** over the printed gap table
(which is 1-based) — an out-of-range index is silently ignored, so a run can quietly produce fewer
dumps than requested. (b) Gaps the equivalence scan drops (`shared-silence → drop`) **are** still
characterized, so they remain available as fixture sources.

> ⚠️ **Finding (a) is superseded — 2026-07-28.** Both halves of it were fixed by the §0 prep PR of
> [TEMP-gap-index-convention-plan.md](TEMP-gap-index-convention-plan.md) § 3 C: `--fingerprint-gap` is now
> **1-based** (matching the table `#`), `0` is rejected rather than underflowing, and an out-of-range
> number is a hard error naming the detected gap count instead of being silently ignored. Corpus
> filenames and `GapFingerprint::index` stay 0-based, so `--fingerprint-gap 3` still writes
> `…_g002_….json`. Finding (b) still holds. This document is an archived record; the note is here
> only because (a) reads as current-behavior guidance.

**Why this still matters if Phase B may skip Phase C.** Arming the golden is the Phase A exit — a
regression guard for *any* end-search change — not optional insurance that Phase B can void. Phase B
decides whether Phase C is worth doing; it does not decide whether the tripwire should exist.
Unused slack today does not mean an unarmed guard: a future scoring change that moves fill length
still turns the golden red.

### Properties (golden / fixture commits)

1. **Legacy fixtures untouched ⇒ pre-existing axes bit-identical.** Held for the additive golden
   re-baseline (01–10). When *adding* fixtures, review only the new rows + golden entries — do not
   require the whole golden to be a no-op.
2. **Tier-1 placement is non-null only where the best bracket passed the gate.**
   `compute_region_measurements` records placement only on the `Ok` branch; `seam_pre` / `seam_post`
   can exist on failures (`stage_of`) with no `fill_*`. Assert the tripwire on the placement cohort
   (and/or patch-tier gaps), not on every gap with seams.

   **The predicate is the *best* bracket, not *any* bracket.** `best_seam_bracket` ranks by min-seam
   over every bracket with a complete seam pair — **failures included** — so a gap whose min-seam
   winner failed the gate yields null `fill_*` even though other brackets passed. Both new fixtures
   were verified against this before selection. A future fixture must be checked the same way, or the
   positive assert will fail for a reason that has nothing to do with a regression.

## Phase B — production-weights second placement (opt-in) + corpus roll-up

### Denominator (do not get this wrong)

The end search's nominal is the **bracket's refined span** — `span_secs` / `params.gap_frames` =
post−pre after anchor widening — **not** the original silent-run gap
(`geometry.duration_secs` / `splice_dualfit.gap_frames`). Slack use is
`|fill_frames − span| / fill_length_slack_secs`.

**Code anchor:** `gate_structure_align` sets `gap_frames = refined.end_frame − refined.start_frame`
from the **candidate bracket** (`patch_region.rs:1398`), and `end_min` / `end_max` center the sweep on
it (`gap_fill_fit.rs:966-970`). Reproducible from any dump without reading code:
`span_secs × rate == splice_dualfit.gap_frames + move_frames` exactly, and within a multi-bracket gap
`corr(move_frames, fill_frames) == 1.00`. Also stated in
[gap-fingerprint.md](../gap-fingerprint.md) *Bracket placement*, which is the live home for this rule.

**Denominator trap:** `|fill − original gap|` is almost entirely **anchor widening**
(`|span − gap|`). On the 17-pair corpus those two medians both read ~2.0 s and look like a saturated
±5 s slack; against span the median excursion is tens of milliseconds. Inside a multi-bracket gap,
`fill_frames` tracks the per-bracket span (widening), not an independent length hunt over the original
hole. This distinction decides the Phase C go/no-go and belongs in every roll-up.

### Corpus roll-up (17/17 pairs, 2026-07-26) — slack exit **MET → Phase C NO-GO**

Corpus: 297 gaps (121 patch / 176 skip), 5064 brackets, **1044** with recorded placement. Dumps are
from-decode / production-weights (`compute_region_measurements`); no `structure_pre`/`structure_post`
fields.

Property 2 has two halves and they landed differently:

- **The `Ok`-branch rule held exactly** — 1044 placed = 5064 − 4020 gate failures, no bracket carrying
  one axis without the other.
- **The *best-bracket* predicate failed 3 times** — 3 of 121 patch gaps (~2.5%) have a best-by-min-seam
  bracket that failed the gate while other brackets in the same gap placed, so the gap reads null
  `fill_*`. The "future fixture" risk below is therefore **measured, not hypothetical**: ~1 in 40.

| Axis | What it measures | Result |
|------|------------------|--------|
| \|fill − **span**\| | end-search excursion (Phase B exit) | signed median **0 ms** (abs median 24 ms), p95 **77 ms** (all 1044) / **105 ms** (best-by-min-seam), **max 388 ms**; nothing ≥1 s (~8% of ±5 s) |
| \|fill − **gap**\| | widening + excursion (trap) | median **~2.0 s** — do not use for the exit |
| \|span − gap\| | anchor widening alone | median **~2.1 s** — explains the trap |
| throat (`span == gap`, n=65): end excursion vs `splice_dualfit` trim | length-correction agreement | same order of magnitude (medians ~3 ms / ~6 ms) but **r ≈ 0.06**, sign agreement at chance — **not redundant** |

So: the end sweep is not inert (~72% of placed brackets move >10 ms off span), but the slack is
**~13× wider than the observed max excursion** (388 ms) and ~65× its p95. Per the exit wording below,
Phase C stops here.

> **Correction (2026-07-26), left inline rather than rewriting the table above:** the two p95 cells
> are wrong. Recomputed over the same 1044 placed brackets in `gap-files/fingerprint-corpus`, p95 is
> **91 ms** (all) / **157 ms** (best-by-min-seam); 77 ms is the p93. The slack ratio is therefore ~55×
> p95, not ~65×. Median (24 ms) and max (388 ms) stand, and the Phase C NO-GO — which rests on the
> max, not the p95 — is unaffected. Current figures live in
> [repair-perf.md §5 #3](../repair-perf.md).

**Residue (not Phase C):** (1) the r≈0.06 disagreement — at least one of end-search length vs dual-fit
trim is reading noise (open research question; no plan); (2) `fill_length_slack_secs = 5.0` vs
388 ms observed max — **tracked in [repair-perf.md §5 #3](../repair-perf.md)** and
[BACKLOG.md](../../../BACKLOG.md) (narrow default → ~1.0 s). Not a Phase C item, and **not**
byte-identical — but not for the reason first recorded here: `search_coarse_step` does *not* move with
the default (it saturates at `bin_frames` for any realistic slack), the **B extract window** does. See
[repair-perf.md §5 #3](../repair-perf.md) for the corrected mechanism and exit checks.

### Optional second placement — **deferred outside this plan**

The Phase A dump already carries **production-weights** placement on brackets. A structure-only /
validated second placement remains useful only if someone retunes weights later; it was **not**
needed to decide Phase C. Checklist cancelled here — reopen only with an explicit weight A/B.

- [ ] ~~`place_on_b` optionally computes a second match with production `UnifiedFitWeights`…~~ —
      deferred outside plan.
- [ ] ~~Ship `seam_z` / `seam_prom` validators with it~~ — deferred with the above.
- [ ] ~~Gate behind a fingerprint flag (default off)~~ — deferred with the above.
- [x] Roll-up: `|fill − span|` (slack use), `|span − gap|` (widening), and end-excursion vs
      `splice_dualfit.trim` on throat brackets — **done on the 17-pair corpus above.**

**Exit (slack):** we can state, on real media, how much of the ±5 s slack is actually being used
**against the bracket-span nominal.** **This is the number that decides whether Phase C is worth
doing at all.** If the end sweep rarely leaves nominal, the whole question is moot and the plan stops
here. **Met 2026-07-26 — rarely leaves nominal → Phase C closed.**

## Phase C — end-dependent seam — **NO-GO (2026-07-26)**

Gated on Phase B's slack exit; that exit failed the "materially used" test (max excursion 388 ms on a
±5 s allowance). Do **not** implement an end-dependent post seam / repeat-post on the strength of this
corpus. Items below are retained only as the design that would have applied if the exit had passed.

- [ ] ~~Make the post seam a function of the candidate's `end` in `unified_search_best_fill_end`~~ —
      cancelled.
- [ ] ~~Affordability / banding / repeat-post / A/B / listening~~ — cancelled with the above.

**What the corpus still says about consolidation.** Even with Phase C closed, the throat comparison
answers the earlier redundancy question: production fill excursion and `splice_dualfit` trim are
**uncorrelated** (r≈0.06). Consolidation of the end search into dual-fit is **not** justified by
"they already agree," and is a separate decision from Phase C. The live residue is which of those two
small length corrections (if either) is signal.

---

## Risks

| Risk | Mitigation |
|------|------------|
| Golden re-baseline churns unrelated axes | Held for the additive 01–10 re-baseline: zero pre-existing values changed |
| Adding a placement cohort churns the golden | **Closed:** 79 insertions, 1 deletion (`gap_count`); legacy fixtures bit-identical |
| Tier-1 placement ships as a null column that reads as coverage | **Closed:** `curated_golden_fill_placement_is_armed` fails if *no* gap carries placement, so the column cannot regress to all-null |
| Placement axes drift apart (one set, one null) | `curated_golden_fill_placement_is_armed` also asserts they are populated together — both project from one `FillAlignment` |
| A future fixture is added whose min-seam winner failed the gate | **Measured at ~2.5% of patch gaps (3/121) — expect it ~1 in 40.** Its `fill_*` would be null and the positive assert would read as a regression; check `best_seam_bracket` before selecting (property 2). `curated_golden_fill_placement_is_armed`'s failure message now says so |
| Trying to re-extract 01–10 from dead provenance | Don't — expand from media still on disk (`anchor-bracket-corpus` / dedicated dumps) |
| Phase B doubles fingerprint runtime | Default off; run once per A/B, not per sweep |
| Phase C ships on a green harness with no listening | Listening is an explicit Phase C exit item, not optional — **moot:** Phase C NO-GO |
| Phase C double-counts with the splice trim | Called out above; resolve before implementing — **moot:** Phase C NO-GO |
| Roll-up uses \|fill − original gap\| as slack use | **Closed in doc:** nominal is bracket span; trap called out in Phase B |

## Related reading

- [repair-perf.md §1a](../repair-perf.md) — the measurement that raised this, and the git-history finding
  that the end search's waveform terms were loop-constant from `e849e64` onward; slack-narrowing
  candidate is §5 #3
- [fill-fitting-plan.md](fill-fitting-plan.md) — origin design, Phase B objective and
  Phase D repeat-window spec
- [TEMP-production-repair-perf-plan.md](TEMP-production-repair-perf-plan.md) — levers 1
  / 1b, and the corpus placement-diff that was specified but never built
- [gap-fingerprint.md](../gap-fingerprint.md), [corpus-validation.md](../corpus-validation.md)
