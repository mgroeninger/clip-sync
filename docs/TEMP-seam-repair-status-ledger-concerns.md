# Seam-repair concerns — fingerprint / identification / fix pipeline audit

**Purpose.** Companion to [TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md). The ledger
indexes claims and proof status; **this doc records bugs, gaps, regressions, and footguns** found in a
read-only review of the repair tool code (fingerprint capture → corpus analyzer → identification/fix path),
especially after bumping `lag_max_lag_ms` to 600 ms.

**Related:** [TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md) §0 (dual-fit theory),
[gap-fingerprint.md](gap-fingerprint.md).

**Status:** Registration fix **IMPLEMENTED** (2026-06-30) — sequential per-shoulder post centering in
`gap_fingerprint.rs`; P2-1/P2-2 harness fixes; `lag_pair_sequential_decouples_pre_offset_from_bridge_mismatch`
regression test. **Next:** full corpus rescan on real media → triage §Open smells → calibrate thresholds (A5)
→ wire §4 repair (A3). Update ledger/plan docs after rescan, not before.

---

## Executive summary

**Fixed (2026-06-30):** sequential post centering in `lag_pair` decouples `L_pre` from `(D_B − D_A)` in the
post search window; `seam_probe` / `wide_envelope` / `donor_interior` follow the same `pre_shift` on the
gate fingerprint path. Regression test proves naive centering fails while sequential succeeds on synthetic
stacked-lag geometry.

**Still true (by design / unchanged):**

- `GapGeometry` remains a **gross map** — one `gap_offset_secs` on both shoulders; `b_mapped_end = start + D_A`.
  Sequential registration recovers the step via lag search, not independent scan endpoints (P0-3).
- **`patch_audio` / gate** unchanged — fingerprint measures; repair still single-placement until A3.
- On-disk `gap-files/` predates this capture — **rescan required** before trusting splice / dualfit stats.
- Ledger B2/B13/C1–C4 remain **overstated** until post-rescan validation.

**Highest-value next step:** **rescan** the 6-pair cohort → read `splice_text` / `dualfit_candidate` → triage
§Open smells from rescan data → then threshold calibration (A5) and §4 repair (A3).

---

## Evaluation cohort (unchanged from ledger)

```text
Primary dual-fit cohort: 6-pair corpus — 19 matched, 6 skipped (6 bracket-exhausted skips = dual-fit targets).
On-disk gap-files/: dirs 1–7 largely predate b_mapped capture (A2) — treat analyzer conclusions as unreliable
  until rescan with a fixed registration model.
```

---

## P0 — Root registration model

| # | Concern | Status | Notes |
|---|---------|--------|-------|
| P0-1 | Post lag stacked `\|L_pre + (D_B − D_A)\|` in one search | **FIXED** | Sequential `lag_pair` (2026-06-30); test `lag_pair_sequential_decouples_pre_offset_from_bridge_mismatch` |
| P0-2 | `lag_pair` post center at `S + D_A` without pre-shift | **FIXED** | Post center now `S + D_A + round(L_pre)` |
| P0-3 | Scan has no independent B gap endpoints | **OPEN (by design)** | Gross map + lag search; no scan change planned. Coarse post placement (§Open R9) if `\|D_B − D_A\| > 600 ms` |
| P0-4 | Diagnostics looked independent but shared rigid geometry | **PARTIAL** | `baseline_lag` fixed; `diag_splice_dualfit` still reads gross `b_mapped_*` anchors + lags — correct *if* capture found peaks |
| P0-5 | Ledger overstates proof | **OPEN** | Reframe after rescan (§Ledger rows) |

### Historical: what ±600 ms alone did / did not do (pre-sequential-fix)

- Widening helped when `\|L_pre + (D_B − D_A)\| ≤ 600 ms` but did **not** decouple stacked lag.
- Sequential fix addresses the decoupling; 600 ms remains the post fine-search half-width.

