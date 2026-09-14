# TEMP — End-clip override warning (A6)

**Status:** design, 2026-09-14. A Change (user-visible warning + additive
JSON field). Changes no offset, confidence, or fill decision.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Timeline integrity* →
*End-clip constrained search hides a timeline step*.

**Companions (do not merge):**

- A5 B-stall warning ([TEMP-b-stall-warning-plan.md](TEMP-b-stall-warning-plan.md))
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
`recommended_offset_secs`, and every downstream fill are unchanged.

---

## 1. Motivating case (pair 54)

Scan-only re-run, 2026-09-14:

| Run | End clip | Recommended | Label |
|-----|----------|-------------|-------|
| default (constrained) | +1.861 s, conf 0.92, "refined ±5s around start" | +2.058 s | clip offsets agree |
| `--no-constrain-end-clip-to-start-offset` | +66.472 s, conf 0.95 | +2.292 s | clip offsets disagree; using start clip |

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

At `align_videos.rs` end-clip branch, the **constrained** arm only
(Chromaprint disagrees, `constrain_end_clip_to_start_offset`,
`refine_offset_with_pcm`, start confidence ≥ `min_match_score`):

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
5. Values are pre high-rate refinement. High-rate may nudge the clip
   offsets afterward; the record describes the constraint decision, not
   the final clip.

Surface it on `AlignmentResult.end_clip_override: Option<EndClipOverride>`
(domain, `clip-sync`). The warn decision is a pure function
`end_clip_override_warns(&EndClipOverride, min_match_score) -> bool` so
both CLIs and the report share one rule.

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
  `(independent +66.472s @ 0.95 overridden)` after `(refined ±5s around
  start)` whenever a record exists, warned or not.

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

- Run every corpus pair scan-only via
  `scripts/measure/scan-registration.ps1` with
  `RUST_LOG=warn,clip_sync=debug` (debug line from rule 3 gives every
  constrained-arm event, warned or not).
- Tabulate per pair: constrained arm taken?, independent offset /
  confidence, constrained offset, measured confidence, Δ.
- Classify each warned pair against known ground truth (settled map
  steps, ear-verified pairs, A5 stalls).

**Pass:** pair 54 warns; no warned pair has a known-flat map.
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
  → fills. Needs its own plan gated on the § 5 table. Open a BACKLOG row
  from the census result.
- **Changing the default** of `constrain_end_clip_to_start_offset`.
- **Stall-segmented map** — separate BACKLOG row.
- **The ≈ 1.4 s residual** on pair 54 (+66.47 s end clip vs +67.9 s at
  the end silence).

---

## 7. Checklist

- [ ] Domain: `EndClipOverride` + `AlignmentResult.end_clip_override` + pure `end_clip_override_warns` (`crates/clip-sync/src/domain/alignment.rs`); fill `None` in every `AlignmentResult` literal.
- [ ] Measured confidence: expose the un-maxed PCM score from `refine_offset_around_prior` (`crates/clip-sync/src/application/offset_refinement.rs:193`) without changing its return value.
- [ ] Record in the constrained arm (`crates/clip-sync/src/application/align_videos.rs:832`); debug line; verbose progress suffix. Tests § 4 1–6.
- [ ] Shared human formatter + print in `crates/clip-sync-cli/src/infrastructure/cli/output.rs` and `crates/clip-sync-repair/src/infrastructure/cli/output.rs`; `tracing::warn!`.
- [ ] JSON DTO (`crates/clip-sync/src/application/report.rs`) + [json-output.md](../json-output.md) + goldens if any take the arm.
- [ ] [cli-output.md](../cli-output.md): *Multi-clip offset merge* row + *Timeline / duration warnings* A6 row.
- [ ] § 5 census; pair 54 warns; BACKLOG row for § 6 opened from the table.

---

## 8. Done when

A default scan of pair 54 prints the § 3.1 line with +66.472 s / +1.861 s,
its JSON carries `end_clip_override.warned: true`, the § 5 census shows
no known-flat pair warning, and no offset or fill changed on any corpus
pair.
