# Gap selection (subset patching) — plan (DRAFT)

Status: **not started**.

Companions: [pipeline.md](../pipeline.md) § Orchestration / Fill plan,
[gap-fill-modes.md](../gap-fill-modes.md), [gap-repair-guide.md](../gap-repair-guide.md),
[cli-output.md](../cli-output.md), [json-output.md](../json-output.md).

Motivating use case: after a full repair run, the user wants to **patch only some gaps**
(iterative retry, partial write, or scripting) without re-running alignment/scan with different
inputs. Today every fillable gap in the scan report enters `GapFillPlan.regions`; there is no
way to exclude gaps that are planned but should be left untouched on A for this invocation.

---

## 1. Problem (one paragraph)

Write mode always plans and attempts every repairable gap. Iterative workflows (“patch 1,2,4,5
first; retry 3 with different flags”) require either editing source video, maintaining multiple
configs, or accepting a full re-patch. A **gap selection** layer at fill-plan time would let the
user name a subset while keeping the **full scan table** for context. Separately, gap **`#` in the
human report is a 1-based index into `GapReport.gaps`** — convenient within one scan recipe, but
**not stable** when `min_gap_ms` or other scan knobs change the detected run list. Selection must
document that contract and offer **time-range** tokens for cross-run stability without pretending
indices are global IDs.

---

## 2. Gap identity contract

| Handle | Stable across rescans? | Meaning |
|--------|------------------------|---------|
| **`#` (table index)** | No (when scan knobs change) | 1-based index into `report.gaps` in chronological order on A; matches stdout `#` column |
| **A time range** `(start_secs, end_secs)` | Yes (same A decode clock, same scan recipe) | Matches `Gap::video_a_start_secs` / `video_a_end_secs`; internal key is `gap_key` in `patch_audio.rs` |
| **Below `min_gap_ms`** | N/A | Not in the report; no `#` |

**Rules for users and docs:**

- Copy `#` from **the table produced by this run** (or JSON `gaps[]` order).
- If `min_gap_ms`, `silence_hold_ms`, `scan_block_ms`, `silence_peak_fraction`, or
  `absolute_silence_rms` will change before the next patch attempt, record **Range** (or JSON
  `video_a_start_secs` / `video_a_end_secs`) and use a **range token** (v1.5), not a remembered `#`.
- Do **not** silently remap old indices onto a new scan.

**Scan params echo (v1):** JSON `GapScanJson` today carries `scan_block_ms` and
`silence_peak_fraction` but not `min_gap_ms`, `silence_hold_ms`, or `absolute_silence_rms`. Add them
so scripts can verify they are on the same scan recipe before reusing a saved gap list.

---

## 3. User-facing semantics

| Rule | Detail |
|------|--------|
| **Write mode only** | Flags apply when `--wav` / `--mux` runs `PatchAudio`. Scan-only runs ignore selection (no fill plan). |
| **Full scan always** | Phases 1–2 unchanged; stdout/JSON still list **all** detected gaps. |
| **Filter at plan time** | Hook: `build_gap_fill_plan` (`domain/gap_fill.rs`). Unselected gaps never enter `regions`. |
| **Audio on A** | Unselected gaps keep **original A** audio in the output (no splice). |
| **Fillability unchanged** | Selection is orthogonal to `fillable` / `unfillable` / `not planned` for track mismatch, B energy, query-reference coverage. |
| **Mutual exclusivity** | `--only-gaps` and `--skip-gaps` cannot both be set (clap `conflicts_with`). |
| **Empty selection** | If resolution yields zero regions and no plan-block reason, exit with a clear error before patch. |

**Status column (write run):** unselected fillable gaps show  
`not planned: gap not selected` (machine: `gap_not_selected`).

---

## 4. CLI and config (v1)

### Flags

```text
--only-gaps <LIST>   Patch only these gaps (1-based report indices; comma-separated)
--skip-gaps <LIST>   Patch all fillable gaps except these (same index semantics)
```

TOML (`[repair]`):

```toml
only_gaps = [2, 4, 5]
# skip_gaps = [3]   # mutually exclusive with only_gaps
```

CLI overrides TOML when both present (same pattern as other repair flags in `cli/mod.rs`).

### Index parsing

- Comma-separated positive integers: `3`, `2,4,5`, ` 2 , 4 `.
- Validate: `1 ≤ index ≤ report.gaps.len()`; duplicate tokens → dedupe or error (pick **error** for clarity).
- Out-of-range → fail fast with: `gap index 7 out of range (6 gaps detected)`.

### v1.5 extension (same flag, mixed tokens)

Auto-detect per token:

