# Review: TEMP-seam-splice-dualfit-plan.md

**Reviewer context:** Corpus analysis via `diag_fingerprint_corpus` on all 6 pairs (19 matched, 6 skipped),
per-gap `corpus.json` bracket inspection, and comparison of patched vs skipped high-step exemplars (5·g3 vs
1·g19). This document records concerns and suggestions for the plan — it does **not** supersede or amend
the plan itself.

**Overall verdict:** The diagnostic premise is sound and well-supported. The repair design (§4) is
plausible but unproven. The plan would benefit from sharper language around *when* dual-fit is needed vs
what today's anchor/boundary search already handles.

---

## What the plan gets right

1. **Refutation of cross-codec validator-swap.** `one-sided-dead = 0` across 6 pairs; 5·g6 and similar
   gaps recover at their own lag once `baseline_lag` (±200 ms) is read instead of `seam_probe` (±25 ms).
   The cross-codec plan should stay archived.

2. **Registration decomposition (offset vs step).** Measuring step at the throat via `baseline_lag` and
   surfacing it in `splice_text` / `SpliceDiag` is the right observability layer. It concretizes the
   vocabulary-plan registration axis.

3. **"Earn the existing gate" (§4.4).** Post-reconciliation validation with unchanged Pearson is the
   correct success criterion. Avoids the validator-loosening trap.

4. **Measurement discipline.** §3.6 → freeze timescales → re-scan → repair is the right sequencing. §3.6a
   results (mono correlation, 1 s uniqueness, energy-weighted level, 100 ms wide-envelope) are well
   motivated on pair-1 data.

5. **Honest caveats (§2).** Non-quantized steps, brittle single-rival uniqueness, and unmeasured donor
   interior are called out rather than hand-waved.

---

## Concerns

### C1. Step magnitude does not predict patch vs skip

The plan's §0 theory and §1 finding (3) read as if a nonzero step causes skips ("single rigid placement
cannot satisfy both seams"). The **full 6-pair corpus contradicts this as a routing rule:**

| Fact | Value |
|------|-------|
| Matched gaps with `\|step\| > 2 ms` | **18 / 19** |
| Patched gaps with large step | 5·g3 +72 ms, 3·g3 +40 ms, 1·g20 +33 ms, 5·g5 −38 ms, … |
| Skipped gaps with smaller step | 2·g1 +13 ms, 1·g22 +12 ms |

**Step at the throat is the normal registration signature, not the skip signature.**

The actual discriminator is whether **anchor/boundary search finds any bracket** where both seams pass
Pearson @ lag 0:

| | Patched 5·g3 (step +72 ms) | Skipped 1·g19 (step +72 ms) |
|---|---|---|
| Per-side `baseline_lag` peaks | 0.93 / 0.97 | 0.998 / 0.974 |
| Brackets passing gate | **18 / 25** | **0 / 16** |
| Best bracket `min(pre,post)` | **0.63** | **0.07** |

Same-order step, opposite outcomes. Skips are "per-side recoverable at throat, but **no bracket in search
space** makes both seams pass @0" — not "step too large."

**Risk if unaddressed:** Implementing dual-fit for all high-step gaps would duplicate work the fit-joint
path already does (boundary sliding + `fill_length_slack` as a coarse length reconciliation).

---

### C2. Throat step ≠ step at winning bracket

`baseline_lag` step is measured at the **structure throat** with **independent per-side lag sweeps**. The
gate decides on a **winning anchor/grid bracket** that may move `refined.start/end` substantially.

Fingerprint data **already refutes** “winning bracket move ≈ throat step” on patched high-step gaps:

| Gap | Throat step | Best passing bracket | Notes |
|-----|-------------|----------------------|-------|
| 5·g3 patch | +72 ms (~3476 smp) | `move_frames` = 124800 (**2600 ms** move), seams 0.63 / 0.63 | Boundary search ≠ interior trim |
| 3·g3 patch | +40 ms (~1935 smp) | `move_frames` = 4800 (**100 ms**), span +100 ms | Closer to length edit, still ≠ 40 ms |
| 1·g19 skip | +72 ms (~3450 smp) | 0 passing; best-fail seams ~0.07 | Same step as 5·g3, search exhausted |

