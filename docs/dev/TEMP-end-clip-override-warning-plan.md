# TEMP — End-clip override warning (A6)

> **Latched 2026-09-16 — not implemented.** The no-code census (§ 9) found
> the constraint hides a step on **1 of 52** pairs (pair 54), which A5
> already warns on and the shipped stall-segmented map already remaps.
> Every other disagreeing pair's step sits inside the ±5 s refine radius,
> where the constrained search finds the same offset (≤ 41 µs). Trigger:
> [settled.md](settled.md) § C.

**Status:** design, 2026-09-14; latched 2026-09-16. A Change (user-visible
warning + additive JSON field). Changes no offset, confidence, or fill
decision — **the warning** is fill-neutral; the constraint itself is not
(§ 9).

**Tracked as:** [settled.md](settled.md) § C — latched. Was
[BACKLOG.md](../../BACKLOG.md) § *Timeline integrity* → *End-clip
constrained search hides a timeline step*.

**Companions (do not merge):**

- A5 B-stall warning ([TEMP-b-stall-warning-plan.md](archive/TEMP-b-stall-warning-plan.md))
  — scan-time, reads B silence. A6 is alignment-time and reads the two
  end-clip estimates. On pair 54 both fire, for the same step, from
  independent evidence.
- *Drift* / *clip offsets disagree* lines — they fire only when the
  **reported** clip offsets disagree. Under the default constraint the
  reported end offset is forced near the start, so they never see the
  step. A6 is the line for exactly that case.
- End-clip confidence de-inheritance (§ 6) — a behavior change; its own
  deliverable. This plan only measures it.

**Media hygiene:** pair index only.

---

## 0. What this ships (normative)

**When the default end-clip constraint replaces an independent end
estimate that matched with confidence ≥ `min_match_score`, and the
offset it used differs from that estimate by more than the agreement
tolerance, say so — with both offsets — instead of silently overriding.**

That is the whole plan. If a later sentence disagrees with this one,
this one wins.

The chosen offset, per-clip confidence, `offsets_consistent`,
`recommended_offset_secs`, and every downstream fill are unchanged **by
this plan** — nothing reads the new field. That is not a claim that the
constraint is fill-neutral: turning it off changed 6 of 1087 gap outcomes
(§ 9).

---

## 1. Motivating case (pair 54)

Scan-only re-run, 2026-09-14:

| Run | End clip | Recommended | Label |
|-----|----------|-------------|-------|
| default (constrained) | +1.861 s, conf 0.92, "refined ±5s around start" | +2.058 s | clip offsets agree |
| `--no-constrain-end-clip-to-start-offset` | +66.472 s, conf 0.95 | +2.292 s | clip offsets disagree; using start clip |

The +1.861 s is the verbose **progress line**, printed inside the clip loop
before high-rate refinement. The final end clip in the 2026-09-15 corpus
dump reads **+1.824 s** (same run shape, same 0.9218163 confidence) — the
37 ms is most likely high-rate refinement, within its 0.1 s adjustment cap,
not re-verified. A6's record is pre-high-rate (§ 2 rule 5), so § 8's
+1.861 s is the value the warning would print.

The independent estimate is **higher** confidence than the start clip
(0.92), and it is thrown away. The constrained 0.92 is not a measurement:
`refine_offset_around_prior` returns `prior.confidence.max(measured)`,
so the end clip reports the start clip's confidence verbatim.

The constraint was added (49b88bf, 2026-06-20) without a recorded
motivating case. Presumed purpose: stop a weak or wrong end Chromaprint
match (trailing silence, credits, repeated material) from derailing a
good start. A6 must stay silent on that case — hence the confidence
floor in rule 2.

---

## 2. Rule (normative)

At `align_videos.rs` end-clip branch, the **constrained** arm only — the
`else if` at `align_videos.rs:832`. It is reached when all of:

- `end_refine_candidate` (line 764): `window.label == End`,
  `refine_end_clip_around_start_offset`, and start prior confidence
  ≥ `min_match_score * 0.5`;
- Chromaprint disagrees (`|prior − independent| > 0.5 s`);
- `constrain_end_clip_to_start_offset`, `refine_offset_with_pcm`, and
  start prior confidence ≥ `min_match_score`.