| Token shape | Resolution |
|-------------|------------|
| Integer `N` | Report index `N` |
| `START-END` | Seconds (`6128.25-6360.0`) or `H:MM:SS` / `H:MM:SS.mmm` using existing `format_timestamp` display conventions |

Range match (default **strict**): gap edges within ε (e.g. 50 ms) of parsed start/end.  
Unmatched range → error listing detected gaps (no silent skip).

`--only-gaps` and `--skip-gaps` accept the same token grammar; skip resolves ranges to indices first, then subtracts.

---

## 5. Implementation sketch

### 5.1 Types

```rust
/// Resolved after scan, before fill plan.
pub struct GapSelection {
    /// 1-based indices into `GapReport.gaps` (chronological).
    pub selected_indices: HashSet<usize>,
}

pub enum GapSelectionMode {
    All,
    Only(HashSet<usize>),
    Skip(HashSet<usize>),
}
```

Parse CLI/TOML → `GapSelectionMode` in infrastructure; resolve to `GapSelection` once `GapReport` exists.

### 5.2 Fill plan hook

Extend signature (names illustrative):

```rust
pub fn build_gap_fill_plan(
    report: &GapReport,
    crossfade_ms: u64,
    selection: &GapSelection,
) -> GapFillPlan
```

After existing fillability / coverage checks, if gap report index `i` (0-based) is not in
`selection.selected_indices` (1-based: `i + 1`), push:

```rust
GapFillSkipped {
    a_start_secs: g.video_a_start_secs,
    a_end_secs: g.video_a_end_secs,
    reason: GapFillSkipReason::GapNotSelected,
}
```

**Order of precedence** (same gap, multiple reasons): keep existing plan-block reasons
(`TrackLayoutMismatch`, etc.) before per-gap reasons; for per-gap, `NotFillable` and
`OutsideReferenceCoverage` win over `GapNotSelected` (selection only applies to gaps that would
otherwise be repairable).

Wire `selection` from `PatchAudioRequest` (or `RepairVideos` config bundle) into
`build_gap_fill_plan` call sites.

### 5.3 New skip reason

`domain/patch_result.rs`:

```rust
pub enum GapFillSkipReason {
    // ... existing ...
    GapNotSelected,
}
```

| Surface | Value |
|---------|-------|
| `format_plan_skip_reason` / JSON `plan_skip_reason` | `gap_not_selected` |
| Human `format_fill_skip_reason` | `gap not selected` |
| `GapTags` / `PlanKind` | `not_planned` (same as other plan-time skips) |

Update [json-output.md](../json-output.md) § `GapFillSkipReason` and golden fixtures.

### 5.4 Progress and logging — report `#` fix

**Bug today:** patch progress uses `plan.regions` enumeration:

```text
gap 2/2 (1:42:08 – 1:46:00): …
```

`2/2` is “second planned region of two,” not report `#2`.

**Fix (v1, same PR):** build `report_index_by_gap_key: HashMap<(u64,u64), usize>` once per run.
When logging `patch-gap` progress and verbose `gap N/M` lines, use **report `#`** and
**total gaps in report** (or **selected count** for denominator — document choice):

Recommended:

```text
gap 4/6 (1:42:08 – 1:46:00): …   # patching report gap 4 of 6 detected; 2 unselected
```

Phase line `Aligning fill regions (N gaps)` should reflect **planned** count; progress denominator
for `patch-gap` should use **planned region count**; verbose line prefixes **report `#`**.

Tracing span `gap_index` should record report index, not region enumeration.

### 5.5 `repairable_count` / scan followup

`GapReport::repairable_count()` stays “all gaps that *could* be patched” (no selection). Optional
stderr when selection active:

```text
Gap filter: patching 3 of 6 detected gaps (only-gaps: 2,4,5)
```

Do not change scan-only `repairable_count` semantics.

---

## 6. Interactions

### `anchored_retry`

`build_patch_anchor_candidates` uses `gap_index` as **index into `plan.regions`**, not report `#`
(`patch_audio.rs`). Pass-2 retry only sees gaps that were **planned and attempted** in pass 1.

| Scenario | Behavior |
|----------|----------|
| Excluded gap was a strong anchor donor in a prior full run | Not available in this run; pass 2 may recover fewer gaps — **document**. |
| User excludes gap that would have been retried | Expected; no special case. |

No v1 change to anchor table indexing; optional v2 note if we expose anchor donor by report `#`.

### Scan-only / dry run

Selection flags ignored; no `GapNotSelected` in output.

### Profiles / `fill_mode`

Selection is independent of `repair_profile`, `fill_mode`, anchor/residual flags.