Across patched gaps, **0 / 18** have `|step|` within 20 ms of any bracket `span_secs` delta from throat —
bracket absorption and throat step are different coordinates. The plan should treat throat step as a
**diagnostic coordinate** for dual-fit length edit, not as a direct read of what anchor search already did.

---

### C3. Status line is stale

Line 4 still says "pending 1&6 re-scan to confirm" and the cross-codec plan is "held superseded pending
1&6." §2 and §5 mark this **resolved** (`one-sided-dead = 0` on all 6 pairs). The header should match §2
or readers will treat the refutation as provisional.

---

### C4. §1 table scope is misleading

§1 is titled "dirs 2–5; 14 gated gaps" but §2/§5 conclusions draw on all 6 pairs (19 matched). The skip
set changed when pairs 1 and 6 were added (6 skips total, not the 3 in the §1 table). Either expand §1's
table or add a footnote that it is an early subset and point to `splice_text` / CSV for the authoritative
full-corpus view.

---

### C5. Mechanism story is inferred, not identified

"Encoder silence trim at the gap" is plausible but §2's quantization test (~0.84× chance) shows steps are
**sub-frame** (13, 32, 72 ms — not clean block sizes). Could be resampler boundaries, PTS quantization, or
padding — doesn't block repair, but the plan should not imply a single physical cause until characterized.

---

### C6. Uniqueness gating may exclude half the skip bucket

Of 6 skips, only 3 pass the strict `both_sides_recoverable` gate (margins 0.31–0.50); 3 are
`alias-suspect` (0.14–0.19). §3.6a shows gap-3-pre becomes decisively unique at 1 s — good — but until
the re-scan applies `peak_z` / prominence at 1 s in capture, the **repair addressable set** under current
`SpliceDiag` may be smaller than "all 6 skips."

Conversely, some **patched** gaps would fail strict uniqueness today (5·g3 margin 0.02) — uniqueness is not
a patch prerequisite in production, only a proposed dual-fit detect gate. The plan should state explicitly
that detect gates and patch gates need not be identical.

---

### C7. Donor interior still unmeasured in committed scans

`DonorInterior` is **implemented** in `gap_fingerprint.rs` (`rms_db`, `silence_fraction`,
`longest_silence_ms`, `continuous`) but **absent from on-disk `gap-files/` corpora** (scans predate the
field). §3 item 1 is coded, not captured. Until re-scan: `structure` across the gap (0.97) is encouraging
but ±3 s envelope; skipped gaps like 2·g1 have strong structure and shoulders but 0 passing brackets —
donor continuity is still assumed, not shown. See § “Proof items vs fingerprint coverage” for a geometry +
media workaround before full re-scan.

---

### C8. §4 repair is entirely unproven

The closure loop that makes this an **implementation** idea rather than an **analysis** conclusion:

> independent fit → length reconciliation → unchanged gate passes → sounds correct

…has not been run. No flagged prototype exists. §4.4 is a design property, not a demonstrated one.

---

### C9. Quiet / level conflation on some skips

1·g19 (skipped, step +72 ms, 0 passing brackets) has post seam SNR **−31.6 dB** on straight mono —
classic center-dominant 5.1 dilution. §3.6a correctly moves level to energy-weighted downmix, but §4
detect still references `both_sides_recoverable` from the **old** 250 ms / single-rival metric until
re-scan. Ensure the repair detect path uses the frozen level representation, or quiet false-negatives
persist.

---

### C10. Dual-fit scope vs anchor search overlap

Today's fit-joint path already approximates length reconciliation:

- Anchor seam brackets move gap boundaries and gap length.
- `fill_length_slack` allows end boundary variation.
- Adaptive seam windows on anchor brackets.

