# Temporary plan: anchored end extraction (symmetric alignment)

> **Status:** Draft (2026-06-17). Motivated by symmetric multi-clip alignment comparing unrelated tail audio when file durations differ (e.g. 40 min excerpt vs 300 min master), and by misleading end-clip drift on near-equal pairs where independent file tails still disagree (recording dropout, edits).
>
> Archive to `docs/archive/anchored-end-extraction-plan.md` when shipped.

**Problem:** Today each file gets its own clip windows from **its own** duration. Start clips are both `[0, clip_length]` — usually fine. End clips are both `[timeline_end − clip_length, timeline_end]` on **native clocks**, so for unequal lengths we fingerprint different story moments (e.g. A `25:00–40:00` vs B `285:00–300:00`). Chromaprint still returns a global `offset_secs`, but the end estimate is not a meaningful check on the start offset.

**Goal:** For symmetric multi-clip alignment (`num_clips ≥ 2`), place the **end** window on both files at the **same nominal timeline coordinates**, anchored on the **shorter effective duration**. No prior offset is required for placement. Compare `Δ_start` and `Δ_end` as an honest drift / consistency signal.

**Non-goals (v1):**

- Changing query-reference mode (short-on-long search).
- Offset-mapped end placement (`B_end = A_end + Δ_start`) — useful follow-up when B has a long leader; not required for the core fix.
- **Required** user-facing scope is anchored **end** only (`num_clips ≥ 2`). Anchored **interior** (`num_clips > 2`) is a [follow-up plan](TEMP-anchored-interior-extraction-plan.md) — may ship in the **same PR** if `clip_windows_paired` is built with the [anti-churn structure](#implementation-sequencing) below (recommended).
- Using anchored end offset for `recommended_offset_secs` when it disagrees with start (start remains primary; see repair policy below).
- Updating `locate_query_spike.rs` — spike intentionally uses independent per-file windows to reproduce clip-count mismatch.

**Workspace split:** Clip planning in **`crates/clip-sync`** (`domain/policies.rs`). Pair extraction / fingerprint loop in **`application/align_videos.rs`**. Config in **`application/config.rs`**. Human report tweaks optional in **`clip-sync-cli`** and **`clip-sync-repair`** output formatters.

---

## Current behaviour (baseline)

| Area | Path | Current state |
|------|------|---------------|
| Per-file windows | `domain/policies.rs` — `clip_windows_with_options` | End clip: `end_start = timeline_end.saturating_sub(clip_length)` per file |
| Extraction | `align_videos.rs` — `extract_clips` | Resolves extent, then calls `clip_windows_with_options` per file |
| End refinement | `align_videos.rs` — `align_extracted_pair` | When start/end agree within 0.5 s, refine end around start prior; else keep independent end estimate |
| Drift | `domain/alignment.rs` — `compute_offset_drift` | `end_offset − start_offset` from clip estimates |
| Repair fill | `build_alignment_result` + repair | `prefer_start_clip: true` when clips disagree |
| Auto mode | `should_use_query_mode` | Short ≪ long → query-reference (avoids broken end clip entirely) |

### Failure modes this plan targets

1. **Unequal length:** excerpt vs full feature — end clips compare unrelated audio.
2. **Equal length, unstable pair:** end clip still uses same wall-clock tail (same placement as today), but drift becomes interpretable as “same region, two estimates” rather than “two file tails.”
3. **Misleading diagnostics:** large `offset_drift_secs` driven by tail mismatch, not true timeline stretch.

---

## Proposed behaviour

### Anchoring rule (v1 — “shared timeline end”)

Given per-file effective timeline ends `T_a`, `T_b` (after `end_tail_inset` / decodable clamp via `effective_timeline_end`):

```
T_anchor = min(T_a, T_b)
end_window = [max(0, T_anchor − clip_length), T_anchor]   // on each file’s clock
start_window = [0, clip_length]                             // unchanged except pair-collapse (below)
```

- **Shorter file** defines where “the end” is.
- **Longer file** extracts the same **absolute** time range, not its file tail.
- When `T_a ≈ T_b`, windows match today’s independent-tail placement (backward compatible).
- When `T_b ≫ T_a`, B’s end clip is **not** B’s file tail.

No start offset is used to choose windows. Fingerprint comparisons yield per-clip `Δ`; drift `Δ_end − Δ_start` is the primary consistency signal (interior clips contribute to `offsets_consistent` but not drift — see [Multi-clip](#multi-clip-num_clips--2)).

### Pair-level collapse (window-count safety)

When `plan.num_clips ≥ 2` but symmetric multi-clip planning cannot produce equal window counts, **collapse both files** to a single Start window:

```text
if plan.num_clips >= 2 AND (
    T_anchor < clip_length
    OR declared_a < clip_length
    OR declared_b < clip_length
):
    → single Start window [0, min(clip_length, T_anchor)] per file
      (per-file end clamped to that file’s timeline_end; same rules as today’s single-window branch)
else:
    → normal start + interior (per file) + end (anchored or file_tail)
```

Example: `12 min / 20 min`, `clip_length = 15 min`, `num_clips = 2` → both return one window (today: 1 vs 2 → `clip count mismatch`). Auto mode would not save forced `--symmetric` on that pair (ratio 0.6 > 0.5).

### Offset semantics (unchanged)

Both estimates remain global: `b_time = a_time + offset_secs`. Anchoring only changes **which audio** is in the end buffers.

### Recommended offset policy (unchanged in v1)

| Condition | `recommended_offset_secs` |
|-----------|---------------------------|
| Start + end agree (≤ `OFFSET_AGREEMENT_TOLERANCE_SECS`) | Fuse / start-first (today) |
| Disagree | `prefer_start_clip` (today) — repair keeps start for fill |
| `require_consistent_offsets` | `None` when disagree (today) |

Anchored end makes disagreement **more meaningful**; it does not automatically trust end.

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Anchor duration** | `min(effective_timeline_end(extent_a), effective_timeline_end(extent_b))` with existing per-file `end_tail_inset` applied before min. |
| **Which file is “shorter”** | Purely duration-based; no A/B role preference. |
| **Start clip** | `[0, clip_length]` unless pair-collapse applies (above). |
| **Interior clips** | **Delivery slice A (end-only ship):** per-file `timeline_end` when interior ships later. **Delivery slice B (combined ship, recommended in code):** `SharedTimeline` uses `T_anchor` for interior — see [interior plan](TEMP-anchored-interior-extraction-plan.md). `FileTail` always per-file. |
| **Window-count mismatch** | Pair-level collapse when either `declared < clip_length` or `T_anchor < clip_length` (see above). |
| **Config** | `alignment.end_clip_anchor = file_tail \| shared_timeline` (serde alias `anchored`). Default **`shared_timeline`** for new installs; **`file_tail`** optional for backward-compat paranoia. Repair inherits via shared `AlignmentConfig`. |
| **Query-reference** | Unaffected — no multi-clip end window. |
| **Extraction API** | Split plan from extract (see API sketch); keep single-file `clip_windows_with_options` for query spike / tests. |
| **Facade** | Re-export `EndClipAnchor` and `clip_windows_paired` on lib facade if needed by tests or CLI. |
| **Verbose output** | Log when B end window differs from B file tail (e.g. `end clip anchored at 40:00, not file tail 300:00`). |
| **Human report** | Optional: `End clip window: A […] B […] (anchored)` in verbose repair/analyzer output. |
| **Silence cross-check** | No change in v1 — separate issue (overlap weighting). |

### Rejected for v1

| Alternative | Reason |
|-------------|--------|
| Offset-mapped end (`+ Δ_start`) | Requires start first; adds complexity; follow-up phase |
| Anchor on longer file | Wrong for excerpt-on-master |
| Use anchored end for fill when start fails | Out of scope; repair already has query-reference |
| Rely on auto query-mode only for window-count mismatch | Misses near-equal pairs (e.g. 12/20 min); bad for forced `--symmetric` |
| Third mode “shared_declared” | Over-engineering; decodable mismatch handled by same `T_anchor` rule |

---

## Gap resolutions (accepted pre-implementation)

| Gap | Resolution |
|-----|------------|
| **`extract_clips` API** | Split: `clip_windows_paired` + `extract_clips_at_windows`; keep `extract_clips` as single-file wrapper (extent → `clip_windows_with_options` → extract). |
| **Window-count mismatch** | Pair-level collapse to single window when either `declared < clip_length` or `T_anchor < clip_length`. |
| **Fixtures** | Generated-only via `write_anchored_end_symmetric_pair`; bounded chirp; **no** committed WAV (5 MB corpus budget). |
| **Decodable mismatch** | Intentional: equal declared, unequal decodable → end windows move to shorter effective tail; document in Risks; `file_tail` escape hatch. |
| **JSON** | Optional `end_clip_anchor`; **v1 contract revision** + golden regen (`docs/json-output.md`, `full_surface_alignment.json`). |
| **Integration tests** | Require `alignment.mode = symmetric`; add control test that `mode = auto` on long/short still picks query-reference. |
| **`locate_query_spike`** | Out of scope (non-goal). |

---

## Multi-clip (`num_clips > 2`)

**Yes — more than two clips are supported today.** Config validates `num_clips ≥ 1` with no upper bound. CLI `--num-clips` and TOML `clip.num_clips` accept any `u32 ≥ 1`. Analyzer default is **1**; repair default is **2**; corpus cases exercise `num_clips = 2`.

### How placement works today

For `effective_num_clips ≥ 2` (`domain/policies.rs`, documented in `PLAN.md`):

| Label | Placement |
|-------|-----------|
| **Start** | `[0, clip_length)` |
| **End** | `[timeline_end − clip_length, timeline_end)` per file |
| **Interior** (`num_clips > 2`) | Timeline divided into `n` equal segments on **that file’s** `timeline_end`; each interior window is `clip_length` centered on the segment midpoint |

Example (`clip_length` 10m, `num_clips` 3, duration 60m): start `[0,10)`, interior `[25,35)`, end `[50,60)`.

### How alignment uses them

- `align_extracted_pair` fingerprints clip pairs **by index** (start↔start, interior↔interior, end↔end).
- `offsets_consistent` requires **every** aligned clip’s offset within `OFFSET_AGREEMENT_TOLERANCE_SECS` (0.5 s) of the others — not just start/end.
- `offset_drift_secs` is still **`Δ_end − Δ_start` only**; interior clips do not affect drift.
- `recommended_offset_secs` uses weighted fusion when all offsets agree; otherwise `prefer_start_clip` (start, then end).

### Behaviour for `num_clips > 2` (by delivery slice)

| Clip | `SharedTimeline` — **end-only ship** | `SharedTimeline` — **combined ship** (recommended) |
|------|--------------------------------------|-----------------------------------------------------|
| **Start** | `[0, clip_length)` both | same |
| **End** | **Anchored** at `T_anchor` | same |
| **Interior** | Per-file `timeline_end` (interim) | **`T_anchor` subdivision on both** + overlap omission |

**End-only ship** fixes start + end on unequal lengths; interior remains a known limitation until the interior plan lands.

**Combined ship** adds little code if `clip_windows_paired` is structured correctly (~helper call + overlap filter). See [Implementation sequencing](#implementation-sequencing).

When `T_a ≈ T_b`, all windows are bit-identical to today’s per-file planner regardless of slice.

### Test coverage for `num_clips > 2`

| Slice | Domain | Integration |
|-------|--------|-------------|
| **End-only** | Equal durations: full parity including `num_clips = 3`. Unequal: assert **end** anchored only — do **not** golden-test per-file interior as long-term expected behaviour. | Not required for ship (defaults use 1–2 clips). |
| **Combined** | Add unequal `num_clips = 3` interior oracle (e.g. 40/300 → interior `[15, 25)` both). | `num_clips = 3` generated chirp; `offsets_consistent == true`. |

---

## API sketch

### Domain (`domain/policies.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EndClipAnchor {
    /// Legacy: each file’s last `clip_length` before its own timeline end.
    FileTail,
    /// Shorter effective duration defines `[T−L, T]` on both timelines.
    #[default]
    SharedTimeline,
}

pub struct ClipPlanningOptions {
    pub end_tail_inset: Duration,
    pub end_clip_anchor: EndClipAnchor,  // used only when num_clips >= 2
}

/// Returns (windows_a, windows_b). End windows may differ in *placement on the longer file*
/// when anchors diverge from file tails; interior windows may differ on unequal lengths.
pub fn clip_windows_paired(
    extent_a: &MediaExtent,
    extent_b: &MediaExtent,
    plan: &ClipPlan,
    options: ClipPlanningOptions,
) -> Result<(Vec<ClipWindow>, Vec<ClipWindow>), DomainError>;
```

**Shared helpers** (extract in Phase 1 — required for anti-churn; interior plan reuses these):

```rust
fn interior_windows_along_timeline(
    timeline_secs: f64,
    clip_length: Duration,
    n: u32,
) -> Vec<ClipWindow>;

fn interior_overlaps_fixed_clip(
    interior: &ClipWindow,
    start: &ClipWindow,
    end: &ClipWindow,
    tolerance: Duration,
) -> bool;
```

Refactor `clip_windows_with_options` to call `interior_windows_along_timeline` (no behaviour change).

**`clip_windows_paired` implementation strategy:**

1. Apply **pair-collapse** rule when triggered; else continue.
2. Compute `timeline_end_a`, `timeline_end_b`; `T_anchor = min(…)`.
3. **Start:** `[0, clip_length)` on both.
4. **End:**
   - **`FileTail`:** per-file `timeline_end.saturating_sub(clip_length)`.
   - **`SharedTimeline`:** `[T_anchor − L, T_anchor]` on both, clamped to each file’s extent.
5. **Interior** (`n > 2`):
   - **`FileTail`:** `interior_windows_along_timeline(timeline_end_a, …)` vs `…(timeline_end_b, …)` (may differ).
   - **`SharedTimeline`:** `interior_windows_along_timeline(T_anchor, …)` **once** → identical on both (combined ship). End-only interim: per-file timeline until interior plan merges.
   - **Overlap** (combined ship): omit interior when overlap with start/end > `INTERIOR_OVERLAP_TOLERANCE` (1 s) — see interior plan.
6. Assemble `windows_a` / `windows_b`; assert equal lengths.

When window extends past decodable extent, clamp / mark `end_clip_unreliable` as today.

### Application (`align_videos.rs`)

```text
resolve_track_extent(session, plan, config) -> (track, extent)     // exists
clip_windows_paired(extent_a, extent_b, plan, options) -> (wa, wb)
extract_clips_at_windows(session, track, extent, windows, ...) -> ExtractedClips
extract_clips(...)  // single-file: extent + clip_windows_with_options + extract_clips_at_windows
```

Symmetric path (`align_single_track_pair`, `align_best_track_pair`):

1. Resolve both extents.
2. `clip_windows_paired` once per pair.
3. `extract_clips_at_windows` for A and B with paired windows.
4. `align_extracted_pair` unchanged except estimates may be more consistent.

`resolve_mode` Tier 2: when `end_clip_anchor == SharedTimeline`, compare window counts from `clip_windows_paired`; when `FileTail`, keep independent `clip_windows_with_options` counts (legacy auto behaviour).

`format_clip_plan`: when `SharedTimeline` and `windows_b[end].end < timeline_end_b`, append `(anchored at {T_anchor}, not file tail {timeline_end_b})`.

---

## Implementation sequencing

Relationship to [TEMP-anchored-interior-extraction-plan.md](TEMP-anchored-interior-extraction-plan.md).

### What churns vs what does not

| Layer | End plan | Interior follow-up | Sequential churn? |
|-------|----------|-------------------|-------------------|
| `align_videos.rs` (extract split, symmetric path, `resolve_mode`) | New | None | **No** — one-time wiring |
| `config.rs` / JSON `end_clip_anchor` | New | Doc: `SharedTimeline` = whole shared span | **No** |
| `clip_windows_paired` | New | Extends interior branch only | **Low** if structured below |
| `clip_windows_with_options` | Helper extract | None | **No** (refactor only) |
| Tests | `num_clips = 2` + equal `num_clips = 3` | Unequal `num_clips = 3` oracle | **Low** if end tests don’t lock interim interior |

**Interior cannot ship before end** — it needs `T_anchor`, `clip_windows_paired`, and paired extraction.

### Recommended approach: one domain feature, two delivery slices

```text
End plan Phase 1 (domain) — single pass
  ├─ extract interior_windows_along_timeline (+ unit test vs today)
  ├─ clip_windows_paired with structured start / interior / end steps
  ├─ [required] SharedTimeline end + FileTail + pair-collapse
  └─ [optional, same PR] SharedTimeline interior + overlap omission
       → interior plan Phase 0–1 collapse to integration tests only

End plan Phase 2 (pipeline) — no interior follow-up work

Interior plan — only if end-only ship
  ├─ Phase 1: flip SharedTimeline interior branch + overlap
  └─ Phase 2: num_clips = 3 integration test
```

| Strategy | When to use |
|----------|-------------|
| **Combined ship** (end + interior domain in one PR) | Recommended when implementing `clip_windows_paired` — incremental cost is small; avoids rewriting interior branch and tests. |
| **End-only ship** | Smaller review; acceptable if unequal `num_clips ≥ 3` symmetric runs are out of scope for the release. Use anti-churn structure so interior is a small follow-up PR. |

### Anti-churn rules (implementer checklist)

1. Extract `interior_windows_along_timeline` **before** writing `clip_windows_paired` interior logic.
2. Do **not** copy-paste the interior loop — single helper for single-file, paired `FileTail`, and paired `SharedTimeline`.
3. Do **not** assert per-file interior on unequal `num_clips = 3` as the permanent oracle unless shipping end-only.
4. Document `SharedTimeline` in `PLAN.md` as “shared span for all multi-clip windows” when interior ships (combined or follow-up).

---

## Phases

### Phase 0 — generator & domain oracles

- [x] Add `write_anchored_end_symmetric_pair` in `audio_fixtures.rs` (bounded chirp; long B full chirp + tail silence/noise; short A = slice `0..shared_secs`).
- [x] CI scale: e.g. `shared_secs = 240`, `long_secs = 1800`, `clip_length = 60 s`, `num_clips = 2` → end `[180, 240]` on both.
- [x] Domain unit tests record expected window times (no committed corpus WAV — 5 MB budget).
- [x] Near-equal pair regression: existing corpus / generated offset chirp pair.

### Phase 1 — paired clip planning (domain)

- [x] Extract `interior_windows_along_timeline` from `clip_windows_with_options` (no behaviour change; unblocks interior plan).
- [x] `EndClipAnchor` enum + config wire-up (`AlignConfig.alignment.end_clip_anchor`).
- [x] `clip_windows_paired` with structured start / interior / end steps ([API sketch](#domain-domainpoliciesrs)).
- [x] **Required:** `SharedTimeline` end, `FileTail`, pair-collapse.
- [x] **Optional (combined ship):** `SharedTimeline` interior from `T_anchor` + overlap omission — defers [interior plan](TEMP-anchored-interior-extraction-plan.md) Phase 0–1.
- [x] Unit tests:
  - `45 min / 45 min` → same windows as today (including `num_clips = 3`).
  - `40 min / 300 min`, `num_clips = 2` → end `[25,40]` on both, not `[285,300]` on B.
  - `40 min / 300 min`, `num_clips = 3` → end anchored; interior per [delivery slice](#behaviour-for-num_clips--2-by-delivery-slice).
  - `12 min / 20 min`, `clip_length = 15 min`, `num_clips = 2` → both collapse to 1 window.
  - `end_tail_inset` applied before `min`.

### Phase 2 — align pipeline wiring

- [x] `extract_clips_at_windows`; refactor `extract_clips` to delegate.
- [x] `align_videos.rs`: symmetric path uses `clip_windows_paired` + `extract_clips_at_windows`; query path unchanged.
- [x] `resolve_mode` Tier 2 uses paired planning when `end_clip_anchor == SharedTimeline`.
- [x] Verbose `format_clip_plan`: anchor mode and when B end ≠ B tail.
- [x] Integration tests (`alignment.mode = symmetric`):
  - Long/short generated chirp, `num_clips = 2`: `Δ_start ≈ Δ_end`, both high confidence.
  - Legacy `FileTail` reproduces old behaviour (end disagree / nonsense drift).
  - Control: same pair with `mode = auto` → `alignment_mode_used == QueryReference`.
  - **Optional (combined ship):** `num_clips = 3` on same generator → `offsets_consistent == true`.

### Phase 3 — reporting & repair UX

- [x] `AlignmentReport` / JSON: optional `end_clip_anchor` field (diagnostic; absent in query-reference and `num_clips < 2`).
- [x] **Contract revision:** update `docs/json-output.md`; regenerate `tests/fixtures/full_surface_alignment.json` (+ repair golden if needed).
- [x] Verbose CLI (analyzer + repair): show end window times for A and B.
- [x] Update `docs/cli-output.md` alignment instability wording: drift after anchor is more likely **real** (edits, speed, damage).
- [x] Adjust `output.rs` instability test if synthetic offsets change meaning (keep warning when `Δ_end − Δ_start` large).

### Phase 4 — docs & cleanup

- [ ] `PLAN.md` symmetric alignment section: anchored end default; `SharedTimeline` covers interior when combined ship (or note interior follow-up if end-only).
- [ ] `README.md` one-line note under clip alignment.
- [ ] `BACKLOG.md` row; archive this plan.

---

## Follow-up (post-v1)

| Item | Benefit |
|------|---------|
| **Offset-mapped end** (`[T_a−L, T_a]` on A, `[T_a−L+Δ, T_a+Δ]` on B after start clip) | Correct when B has long leader but equal “content length” |
| **Anchored interior clips** | Multi-clip symmetry on unequal lengths (`num_clips > 2`) — [interior plan](TEMP-anchored-interior-extraction-plan.md); **may ship in same PR** as end ([sequencing](#implementation-sequencing)) |
| **Skip end fingerprint when `T_anchor − L < start_end_overlap`** | Avoid comparing overlapping windows |
| **Weighted drift in repair warning** | Down-rank end when end confidence low or tail damaged |

---

## Test matrix

| Scenario | FileTail end | SharedTimeline end |
|----------|--------------|-------------------|
| 45 min / 45 min, Δ=+12 s | Δ_start ≈ Δ_end ≈ 12 | Same |
| 40 min / 300 min, same audio 0–40 min, `mode = symmetric` | Δ_end spurious / low conf | Δ_start ≈ Δ_end ≈ true offset |
| 40 min / 300 min, `mode = auto` | N/A (query-reference) | N/A (query-reference) |
| Joe-like: equal len, tail dropout on A | Drift may be large (damaged tail) | Drift reflects same-time region (still large if real edit/damage) |
| Equal declared, unequal decodable tail | Per-file end tails | Both end at shorter effective `T_anchor` |
| `12 min / 20 min`, `clip_length = 15 min`, `num_clips = 2` | 1 vs 2 windows → mismatch | Both collapse to 1 window |
| `num_clips = 1` | Unchanged | Unchanged |
| `num_clips = 3`, equal durations | Interior + end per-file tails | Same as today (all labels) |
| `num_clips = 3`, unequal durations | Interior compares unrelated regions | **End-only:** end anchored; interior unrelated. **Combined:** end + interior anchored |
| Query-reference auto | N/A | N/A |

---

## Risks

| Risk | Mitigation |
|------|------------|
| Default behaviour change on unequal-length symmetric runs | Config `file_tail` escape hatch; auto mode usually picks query-reference when ratio < 0.5 |
| Default change when declared equal but decodable differs | Intentional — shorter effective tail anchors both ends; document; `file_tail` escape hatch |
| B window past decodable extent | Clamp to `timeline_end_b`; existing `end_clip_unreliable` / skip paths |
| Equal-length regression | Proven identical windows when `T_a = T_b` (all clip labels) |
| User forced `--symmetric-align` on excerpt + master | Anchored end fixes end clip; document query-reference preference |
| `num_clips > 2` on unequal lengths (end-only ship) | Documented interim; interior plan or combined ship |
| Implementing interior twice (churn) | Extract helper first; structured `clip_windows_paired`; see [sequencing](#implementation-sequencing) |

---

## Acceptance criteria

1. `clip_windows_paired` places B end at shorter file’s clock, not B file tail, when durations differ.
2. Symmetric integration test (generated chirp, `mode = symmetric`): long/short pair yields agreeing start/end offsets within tolerance.
3. Equal-duration pairs produce bit-identical window bounds to current `clip_windows_with_options` for all labels (start, interior, end).
4. `end_clip_anchor = file_tail` restores legacy placement.
5. Pair-collapse produces equal window counts for `12/20 min` + `clip_length 15 min` case.
6. Repair still uses start offset for fill when clips disagree; drift line still computed from start/end clip estimates only.
7. JSON contract updated if `end_clip_anchor` field ships.
8. **(Combined ship only)** Unequal `num_clips = 3`: interior windows anchored at `T_anchor` on both files; `offsets_consistent` true on generated integration pair.