1. **Record** `EndClipOverride` whenever the constrained arm runs:
   - `independent_offset_secs`, `independent_confidence` — the
     already-computed `independent` (post PCM refine), currently dropped.
   - `constrained_offset_secs` — the offset the arm returned.
   - `constrained_measured_confidence: Option<f32>` —
     `correlation_to_confidence(score)` from the PCM search **without**
     the `max(prior)`; `None` when the search found nothing. Diagnostic
     only (feeds § 6); nothing reads it.
2. **Warn** when `independent_confidence >= min_match_score` **and**
   `|independent_offset_secs − constrained_offset_secs| >
   OFFSET_AGREEMENT_TOLERANCE_SECS` (0.5 s). Compare against what was
   **used**, not the start prior: if the ±radius search happened to land
   on the independent lag, nothing was hidden.
3. Below the floor, or within tolerance: record only, plus a
   `tracing::debug!` line (for the § 5 census).
4. Agree arm, unconstrained arm, single-clip and query-reference runs:
   no record (`None`).
5. Values are pre high-rate refinement **and pre repetition downgrade**.
   High-rate may nudge the clip offsets afterward, and the repetition
   check (`align_videos.rs:926`) may halve `clip.confidence` after the
   loop, so `independent_confidence` can differ from the end clip's
   reported confidence. The record describes the constraint decision, not
   the final clip.

Surface it on `AlignmentResult.end_clip_override: Option<EndClipOverride>`
(domain, `clip-sync`). The warn decision is a pure function
`end_clip_override_warns(&EndClipOverride, min_match_score) -> bool` so
both CLIs and the report share one rule.

**Plumbing.** The record is produced inside the per-window loop, but
`AlignmentResult` is only built afterwards by `build_alignment_result`
(`align_videos.rs:893`). Hold an `Option<EndClipOverride>` local across the
loop and assign it to `result.end_clip_override` after the build. The
query-reference path builds its result separately and stays `None`.

**Measured confidence.** `refine_offset_around_prior` has a second caller
(`locate_query.rs:469`). Add a sibling that returns
`(ClipMatchEstimate, Option<f32>)` (the un-maxed
`correlation_to_confidence(score)` from `pcm_search_near_offset`, `None`
when it found nothing) and make the existing function a thin wrapper over
it, so its return value and the query caller are unchanged. The score is
taken at the discovered offset, before the trailing lag adjustment.

**Facade.** Application crates use only `crates/clip-sync/src/lib.rs`
re-exports: add `EndClipOverride`, `end_clip_override_warns`, the § 3.1
formatter, and the § 3.2 DTO there (as `format_end_clip_anchor_line` is).

---

## 3. Output

### 3.1 Human (stdout, both binaries)

Default verbosity, under the alignment block, `Warning:` prefix as the
existing repair warnings:

```
Warning:   end clip matched independently at +66.472s (confidence 0.95) but was constrained to +1.861s near the start offset (Δ +64.611s) — possible timeline step between clips; --no-constrain-end-clip-to-start-offset shows the independent estimate
```

- Δ = independent − constrained, signed, 3 dp.
- Mirror to `tracing::warn!` (same `not_applied_count` pattern as A5).
- `clip-sync-cli` `output.rs` and `clip-sync-repair` `output.rs` both
  print it; format lives in one shared function in `clip-sync`
  (`report.rs` neighbourhood) so the text cannot diverge.
- Verbose: the end-clip progress line gains
  `(independent +66.472s @ 0.95 overridden; measured 0.41)` after
  `(refined ±5s around start)` whenever a record exists, warned or not.
  `; measured …` is omitted when `constrained_measured_confidence` is
  `None`. (0.41 is illustrative, as in § 3.2.)

### 3.2 JSON (additive, stays v1)

`alignment.end_clip_override` on `AlignmentReport`,
`skip_serializing_if = "Option::is_none"`:

```json
"end_clip_override": {
  "independent_offset_secs": 66.472,
  "independent_confidence": 0.95,
  "constrained_offset_secs": 1.861,
  "constrained_measured_confidence": 0.41,
  "warned": true
}
```

(`constrained_measured_confidence` above is illustrative — pair 54's
real value is unmeasured until the domain item lands.)