5·g3 patches with +72 ms throat step because **18 brackets pass** with balanced ~0.38 Pearson — boundary
search found a lag-0 compromise. Dual-fit is needed for the **residual** where that search fails (6 skips),
not as a replacement for anchor/boundary search.

---

## Suggestions for the plan

### S1. Add a section: "Why step doesn't predict outcome"

Document the 5·g3 vs 1·g19 contrast as the canonical pair. State clearly:

- Step at throat is nearly universal (18/19 matched).
- Patch vs skip is **bracket search success**, not step magnitude.
- Dual-fit targets gaps where per-side lag says recoverable but **all brackets fail** (`best_bracket_seam`
  ≪ floor).

Include the gate summary from `gate_text()`:

- Patched: best-bracket seam median **0.62** (throat median 0.38 — often weak at throat, rescued by
  bracket).
- Skipped: best-bracket seam max **0.11**.

---

### S2. Refine §0 / §1 finding (3) wording

Replace language implying "step ⇒ skip" with:

> At the **throat**, independent per-side lag sweeps show a step. The patcher uses a **single lag-0
> placement** and boundary search to absorb it. When search finds a bracket with both seams passing,
> the gap patches **despite** a large throat step. When search exhausts, the gap skips **despite** per-side
> recoverability — that residual is the dual-fit target.

---

### S3. Update header status

Align line 4 with §2: all 6 pairs analyzed, cross-encoding refuted, cross-codec validator-swap closed.

---

### S4. Define dual-fit's relationship to existing search

Add a decision tree:

```text
structure placement OK?
  → anchor/boundary search finds lag-0 bracket?  → patch (today)
  → else: both shoulders recoverable at own lag + donor continuous?  → dual-fit candidate
  → else: skip
```

This prevents dual-fit from running on gaps like 5·g3 that already patch.

---

### S5. Add open proof items before §4 implementation

| # | Claim | How to prove | Falsifier |
|---|-------|--------------|-----------|
| P1 | Post-reconciliation passes existing gate on clean-splice skips | Flagged prototype on 1·g19, 1·g22, 6·g6 (and 2·g1, 5·g6) | Gate still fails after trim/pad |
| P2 | Step at throat ≈ required length edit | Compare `seam_step_ms` to samples trimmed for P1 successes | Large mismatch → wrong model |
| P3 | Bracket absorption ≠ dual-fit on patched high-step gaps | On 5·g3, log winning bracket boundary move vs throat step | If equal, boundary search already does it |
| P4 | Donor continuity | §3 capture item 1 on skip targets | B silent/discontinuous through hole |
| P5 | No regression | Dual-fit flag off; existing patches unchanged | Any patched gap breaks |
| P6 | Audibility | Listen on P1 successes; compare trim-at-min-energy vs center | Audible artifact at splice point |
| P7 | Decoy safety | F1-style wrong placement still fails post-reconciliation | False rescue |

See **§ Proof items vs fingerprint coverage** for which items can be advanced from existing scan data,
which need re-scan, and which need fingerprint + media diagnostics (including proposed
`diag_splice_dualfit` for P1/P2 before repair is wired).

---

## Proof items vs fingerprint coverage

The gap-fingerprint scan is licensing-safe numeric characterization only — **no samples** — but it already
carries enough geometry and scores to advance several proof items **without** wiring §4 repair. Source:
`application/gap_fingerprint.rs`, on-disk `gap-files/*/corpus.json`.

### What the scan captures today (`gap-files/`)

