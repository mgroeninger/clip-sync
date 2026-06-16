# Query-reference alignment when the donor (B) is the longer file (archived)

> **Status:** Shipped and archived (2026-06-16) — B0–B4 complete. Follows up [query-reference-alignment-plan.md](query-reference-alignment-plan.md) (Q0–Q4).
>
> **Post-ship (2026-06-16):** Discovery-window rebase — synthetic `ClipMatch` and `discovery_windows` use `winning_window_on_a_timeline` so high-rate/verify hold-out placement sees A time when B is reference. `query_localization.winning_window_*` remain on the reference timeline for diagnostics.

**Problem:** Query-reference localization searches a short *query* fingerprint across a long *reference* timeline — so the **reference must be the longer file and the query the shorter**, by construction. Before this work, `AlignVideos::execute()` hard-wired `reference = A` and only ran query mode when `extent_b.effective() <= extent_a.effective()` (B is the shorter file). When **B is the longer file** (e.g. A = a short clip with a dropout, B = the full event recording), query mode was *selected* by Auto/ratio but then **fell back to symmetric** with a logged note — which failed the way the feature was meant to fix.

**Why it matters:** The repair roles are *target* (A, the file to fix) vs *donor* (B, the source to fix from) — **independent of length**. The donor is often the *longer* file (a full recording is the ideal donor for a short clip). So "B is longer" is a first-class repair scenario, not an edge case.

**Goal:** Let query mode run whenever *either* input is the shorter file, and produce a correct **A/B-framed** result either way. All work is in the **`clip-sync` library**; `clip-sync-repair` consumes the result unchanged.

---

## Key insight

The *search* (`locate_query_in_reference`) is already orientation-neutral — it slides the shorter file across the longer one and reports an anchor on the **reference** timeline. Only the *framing* of the result (offset sign + which span is A vs B) depends on whether the longer file is A or B. So this is a **result re-mapping** problem, not a new search algorithm.

### The load-bearing math

Let `anchor_ref` = position on the **longer (reference)** file where the shorter (query) file's `t = 0` aligns, `qdur` = query duration. Offset convention unchanged: `b = a + offset`.

| | Longer = **A** (today) | Longer = **B** (new) |
|---|---|---|
| query (shorter) is | B | A |
| `recommended_offset_secs` | `-anchor_ref` | `+anchor_ref` |
| `mapped_region.video_a` (A span) | `[anchor_ref, anchor_ref+qdur] ∩ A` | `[0, qdur] ∩ A` (all of the short A) |
| `mapped_region.video_b` (B span) | `[0, qdur] ∩ B` | `[anchor_ref, anchor_ref+qdur] ∩ B` |

So B-longer = **negate the offset** and **swap which timeline gets the `[anchor, anchor+qdur]` range vs the `[0, qdur]` range**. `compute_mapped_region` already produces "video_a = reference span, video_b = query span"; for B-longer, call it with B as the reference and then **swap a↔b** in the resulting `TimelineOverlap`.

### What stage 2 (repair) consumes — confirmed orientation-neutral

| Consumer | Reads | Correct for B-longer because |
|----------|-------|------------------------------|
| Gap → donor mapping (`scan_gaps.rs:160`) | `recommended_offset_secs` as `b = a + offset` | `+anchor_ref` maps A-gap `t` → B `t+anchor_ref`; `b_start ≥ 0` guard satisfied |
| Mapped-region gate (`gap_outside_reference_coverage`) | `overlap.video_a_*` (A span) | `video_a = [0,qdur]∩A` ⇒ all of short A covered ⇒ all gaps fillable |
| Silence cross-check (`check_gap_offset_agreement_in_overlap`) | `overlap` (A/B) + offset | uses `video_a` for A, `video_b` for B; A/B-framed |

**Single point of failure:** correctness hinges entirely on the library emitting (a) the right offset **sign** and (b) the right A/B **span assignment**. A wrong sign would make repair silently pull donor audio from the wrong part of B and splice plausible-but-wrong audio with no error. Hence the explicit `b = a + offset` assertions (both orientations) and a *content*-checking repair integration test are required, not optional.

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Where the work lives** | `clip-sync` library only (`align_videos.rs`, `query_localization.rs`). `clip-sync-repair` unchanged. |
| **Reference selection** | `reference = longer effective duration`, `query = shorter`, regardless of A/B. Tie (equal effective durations) → `reference_is_a = true` (deterministic). |
| **Gating** | Run query mode whenever the resolved mode is `QueryReference` (drop the `extent_b <= extent_a` guard and the "video A is shorter; using symmetric" fallback). |
| **Offset** | Store `recommended_offset_secs` **explicitly** on `QueryLocalization` (set per-orientation), instead of deriving it as `-anchor_a_secs`. Domain-only field; the serialized offset stays the top-level `AlignmentReport.recommended_offset_secs`. |
| **Source of truth** | `mapped_region` is the **single source of truth** for placement; `clip_on_a_*` / `clip_on_b_*` and `anchor_a_secs` are **derived** in `from_reference_outcome`. |
| **`anchor_a_secs`** | Derive from `mapped_region`: longer-file anchor (= `video_a_start` when A is reference, else `video_b_start`). |
| **Synthetic `Start` clip** | `query_localization.winning_window_*` on reference timeline; `ClipMatch` + `discovery_windows` rebased to A via `winning_window_on_a_timeline`. |
| **`skipped()` outcomes** | `recommended_offset_secs: None`; placement fields zeroed. |
| **try_all_tracks** | Query path keeps using the best single track pair (unchanged). |
| **High-rate/verify region placement** | Shipped: hold-outs confined to `mapped_region` via `resolve_holdout_candidates`. |

---

## Implementation phases (all complete)

### Phase B0 — Split "search" from "framing" (lib)

- [x] `locate_query_in_reference` returns `ReferenceLocalizationOutcome`; callers frame via `from_reference_outcome`.

### Phase B1 — Oriented constructor + offset field (lib)

- [x] Stored `recommended_offset_secs`; `from_reference_outcome`; `from_anchor` wrapper; `build_query_alignment_result` reads stored offset.

### Phase B2 — `execute()` picks reference by length (lib)

- [x] Drop `extent_b <= extent_a` guard; session swap when B is reference; remove symmetric fallback log.

### Phase B3 — Tests

- [x] Unit, lib integration, repair integration, corpus oracle split (`expect_anchor_on_reference_secs`).

### Phase B4 — Docs / contract

- [x] `json-output.md`, formatter offset parameter, archive deferrals, B-longer corpus case.

---

## Edge cases

| Case | Handling |
|------|----------|
| Negative anchor after refine | `.max(0.0)` clamp in `compute_mapped_region` covers both sides after the swap |
| Equal effective durations | `reference_is_a = true` (deterministic) |
| Repair gap → donor coordinates | Short A inside long B ⇒ `gap_b = gap_a + offset` |
| High-rate/verify | Discovery windows on A timeline; region-bounded placement inside mapped region still deferred |
| B-long outside-mapped gaps | Entire short A is in mapped region — outside-gap skip path does not apply |

---

## References

- Parent feature: [query-reference-alignment-plan.md](query-reference-alignment-plan.md)
- Localization: `crates/clip-sync/src/application/locate_query.rs`
- Framing: `crates/clip-sync/src/domain/query_localization.rs` (`from_reference_outcome`, `winning_window_on_a_timeline`)
- Mode branch: `crates/clip-sync/src/application/align_videos.rs`
- Repair consumers: `scan_gaps.rs`, `gap.rs`, `cross_check.rs`
