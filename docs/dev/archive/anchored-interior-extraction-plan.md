# Anchored interior extraction (symmetric alignment)

> **Status:** Shipped (2026-06-17). Delivered **combined** with [anchored end extraction](anchored-end-extraction-plan.md). Archived from `docs/dev/archive/TEMP-anchored-interior-extraction-plan.md`.

**Problem:** After anchored end, symmetric runs with `num_clips > 2` still place **interior** windows by dividing **each file’s own** `timeline_end`. On unequal lengths (e.g. 40 min excerpt vs 300 min master), start and end compare the same clock times on both files, but interior clips fingerprint unrelated story moments (e.g. A ≈ 13–27 min vs B ≈ 100–150 min). `offsets_consistent` can fail spuriously; per-clip diagnostics mislead; extra fingerprint work is wasted.

**Goal:** When `num_clips > 2` and paired planning uses **`SharedTimeline`**, subdivide the **shared span** `T_anchor` for interior placement on **both** files — same rule as anchored end uses for the tail. Every extracted clip pair then samples the same nominal timeline coordinates. No prior offset is required.

**Audience:** Low volume today (analyzer default `num_clips = 1`, repair `2`). Value is for forced `--symmetric` with `num_clips ≥ 3` on unequal-length pairs, or future multi-point drift workflows.

**Non-goals (v1):**

- Changing query-reference mode.
- Offset-mapped interior (`+ Δ_start` after start clip) — separate follow-up (leader on long file).
- Changing `offset_drift_secs` (still `Δ_end − Δ_start` only).
- Re-anchoring start clip (already `[0, clip_length)` on both clocks).
- New config knob separate from `end_clip_anchor` — interior follows the same `SharedTimeline` / `FileTail` mode.
- Updating `locate_query_spike.rs` or single-file `clip_windows_with_options` (spike / legacy tests).

**Workspace split:** Interior placement logic in **`domain/policies.rs`** (`clip_windows_paired` + shared helper). No change to **`align_extracted_pair`** merge policy unless overlap omission reduces window count (pairing still by index).

**Prerequisite:** [anchored-end-extraction-plan.md](anchored-end-extraction-plan.md) — `clip_windows_paired`, `extract_clips_at_windows`, pair-collapse, `end_clip_anchor`, and `interior_windows_along_timeline` helper.

**Delivery:** Shipped combined with the end plan (2026-06-17).

---

## Current behaviour (after anchored end — end-only ship)

If end plan shipped **without** combined interior, `SharedTimeline` on unequal lengths looks like:

| Clip | `SharedTimeline` (end-only interim) |
|------|-------------------------------------|
| Start | `[0, clip_length)` on both — OK |
| End | `[T_anchor − clip_length, T_anchor]` on both — OK |
| Interior (`num_clips > 2`) | Still `timeline_end * i / n` per **file** — **broken** |

### Failure modes this plan targets

1. **Spurious `offsets_consistent: false`** — interior offset disagrees because regions differ, not because timeline drift is real.
2. **Misleading per-clip report** — interior row shows confident match on unrelated audio or nonsense offset.
3. **Wasted work** — fingerprint + optional repetition on interior buffers that are not a consistency check.

### What success looks like

40 min / 300 min, `clip_length = 10 min`, `num_clips = 3`, `mode = symmetric`:

| Clip | Today (after anchored end) | After this plan |
|------|---------------------------|-----------------|
| Start | `[0, 10)` both | same |
| Interior | A `[12.5, 22.5)` (~15–25 min), B `[145, 155)` (~150 min) | **both `[15, 25)`** (segment math on `T_anchor = 40`) |
| End | `[30, 40)` both | same |

All three clip offsets should agree within tolerance when audio is identical on `0..40 min`.

---

## Proposed behaviour

### Anchoring rule (interior)

Reuse the existing interior algorithm from `clip_windows_with_options`, but drive subdivision from **`T_anchor`** instead of per-file `timeline_end` when `end_clip_anchor == SharedTimeline` and `effective_num_clips > 2`:

```text
T_anchor = min(timeline_end_a, timeline_end_b)   // same as anchored end
n        = effective_num_clips (pair-aligned; see collapse)

For i in 1 .. n-1:
  seg_start = T_anchor * i / n
  seg_end   = T_anchor * (i + 1) / n
  center    = (seg_start + seg_end) / 2
  window    = [center - clip_length/2, center + clip_length/2]
              clamped to [0, T_anchor]

Emit the same interior windows on both A and B clocks (absolute times).
```

- **`FileTail`:** unchanged — interior still uses each file’s own `timeline_end` (legacy).
- **`num_clips ≤ 2`:** no interior clips — **no code path change**.
- **Equal durations:** `T_anchor ≈ timeline_end_a ≈ timeline_end_b` → bit-identical to today’s per-file interior placement.

### Overlap with start / end windows

When `T_anchor` is small relative to `n * clip_length`, centered interior windows can overlap the fixed start `[0, L]` or end `[T_anchor−L, T_anchor]` buffers.