| Field | Proof relevance |
|-------|-----------------|
| `baseline_lag` | Per-side `peak_r`, `frac_lag_ms`, `second_peak_r` → **step**, both-sides-recoverable |
| `brackets[]` | Every anchor bracket: `seam_pre/post`, `failure_stage`, `move_frames`, `pre/post_time_secs` |
| `structure` | Envelope placement at throat |
| `seam_probe` | `recovered_r`, `recovered_lag_ms`, `snr_db`, R2/R4 — why Pearson@0 is dead |
| `geometry` | `a_refined_*`, `b_mapped_*` — reconstruct windows on source media |
| `levels` / `silence` | A-side context (not B interior) |
| `residual` | Same-source cancellation at throat |
| `outcome` | Ground-truth patch/skip |

### Implemented in code but missing from committed corpora

Re-scan with the current binary populates these without new repair work:

| Field | Proof relevance |
|-------|-----------------|
| `donor_interior` | P4 — B RMS / silence / `continuous` through the mapped hole |
| `splice` | Promoted `step_ms`, peaks, `peak_z` for detect predicate |
| `wide_envelope` | Cross-scale lag agreement (§3.6a confirmer) |
| `baseline_lag` @ **1 s** | `peak_z`, `prominence`, `top2_spacing_ms` (current scans still show `window_ms: 250`, `peak_z: null`) |

### Proof item by proof item

| # | From fingerprint JSON alone? | From fingerprint + method | Notes |
|---|------------------------------|---------------------------|-------|
| **P1** | **No** — no repaired fill or post-trim seam scores | **Yes** — offline dual-fit gate simulation (below) | Highest-value de-risk before §4 |
| **P2** | **Partial** — step from `baseline_lag`; bracket data shows move ≠ step | **Yes** — simulation compares trim magnitude to `step_ms` | Fingerprint already refutes “bracket move = step” (C2) |
| **P3** | **Yes** — `brackets[]` + `outcome` | `diag_fingerprint_corpus` | Patch vs skip = any bracket passes, not `\|step\|` |
| **P4** | **No** in old scans (`donor_interior` absent) | **Yes** — geometry + B decode in diagnostic; or re-scan | `structure` is a weak proxy only |
| **P5** | Baseline fixture list only | Repair re-run (flag off vs on) | Fingerprints record expected outcomes, don't prove non-regression |
| **P6** | **No** | Listening | Geometry gives splice timestamps |
| **P7** | Weak — failed brackets as near-miss negatives | Simulation with deliberately wrong B offset | No F1 decoys in corpus |

### P1 / P2: offline dual-fit gate simulation (proposed)

Extend the `diag_splice_timescale` pattern — `corpus.json` as spec, ffmpeg for samples:

1. Read `geometry` + `baseline_lag` for the gap.
2. Decode A/B around the gap (same env vars as `diag_splice_timescale`).
3. **Simulate** dual-fit: align donor using per-side `frac_lag_ms`, trim/pad `|step|` at min-energy interior.
4. Run the same Pearson seam windows the gate uses (`fill_seam_search_secs` border).
5. Report whether `min(pre, post)` clears `min_fill_correlation` / `fill_absolute_floor`.

This is not the repair pipeline, but it uses fingerprint data as the experiment spec and directly tests
P1 (gate pass after reconciliation) and P2 (whether required edit ≈ `step_ms`). Proposed name:
`diag_splice_dualfit` (not yet implemented).

### P3: formalize the decision tree from fingerprints alone

Aggregate over matched gaps (from `diag_fingerprint_corpus` / CSV):

- **Patched:** best-bracket seam median **0.62**; throat seam median **0.38** (often weak at throat,
  rescued by bracket).
- **Skipped:** best-bracket seam max **0.11**; all 6 fail at `waveform_floor`.

Decision tree implementable from `brackets[]` + `baseline_lag` without media:

```text
any bracket with failure_stage == null?  →  patch (today — do not dual-fit)
else if both baseline_lag peaks ≥ floor?  →  dual-fit candidate
else  →  skip (other mechanism)
```

Canonical contrast: **5·g3** (step +72 ms, 18/25 brackets pass) vs **1·g19** (step +72 ms, 0/16 pass).

### P4: donor continuity before full re-scan