- `warned` is the § 2 rule 2 result, stamped so consumers need no
  threshold.
- DTO in `clip-sync/src/application/report.rs`; [json-output.md](../json-output.md)
  additive entry. Goldens: regenerate only those whose fixture takes the
  constrained arm (expected: none — check, don't assume).
- One field covers **both** binaries: the repair CLI's human output and
  its JSON are both built from `AlignmentReport::from(&AlignmentResult)`
  (`clip-sync-repair/src/infrastructure/cli/output.rs:47`, `:869`). No
  plumbing through the repair-domain `ScanAlignment` is needed.

---

## 4. Tests

Library (`align_videos.rs` tests, fake aligner/correlator as the
existing constrain tests at ~2015 / ~2055):

1. Constrained arm, independent conf ≥ floor, Δ > 0.5 → record, warns.
2. Constrained arm, independent conf < floor → record, does not warn.
3. Constrained arm lands within 0.5 s of independent → record, no warn.
4. Agree arm → `None`.
5. `constrain_end_clip_to_start_offset = false` → `None` (and existing
   behavior unchanged).
6. Constrained arm does not change `clips[end].offset_secs`,
   `confidence`, or `recommended_offset_secs` vs today (pin current
   values).

Pure: `end_clip_override_warns` boundary (== floor warns; Δ == 0.5 does
not).

Output: human line present / absent in both CLIs; JSON field absent when
`None`, present with `warned` when `Some`.

---

## 5. Census (ship gate, not code gate)

Before shipping the warning at default verbosity:

- **Before running**, write down the known-flat pair list and its source
  (settled map steps, ear-verified pairs, A5 stalls) in this plan, so the
  pass/stop call is not made after seeing the results. A pair on neither
  list that warns is recorded as *unclassified* and needs its own look
  before shipping.
- Run every corpus pair scan-only via
  `scripts/measure/scan-registration.ps1`. Every constrained-arm event
  produces a record, and scan-only JSON carries it, so read
  `alignment.end_clip_override` from the per-pair JSON the script already
  writes. Rule 3's `debug!` line is a fallback, not the census source.
- Tabulate per pair: constrained arm taken?, independent offset /
  confidence, constrained offset, measured confidence, Δ, `warned`.
- Classify each warned pair against the pre-registered list.
- Keep a pre-change scan JSON set (same script, same binary minus this
  change) for the offset-identity check in § 8.

**Pass:** pair 54 warns; no warned pair is on the known-flat list; no
warned pair is left unclassified.
**Stop and redesign** if a known-flat pair warns (the presumed trailing-
silence case exists at conf ≥ floor) — the floor alone is not enough and
§ 2 needs a second condition (e.g. independent end window silent on
either side) before shipping.

Also record from the same run (for § 6, not a gate): how often the
constrained arm fires, and the measured-vs-inherited confidence gap.

---

## 6. Not in this plan

- **Confidence de-inheritance** (stop `prior.confidence.max(…)`): changes
  `end_aligned`, fusion weighting, and possibly `recommended_offset_secs`
  → fills. Needs its own plan gated on the § 5 table. Sized in § 9 and
  latched in [settled.md](settled.md) § C — no defect traced to it yet.
- **Changing the default** of `constrain_end_clip_to_start_offset`.
- **Stall-segmented map** — shipped 2026-09-15
  ([archive/TEMP-stall-segmented-map-plan.md](archive/TEMP-stall-segmented-map-plan.md)).
- **The ≈ 1.4 s residual** on pair 54 (+66.47 s end clip vs +67.9 s at
  the end silence).

---

## 7. Checklist

- [ ] Domain: `EndClipOverride` + `AlignmentResult.end_clip_override` + pure `end_clip_override_warns` (`crates/clip-sync/src/domain/alignment.rs`); fill `None` in every `AlignmentResult` literal — ~50 across 13 files, including `clip-sync-repair-harness`, `clip-sync-cli/tests/cli_output.rs` (9), `clip-sync-repair` `output.rs` (7) and `test_support.rs` (7). Mechanical, but the bulk of the diff.
- [ ] Measured confidence: sibling of `refine_offset_around_prior` (`crates/clip-sync/src/application/offset_refinement.rs:193`) returning `(ClipMatchEstimate, Option<f32>)`; the existing fn wraps it, so `locate_query.rs:469` is untouched.
- [ ] Record in the constrained arm (`crates/clip-sync/src/application/align_videos.rs:832`), carried out of the loop and assigned after `build_alignment_result` (`:893`); debug line; verbose progress suffix. Tests § 4 1–6.
- [ ] `lib.rs` facade re-exports for the new domain type, pure fn, formatter and DTO.
- [ ] Shared human formatter + print in `crates/clip-sync-cli/src/infrastructure/cli/output.rs` and `crates/clip-sync-repair/src/infrastructure/cli/output.rs`; `tracing::warn!`.
- [ ] JSON DTO (`crates/clip-sync/src/application/report.rs`) + [json-output.md](../json-output.md) + goldens if any take the arm.
- [ ] [cli-output.md](../cli-output.md): *Multi-clip offset merge* row + *Timeline / duration warnings* A6 row.
- [ ] Known-flat list written into § 5 before the census runs.
- [ ] § 5 census; pair 54 warns; BACKLOG row for § 6 opened from the table.

---

## 8. Done when

A default scan of pair 54 prints the § 3.1 line with +66.472 s / +1.861 s,
its JSON carries `end_clip_override.warned: true`, the § 5 census passes,
and every corpus pair's scan-only JSON is identical to its pre-change
report apart from the added `alignment.end_clip_override` field. Fills
are unchanged by construction — no decision reads the new field — and
this plan does not run a patch-outcomes pass to re-prove it.

---

## 9. Census result (2026-09-16) — latched

Run without implementing: every corpus pair through `--gap-fingerprints`
twice, default vs `--no-constrain-end-clip-to-start-offset`
(`gap-files/2026-09-15-corpus` and `…-corpus-no-contrain`, both build
`174a3be2a7dc` dirty, identical scan/gate/probe recipes; 52 pairs — 6 and
16 absent). The `source.alignment` block in each `corpus.json` carries both
clips' offset and confidence, which is the § 5 table minus the measured
column. Scan-only would have been enough; the fingerprint pass was not
needed for this question.

**End offset.** 29 pairs disagree with the start clip by > 0.5 s (drift
−4.12 … +3.67 s), so they take the constrained arm by default. On every one
the step is inside `end_clip_refine_radius_secs` (5 s) and the constrained
search lands on the independent offset: max |Δ end offset| across runs is
**41 µs** over 51 pairs. Only pair 54 moves: +1.824 → **+66.472 s** (conf
0.9516), drift −0.468 → **+64.180 s**, `offsets_consistent` true → false.
Under § 2 rule 2, A6 warns on **1 of 52**. The constraint hides a step only
when it exceeds the refine radius.

**Confidence (§ 6 sizing).** On the 29 constrained pairs, inherited −
measured end confidence: min **−0.333**, median **+0.007**, max **+0.075**.
The measurement is *higher* than the inherited value on 8 pairs (23, 27, 28,
33, 38, 39, 43, 45; pair 38 0.548 → 0.881). No measured value falls below
0.88, so de-inheritance would not flip `end_aligned` at `min_match_score`
0.5 on this corpus.

**Fill outcomes.** 6 of 1087 gap outcomes differ between runs (48
summary-tier g000 entries carry no outcome and are excluded; identical in
both). Pair 54 g002 and g006 skip → patch, from its 234 ms recommended-offset
change (+2.058 → +2.292 s). Pair 3 g013 patch → skip, pair 24 g001 patch →
skip, pair 37 g001 skip → patch and g002 patch → skip, from recommended
shifts of 1.4–5.0 ms — knife-edge gate decisions, not steps. The build
predates `5537d8d`, so pair 54 g012 reads skip in both runs; the shipped
stall-segmented map patches it.

**Why latch, not ship.** Pair 54 is already covered twice: A5 names the
66.6 s stall, and the stall-segmented map remaps later gaps from its
`shoulder` shift. A6 would add a third voice on the only case, for ~50
struct-literal edits. Pair 50's 2.75 s stall (`shoulder`, shift 2.8 s) is
inside the radius and is already handled by the same map.