**Policy (v1):** Before emitting an interior window, if its intersection with the start or end window exceeds **`INTERIOR_OVERLAP_TOLERANCE`** (default **1 s** of shared timeline), **omit that interior clip on both files**. Remaining windows stay index-aligned (start, surviving interior(s), end). Log verbose: `interior clip i omitted (overlaps start/end on short shared span)`.

```rust
// domain/policies.rs — new constant
pub const INTERIOR_OVERLAP_TOLERANCE: Duration = Duration::from_secs(1);
```

| Alternative | Verdict |
|-------------|---------|
| Omit overlapping interiors | **Accepted** — clean indices, honest diagnostics |
| Emit overlapping windows | Wasteful; duplicate audio in fingerprints |
| Fail planning with `EmptyClip` | Too harsh for valid short-span configs |

**Feasibility hint:** Non-overlapping interior requires roughly `T_anchor ≥ n * clip_length` (sufficient, not necessary). Document in verbose output when interiors are dropped.

### Pair-level collapse (unchanged)

Inherited from anchored end — when collapse triggers, no interior clips are planned. No change to collapse rules in this plan.

### Offset / merge policy (unchanged)

- Per-clip offsets remain global: `b_time = a_time + offset_secs`.
- `offsets_consistent` still requires **all** aligned clips within `OFFSET_AGREEMENT_TOLERANCE_SECS` — becomes trustworthy on unequal lengths after this change.
- `offset_drift_secs` still **end − start** only.
- `recommended_offset_secs` / `prefer_start_clip` unchanged.

---

## Decisions

| Topic | Decision |
|-------|----------|
| **When it applies** | `end_clip_anchor == SharedTimeline` AND `effective_num_clips > 2` in `clip_windows_paired` only. |
| **Config** | **No new field** — `alignment.end_clip_anchor` governs end + interior together. Document in `PLAN.md` that `SharedTimeline` means “shared span for all multi-clip windows.” |
| **`FileTail`** | Per-file interior subdivision (today’s behaviour). |
| **Subdivision basis** | `T_anchor` seconds, same formula as `policies.rs` lines 118–124 with `timeline_secs` replaced. |
| **Window identity** | Identical `(start, end, label)` on A and B for each interior index. |
| **Overlap** | Omit interior on both sides when overlap with start/end > 1 s. |
| **Single-file API** | `clip_windows_with_options` unchanged (still per-file `timeline_end` for interior). |
| **Query-reference** | Unaffected. |
| **Verbose** | Log when B interior would have been at file-local fraction but is anchored (e.g. `interior clip at 20:00, not B-local 150:00`). |
| **JSON / human report** | No new fields required; optional verbose CLI lines only. |

### Rejected for v1

| Alternative | Reason |
|-------------|--------|
| Separate `interior_clip_anchor` config | Sprawl; same mental model as end |
| Anchor interior only when lengths differ | Extra branch; equal-length case is identical anyway |
| Offset-mapped interior | Needs start offset first; follow-up |
| Extend `offset_drift` to interior clips | Scope creep; start/end drift signal sufficient for v1 |

---

## API sketch

### Domain (`domain/policies.rs`)

Extract shared helper (used by single-file path and paired path):

```rust
/// Interior windows for segment indices `1..n-1` along `timeline_secs`.
/// Does not include start/end. Caller applies overlap omission against those bounds.
fn interior_windows_along_timeline(
    timeline_secs: f64,
    clip_length: Duration,
    n: u32,
) -> Vec<ClipWindow>;

/// True when `interior` shares more than `tolerance` with `start` or `end`.
fn interior_overlaps_fixed_clip(
    interior: &ClipWindow,
    start: &ClipWindow,
    end: &ClipWindow,
    tolerance: Duration,
) -> bool;
```

**`clip_windows_paired` change** (step 4 in end plan):

```text
4. Start: [0, clip_length) on both.
5. Interior:
   - FileTail: per-file interior_windows_along_timeline(timeline_end_*, …) per file
   - SharedTimeline: interior_windows_along_timeline(T_anchor, …) once → same vec on both
   - Filter out interiors that overlap start/end (both files)
6. End: anchored or file_tail (unchanged from end plan)
```

Ensure `windows_a.len() == windows_b.len()` always.

### Application

No new application entry points if anchored end is shipped — only richer windows from existing `clip_windows_paired` call.

`resolve_mode` Tier 2: window **counts** may decrease when interiors are omitted; paired planner must return matching counts on both sides (already required).

---

## Implementation sequencing

