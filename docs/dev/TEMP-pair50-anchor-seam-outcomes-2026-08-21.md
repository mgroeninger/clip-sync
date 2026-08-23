# TEMP — Pair 50's map-step region: anchor-seam outcomes (2026-08-21)

**Status:** findings note, not a Change, 2026-08-21. Split out of
[TEMP-collapsed-bridge-census-2026-08-20.md](TEMP-collapsed-bridge-census-2026-08-20.md) (Results 6–7 and
its Questions 3 / 7-partial), which had accumulated a large body of pair-50 evidence that answers
nothing about the `collapsed_bridge` predicate (`|gap + post_lag − pre_lag| ≈ 0`) — that census's
actual charter. **This note proposes no change.**

**The question this note owns:** pair 50 carries a persistent ~**+2.8 s** map step. Production's
`lag_decision` cannot reach it (600 ms nominal / 2000 ms envelope radii), yet six `patch`-tier gaps in
the region are nonetheless placed at the correct offset by a *different* mechanism — the editorial
anchor-seam bracket search. So the offset itself is settled. What is not settled is what that
mechanism does at the **edges** of a wide map step:

- one gap patches at the correct offset and still has an audible defect (50/g10's echo);
- three gaps sit *between* two correctly-resolved flanks, at the same offset, and skip entirely
  (50/g11–g13).

Those are two faces of one question, which is why they live together here.

Per [gap-vocabulary-redesign / licensed-media-names-never-recorded], gaps are referred to by corpus
pair index + gap index only. Source dumps are gitignored (`gap-files/`) and may be deleted; the
numbers below are the durable record — see **Reproduction**.

**Related:**
[BACKLOG.md](../../BACKLOG.md) § *Fill placement* owns the *sizing* dump (flat
`--lag-envelope-max-lag-ms`; pair 50's +2.789 s interior is the named target). The withdrawn
adaptive-ladder draft is
[archive/TEMP-envelope-adaptive-widen-plan.md](archive/TEMP-envelope-adaptive-widen-plan.md).
Result 2 below slightly lowers that dump's stakes — the cluster is corrected downstream
regardless — but does not answer it, and the g11–g13 failure is downstream of anything a wider
envelope would change. Do not merge the two.

---

## Data sources

- **Full-corpus post-fix re-scan**, `gap-files/2026-08-19-decision-map-oriented/` — 53 pairs, current
  binary (map-coordinates search, ±600 ms window). Source of the `lag_decision` fields quoted below.
- **`--gap-listen` export for the 9 `patch`-tier both-dead gaps** (2026-08-20 run), including pair 50's
  g8, g9, g10, g14, g15, g16 — `_a_surround` / `_b_surround` / `_a_patched` WAVs at
  `gap_signature_context_secs` = 3.0 s per side.
- **Production patch baseline**, `gap-files/2026-08-13-repeat-terms/` — 53 pairs,
  `measure-patch-outcomes` at production defaults (`gap-files/README.md`: "the current patch-report
  baseline"). Source for Result 2. Records what production *actually did* with each gap, which the
  `lag_decision` dumps do not.

Corrections marked below come from
[TEMP-collapsed-bridge-census-result5-review-2026-08-21.md](TEMP-collapsed-bridge-census-result5-review-2026-08-21.md),
a logic review of the census's Results 5–6 against the original ear notes and re-runs of the sweep and
sliver instruments. Overturned claims are struck in place rather than deleted, with the reason.

---

## Result 1 — the cluster shares one ~2.8 s map step, and the content is demonstrably there

Built `gap-files/scratch_patch_registration_sweep.py` (ad hoc, not committed): a sliding 250 ms-window
FFT cross-correlation of `_a_patched.wav` (continuous — real shoulders + production's actual splice,
no hole) against `_b_surround.wav` (continuous donor B, same nominal span), searching ±3000 ms per
window. Ran it against all 6 pair-50 `patch`-tier gaps (g8, g9, g10, g14, g15, g16).

All 6 show the identical shape: the correlation locks onto a consistent **+2.8 s extra lag** (r up to
1.000) — beyond production's own `frac_lag_ms`/nominal mapping — for the pre-shoulder and the early
part of the patch, then collapses to noise (r < 0.5, no consistent lag) partway through. This matches
the pair-50 map-step offset already documented in
[archive/TEMP-envelope-adaptive-widen-plan.md](archive/TEMP-envelope-adaptive-widen-plan.md) (+2.789 s, "outside
2000 ms" — i.e. outside the default envelope rescue's own search reach).

**Why the sweep's post side reads as "no match": the extraction window runs out of matching content
before the correlation does.** Every WAV clip is extracted with a fixed `gap_signature_context_secs`
margin (default 3.0 s) on each side of the gap. Given true extra offset `x` beyond nominal, the true B
content for `a_patched` local time `t` needs B-time `t + x` past the clip's nominal start — which
exceeds the clip's right edge once `t > duration_secs + 2×margin − x`. Below that break point,
correlation should hold; above it, there is no corresponding B content *in the extracted file* to find,
defect or not. That formula predicted every observed breakdown to within one 250 ms hop:

| gap | duration_secs | extra offset (2nd plateau) | predicted break (`duration+6−offset`) | observed break |
|---|---|---|---|---|
| 50/g8 | 0.551 s | 2.809 s | 3.74 s | 3.5 s |
| 50/g9 | 1.868 s | 2.843 s | 5.03 s | 5.0 s |
| 50/g10 | 2.578 s | 2.818 s | 5.76 s | 5.5 s |
| 50/g14 | 1.799 s | 2.834 s | 4.97 s | ~4.75–5.0 s |
| 50/g15 | 1.833 s | 2.836 s | 5.00 s | ~5.0 s |
| 50/g16 | 1.269 s | 2.817 s | 4.45 s | ~4.25–4.5 s |

Mapped onto the shoulders specifically: the pre-shoulder (last ~1000 ms of real A audio before the
gap, at local `t` well under 3.0 s) sits entirely inside the overlap for every gap — consistent with
every plateau starting at r ≈ 1.0 from `t = 0.000`. The post-shoulder (`lag_decision`'s probe window,
`window_ms: 1000` immediately after the gap) is a different story: overlap remaining inside that
1000 ms window = `3.0 s − extra_offset` ≈ **0.15–0.19 s out of 1000 ms (16–19%)** for all 6 gaps —
i.e. 81–84% of the post-shoulder's probe window has no corresponding B content anywhere in the
*extracted* files, regardless of registration. The hole's own end point (`t = 3.0 + duration_secs`,
where real A audio resumes) lands just inside the overlap too, by that same 0.15–0.19 s margin, for
every gap.

### The sliver check — is the shared span real content?

The arithmetic above is a *time-range* overlap check only — it says the clips share some common
absolute B-time span, not that the audio inside that span is useful (non-silent, distinctive, not
more overlapping dialogue like 50/g8). Checked directly (`scratch_sliver_check.py` extracted the A
sliver right after the hole and the B sliver at `b_surround`'s tail edge; both read real signal, not
silence, at closely matched levels, all 6 within ~0–1.7 dB of each other) and then correlated the two
slivers against each other (±200 ms local search):

**Corrected 2026-08-21 (review).** The first pass correlated the *padded* sliver WAVs —
`scratch_sliver_check.py` adds `PAD = 0.15 s` for listenability, so the files are ~300–480 ms and
include material outside the true overlap (on the A side, 150 ms of *fill* before the post and 150 ms
past the overlap's end). Re-run on the overlap-only remnant (`3.0 s − x`, the true shared span), both
columns shown:

| gap | remnant | **overlap-only r / lag** | padded-WAV r / lag (first pass) |
|---|---|---|---|
| 50/g8 | 191 ms | **1.000** / 0.1 ms | 1.000 / 0.1 ms |
| 50/g9 | 157 ms | **1.000** / 0.1 ms | 1.000 / 0.1 ms |
| 50/g10 | 182 ms | **1.000** / 13.2 ms | 1.000 / −13.2 ms |
| 50/g14 | 166 ms | **0.999** / 18.1 ms | 0.879 / 0.0 ms |
| 50/g15 | 164 ms | **0.991** / 26.0 ms | 0.650 / 26.0 ms |
| 50/g16 | 183 ms | **0.997** / 13.6 ms | 0.575 / 157.6 ms |

The first pass's own caveat — "or an artifact of correlating a ~300–480 ms clip with edge effects" —
was the right hypothesis, and it is now **confirmed**. On the true remnant, **all 6 lock at r ≥ 0.991**.
g16's 157.6 ms padded lag was pinned near the ±200 ms search edge; on the remnant it is 13.6 ms at
r = 0.997. So the earlier reading — "4 of 6 genuinely match; g15/g16 plausibly degraded overlap" — is
**withdrawn**: the whole cluster has real, matching, findable content in the true overlap. g15/g16 were
padded-window artifacts, not weaker material.

Cross-checked against 50/g10's raw `lag_decision` fields: `pre_anchor`/`post_anchor` both search
`max_lag_ms: 600` at nominal, then `envelope_rescue` widens to `±2000 ms` — both well inside the
~2.8 s true offset, so neither the ordinary probe nor the envelope rescue could ever reach the real
peak (the ad hoc sweep only found it because it searched ±3000 ms). Pre's reported `envelope_lag_ms:
-1600, r 0.57` and post's `-1800, r 0.45` are real but weaker local peaks the ±2000 ms search landed
on — not the true (+2.8 s) peak — hence "declined" on both sides.

### Two instruments, two different "not looking far enough" — do not merge them

The section above measures **listen clips**; the declined shoulders happen in **production**. These are
different B lengths, and the original conclusion conflated them:

| | B available after the post | Consequence at +2.8 s |
|---|---|---|
| Listen `_b_surround` (`--gap-listen`) | 3 s tail (`gap_signature_context_secs`) | True match dies ~0.16–0.19 s into the 1000 ms post template — **context-margin overrun** |
| Production `lag_decision` haystack | `pad_lead` ≈ 14 s, `pad_tail` ≈ 15–19 s | Peak sits **well inside** the extract, ~12 s of B still beyond it |

The A shoulders in the listen clips **are** the segments production scores (pre = last 1 s before the
hole, post = first 1 s after, both within ±3 s of A). The listen **B** clip is *not* production's B
haystack — it is a deliberately truncated centre of it
([archive/TEMP-gap-listen-wav-plan.md](archive/TEMP-gap-listen-wav-plan.md) §10 considered exporting
the fingerprint pad and rejected it, to keep the ear-comparison window from becoming the search
window). So:

- **In the listen clips**, 81–84% of the post template has no B counterpart. That is real, and it is
  why the sliding sweep dies at the post — but it is an artifact of the *export*, not of production.
- **In production**, all 1000 ms of the post template have B content at +2.8 s. What blocks
  `post_anchor` is the **search radius** (2000 ms < 2800 ms) — not the window length, and not missing
  content.

The slivers show that **if** production searched to +2.8 s the post-onset would lock. They do **not**
show that the 1000 ms window is a production defect.

**Conclusion.** Pair 50's six `patch`-tier gaps share **one map step of ~2.8 s**. Production's envelope
rescue declined on all of them for a single sufficient reason: **2800 ms exceeds the ±2000 ms search
radius.** No content-mismatch, silent-onset, or window-length theory is needed — and the content is
demonstrably there (r ≥ 0.991 across the cluster).

Two corrections to how this was first stated:

- **"Contiguous region" is wrong.** They share an offset, not adjacency: `geometry.a_start_secs` runs
  2494, 2627, 2711, 3424, 3544, 3777 — spanning ~21 minutes, with `skip`-tier gaps interleaved
  (50/g11–g13). This is a persistent **pair-level** step, not six neighbouring holes. The coordinate
  check the census dismissed as "adding nothing" would in fact have shown this.
- **Clip overrun does not explain the production declines.** It explains the *sweep's* post-side
  dropout only. The production explanation is the search radius, per the table above.

## Result 2 — the cluster *is* corrected in production, by the anchor-seam bracket search

Result 1 (and the census's Result 5–6 review) left one thing unnamed: the review observed that 50/g10's
fill sits at the true +2.8 s offset — `residual.placement_slide_frames` 135336 → 2.819 s, matching the
fill plateau — and concluded "production placed the fill at the true offset **on a different path than
`lag_decision`**", without identifying that path. This result names it, and settles what it means for
the cluster.

**Evidence.** `gap-files/2026-08-13-repeat-terms/50.json` — an existing full-corpus, production-default
`measure-patch-outcomes` baseline; no new run needed:

| gap (0-based) | status | `align_adjustment_secs` | `anchor_seam_used` |
|---|---|---|---|
| 8 | patched, high | 2.771 | true |
| 9 | patched, marginal | 2.833 | true |
| 10 | patched, marginal | 2.821 | true |
| 14 | patched, high | 2.824 | false |
| 15 | patched, high | 2.826 | true |
| 16 | patched, high | 2.807 | false |

All six patch, at the true ~2.8 s offset. **The cluster is not stuck** — the region production
"couldn't reach" by `lag_decision` is nonetheless resolved correctly at patch time.

**The path is the editorial anchor-seam bracket search** (`patch_region.rs`:
`evaluate_seam_gate_fit_joint` → `anchor_brackets` / `should_run_anchor_seam` /
`list_anchor_candidates_a`, flag set in `mark_anchor_outcome`, `patch_region.rs:747`). Three
properties matter here:

- **Its trigger is the failure Result 1 measured.** It runs when the baseline/grid seam correlation is
  below threshold — i.e. "both shoulders declined" is this mechanism's *entry condition*, not a dead
  end. Declined shoulders don't just fail to predict an audible defect (census Result 5); they don't
  even mean the gap was left at the wrong offset.
- **Its reach is `max_anchor_bracket_secs`, default 5.0 s** (`config.rs:508`) — comfortably past this
  cluster's 2.8 s, and far past `lag_decision`'s 600 ms / 2000 ms radii. Different instrument,
  different budget.
- **It is per-gap and A-side.** It searches A's own editorial-cut candidates; it needs nothing from
  neighboring gaps.

**Two independent correction mechanisms, not one.** This is the durable lesson. `fill_offset.rs`'s
`AnchoredRetry` (the default fill-offset mode) is a *cross-gap* two-pass resolver: pass 1 places every
gap off a two-point whole-clip line (`clip_only_offset_secs`), successful high-confidence patches
become patch anchors (`anchor_eligible`, `patch_anchor.rs:102`), and pass 2 re-resolves retryable gaps
by weighted interpolation across those anchors (`weighted_anchor_points`, `patch_anchor.rs:136`). It
genuinely *does* have a bootstrap limitation — a map step wider than pass 1's own search radius can
leave no gap inside it eligible to seed an anchor. A same-day theory (since deleted) took that
limitation to mean the region could never be corrected, and predicted the wrong outcome, because
`AnchoredRetry` is not the only lever: the anchor-seam bracket search covers this cluster on its own.
**Any future theory about wide map steps must check both mechanisms before predicting an outcome —
and check it against real patch output, not against the mechanism alone.**

---

## Open questions

| # | Question | Why it matters | Priority |
|---|---|---|---|
| 1 | **Why is 50/g10's post join audible when the post-onset remnant matches at r = 1.000?** g10 patched successfully (marginal confidence, `anchor_seam_used`, correct ~2.8 s offset), so the echo is a quality defect on a technically-successful patch. Both prior models are dead: the **silent-onset hypothesis is withdrawn** (it was n=1 inside the cluster; the envelope guesses that motivated it — pre −1600 ms, post −1800 ms — are the *wrong sign* against the true +2.789 s; all six gaps declined, so "declined" cannot single g10 out; and the pre-shoulder is not starved, reading r ≈ 0.99 at +2.8 s by `t = 2–3 s`), and the **unreached-region theory is refuted** by Result 2. One live lead: the overlap-only remnant locks at **13.2 ms**, not 0, and that is the same size as g10's own first/second-plateau split (2.805 s early-context vs 2.831 s; 50/g8 shows the same split, 2.767 vs 2.809). A seam residual of that order is a plausible echo mechanism — untested. | Only confirmed defect in the census's listen batch with no causal model at all (31/g10's blip already fits `shared_far_offset`). A defect that survives a correct offset points at the join itself, not at registration. | **High** |
| 2 | **Why do 50/g11–g13 fail when both flanks succeed at the same offset?** Three gaps *inside* the same map-step region, between the two successfully-resolved flanks, still skip. | Narrow and tractable, not a region-wide defect — but it is the only unexplained *failure* left in the region, and it sits exactly where a wide-map-step fix would be judged. | Medium |

Question 2's data in hand (`2026-08-13-repeat-terms/50.json`):

| gap (0-based) | reason (`min_correlation`) | pre / post | anchor attempt |
|---|---|---|---|
| 11 | `correlation_below_threshold` (0.55) | −0.018 / 0.393 | no `best_attempt` recorded |
| 12 | `correlation_below_threshold` (0.55) | −0.072 / 0.567 | no `best_attempt` recorded |
| 13 | `correlation_below_threshold` (0.12) | 0.123 / 0.117 | **yes** — source `anchor`, 0.188 / 0.226, still below gate |

g13 shows the anchor-seam search ran and still missed its gate; g11/g12 record no `best_attempt` at
all, meaning either no A-side anchor candidate was found or none beat baseline. Distinguishing those
two cases is the first concrete step.

A hypothesis worth testing on both questions together: the anchor-seam search is A-side and per-gap, so
its success depends on A carrying a usable editorial cut within 5.0 s of the gap. That would predict
both a *quality* gradient (g10 patching at the correct offset off a weaker bracket → marginal
confidence, audible join) and an outright *failure* mode (g11–g13, no usable bracket at all) from the
same cause. Untested.

---

## Reproduction

Source dumps are gitignored (`gap-files/`) and not guaranteed to survive cleanup — this doc is the
durable record of what they showed.

- **Result 1 registration sweep:** `gap-files/scratch_patch_registration_sweep.py` against each
  pair-50 `patch`-tier gap's `_a_patched.wav` / `_b_surround.wav` from a `--gap-listen` run. Sliding
  250 ms window, ±3000 ms search. The default ±2000 ms will not find these peaks.
- **Sliver check:** `gap-files/scratch_sliver_check.py` writes the padded listening WAVs; for
  correlation use the **overlap-only remnant** (`3.0 s − extra_offset` after the hole end), not the
  padded files — see the Non-goal below.
- **Result 2:** `gap-files/2026-08-13-repeat-terms/50.json`, `patch.gaps[i].status` — `patched`
  carries `align_adjustment_secs` / `anchor_seam_used` / `confidence`; `skipped` nests its reason one
  level deeper (`status.skipped.reason`), itself either a bare snake_case string or a single-key dict
  for data-carrying variants (`{"correlation_below_threshold": {…}}`).

---

## Non-goals

- Not a Change.
- Not an envelope-sizing plan. That is
  [archive/TEMP-envelope-adaptive-widen-plan.md](archive/TEMP-envelope-adaptive-widen-plan.md)'s question. Result 2
  lowers its stakes (the cluster is corrected downstream anyway) but does not answer it, and neither
  open question above would be resolved by widening `lag_envelope_max_lag_ms`.
- Do not treat pair 50's cluster as an unreachable region — Result 2 refutes that by direct production
  output: all 6 `patch`-tier gaps resolve at the correct ~2.8 s offset via the anchor-seam bracket
  search (`max_anchor_bracket_secs`, default 5.0 s), a mechanism independent of `anchored-retry`'s
  cross-gap triangulation.
- Before proposing any fix aimed at "wide map steps," check **both** correction mechanisms
  (`fill_offset.rs`'s `AnchoredRetry` cross-gap triangulation *and* `patch_region.rs`'s per-gap
  anchor-seam bracket search) — a theory built from only one of them predicted the wrong outcome here.
  And check it against real patch output, not against the mechanism alone.
- Do not treat `envelope_rescue: declined` as a defect signal by itself — Result 1 shows it is
  structurally expected for the whole cluster (true offset exceeds both the 600 ms nominal and 2000 ms
  envelope search radii), and 5 of 6 patched cleanly by ear anyway. 50/g10's echo (the one exception)
  still needs its own explanation — Question 1 — but it is not explained by "declined."
- Do not read a `_a_surround`/`_b_surround`/`_a_patched` WAV comparison (whole-clip or sliding) as
  proof of content mismatch when the gap's true offset is large relative to `gap_signature_context_secs`
  (default 3.0 s per side). **Context-margin overrun**: true overlap between the extracted clips ≈
  `duration_secs + 2×margin − |extra_offset|`; once offset approaches or exceeds `2×margin` (6.0 s),
  the windows share no true corresponding content at all, independent of whether the underlying
  gap/patch is defective. Check this arithmetic before treating a "no match" region as evidence of
  anything.
- **Context-margin overrun is a fact about the exported listen clips, not about `lag_decision`'s B
  haystack.** Production extracts ~14 s lead / ~15–19 s tail; the listen export is a deliberately
  truncated ±3 s centre of that. Never explain a production decline with clip-margin arithmetic —
  check the search radius instead. And do not "fix" the overrun by raising `gap_signature_context_secs`
  on the export: that would merge the ear-comparison window with the search haystack, which
  [archive/TEMP-gap-listen-wav-plan.md](archive/TEMP-gap-listen-wav-plan.md) §10 deliberately kept apart.
- **Do not correlate the padded sliver WAVs and report the result as overlap evidence.**
  `scratch_sliver_check.py` pads by 150 ms per side for listenability; on ~160–190 ms remnants that
  padding is most of the file and drags r down (g16: 0.575 padded vs 0.997 on the remnant). Correlate
  the overlap-only remnant.
- Not a `collapsed_bridge` question. Nothing in this note bears on
  `|gap + post_lag − pre_lag| ≈ 0`; see
  [TEMP-collapsed-bridge-census-2026-08-20.md](TEMP-collapsed-bridge-census-2026-08-20.md) for that
  predicate, which remains at n=1.