### Rescan timing

| Goal | Action |
|------|--------|
| Validate sequential fix on real gaps | **Do now** — primary 6-pair rescan |
| Threshold calibration (A5) | **After** rescan |
| Coarse post placement (R9) | **Only if** rescan shows edge-pinned post peaks or `\|step\|` near 600 ms |
| §4 repair (A3) | **After** trustworthy `baseline_lag` + dualfit diag (C3/C7) |

---

## P1 — Self-inflicted measurement contradictions

| # | Concern | Status | When to address |
|---|---------|--------|-----------------|
| P1-1 | Three lag widths (`baseline_lag` ±600, `seam_probe` pre ±25 / post ±600, `wide_envelope` pre ±400 / post ±600) | **PARTIAL** | Raise pre `seam_probe` to ±600 **before rescan** only if you will read `seam_probe_text` during triage; else **ignore `seam_diag`**, use `splice_diag` |
| P1-2 | Stale `patch_region` comment (throat vs `b_mapped`) | **OPEN** | Anytime — doc-only; prevents comparing wrong fields |
| P1-3 | `donor_interior` used naive A-length span | **FIXED** | Gate path uses aligned `[b_pre, b_post]` shoulders |
| P1-4 | `splice` step GIGO when post peak missed | **OPEN** | Add edge-pinned / search-exhausted flag during **A5** if rescan shows borderline peaks |
| P1-5 | No edge-pinned detection in capture/analyzer | **OPEN** | **A5** calibration; flag `\|frac_lag_ms\| ≈ lag_max_lag_ms` in harness |

---

## P2 — Harness / analyzer footguns

| # | Concern | Status | When to address |
|---|---------|--------|-----------------|
| P2-1 | `gap_row` `.first()` vs mono | **FIXED** | `mono_entry()` by `LagChannel::Mono` |
| P2-2 | Stale ±200 ms harness copy | **FIXED** | User-facing reports now say ±600 ms + sequential |
| P2-3 | `verdict` / `skew` use one side | **OPEN** | Low priority; prefer `splice` / `seam_step_ms`. Fix if headline `verdict` column misleads after rescan |
| P2-4 | `dualfit_candidate` narrow | **OPEN (intentional?)** | Revisit after rescan if `AliasSuspect` skips look recoverable in dualfit diag |
| P2-5 | Fallback to legacy `gap.lag` | **OPEN** | Only when mixing pre-A2 JSON in one report; exclude old files from rescan analysis |
| P2-6 | `both_sides_recoverable` requires exact `Splice` | **OPEN** | Same as P2-4 — after rescan |

---

## P3 — Repair path (`patch_audio`) — unchanged by capture fix

| # | Concern | When to address |
|---|---------|-----------------|
| P3-1 | Repair B mapping still one offset both shoulders | **A3** — dual-fit repair consumes `baseline_lag` lags |
| P3-2 | `fill_length_slack` ≠ dual-fit | **A3** |
| P3-3 | Gate brackets vs registration metrics differ | **Always** — compare consciously during investigation |
| P3-4 | `gap_offset` from unrefined A start | **If** rescan shows systematic sub-ms placement bias |
| P3-5 | Haystack decode vs search center | **FIXED** for post search (sequential); gross map unchanged |

---

## Open smells — post-implementation triage

Items from the implementation review (2026-06-30). **Not a pre-rescan backlog** — use rescan results to
decide which matter.

