# Gap selection — deferred and refused sketches

Status: **nothing here is planned.** Recorded so it is not rediscovered, proposed again from scratch,
or mistaken for scope.

Split out of `TEMP-gap-selection-plan.md` on 2026-07-29 (its §7.2 and §9). Both items kept turning up
as "obvious next features" during selection design; both have a reason not to be built yet, and the
reasons are the content.

**Siblings:** [archive/TEMP-gap-selection-plan.md](archive/TEMP-gap-selection-plan.md) (v1 — **archived**),
[archive/TEMP-gap-selection-ranges-plan.md](archive/TEMP-gap-selection-ranges-plan.md) (v1.5 — **archived**),
[TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) (prerequisite for the manifest below; **unparked 2026-07-30**).

---

## 1. `--scan-window` — refuse the cheap version, measure before the expensive one

Shape, if it were ever built:

```text
--scan-window <START-END>   Detect gaps only within this A-timeline interval
```

- A **scan** knob, not a repair knob: lives on `ScanGapsRequest` next to `min_gap_secs` /
  `silence_hold_blocks`, set from `RepairConfig` in `composition.rs::repair_run_input`.
- Must join `ScanRecipe` ([TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md)) and the manifest
  `scan` block below — otherwise a saved gap list cannot be checked against its recipe.
- Named `--scan-window`, **never** as a mode of `--only-gaps`. Sharing a flag between "which gaps
  exist" and "which gaps to patch" is exactly the conflation that makes two-lists confusion real.
- Alignment is unaffected: it needs broad coverage regardless, so a window would apply to gap
  detection, not to `Aligner`.

### Why it is deferred: the perf argument is weaker than it looks

From [repair-perf.md](repair-perf.md) §1c (17-pair, post-lever-1b(b), root span `patch_audio`):

| Cost | Share of `patch_audio` | Sensitive to gap selection? |
|------|------------------------|------------------------------|
| `char_gate_search` (inclusive) | 73.8% | **Yes** — per gap. Selection already removes it for unselected gaps |
| `patch_decode_a` + `patch_decode_b` | ~25–35% | **No** — 17 calls for 17 pairs. Per *file*, not per gap |

So there are two different features hiding here:

1. **A window that narrows only *detection*** (decode everything as usual, just don't report gaps
   outside the interval) reaches **none** of the decode cost. It saves the silence-scan pass over A and
   nothing else — while adding a scan knob that destabilizes every `#` precisely where selection is
   trying to make gap numbers dependable. Bad trade; **this is the version to refuse.**
2. **A window that narrows *decode*** is the one that would pay, because decode is the only large cost
   selection cannot touch. But that is the "partial scan" non-goal — a perf project, not a CLI flag.
   It is tractable (the B-side haystack for a gap at 1:42 in A sits near 1:42 in B via the offset) but
   carries real correctness risk at the alignment boundary, and the **scan phase's own cost is not in
   the profile tables at all** — the measured root is `patch_audio`. That hole in the data must be
   closed before anyone commits to the work.

**If this is ever revisited, measure first:** instrument `ScanGaps` as a sibling root to `patch_audio`
and record scan-vs-patch share in [repair-perf.md](repair-perf.md). Without that number the payoff is
unknown — and the standing rule in that doc is that the *numbers* decide, not projections.

**Note for the iterative workflow that motivates selection:** every invocation re-runs the full scan,
so `--only-gaps` does not spare that cost. The lever for *that* is the manifest below (reuse a prior
gap list instead of re-deriving it), not a scan window.

### If it is ever built: how it composes with selection

They are different axes and must never share a flag:

| Axis | What it does | Effect on gap identity |
|------|--------------|------------------------|
| **Selection** (`--only-gaps` / `--skip-gaps`) | Filters an existing report at fill-plan time | **None.** The report is unchanged; every `#` still means what the table says |
| **Identification window** (`--scan-window`) | Restricts `ScanGaps` to an interval, so gaps outside it are never detected | **Changes the report.** A scan knob in the same family as `min_gap_ms` — every `#` shifts |

They are **not** mutually exclusive and cannot produce conflicting lists. They compose in sequence:

```text
scan window  →  GapReport (the detected list)  →  selection  →  GapFillPlan.regions
```

The only rule this needs: **selection gap numbers always refer to the post-window report.**
`--scan-window 1:00:00-1:30:00 --only-gaps 2` means "the second gap found inside that window", never
"report gap 2, if it happens to fall in the window". The risk of two competing gap lists is a
documentation problem, not a semantic conflict.

---

## 2. `--gaps-from` manifest (would be v2)

Blocked on selection v1.5 (range tokens) and on
[TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) (the recipe type it embeds).

Minimal manifest:

```json
{
  "scan": {
    "min_gap_ms": 1000,
    "scan_block_ms": 250,
    "silence_hold_ms": 500,
    "silence_peak_fraction": 0.01,
    "absolute_silence_rms": 0.001007
  },
  "select": [
    { "start_secs": 6128.25, "end_secs": 6360.0 },
    { "index": 4 }
  ]
}
```

Loader: run scan → if the embedded `scan` block ≠ the current scan recipe, **error** on index entries;
range entries resolve by matching `video_a_*_secs` (within the v1.5 dual ε) onto report gaps / their
`gap_index`. Accept a full `RepairJsonOutput` as a convenience alias.

- **The `scan` block is `domain::ScanRecipe`**, so the mismatch check is `manifest.scan !=
  report.recipe` — one `PartialEq`, not a hand-rolled five-field comparison that drifts the first time
  a knob is added. This is the main reason the recipe PR extracts a type instead of adding flat fields.
- `decode_chunk_secs` is **not** in the block: it cannot change which gaps are detected, so including
  it would make a saved gap list spuriously invalid after a throughput tweak.
- **`absolute_silence_rms` is a normalized float, not an integer** (corrected 2026-07-29). An earlier
  sketch wrote `33`, which is the wrong *units*, not just the wrong type: the default is
  `33.0 / 32767.0` ≈ `0.001007`, and the field is `f32` everywhere — `RepairConfig`,
  `ScanGapsRequest`, and the fingerprint corpus recipe, which already serializes it as a float.
- The corpus DTO (`CorpusScanRecipe`) serializes the same five knobs with `Option` fields for backward
  compat. Same field names and types — manifest `scan` block, JSON echo, and corpus recipe stay one
  shape instead of three.