`geometry.b_mapped_start_secs` / `b_mapped_end_secs` are already in every full-tier gap. A one-off
diagnostic pass (same media paths as §3.6) can RMS B over that span and compute continuity **without**
waiting for `donor_interior` in capture — or re-scan to freeze it into the schema.

### P7: what fingerprints offer for decoys

- **Failed brackets** (`failure_stage: waveform_floor`, low `seam_pre/post`) are near-miss negatives in
  the real search space — not deliberate wrong-content decoys.
- Deliberate wrong-B placement is **not** in the corpus; test via offline simulation (shift haystack) or
  synthetic fixtures.

### Methods summary

| Method | Inputs | Addresses |
|--------|--------|-----------|
| `diag_fingerprint_corpus` | `gap-files/` dirs | P3, skip taxonomy, bracket exhaustion |
| CSV export (`GAP_FP_CSV=1`) | Same | Cross-gap step vs outcome, bracket stats |
| `diag_splice_timescale` | `corpus.json` geometry + media | Uniqueness timescale, level representation (§3.6a) |
| **`diag_splice_dualfit` (proposed)** | geometry + `baseline_lag` + media | **P1, P2, partial P7** |
| Donor-interior pass in diagnostic | geometry + media | **P4** before or without full re-scan |
| Re-scan with current binary | Same media | `donor_interior`, `splice`, `wide_envelope`, 1 s `peak_z` |

### Bottom line for sequencing

You do **not** have to wait for §4 repair to de-risk the idea:

1. **Now (fingerprint only):** P3 — scope dual-fit to bracket-exhausted skips.
2. **Next (fingerprint + media diagnostic):** P1, P2, partial P4/P7 via `diag_splice_dualfit` + donor RMS pass.
3. **Then:** Re-scan for frozen capture fields (`donor_interior`, 1 s uniqueness).
4. **Last:** §4 repair prototype on skips that pass the offline simulation.

---

## Fingerprint implementation audit (2026-06-30)

Code review of `application/gap_fingerprint.rs` and the `--gap-fingerprints` bin path
(`characterize_gaps` → `characterize_gaps_with_gate`). This records **bugs, smells, and speed**
concerns for the capture layer that feeds §3 / proof items — not a verdict on the dual-fit theory.

**Capture status:** `donor_interior`, `splice`, `wide_envelope`, 1 s `peak_z` / `prominence`, and
energy-weighted seam level are **implemented in the gate path**; committed `gap-files/` corpora still
predate most of that until re-scan. The harness projects the new fields and gates on `peak_z` +
prominence when present (legacy `second_peak_r` margin fallback otherwise).

