# Temporary plan: query-reference alignment when the donor (B) is the longer file

> **Status:** Not started — **design signed off (A′)**, ready to implement (start at Phase B0). Follows up the shipped query-reference feature ([archive/query-reference-alignment-plan.md](archive/query-reference-alignment-plan.md), Q0–Q4). Archive into that doc (or alongside it) when shipped.

**Problem:** Query-reference localization searches a short *query* fingerprint across a long *reference* timeline — so the **reference must be the longer file and the query the shorter**, by construction. Today `AlignVideos::execute()` hard-wires `reference = A` and only runs query mode when `extent_b.effective() <= extent_a.effective()` (B is the shorter file). When **B is the longer file** (e.g. A = a short clip with a dropout, B = the full event recording), query mode is *selected* by Auto/ratio but then **falls back to symmetric** with a logged note — which fails the way the feature was meant to fix.

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
| **Offset** | Store `recommended_offset_secs` **explicitly** on `QueryLocalization` (set per-orientation), instead of deriving it as `-anchor_a_secs`. This is the load-bearing debt-clearing change: it removes the latent "A is always the reference" invariant that makes the current derivation orientation-fragile. Domain-only field; the serialized offset stays the top-level `AlignmentReport.recommended_offset_secs`. |
| **Source of truth** | `mapped_region` is the **single source of truth** for placement; `clip_on_a_*` / `clip_on_b_*` (already documented aliases) **and `anchor_a_secs`** are **derived from it in one place** (`from_reference_outcome`), never stored independently, so they cannot drift. `mapped_region` stays strictly A/B-oriented (`video_a` = A always); compute via the table (swap a↔b when `!reference_is_a`). |
| **`anchor_a_secs`** | **Derive (not store)** from `mapped_region`, with the meaning "position on the **longer (reference)** file where the short clip's `t = 0` sits" (= `anchor_ref`): `if reference_is_a { mapped_region.video_a_start } else { mapped_region.video_b_start }`. Common case (A long) is unchanged (the A-position, e.g. 2700); B-long case = the B-position. Keeps the JSON key + diagnostic with no duplication/drift cost. See [anchor_a_secs design](#anchor_a_secs-design-decision). |
| **Synthetic `Start` clip** | Window stays on the **reference** (longer file) timeline; it only drives `start_clip()` headline confidence. Human "Match on video A" uses `clip_on_a_*`, so repair-facing output stays correct. High-rate/verify still treat discovery windows as A-timeline (deferred) — when B is reference the synthetic window is on B, so hold-out placement remains a graceful no-op; see [Edge cases](#edge-cases). |
| **`skipped()` outcomes** | `QueryLocalization::skipped` sets `recommended_offset_secs: None` (same as today via `recommended_offset_secs()`); all other placement fields zeroed. No separate skip-path offset derivation. |
| **try_all_tracks** | Query path keeps using the best single track pair (unchanged). |
| **High-rate/verify region placement** | Still deferred (graceful no-op in query mode); orthogonal to orientation. |

---

## anchor_a_secs design decision

`QueryLocalization` today derives the offset as `recommended_offset_secs() = -anchor_a_secs`. That identity only holds when **A is the reference** (offset `= -anchor`). For B-longer the offset is `+anchor`, so the derivation breaks. Untangling it has two parts: **(1)** how to represent the offset, and **(2)** what `anchor_a_secs` means cross-orientation.

### Part 1 — the offset (the actual debt)

The real debt is the *derivation* `offset = -anchor`: it bakes in a hidden "A is always the reference" invariant that silently produces a wrong-sign repair when the assumption breaks. **Fix: store `recommended_offset_secs` explicitly**, set deliberately per orientation. This is not optional — it's the correctness change, independent of how `anchor_a_secs` is handled. It removes the latent invariant and concentrates the sign decision in one tested place (`from_reference_outcome`).

### Part 2 — `anchor_a_secs` representation

With the offset explicit, `anchor_a_secs` is no longer load-bearing; it's a **diagnostic that is fully recoverable** from `mapped_region` (and `recommended_offset_secs`) in either orientation. So the question is only how to keep it without introducing a *new* driftable source of truth.

| Option | What it does | Verdict |
|--------|--------------|---------|
| **A — generalize, store independently** | `anchor_a_secs = anchor_ref` (longer-file anchor), stored | Keeps the diagnostic, but a separately-stored redundant field can drift from `mapped_region`. |
| **B — redefine as `clip_on_a_start`, store** | Always an A-timeline value (2700 / 0) | Honest name, but becomes a permanent duplicate of `clip_on_a_start_secs` — duplicate fields invite drift and signal indecision. |
| **C — rename + add `reference_is_a` to JSON** | `anchor_on_reference_secs` + flag | **Breaks v1 JSON contract** (renamed key) + forces consumers to learn an orientation flag for a mostly-diagnostic field. Overkill. |
| **A′ — generalize, *derive* from `mapped_region`. ✅ Recommended.** | Same meaning as A, but computed in one place, never stored independently | Keeps the JSON key + the "where on the long file" diagnostic with **no duplication and no drift risk**. |

**Recommendation: A′ — derive, don't store.** Compute in `from_reference_outcome`:

```text
anchor_a_secs = if reference_is_a { mapped_region.video_a_start_secs }
               else              { mapped_region.video_b_start_secs }
```

- A-long: `= video_a_start = 2700` (unchanged; corpus passes).
- B-long: `= video_b_start` = the position on the long donor B where the short clip begins.

This is consistent with how the type **already** works — `clip_on_a_*` / `clip_on_b_*` are documented aliases of `mapped_region.video_*`. The type is a denormalized view with `mapped_region` as the single source of truth; every convenience field (`clip_on_*`, `anchor_a_secs`) is derived there, so they physically cannot disagree. Net effect: we clear the dangerous derivation debt (Part 1) **and** avoid leaving a redundant, driftable field behind (Part 2) — and because `anchor_a_secs` is a brand-new field nothing external relies on yet, this is the cheapest moment to get it right.

The only residual is cosmetic: in B-long mode the `_a`-named field carries a B-timeline value. Acceptable, because (1) it only occurs in B-long mode, (2) `clip_on_a_*` fully describes the A-timeline clip position, and (3) `recommended_offset_secs` is the canonical quantity consumers should rely on. Documented in `json-output.md` and human formatters (see Phase B4).

> **✅ Signed off (A′):** store `recommended_offset_secs` explicitly; derive `anchor_a_secs` from `mapped_region` (longer-file anchor) rather than storing it independently. (A stores redundantly; B duplicates `clip_on_a_start`; C breaks the contract.) Downstream is mechanical — proceed to Phase B0.

---

## Implementation phases

### Phase B0 — Split "search" from "framing" (lib)

- [x] Refactor `locate_query_in_reference` to return an orientation-neutral outcome — `{ anchor_ref_secs, winning_window_start/end_secs, confidence, ambiguous, windows_scored, search_stride_secs, query_duration_secs, skip_reason }` — all in reference-timeline terms. Rename internal `Candidate.anchor_a_secs` → `anchor_ref_secs` while touching this code. It no longer builds `QueryLocalization` directly; wire through `from_reference_outcome(..., reference_is_a: true, ...)` at the call site so A-reference behavior stays identical.

### Phase B1 — Oriented constructor + offset field (lib)

- [ ] Add stored `recommended_offset_secs: Option<f64>` to `QueryLocalization` (per [Part 1](#part-1--the-offset-the-actual-debt) of the anchor design — explicit per-orientation offset, independent of `anchor_a_secs` handling); `recommended_offset_secs()` returns it.
- [ ] `QueryLocalization::from_reference_outcome(outcome, reference_is_a, extent_a, extent_b)` — `mapped_region` is the single source of truth; all convenience fields derive from it here:
  - `mapped_region` per the table (call `compute_mapped_region` with the reference as its first extent; swap a↔b when `!reference_is_a` — small `swap_timeline_overlap_a_b` helper or inline swap in this one place);
  - `recommended_offset_secs = if reference_is_a { -anchor } else { +anchor }` (stored explicitly as `Some(...)`);
  - `anchor_a_secs = if reference_is_a { mapped_region.video_a_start_secs } else { mapped_region.video_b_start_secs }` (derived at construction from `mapped_region`, not passed in from search);
  - `clip_on_*` from the (possibly swapped) mapped region; `winning_window_*` on the reference timeline.
- [ ] Update `QueryLocalization::skipped` to set `recommended_offset_secs: None` (and keep placement fields zeroed) — no `-anchor` derivation on the skip path.
- [ ] Keep `from_anchor` as a thin wrapper (`reference_is_a = true`) so existing unit tests don't churn.
- [ ] `build_query_alignment_result` reads the stored `recommended_offset_secs` (not `-anchor`).

### Phase B2 — `execute()` picks reference by length (lib)

- [ ] In `align_videos.rs`, drop the `extent_b <= extent_a` guard; run query mode whenever resolved mode is `QueryReference`.
- [ ] `align_query_reference` chooses `reference`/`query` = longer/shorter of (A, B), sets `reference_is_a = extent_a.effective() >= extent_b.effective()` (tie → A is reference), threads `reference_is_a` into `from_reference_outcome`.
- [ ] **Session argument order:** pass the longer file's session/track/extent as `locate_query_in_reference`'s `reference` args and the shorter as `query`. Concretely, when B is longer:
  ```text
  locate_query_in_reference(session_b, &track_b, session_a, &track_a, extent_b, extent_a, …)
  ```
  When A is longer, keep today's argument order unchanged. `AlignmentOutcome` still carries `(track_a, track_b, extent_a, extent_b)` in A/B repair roles — only the localization call swaps.
- [ ] Remove the "video A is shorter; using symmetric" fallback log.

### Phase B3 — Tests

- [ ] **Unit (no chromaprint):** `from_reference_outcome` with `reference_is_a = false` → assert `recommended_offset = +anchor`, `mapped_region.video_a = [0,qdur]∩A`, `video_b = [anchor,anchor+qdur]∩B`. Explicit `b = a + offset` check at the anchor for **both** orientations. Mirror with `reference_is_a = true` to lock A-long framing unchanged.
- [ ] **Library integration (chirp, bounded):** localize a short **A** inside a long **B** (mirror of the A-long test); assert offset sign + A/B spans.
- [ ] **Repair integration (headline capability):** new fixture in `query_reference_integration.rs` — short **A** with gap, long **B** donor (mirror of today's A-long fixture) → query mode runs, gap in A fillable from B, **patched with correct audio** (verify content/correlation or donor-sample match, not just `GapPatchStatus::Patched`).
- [ ] **Regression:** existing A-long corpus oracle + repair integration tests stay green unchanged.
- [ ] **Corpus oracle split (if adding B-long case):** today `expect_clip_on_a_start_secs` is asserted against *both* `clip_on_a_start_secs` and `anchor_a_secs` (`corpus_fixtures.rs`). After A′ those diverge when B is reference (`clip_on_a_start = 0`, `anchor_a_secs` = position on B). Add optional manifest field `expect_anchor_on_reference_secs`; when present, assert `anchor_a_secs` against it; always assert `clip_on_a_start_secs` against `expect_clip_on_a_start_secs`. A-long cases omit the new field (both expectations equal today).

### Phase B4 — Docs / contract

- [ ] `docs/json-output.md`:
  - `anchor_a_secs` → "position on the **longer (reference)** file where the short clip's `t = 0` aligns" (not always A-timeline); drop `recommended_offset_secs = -anchor_a_secs` identity.
  - `clip_on_a_*` / `clip_on_b_*` → always A/B timelines regardless of which file was reference.
  - `winning_window_*` → reference-timeline bounds (A when A is longer, B when B is longer) — not always A-timeline.
  - No field added/removed → additive; golden fixtures unchanged.
- [ ] `crates/clip-sync/src/application/report.rs` — `format_query_localization_lines`: add a `recommended_offset_secs: Option<f64>` parameter (from parent `AlignmentReport` / domain `QueryLocalization`); diagnostic offset line uses it, **not** `-loc.anchor_a_secs` (wrong sign/meaning when B is reference). Update call sites in `clip-sync-cli` and `clip-sync-repair` output formatters.
- [ ] Flip the two open deferral bullets in [archive/query-reference-alignment-plan.md](archive/query-reference-alignment-plan.md) (A-as-query orientation) from open to done.
- [ ] Optional: add B-longer generated corpus case `wav_query_reference_b_longer_anchor` (mirror `wav_query_reference_45min_anchor` with A/B swapped in the generator; `expect_clip_on_a_start_secs = 0`, `expect_anchor_on_reference_secs = 2700`, `expected_offset_secs = +2700`).

---

## Edge cases

| Case | Handling |
|------|----------|
| Negative anchor after refine (short A starts just before B's first sample) | Existing `.max(0.0)` clamp in `compute_mapped_region` covers both sides after the swap |
| Equal effective durations | `reference_is_a = true` (deterministic); query mode on equal lengths is unusual but defined |
| `try_all_tracks` | Best single track pair (unchanged) |
| Repair gap → donor coordinates | Short A sits entirely inside long B ⇒ `gap_b = gap_a + (+anchor) ≥ 0` always valid; assert in integration test |
| High-rate/verify | Still graceful no-op in query mode (deferred, orthogonal) |
| Synthetic `Start` clip on B timeline | When B is reference, `winning_window_*` / synthetic `ClipMatch` windows sit on **B**, but high-rate/verify decode hold-outs on **A** — hold-out candidates empty → graceful skip (same as today's A-long query path with negative offset). Repair does not use `start_clip()` windows. Do **not** remap the synthetic clip to A-timeline in B2; fixing discovery-window placement is the separate high-rate deferral. |

---

## Out of scope

- Region-bounded hold-out placement for high-rate/verify in query mode (separate deferral).
- Multiple disjoint donor regions / multi-anchor donors.

---

## References

- Shipped feature + deferral notes: [archive/query-reference-alignment-plan.md](archive/query-reference-alignment-plan.md)
- Localization core: `crates/clip-sync/src/application/locate_query.rs`
- Result builder + types: `crates/clip-sync/src/domain/query_localization.rs`
- Mode branch: `crates/clip-sync/src/application/align_videos.rs` (`execute`, `align_query_reference`)
- Stage-2 consumers: `crates/clip-sync-repair/src/application/scan_gaps.rs` (`b = a + offset`, line ~160), `crates/clip-sync-repair/src/domain/gap.rs` (`gap_outside_reference_coverage`), `crates/clip-sync-repair/src/domain/cross_check.rs` (`check_gap_offset_agreement_in_overlap`)
- JSON contract + human formatters: [json-output.md](json-output.md), `crates/clip-sync/src/application/report.rs` (`format_query_localization_lines`)
- Corpus oracle: `crates/clip-sync/src/application/testing/corpus_fixtures.rs` (`expect_clip_on_a_start_secs` / new `expect_anchor_on_reference_secs`)