---

## 7. Non-goals

- **Cross-rescan index preservation** without time ranges or a manifest file.
- **Silent remapping** of stale indices onto a new gap list.
- **Partial scan** (only decode regions around selected gaps) — always full `ScanGaps`.
- **Replacing** `limit_fill_to_mapped_region` or track-compatibility gates.
- **v2 `--gaps-from` manifest** in v1 (see §9).

---

## 8. Phased delivery

| Phase | Scope |
|-------|-------|
| **v1** | `--only-gaps` / `--skip-gaps` (indices only); `GapNotSelected`; report `#` in progress lines; scan params in JSON; user docs |
| **v1.5** | Mixed tokens (index + range) on same flags; range parser + strict match tests |
| **v2** | `--gaps-from` manifest; optional scan-param mismatch error when manifest embeds scan recipe |

---

## 9. v2 sketch: `--gaps-from` (not v1)

Minimal manifest:

```json
{
  "scan": {
    "min_gap_ms": 1000,
    "scan_block_ms": 250,
    "silence_hold_ms": 500,
    "silence_peak_fraction": 0.01,
    "absolute_silence_rms": 33
  },
  "select": [
    { "start_secs": 6128.25, "end_secs": 6360.0 },
    { "index": 4 }
  ]
}
```

Loader: run scan → if embedded `scan` ≠ current scan params, **error** on index entries; range
entries still resolve by `gap_key`. Accept full `RepairJsonOutput` as a convenience alias.

---

## 10. Implementation checklist (v1)

- [ ] `RepairConfig`: `only_gaps: Option<Vec<usize>>`, `skip_gaps: Option<Vec<usize>>`; validate mutual exclusivity in `RepairConfig::validate`
- [ ] `Args`: `--only-gaps`, `--skip-gaps` (comma-separated); wire in `cli/mod.rs`
- [ ] `GapSelection` resolve + unit tests (out of range, empty, skip vs only)
- [ ] `GapFillSkipReason::GapNotSelected` + all formatters / `gap_tags` mapping
- [ ] `build_gap_fill_plan(..., selection)` + domain tests
- [ ] `PatchAudioRequest` + `build_gap_fill_plan` call path
- [ ] Report `#` in progress / verbose / tracing spans
- [ ] `GapScanJson`: add `min_gap_ms`, `silence_hold_ms`, `absolute_silence_rms`; populate from `GapReport` (extend struct if needed)
- [ ] `format_unified_gap_report` / patch summary: `not planned: gap not selected`
- [ ] Golden JSON fixture update per [json-output.md](../json-output.md) revision rules
- [ ] Docs: [gap-repair-guide.md](../gap-repair-guide.md) workflow, [cli-output.md](../cli-output.md) flags, [pipeline.md](../pipeline.md) §3 one paragraph, [gap-fill-modes.md](../gap-fill-modes.md) cross-link
- [ ] Integration test: 3-gap fixture, `--only-gaps 2`, assert gap 1 and 3 unchanged on A, gap 2 patched, status strings correct

---

## 11. Test plan

| Layer | Cases |
|-------|-------|
| **Parse** | `only` / `skip` mutual exclusion; out-of-range; duplicates; whitespace |
| **Plan** | All selected; none selected → error; skip all fillable; unfillable gap in `only` list still `not_fillable` not `gap_not_selected` |
| **Selection + coverage** | Gap outside query-reference region: `outside_reference_coverage` beats `gap_not_selected` |
| **Patch** | Subset patch leaves unselected samples identical to input A |
| **Output** | Human + JSON `plan_skip_reason`; progress line shows report `#` |
| **v1.5** | Range strict match; no match error; mixed token list |

---

## 12. Open decisions

1. **Progress denominator:** report total gaps vs planned count in `gap N/M` — recommend report `#` with planned count only in phase header.
2. **Duplicate indices in CLI:** error vs last-wins — recommend **error**.
3. **JSON contract revision:** adding `min_gap_ms` / `silence_hold_ms` / `absolute_silence_rms` to `scan` is additive; bump [json-output.md](../json-output.md) changelog.
4. **Range ε:** 50 ms default strict match — confirm against corpus edge refine behavior.

---

## 13. Promotion / done criteria

When v1 ships:

- Mark status **v1 done**; move user-facing contract from this file into [gap-repair-guide.md](../gap-repair-guide.md) and [cli-output.md](../cli-output.md).
- Keep v1.5/v2 sections here until implemented or moved to archive.
- Link from [pipeline.md](../pipeline.md) fill-plan section to the promoted user doc (not this TEMP file).