See [end plan — Implementation sequencing](anchored-end-extraction-plan.md#implementation-sequencing).

### Layer impact

| Layer | This plan | Churn if end plan used anti-churn structure? |
|-------|-----------|---------------------------------------------|
| `align_videos.rs` | **None** | No |
| `config.rs` / JSON | Doc only (`SharedTimeline` = whole span) | No |
| `clip_windows_paired` | `SharedTimeline` interior branch + overlap filter | **Minimal** — one branch, not a second pipeline pass |
| `clip_windows_with_options` | Already refactored to helper in end Phase 1 | No |

### Delivery options

| Option | Scope | Interior plan phases |
|--------|-------|---------------------|
| **Combined** (recommended) | End + interior domain in end plan PR | Phase 0–1 **done in end PR**; this doc → Phase 2 integration + Phase 3 docs only |
| **End-only then follow-up** | End plan PR first | Full Phase 0–3 here; flip interior branch in `clip_windows_paired` |

### Combined-ship checklist (end plan implementer)

When landing interior with end, verify:

- [ ] `SharedTimeline` calls `interior_windows_along_timeline(T_anchor, …)` once for both files
- [ ] `interior_overlaps_fixed_clip` + `INTERIOR_OVERLAP_TOLERANCE` applied before assemble
- [ ] Unequal `num_clips = 3` domain test (40/300 interior oracle)
- [ ] Optional `num_clips = 3` integration test on generated chirp pair
- [ ] `PLAN.md`: `SharedTimeline` documents full shared span (not “end only”)

### End-only follow-up checklist

If end ships without interior:

- [ ] Do **not** rely on tests that golden per-file interior on unequal pairs
- [ ] Interior PR only touches `clip_windows_paired` interior step + overlap + tests
- [ ] No `align_videos.rs` changes

---

## Phases

> **If combined with end plan:** Phase 0–1 shipped in [end plan Phase 1](anchored-end-extraction-plan.md#phase-1--paired-clip-planning-domain); Phase 2–3 shipped with end plan Phases 2–4.

### Phase 0 — refactor & oracles

- [x] Extract `interior_windows_along_timeline` from `clip_windows_with_options` (end plan Phase 1).
- [x] Unit tests: helper matches `clip_windows_three_clips_with_interior` for 60 min / 3 clips.

### Phase 1 — paired interior anchoring (domain)

- [x] In `clip_windows_paired` `SharedTimeline` branch: interior from `T_anchor` (end plan Phase 1).
- [x] Overlap omission + `INTERIOR_OVERLAP_TOLERANCE` (end plan Phase 1).
- [x] Unit tests: equal 45/45 parity; 40/300 three-clip shared; pair-collapse; `FileTail` per-file interior.

### Phase 2 — integration

- [x] Integration test: long/short pair, `num_clips = 3` → offsets agree (`symmetric_three_clip_shared_timeline_offsets_consistent`).
- [x] Control: `FileTail` on unequal pair → end disagrees (`symmetric_file_tail_end_clip_disagrees_on_unequal_pair`).

### Phase 3 — docs

- [x] `PLAN.md` clip window policy: `SharedTimeline` covers interior when `num_clips > 2` (end plan Phase 4).
- [x] Archive this plan (combined with end plan, 2026-06-17).
- [x] `BACKLOG.md` follow-up rows for post-v1 items.

---

## Test matrix

| Scenario | FileTail interior | SharedTimeline interior |
|----------|-------------------|-------------------------|
| `60 / 60 min`, `num_clips = 3` | `[25, 35)` both | Same |
| `40 / 300 min`, `num_clips = 3`, same audio 0–40 | A ~mid-short, B ~mid-long | Same absolute interior on both |
| `40 / 300 min`, `mode = auto` | N/A (query-reference) | N/A |
| `num_clips = 2` | No interior | No interior — unchanged |
| `T_anchor < n * L` (overlap) | Per-file omit N/A today | Interiors omitted; start + end only |
| `num_clips = 1` | Unchanged | Unchanged |

---

## Risks

| Risk | Mitigation |
|------|------------|
| Fewer interior clips than `num_clips - 2` on short shared spans | Document; verbose log; start + end still anchor |
| Default behaviour change only for `num_clips > 2` + `SharedTimeline` + unequal length | Rare config; equal length unchanged |
| User expects exactly `num_clips` windows | Log omitted interiors; effective count in verbose plan output |
| Depends on anchored end | End plan must land first (or same PR) |
| Shipped twice in one PR then this doc stale | Archive interior plan when combined; note in end plan Phase 4 |
| End-only ship then interior follow-up | Small domain PR if anti-churn structure used — see [sequencing](#implementation-sequencing) |

---

## Acceptance criteria

1. `clip_windows_paired` + `SharedTimeline` places identical interior absolute times on A and B when `num_clips > 2` and durations differ.
2. Equal-duration pairs: interior windows bit-identical to pre-change `clip_windows_with_options`.
3. `FileTail` restores per-file interior placement.
4. Overlapping interiors omitted symmetrically; window counts always match.
5. Integration test (`num_clips = 3`, symmetric, generated chirp): `offsets_consistent` true on 40/300-style pair with shared audio.
6. `offset_drift_secs` and `recommended_offset_secs` policy unchanged.

**If shipped combined with end plan:** criteria 1–4 satisfied in end plan PR; this document archives after Phase 2 integration + docs.

---

## Follow-up (post-v1)

| Item | Benefit |
|------|---------|
| **Offset-mapped clips** (interior + end on B at `t + Δ_start`) | Correct when longer file has leader silence but shared content length |
| **Interior-aware drift** | e.g. max spread across all clips, or weighted by confidence |
| **Rename config** `end_clip_anchor` → `clip_timeline_anchor` | Clearer naming once interior is included (breaking TOML alias migration) |