| ID | Smell | Severity | When to address |
|----|-------|----------|-----------------|
| **R1** | `lag_pair` regression test | — | **Done** — `lag_pair_sequential_decouples_pre_offset_from_bridge_mismatch` |
| **R2** | Post `lag0_r` / `LagVerdict` wrong when gross-shifted curve omits lag 0 | Low | After rescan **if** using `gap_row.verdict` / skew; `splice_diag` unaffected |
| **R3** | Pre `seam_probe` still ±25 ms; post ±600 ms | Med | Before rescan **only if** reading `seam_probe_text`; else defer |
| **R4** | Pre `wide_envelope` still ±400 ms; post ±600 ms | Low | After rescan if `wide_env_agrees` pre-side disagrees often |
| **R5** | `build_gap_fingerprint` Full tier omits `seam_probe` / `donor_interior` / `splice` (gate path only) | Low | When using non-gate builder path; add comment or unify |
| **R6** | Diagnostic `fp.lag` at structure throat ≠ `baseline_lag` at `b_mapped` | Low | **Never compare** — doc/comment only |
| **R7** | `donor_interior` falls back to naive A-length span when post peak missing | Low | After rescan if post-miss gaps still matter for C4/A4 |
| **R8** | `round(L_pre)` for centering vs fractional gross relabel | Nit | Only if rescan shows systematic bias |
| **R9** | Coarse post placement not implemented | Med | **If** rescan shows post peaks pinned at ±600 ms or `\|D_B − D_A\| > 600 ms` |
| **R10** | Stale ±200 ms in dualfit plan / ledger / vocab plan | Low | **After** rescan confirms sequential + 600 ms on cohort |
| **R11** | No edge-pinned flag in capture/harness | Med | **A5** threshold calibration |
| **R12** | Plan/ledger still say A2 “DONE” without sequential fix called out | Low | Update ledger B13/A2 wording post-rescan |

### What not to do before rescan

- Do not implement R9 (coarse post) speculatively.
- Do not recalibrate `peak_z` / `SpliceDiag` thresholds (A5).
- Do not wire §4 repair (A3).
- Do not update ledger rows to PROVEN.

### Optional pre-rescan slice (only if manual triage needs it)

- **R3:** align pre `seam_probe` to `lag_max_lag_ms` so `seam_probe_text` agrees with `splice_text`.

---

## Ledger rows affected (recommended reframe)

| Ledger # | Current | Suggested reframe |
|----------|---------|-------------------|
| **B2** | PROVEN — no genuine cross-encoding; one-sided-dead is placement artifact | **SUPP** — true for spot-checked pairs under widened manual search; not proven corpus-wide at production capture |
| **B13** | PROVEN — `b_mapped` + ±200 ms lag search | **PARTIAL → rescan** — sequential centering landed; prove on full cohort at ±600 ms |
| **A2** | DONE (CAP) | **PARTIAL → rescan** — `b_mapped` + sequential registration in code; corpus not re-measured |
| **C1/C2/C4** | PROVEN (spot-checks) | **SUPP** — conditional on manual ±600 ms / cherry-picked gaps |
| **A3** | OPEN (unbuilt) | Unchanged — after rescan + C3/C7 dualfit diag |

---

## Investigation hygiene (post-sequential-fix)

1. **Rescan first** — on-disk `gap-files/` predates sequential capture.
2. **Prefer** `baseline_lag` + `splice` + `splice_diag` over `seam_probe` / `seam_diag` for classification.
3. **Spot-checks:** `diag_splice_timescale` with `SPLICE_EXP_FINE_LAG_MS=600`; watch for edge-pinned post lags.
4. **Run** `diag_splice_dualfit` on bracket-exhausted skips after rescan (C3/C7).
5. **Compare fields consciously:** `residual` @ gate throat; `baseline_lag` / `splice` @ `b_mapped`; do not use `fp.lag` vs `baseline_lag` interchangeably.

---

## Registration fix — design (IMPLEMENTED 2026-06-30)

**What “fix registration” means.** Dual-fit theory (dualfit plan §0) requires measuring `(L_pre, L_post)`
such that each shoulder aligns independently and the B **bridge** between aligned shoulders may differ in
length from A's hole. Capture must **decouple clip offset (pre)** from **bridge-length mismatch (post)** in
the search geometry — not only widen `lag_max_lag_ms`.