**Uniqueness model gap (intentionally deferred):** capture still summarizes lag curves as **top-2
scalars** (`peak_z` over the whole curve, `prominence` = #1−#2, `top2_spacing_ms`) — not the §3.2
**top-K peak list** or time-range periodicity structure. `diag_splice_timescale` finds K local maxima
but only reports #1 vs #2 stats; the corpus cannot reconstruct K-peak uniqueness from JSON alone.
Leave as-is for now; see §3.2 / harness when promoting multi-peak capture.

### Correctness risks

| # | Issue | Why it matters |
|---|--------|----------------|
| **F1** | **Two throat placements** | Gate seam scores come from `oracle_score_fit_candidate` on `FitHaystackCache`; `baseline_lag`, `seam_probe`, `wide_envelope`, `donor_interior`, and `splice` are measured at a **separate** `place_on_b` in `gap_fingerprint.rs`. Different config paths (`FingerprintConfig` vs `SeamGateConfig`). If placements diverge, authoritative seam scores and registration metrics describe **different B offsets**. |
| **F2** | **Gate brackets drop structure** | Per-bracket `structure_pre` / `structure_post` are always `None` in the gate-written `brackets[]`; only oracle seam scores + `failure_stage` are filled. Misleading vs schema/docs that imply full bracket scoring. |
| **F3** | **Stub `GateOutcome`** | `seam_shape`, `fit_path`, `signature_mode` are empty strings in the gate path. Fine if diagnostic-only; misleading if read as production-parity tags. |

**F1 is the highest-priority fix:** take `structure_start_frame` (and selected channels) from the
zero-move oracle outcome and run lag / probe / envelope / donor there; remove the redundant throat
`place_on_b`.

### Smells (not necessarily wrong today)

| Item | Detail |
|------|--------|
| Double summary build | `characterize_gaps_with_gate` calls `characterize_gaps` → `build_gap_fingerprint(Summary)`, which already runs `place_on_b` for baseline structure/seam; the gate pass refines, re-enumerates anchors, oracle-scores every bracket, and calls `place_on_b` again. |
| Full-tier builder ≠ bin path | `build_gap_fingerprint(Full)` uses `classify_bracket_stage`, not oracle; production fingerprints always go through `characterize_gaps_with_gate`. |
| Harness `uniqueness_z` with partial `splice` | If `splice` carries `peak_z` on only one side, the harness uses that single value as the gap minimum — slightly optimistic vs requiring both sides. |
| Wide-envelope test | `prominence >= 0.0` is vacuous (prominence is `peak − rival` or `0.0` by construction). |
| Docs drift | `gap-fingerprint.md` omits `baseline_lag`, `seam_probe`, `splice`, `wide_envelope`, `donor_interior`; dualfit plan §3 checkboxes still show wide-envelope / harness as open. |

### Speed — where time goes

Per gap (rough order, gate path):

1. **Oracle bracket loop** — N × `evaluate_seam_gate_fit_candidate` (unified structure search). Required for authoritative `failure_stage`; hard to skip.
2. **`lag_at_placement` × 2** (throat + best-energy bracket) — **largest avoidable cost**: 1 s window @ 48 kHz ≈ 48k samples × ~401 lags × pre/post × (mono + optional selected channel) × `normalized_correlation` per lag.
3. **`seam_probe_at_placement`** — ±25 ms sweep + bandlimited + spectrum per side.
4. **`wide_envelope_at_placement`** — 2 s window, 100 ms envelope bins, envelope lag sweep (cheaper than fine lag).
5. **Duplicate Summary `place_on_b`** — extra unified search before gate overlay.
6. **Per-gap `interleaved_to_mono` / `interleaved_to_channels`** — realloc each gap.
7. **`decode_ab` again** in `composition.rs` `dump_gap_fingerprints` after repair/scan — full A/B decode duplicated for fingerprinting.

### Speed — recommended optimizations (by impact)

| Priority | Change | Impact | Risk |
|----------|--------|--------|------|
| 1 | **Reuse oracle throat frame** for all decision-seam metrics (fixes F1) | Correctness + drops one `place_on_b` | Medium — verify frame parity with gate |
| 2 | **Skip Summary `place_on_b`** when gate overlay follows; build A-only intrinsic fields in summary | One fewer unified search per gap | Low |
| 3 | **Share one border extract** at throat for lag + `seam_probe` + `wide_envelope` | Less redundant template work | Low |
| 4 | **Reuse decode** from repair/scan in `dump_gap_fingerprints` when PCM already in memory | Saves full-file decode | Low |
| 5 | **Coarse-to-fine or FFT lag** on 1 s window | Large CPU cut on rescans | Medium — must match current peaks |
| 6 | Optional flags: skip diagnostic `lag` (best-energy bracket), skip `wide_envelope` / residual on quick rescans | Linear savings | Product decision |

**Do not optimize first:** `donor_interior` RMS pass; parallel per-gap loops before removing duplicate search and aligning placement.

### Relation to proof items

| Proof item | Audit implication |
|------------|-------------------|
| **P1 / P2** (`diag_splice_dualfit`) | Simulation should use the **same B placement the gate used**, not a second `place_on_b` read — until F1 is fixed, offline sim may disagree with bracket seam scores on the same gap. |
| **P3** | Bracket oracle path is authoritative; structure columns missing (F2) does not block bracket-exhaustion analysis. |
| **P4** | `donor_interior` is coded; re-scan or geometry+media diagnostic still needed for on-disk corpora (C7). |
| Re-scan | Land F1/F2 fixes **before** a long corpus re-scan if possible — otherwise new fields may sit at the wrong placement. |

---

### S6. Characterize skip sub-types

The 6 skips are not homogeneous. Suggested buckets for the plan:

| Skip | Step | Uniqueness | Best bracket | Likely blocker |
|------|------|------------|--------------|----------------|
| 1·g19 | +72 ms | splice (0.50) | 0.07 | No lag-0 compromise; quiet post on mono |
| 1·g22 | +12 ms | splice (0.31) | — | Small step, still no bracket |
| 6·g6 | +54 ms | splice (0.46) | — | Large step, no bracket |
| 1·g3 | +10 ms | alias (0.14) | — | Uniqueness thin at 250 ms (may clear at 1 s) |
| 2·g1 | +13 ms | alias (0.16) | — | No bracket; decent SNR |
| 5·g6 | −32 ms | alias (0.19) | 0.08 | Post lag outside ±25 ms probe |

Dual-fit priority: start with the three `splice`-class skips after P1; treat `alias-suspect` as pending
§3 re-scan with 1 s `peak_z`.

---

### S7. Clarify §3.6 vs §3.6a status

§3.6 still reads as pending ("Needs: the A/B media paths") while §3.6a and §5 mark it done. Merge or
strike the pending language in §3.6 so the document has one timeline.

---

### S8. Note patched gaps that fail strict uniqueness

5·g3 patches with uniqueness margin **0.02** — if dual-fit detect requires `both_sides_recoverable` under
the old metric, it would never run on gaps that already patch fine. The new 1 s `peak_z` thresholds should
be calibrated on **both** patched and skipped distributions, not skips alone.

---

## What must be true for this to be a useful implementation

1. **Repair works on skips** — P1 above; offline `diag_splice_dualfit` can de-risk before §4 is wired.
2. **Dual-fit is scoped to bracket failures** — not all stepped gaps (S4); **P3 is answerable from
   fingerprints now**.
3. **Donor continuity measured** — `donor_interior` on re-scan, or geometry + B decode diagnostic (P4).
4. **Uniqueness at decision timescale** — 1 s `peak_z` / prominence in capture, not 250 ms `second_peak_r`.
5. **Level on energy-weighted downmix** — so detect doesn't false-flag center-dominant 5.1 (§3.6a).
6. **Listening + decoy checks** — gate pass is necessary, not sufficient (P6, P7).

---

## Minor nits

- §4.1 references "§3.4" for donor-continuity; there is no §3.4 in the plan (should be §3 item 1).
- §4.1 references "§3.2" for detect; §3.2 is the dual-scale uniqueness bullet list — detect criteria
  should point to frozen thresholds in §3.6a or a new §4 detect table.
- `fill-fitting.md` is in the reading list but may not exist at that path (verify or fix link).

---

## Recommended next steps (unchanged in spirit, refined in scope)

1. **P3 from fingerprints now** — run `diag_fingerprint_corpus`, export CSV; confirm decision tree
   (bracket pass ⇒ patch; else dual-fit candidate).
2. **P1/P2 offline** — implement `diag_splice_dualfit` (geometry + `baseline_lag` + media); run on the
   six bracket-exhausted skips before §4 repair.
3. **P4** — donor-interior diagnostic pass on skip targets (or re-scan once binary is current).
4. Re-scan with frozen §3.6a capture (`donor_interior`, 1 s `peak_z`, energy-weighted level).
5. Prototype §4 **only on skips where offline simulation passes** (not high-step patches like 5·g3).
6. Add S1/S4 content to the plan when editing resumes (or keep this review as the authoritative
   bracket-vs-step and fingerprint-coverage analysis).