**Note on `b_mapped_end`.** `b_mapped_end` and `start + D_A` are the same frame under the gross map. The
bug is not “end is derived from start” alone — it is that post fine-lag search at `S + D_A` forces
`|L_pre + (D_B − D_A)|` into a single ±window. Sequential centering (below) fixes that without new scan
fields.

### Recommended fix: sequential per-shoulder registration

Minimal change; matches dualfit plan §4 and `diag_splice_dualfit`; `GapGeometry` stays the **gross map**.

```text
Notation:  S = b_mapped_start (haystack frame),  D_A = A gap frames,  D_B = true B bridge frames,
          L_pre = pre lag,  L_post_fine = post lag at shifted center.

1. Pre (unchanged)
   Center:  S  (= b_mapped_start)
   Search:  ± lag_max_lag_ms
   → L_pre

2. Post (fixed)
   Center:  S + D_A + round(L_pre)     // pre-aligned nominal post — NOT S + D_A alone
   Search:  ± lag_max_lag_ms
   → L_post_fine  ≈ (D_B − D_A)       // bridge step only; L_pre no longer stacked into post search

3. Serialize gross lags (JSON / diag_splice_dualfit compatibility)
   L_pre_gross   = L_pre
   L_post_gross  = L_pre + L_post_fine    // lag relative to geometry.b_mapped_end

   Aligned B positions (repair + diag):
   b_pre_aligned  = b_mapped_start + L_pre_gross
   b_post_aligned = b_mapped_end   + L_post_gross   // = S + L_pre + D_B in frames

   splice.step_ms = L_post_gross − L_pre_gross ≈ D_B − D_A  (length-reconciliation amount)
```

**Why this works.** True post shoulder is at `S + L_pre + D_B`. Old search at `S + D_A` needs
`L_post_old = L_pre + (D_B − D_A)` inside ±window. New search at `S + D_A + L_pre` needs only
`|D_B − D_A| ≤ lag_max_lag_ms`. Aligned positions are identical when the peak is found; the post
correlation window is centered near the true shoulder.

**Example.** `L_pre = −131 ms`, `D_B − D_A = +322 ms` ⇒ old needs `|191 ms|` in post window; new needs
`|322 ms|`. When `L_pre` and `D_B − D_A` are large with the same sign, old stacks them; new does not.

### Code touchpoints

| Area | Change |
|------|--------|
| `lag_pair` | Split or extend: `lag_pre(...)` + `lag_post(..., post_base_frame)` |
| `lag_at_placement` | Pre first; post with `post_base = start_frame + gap_frames + pre_shift` |
| `splice_summary_from_lag` | Keep `step_ms = post_gross − pre_gross` (≈ bridge-length mismatch after fix) |
| `seam_probe_at_placement` | Same sequential post base; **raise** fine lag to `lag_max_lag_ms` or demote from classification |
| `wide_envelope_at_placement` | Same post base; align `WIDE_ENV_MAX_LAG_MS` with `lag_max_lag_ms` |
| `donor_interior_at` | Span aligned shoulders `[S + L_pre, S + L_pre + D_B]`, not `[S, S + D_A]` |
| `gap_fingerprint_corpus::gap_row` | Use `mono_lag_side` / longest window, not `.first()` |

`GapGeometry` (`b_mapped_*`, `fill_offset_secs`) unchanged — gross map only. Effective registration lives
in `baseline_lag` / `splice` with gross-relative lags as above.

### When sequential is not enough: coarse post placement

If `|D_B − D_A|` alone exceeds `lag_max_lag_ms`, or post peak pins at the window edge:

```text
pre_shift = round(L_pre)
for post_base in (S + pre_shift + D_A − bridge_slack) .. (S + pre_shift + D_A + bridge_slack) step coarse_step:
    score = peak correlation of A_post template vs B around post_base (lag-0 or ±small)
pick best post_base → fine lag sweep ± lag_max_lag_ms
```

`bridge_slack` ≈ max expected trim/pad (e.g. 1–2 s or a fraction of gap length). Independent shoulder
registration without new scan fields.

### What not to do

| Approach | Why skip |
|----------|----------|
| Only bump `lag_max_lag_ms` | Still stacks `L_pre` into post lag requirement |
| Treat `b_mapped_end` as “independent” without sequential centering | `end = start + duration` under one offset — same frame as `S + D_A` |
| Per-gap `video_b_end − video_b_start` from scan | Scan uses one scalar offset for both ends today |
| Move registration back to gate throat | Quiet-gap wander (ledger A1); gate stays for bracket scoring only |
| Outward-anchor as primary (ledger D10) | Pair-6 showed `b_mapped` + correct search geometry is sufficient |

### Repair path (`patch_audio`) — separate from capture fix

Sequential capture fixes **measurement**. Dual-fit **repair** (ledger A3, dualfit plan §4) remains unbuilt:

1. Read `L_pre`, `L_post` from `baseline_lag` (gross-relative).
2. Align shoulders: `b_mapped_start + L_pre`, `b_mapped_end + L_post`.
3. Bridge = B between them; reconcile `len(bridge) − gap_frames`.
4. Re-run the **unchanged** production gate.

Do not change production patch until capture produces trustworthy lags. `refined_b_end = refined_a_end +
gap_offset` in `patch_audio` is acceptable until §4 repair lands.

### Validation

1. ✅ **Unit test** — `lag_pair_sequential_decouples_pre_offset_from_bridge_mismatch`.
2. **Replay** pair-6 / pair-7 one-sided-dead gaps — post-rescan.
3. **Full rescan** 6-pair cohort → `diag_fingerprint_corpus` → `diag_splice_dualfit` on bracket-exhausted skips.
4. **Cross-check** `splice.step_ms` vs `bridge_frames − gap_frames` from dualfit diag.

### Implementation order

1. ✅ Sequential `lag_at_placement` + gross lag conversion (`lag_pair` refactor).
2. ✅ Align `seam_probe`, `wide_envelope`, `donor_interior` to the same post base.
3. ✅ Harness: mono lag selection + update stale ±200 ms copy.
4. **Not done** — optional coarse post placement, only if the rescan below still shows edge-pinned post peaks.
5. **Not done** — **one** full rescan (needs original A/B media, not available in this checkout) → calibrate
   thresholds (A5) → wire §4 repair (A3).

---

## Code reference index

| Topic | File | Lines (approx.) |
|-------|------|-----------------|
| Rigid `GapGeometry` | `crates/clip-sync-repair/src/application/gap_fingerprint.rs` | 1360–1366 |
| `lag_pair` sequential + regression test | same | ~956–1008, test ~2254–2340 |
| `seam_probe` pre ±25 / post ±600 | same | ~1149–1217 |
| `wide_envelope` pre ±400 / post ±600 | same | ~1281–1352 |
| `donor_interior` aligned span | same | ~441–474, ~1873–1881 |
| `lag_max_lag_ms` 600 + comment | same | 703, 1598–1603 |
| Gate path geometry | same | 1709–1726 |
| Stale throat comment | `crates/clip-sync-repair/src/application/patch_region.rs` | 1539–1541 |
| Repair rigid B map | `crates/clip-sync-repair/src/application/patch_audio.rs` | 1457–1458 |
| Harness `mono_entry` | `crates/clip-sync-repair-harness/src/gap_fingerprint_corpus.rs` | ~637–650 |
| `splice_diag` / `dualfit_candidate` | same | 438–481 |
| `diag_splice_dualfit` anchors | `crates/clip-sync-repair/tests/diag_splice_dualfit.rs` | 386–402 |
| `diag_splice_timescale` | `crates/clip-sync-repair/tests/diag_splice_timescale.rs` | 427–428, 82–87 |
