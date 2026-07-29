# Gap selection (subset patching) v1 — plan

Status: **v1 done** (thin subset patching). User-facing contract promoted (§11) into
[gap-repair-guide.md](../gap-repair-guide.md) § Iterative subset patching,
[cli-output.md](../cli-output.md), [gap-vocabulary.md](gap-vocabulary.md) § Gap numbering.
Archive this file when v1.5 ships or is abandoned. Sequencing meta archived:
[archive/TEMP-gap-selection-sequencing-plan.md](archive/TEMP-gap-selection-sequencing-plan.md).


**This document was split on 2026-07-29.** It had grown to ~1200 lines covering four independent
deliverables, and the cost showed up as repeated false alarms: seven "bugs" were recorded and then
retracted, every one of them a stale claim about current source rather than a design error. Each piece
now lives where it can be verified in one pass. Sequencing was revisited the same day: ship this v1
before the recipe provenance PR.

| Where it went | What |
|---------------|------|
| [archive/TEMP-gap-index-convention-plan.md](archive/TEMP-gap-index-convention-plan.md) | The gap-index prep PR (was §0). **Shipped 2026-07-28.** Its one durable rule now lives in [gap-vocabulary.md](gap-vocabulary.md) § Gap numbering |
| [archive/TEMP-gap-selection-sequencing-plan.md](archive/TEMP-gap-selection-sequencing-plan.md) | Meta: thin v1 before recipe — **archived** after promote |
| [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) | `ScanRecipe` on `GapReport` + the JSON scan-params echo. **Not a selection feature**; **parked** (no code dependency from v1) |
| [TEMP-gap-selection-ranges-plan.md](TEMP-gap-selection-ranges-plan.md) | v1.5 range tokens: `START-END` identity, `START..END` containment, dual ε, straddler diagnostics |
| [TEMP-gap-selection-deferred.md](TEMP-gap-selection-deferred.md) | `--scan-window` (refused in its cheap form) and the `--gaps-from` manifest (v2) |

**What still binds this document to its siblings** (do not re-litigate these in isolation):

- §2.1's identity rule was forced by v1.5's containment token, not by anything in v1. It must be
  settled *here* because v1 ships the integer tokens whose meaning it fixes.
- The index convention is the reason v1 owes nothing on numbering or display. Selection adds a fifth
  reason for `plan.regions` to be shorter than `report.gaps`, which is what made those defects visible.

> **Verification rule.** A `file:line` reference or a claim about current behavior belongs **only** in
> §8 (the checklist), where it is about to be executed and therefore checked. In design sections, state
> the decision and the reason; cite source only where the citation *is* the evidence, and re-verify it
> when you touch that paragraph.

Companions: [pipeline.md](../pipeline.md) § Orchestration / Fill plan,
[gap-fill-modes.md](../gap-fill-modes.md), [gap-repair-guide.md](../gap-repair-guide.md),
[cli-output.md](../cli-output.md), [json-output.md](../json-output.md),
[gap-vocabulary.md](gap-vocabulary.md).

---

## 1. Problem

Write / repair-preview always plans and attempts every repairable gap that survives the fillability,
coverage, and (when enabled) equivalence gates. Iterative workflows ("patch 1,2,4,5 first; retry 3 with
different flags") require either editing source video, maintaining multiple configs, or accepting a
full re-patch. A **gap selection** layer at fill-plan time lets the user name a subset while keeping the
**full scan table** for context.

Separately, a gap's `#` in the human report is a 1-based index into `GapReport.gaps` — convenient
within one scan recipe, but **not stable** when `min_gap_ms` or other scan knobs change the detected
run list. Selection must document that contract and (in v1.5) offer time-range tokens for cross-run
stability, without pretending gap numbers are global IDs.

---

## 2. Gap identity contract

| Handle | Stable across rescans? | Meaning |
|--------|------------------------|---------|
| **`#` (table gap number)** | No (when scan knobs change) | 1-based index into `report.gaps` in chronological order on A; matches the stdout `#` column |
| **A time range** `(start_secs, end_secs)` | Yes (same A decode clock, same scan recipe) | Matches `Gap::video_a_start_secs` / `video_a_end_secs` |
| **Internal join key** | Per run | `FillRegion::gap_index` / `GapFillSkipped::gap_index` — **0-based** index into `GapReport.gaps` (M-GAPKEY, 2026-07-27). Prefer this over float timestamps; the old `gap_key` helper is **deleted** |
| **Below `min_gap_ms`** | N/A | Not in the report; no `#` |

**Rules for users and docs:**

- Copy `#` from **the table produced by this run** (or JSON `gaps[]` order).
- If `min_gap_ms`, `silence_hold_ms`, `scan_block_ms`, `silence_peak_fraction`, or
  `absolute_silence_rms` will change before the next patch attempt, record the **Range** (or JSON
  `video_a_start_secs` / `video_a_end_secs`) and use a **range token** (v1.5), not a remembered `#`.
  Those five knobs are exactly `ScanRecipe` — see [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md),
  which makes the report state its own recipe so a script can check this instead of trusting the user.
- Do **not** silently remap old gap numbers onto a new scan.

### 2.1 Selection tokens are **identities**, never counts — settled 2026-07-28

This is the same distinction the index convention draws in output, applied to input. In output, `#4`
is a label and `3 of 6 planned` is a count. On input, **`--only-gaps` / `--skip-gaps` take labels
only.**

> **Every token names a gap. No token is a position within a derived subset.**
> Each token resolves independently against the whole `GapReport.gaps` as printed by this run;
> the results are **unioned**. A token can never narrow the resolution domain of another token.

`--only-gaps 4` means "the gap the table calls #4". It does **not** mean the 4th fillable gap, the 4th
gap in some window, the 4th planned region, or the 4th of those already selected.

**Consequences (v1 integers now; mixed kinds when v1.5 lands — all testable):**

| Property | Phase | Because |
|----------|-------|---------|
| Order-insensitive: `5,2` ≡ `2,5` | v1 | A set of labels has no sequence |
| Duplicates are typos → **error**, not "twice" | v1 | Naming the same gap twice cannot mean anything under an identity reading |
| `--skip-gaps` = report set **minus** the union | v1 | Same resolution domain as `--only-gaps`; the two differ only in polarity |
| Validation is `1 ≤ n ≤ report.gaps.len()` | v1 | Bounds come from the report, never from a filtered count |
| `GapSelection` is a `HashSet<usize>` (§5.1) | v1 | The data structure already commits: no order, no multiplicity |
| Mixed token kinds compose by **union**, never as a pipeline | v1.5 | `--only-gaps 2,1:42:00..1:50:00` = `{#2} ∪ {gaps in window}`, resolved against the same report |

**Why this must be settled before v1.5.** The containment token `START..END` turns an interval into a
*set of gaps* — which creates a second enumeration and, under a count reading, a second meaning for
integers. `--only-gaps 1:42:00..1:50:00,2` would become ambiguous: gap #2, or the 2nd gap inside the
window? The identity rule kills the question before it is asked, and it kills it the same way §7.1
keeps a hypothetical `--scan-window` from competing with selection: **there is one gap list, the
report, and every handle points into it.**

**Deliberate non-capability:** there is no way to say "the Nth gap of a subset", in v1 or v1.5. If that
is ever wanted, it needs its own syntax and its own justification — not a reinterpretation of integers.

**Stability is a separate axis from identity.** Both token kinds are identities; they differ in how long
they stay valid. Integer tokens are **run-scoped** labels (invalidated by a scan-recipe change);
range tokens are **recipe-stable** identities. Neither is ever a count.

**Vocabulary:** user-facing docs and error messages say *gap number* (matching the `#` column), not
"gap index". Reserve "index" for the 0-based internal `gap_index`. This is enforced in code already —
`resolve_fingerprint_gap_select` says "gap number" in both messages; see
[gap-vocabulary.md](gap-vocabulary.md) § Gap numbering.

---

## 3. User-facing semantics

| Rule | Detail |
|------|--------|
| **Patch / preview modes** | Flags apply when a fill plan is built: `--wav` / `--mux` **and** `--repair-preview`. |
| **Scan-only** | Tokens are **validated** against the report (bad `#` → exit 2) but do not change scan output; no filter note (no fill plan). |
| **Full scan always** | Phases 1–2 unchanged; stdout/JSON still list **all** detected gaps. |
| **Filter at plan time** | Hook: `build_gap_fill_plan` (`domain/gap_fill.rs`). Unselected gaps never enter `regions`. |
| **Audio on A** | Unselected gaps keep **original A** audio in the output (no splice). Preview never splices; status still shows `gap_not_selected`. |
| **Fillability unchanged** | Selection is orthogonal to `fillable` / `unfillable` / `not planned` for track mismatch, B energy, query-reference coverage, and equivalence. |
| **Mutual exclusivity** | `--only-gaps` and `--skip-gaps` cannot both be set (clap `conflicts_with`). |
| **Empty selection** | If resolve yields an empty `GapSelection` (no gap matched the tokens), exit with a clear error before the plan is built. A non-empty selection that later plans zero regions (all unfillable / coverage / equivalence) keeps today's `Ok` path. **Deliberate asymmetry** — see below. |

**Empty-plan asymmetry (deliberate).** An empty fill plan is *not* an error today: `patch_audio`
prints `No gaps planned for patch; skipping audio decode.` and returns `Ok` with an all-`NotPlanned`
summary. That stays. Selection is different because an empty `GapSelection` means the **user's own
arguments** named nothing in the report — a silent success there is indistinguishable from "worked".
So: empty selection after resolve errors (`RepairError::GapSelection`, exit **2**); a non-empty selection
that the plan later empties keeps the existing `Ok` + phase-line behavior. This check lives in the
post-scan resolve step (§5.6), *before* the plan is built, so the error mentions the selection, not the
plan.

**Ordering caveat — the report still prints first.** "Before patch/preview" is true of the *plan*, not
of stdout. The resolve error lands in `RepairRunOutcome::patch_result`, and `print_repair_outcome`
prints the full report — including a complete JSON document under `--format json` — and only then
returns the error. So the user sees a normal-looking scan report, then the error, then exit 2. That is
acceptable for the human format (the table is genuinely useful context for fixing the selection) but
wrong for JSON, where a well-formed success document accompanied by a nonzero exit invites scripts to
parse it and proceed.

**Settled 2026-07-29: suppress the JSON document.** [cli-output.md](../cli-output.md) already makes
this normative — its failure row says stdout is *"Empty — no partial report"* and the scripting
guidance is *"prefer `--format json --quiet` and parse stdout only"*. Emitting a success-shaped
document on a failing run breaks the contract that makes that guidance safe. So:

| Format | On a selection error |
|--------|----------------------|
| `--format json` | **Nothing on stdout**; message on stderr; exit 2 — the documented failure shape |
| Human | **Keep printing the gap table**, then the error, then exit 2 — a deliberate, documented exception for post-scan selection/config errors, because the operator needs the `#` column to fix the selection |

Add the exception to [cli-output.md](../cli-output.md) next to that failure table; it is a doc change,
not a new rule for the human format (which already prints context before failing).

**Implementation note.** `print_repair_outcome` calls `print_repair_output(…, args.format, …)`
**unconditionally** and only then returns `outcome.patch_result.map(|_| ())`. The suppression belongs
at that call, not inside `run_repair`. Do **not** gate it on `patch_result == Err(RepairError::Config)`:
that is not unique to selection — `repair_videos.rs` raises a post-scan `Config` for an internal
invariant ("patched run missing decoded PCM"). Carry an explicit signal instead (a dedicated error
variant, or a `suppress_json` flag on the outcome). If the internal-invariant error ends up suppressed
too that is defensible, but make it a decision, not a side effect of pattern-matching.

**Status column (write / preview):** unselected gaps that would otherwise be repairable show
`not planned: gap not selected` (machine: `gap_not_selected`).

**Summary counts.** Plan-time skips flow into `GapPatchStatus::NotPlanned` and therefore into
`PatchSummary::not_planned_count`, which the human summary prints. `--only-gaps 2` on a 6-gap run will
read `… 5 not planned` next to the §5.5 filter note. That is correct and intended — no special casing;
the filter note is what disambiguates it.

---

## 4. CLI and config

### Flags

```text
--only-gaps <LIST>   Patch only these gaps (1-based gap numbers from the table; comma-separated)
--skip-gaps <LIST>   Patch all fillable gaps except these (same gap-number semantics)
```

TOML (`[repair]`):

```toml
only_gaps = ["2", "4", "5"]
# skip_gaps = ["3"]   # mutually exclusive with only_gaps
```

CLI overrides TOML when both are present (same pattern as other repair flags).

### Token type: strings from day one (not `Vec<usize>`)

v1 accepts **only** integer tokens, but the stored type on both `Args` and `RepairConfig` is
`Option<Vec<String>>`, parsed into gap numbers during resolve. Typing v1 as `Vec<usize>` would force a
breaking type change on the TOML key when v1.5 adds range tokens (`"6128.25-6360.0"`). Cost today is
one `parse::<usize>()` with a friendly error; cost of deferring is a config-compat break.

TOML numbers are **not** accepted in v1 (`only_gaps = [2, 4, 5]` fails serde into `Vec<String>`).
Use quoted strings. An untagged `GapSelectorToken` remains optional follow-up if bare integers prove
painful; string form stays normative for range tokens in v1.5.

### Token parsing

Tokens are **gap numbers** (labels), not positions in a filtered list — §2.1.

- Comma-separated positive integers: `3`, `2,4,5`, ` 2 , 4 `.
- Validate: `1 ≤ n ≤ report.gaps.len()`; duplicate tokens → **error**.
- Out-of-range → fail fast with: `gap number 7 out of range (6 gaps detected)`.
- `0` → `gap number 0 is invalid (gap numbers are 1-based)`.
- Both strings are **verbatim** the ones `resolve_fingerprint_gap_select` already ships. One message
  shape across both surfaces; if either is ever reworded, change **both** together.
- Resolution is order-insensitive and unions across tokens.
- Empty list is **not** the same as absent for `--only-gaps` / `only_gaps`: `only_gaps = []`
  (or `--only-gaps ""` yielding no tokens) resolves to "nothing selected" → the §3 empty-selection
  error (except on an empty report, where the empty set is vacuous success).
- Empty `--skip-gaps` / `skip_gaps = []` means skip nothing → all gaps selected (filter present,
  note silent because every gap remains). Documented deliberate asymmetry vs empty only.

### Index base: 1-based

`--only-gaps` is **1-based** (matching the stdout `#` column), per the crate rule: data is 0-based and
positional; CLI arguments and rendered text are 1-based. `--fingerprint-gap` was reconciled onto the
same base by the shipped index-convention PR, so v1 **inherits** the base rather than establishing it.

Range tokens (`START-END`, `START..END`) are v1.5 —
[TEMP-gap-selection-ranges-plan.md](TEMP-gap-selection-ranges-plan.md).

---

## 5. Implementation sketch

### 5.1 Types

```rust
/// Resolved after scan, before fill plan. **0-based** — same base as `FillRegion::gap_index`.
pub struct GapSelection {
    /// 0-based indices into `GapReport.gaps` (chronological). Empty set is unreachable:
    /// resolution errors out first (§3), so `is_selected` never silently plans nothing.
    selected: HashSet<usize>,
}

impl GapSelection {
    /// Every gap selected — the default when no flag is given.
    pub fn all(gap_count: usize) -> Self { /* 0..gap_count */ }
    pub fn is_selected(&self, gap_index: usize) -> bool { self.selected.contains(&gap_index) }
    /// True when a selection flag was actually in effect (drives the §5.5 stderr note).
    pub fn is_filtered(&self, gap_count: usize) -> bool { self.selected.len() != gap_count }
}

/// Unresolved user intent; tokens are still strings (§4).
pub enum GapSelectionMode {
    All,
    Only(Vec<String>),
    Skip(Vec<String>),
}
```

**Index base:** user-facing tokens are 1-based; `GapSelection` stores **0-based** indices, converted
once during resolve. Every internal gap identity in the crate (`FillRegion::gap_index`,
`GapFillSkipped::gap_index`, `report.gap_equivalence_at(index)`) is 0-based, so a 1-based set would put
an `i + 1` conversion inside the plan loop and invite an off-by-one at each new call site. Convert at
the boundary, once.

Parse CLI/TOML → `GapSelectionMode` in infrastructure; resolve to `GapSelection` once `GapReport`
exists (§5.6).

### 5.2 Fill plan hook

Extend — do **not** replace `skip_equivalent_gaps`:

```rust
pub fn build_gap_fill_plan(
    report: &GapReport,
    crossfade_ms: u64,
    skip_equivalent_gaps: bool,
    selection: &GapSelection,
) -> GapFillPlan
```

`FillRegion` / `GapFillSkipped` already carry `gap_index: usize` (0-based report index). After the
existing fillability / coverage / equivalence checks, if `!selection.is_selected(index)`, push:

```rust
GapFillSkipped {
    gap_index: index,
    a_start_secs: g.video_a_start_secs,
    a_end_secs: g.video_a_end_secs,
    reason: GapFillSkipReason::GapNotSelected,
}
```

**Order of precedence** (same gap, first matching reason wins — matches current loop order):

1. Plan-block: `TrackCompatibilityUnavailable` / `TrackLayoutMismatch`
2. `NotFillable`
3. `OutsideReferenceCoverage` (when `limit_fill_to_mapped_region`)
4. `AlreadyMatchesReference` (when `skip_equivalent_gaps` and verdict drops)
5. **`GapNotSelected`** (selection only applies to gaps that would otherwise enter `regions`)

So: selected-but-equivalent → `already_matches_reference`, **not** `gap_not_selected`. Equivalence
beats selection (same rule as
[archive/TEMP-gap-equivalence-plan.md](archive/TEMP-gap-equivalence-plan.md)).

**Plan-block arm is not selection-aware — by design.** `build_gap_fill_plan` early-returns before the
per-gap loop when `track_compatibility` is `None` or `Mismatch`, marking every gap with the block
reason (or `NotFillable`). Selection is never consulted there: with no compatible track layout,
*nothing* is patchable, and reporting `gap_not_selected` for gaps the user did select would be a lie.
Stated here so a later reader does not "fix" the early return into a selection-aware one.

### 5.3 New skip reason

`domain/patch_result.rs`, alongside existing `AlreadyMatchesReference`:

```rust
pub enum GapFillSkipReason {
    // ... existing ...
    AlreadyMatchesReference,
    GapNotSelected,
}
```

| Surface | Value |
|---------|-------|
| `format_plan_skip_reason` / JSON `plan_skip_reason` | `gap_not_selected` |
| Human `format_fill_skip_reason` | `gap not selected` |
| `GapTags` / `PlanKind` | `not_planned` (same arm as coverage / track / equivalence plan-time skips) |

Update [json-output.md](../json-output.md) § `GapFillSkipReason` and golden fixtures.

### 5.4 Progress and logging — v1 owes nothing

The shipped index convention already made every gap number on these surfaces a report identity and
split identity from count:

```text
gap #4 (3 of 6 planned): A 1:42:08 – 1:46:00
```

Skip / marginal warn lines use `region.gap_index` via `format_skip_gap_fill_log` (M-GAPKEY — no
`gap_key` HashMap). Progress-bar args and the `Aligning N fill region(s)` phase line are unchanged.
**v1's only obligation is not regressing this** — selection changes which gaps reach `regions`, and the
display already reports report identity correctly. See
[archive/TEMP-gap-index-convention-plan.md](archive/TEMP-gap-index-convention-plan.md).

### 5.5 Filter note / `repairable_count`

`GapReport::repairable_count()` stays "all gaps that *could* be patched" (no selection; also ignores
the equivalence drop today — leave that semantics alone). Add a stderr note when selection is active:

```text
Gap filter: patching 3 of 6 detected gaps (only-gaps: 2,4,5)
```

**Emission site:** `run_repair.rs`, immediately after `resolve_gap_selection` succeeds (§5.6) — the
`progress` handle is already in scope there. Use `progress.phase(...)`, matching the sibling
`format_scan_fillable_followup` line: this is an unconditional stderr note, not `phase_verbose`. Build
the string in `domain/gap_fill.rs` next to that sibling formatter so both scan-followup lines live
together.

The note fires only when `selection.is_filtered(report.gaps.len())` — an `--only-gaps` list that
happens to name every gap prints nothing.

Do not change scan-only `repairable_count` semantics.

> v1.5 also routes containment straddlers through this line — it is the only place that exclusion
> becomes visible when the selection is otherwise non-empty. See
> [TEMP-gap-selection-ranges-plan.md](TEMP-gap-selection-ranges-plan.md) § 3.

### 5.6 Wiring: where selection is stored, resolved, and validated

**The only production `build_gap_fill_plan` call site is inside `PatchAudio::run`**, which has access
to `request` and nothing else. There is no composition-level or orchestration-level call to pass a
`GapSelection` into — an earlier draft of this plan assumed one. Selection must therefore reach the
plan builder **on the request**, exactly as `skip_equivalent_gaps` does.

Three-stage flow:

| Stage | Where | Type | Why here |
|-------|-------|------|----------|
| **1. Parse** | `cli/args.rs` + `cli/mod.rs` → `RepairConfig` | `Option<Vec<String>>` ×2 (`only_gaps`, `skip_gaps`) | Same CLI-overrides-TOML pattern as every other repair knob |
| **2. Carry** | `RepairConfig::patch_settings()` → `PatchRequestSettings.gap_selection: GapSelectionMode` | unresolved | `patch_settings()` is the single "policy moves in whole" boundary; a new knob that skipped it would become a second source of truth |
| **3. Resolve + validate** | `application/run_repair.rs`, after `ScanGaps` returns the report | `GapSelection` on `PatchAudioRequest` | First point where both the intent and the gap count exist, and the last point that can still return `Result` |

**Stage 3 detail.** `PatchRequestSettings::into_request(report)` returns `Result<PatchAudioRequest, String>`
and **resolves** `gap_selection` against the report (bounds / duplicates / empty-selection). `run_repair`
maps `Err` to `RepairError::GapSelection`. Callers that only ever use `GapSelectionMode::All` can
`.expect("default All gap selection")`.

```rust
// run_repair.rs — both arms
let request = patch_settings
    .into_request(report)
    .map_err(RepairError::GapSelection)?;
if let Some(note) = format_gap_selection_filter_note(&request.gap_selection, request.report.gaps.len()) {
    progress.phase(&note);
}
```

**Blast radius (source audit, 2026-07-28).** Adding a field to `PatchRequestSettings` costs **one
edit**: `RepairConfig::patch_settings()`. Nothing else constructs one exhaustively — both literals in
the workspace end in a spread seed (`..RepairConfig::default().patch_settings()`, by deliberate design
of the config-bundles refactor), and the fixtures crates never build a literal at all; they call
`patch_settings()` and attach a report. A new field is inherited everywhere.

The real cost is `build_gap_fill_plan`'s new parameter: ~11 call sites (9 in `domain/gap_fill.rs`
tests, 2 in `tests/query_reference_integration.rs`) plus the production caller. All mechanical
(`GapSelection::all(n)`), single-crate, no cross-crate sweep.

**Diagnostic path is unaffected.** `dump_gap_fingerprints` builds a `PatchAudioRequest` but never calls
`build_gap_fill_plan` — it goes through `characterize_gaps_from_decode`, which does its own selection
via `--fingerprint-gap`. It inherits the new field harmlessly. Do **not** wire `--only-gaps` into it:
the two flags are separate user intents (which gaps to *patch* vs which to *characterize*), and both
already sit on the same 1-based gap-number base.

---

## 6. Interactions

### `anchored_retry`

`PatchAnchorCandidate::gap_index` and `PatchOffsetAnchor::source_gap_index` are **report** indices as
of the shipped index-convention PR; the region ordinal is `region_num`. Pass-2 retry still only sees
gaps that were **planned and attempted** in pass 1, so selection narrows the donor pool.

| Scenario | Behavior |
|----------|----------|
| Excluded gap was a strong anchor donor in a prior full run | Not available in this run; pass 2 may recover fewer gaps — **document**. |
| User excludes a gap that would have been retried | Expected; no special case. |

No v1 change to anchor table indexing.

### Scan-only

Selection flags do not change the scan table, but tokens are still **validated** against
`report.gaps` (same resolve as write/preview) so a mistyped `#` fails instead of exit 0.

### `--repair-preview`

Selection **applies** (fill plan is built; outcomes are characterize-only). Status strings use the same
`gap_not_selected` path as write mode.

### Equivalence (`skip_equivalent_gaps`)

Orthogonal user intent: automatic drop of mutual/ambient silence vs manual subset. Precedence in §5.2.
Selecting an equivalent gap does not force a patch when the gate is on.

### Profiles / `fill_mode`

Selection is independent of `repair_profile`, `fill_mode`, dual-fit, anchor/residual flags.

---

## 7. Non-goals

- **Cross-rescan gap-number preservation** without time ranges or a manifest file.
- **Silent remapping** of stale gap numbers onto a new gap list.
- **Partial scan** (only decode regions around selected gaps) — always full `ScanGaps`. §7.1.
- **Restricting *where gaps are detected*** to a time window — a distinct axis; §7.1.
- **Replacing** `limit_fill_to_mapped_region`, track-compatibility, or equivalence gates.
- **Reintroducing float `gap_key` joins** — use `gap_index` on plan structs.
- **`--gaps-from` manifest** —
  [TEMP-gap-selection-deferred.md](TEMP-gap-selection-deferred.md).

### 7.1 Selection vs. identification — two axes, one of them out of scope

A recurring design question: should there be a *second* time-window argument that limits **where gaps
are looked for**, alongside the selection flags that limit **which detected gaps are patched**?

They are genuinely different operations and must never share a flag. Selection filters an existing
report at fill-plan time and changes no gap's identity; an identification window changes the report
itself, shifting every `#`. They compose in sequence — `scan window → GapReport → selection →
GapFillPlan.regions` — and the only rule that needs stating is that **selection gap numbers always
refer to the post-window report**.

The window is **not in this plan**, and its cheap form is refused outright on measured grounds. Full
reasoning, shape, and the measurement anyone revisiting it must take first:
[TEMP-gap-selection-deferred.md](TEMP-gap-selection-deferred.md) § `--scan-window`.

---

## 8. Implementation checklist

No recipe prerequisite. [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) is parked; accept any
golden churn in this PR (beyond the new `plan_skip_reason` value). Hard scope rule: adjacent scan /
display / provenance defects discovered during prep go to BACKLOG or a tiny separate PR — see
[archive/TEMP-gap-selection-sequencing-plan.md](archive/TEMP-gap-selection-sequencing-plan.md) §4 and
[BACKLOG.md](../../BACKLOG.md) § Gap-selection parked debt.

- [x] `RepairConfig`: `only_gaps: Option<Vec<String>>`, `skip_gaps: Option<Vec<String>>` (string tokens, §4); validate mutual exclusivity in `RepairConfig::validate` (TOML path — clap cannot see config keys)
- [x] `Args`: `--only-gaps`, `--skip-gaps` (comma-separated, `conflicts_with` each other); wire in `cli/mod.rs`
- [x] `GapSelection` (0-based) / `GapSelectionMode` + `resolve_gap_selection(mode, report)` returning `Result<_, String>`; unit tests (out of range, non-integer token, empty list vs absent, skip vs only, duplicates → error)
- [x] Error strings **verbatim** from `composition.rs::resolve_fingerprint_gap_select` (`gap number 0 is invalid (gap numbers are 1-based)`, `gap number N out of range (M gaps detected)`)
- [x] `GapFillSkipReason::GapNotSelected` + all formatters / `gap_tags` mapping (`PlanKind::NotPlanned`) — three sites: `gap_tags::format_plan_skip_reason`, `gap_tags::derive_gap_tags_from_status` match arm, `cli/output::format_fill_skip_reason`
- [x] `build_gap_fill_plan(..., skip_equivalent_gaps, selection)` + domain tests (incl. precedence vs equivalence / coverage); ~11 call sites updated (§5.6)
- [x] **Wiring per §5.6:** `GapSelectionMode` on `PatchRequestSettings` (via `patch_settings()`); resolve in **`run_repair.rs`** both arms; resolved `GapSelection` as a direct field on `PatchAudioRequest` (`measure_residual` precedent). Not composition — the only `build_gap_fill_plan` caller is inside `PatchAudio::run`
- [x] `PatchRequestSettings` new field — **`config.rs:610` only**; all other constructions inherit it via spread seed or `patch_settings()` (§5.6)
- [x] Empty-selection error → `RepairError::GapSelection` (exit 2; suppresses JSON stdout); confirm the non-selection empty plan keeps its current `Ok` + "No gaps planned" behavior (§3)
- [x] Selection filter note (§5.5) in `domain/gap_fill.rs` next to `format_scan_fillable_followup`; emitted from `run_repair.rs` via `progress.phase(...)` only when `is_filtered`
- [x] **JSON suppression (§3):** on a selection error under `--format json`, print **nothing** on stdout (message on stderr, exit 2); human format keeps printing the table. Gate at the `print_repair_output` call in `print_repair_outcome` via an explicit signal — **not** by matching `Err(RepairError::Config)`, which `repair_videos.rs` also raises post-scan
- [x] Document the human-format exception in [cli-output.md](../cli-output.md) next to its failure table (stdout "Empty — no partial report"): post-scan selection/config errors still print the gap table because the operator needs `#` to fix the selection
- [x] `format_unified_gap_report` / patch summary: `not planned: gap not selected`
- [x] Golden / wire spelling pin for `plan_skip_reason: gap_not_selected` (serde unit + tags verbose); full-surface golden unchanged (no new status row required — additive enum value only, documented in [json-output.md](../json-output.md))
- [x] Docs: [gap-repair-guide.md](../gap-repair-guide.md) workflow, [cli-output.md](../cli-output.md) flags, [pipeline.md](../pipeline.md) fill-plan paragraph, [gap-fill-modes.md](../gap-fill-modes.md) cross-link
- [x] Integration test: 3-gap fixture, `--only-gaps 2`, assert gaps 1 and 3 unchanged on A, gap 2 patched, status strings correct

---

## 9. Test plan

| Layer | Cases |
|-------|-------|
| **Parse** | `only` / `skip` mutual exclusion (both CLI and TOML paths); out-of-range; `0`; non-integer token; duplicates → error; whitespace; `only_gaps = []` errors while absent selects all |
| **Identity semantics (§2.1)** | `--only-gaps 5,2` ≡ `2,5` (order-insensitive); a token names the same gap regardless of what else is in the list; bounds validate against `report.gaps.len()`, never against a filtered count; `--skip-gaps` selects exactly the report-set complement of the equivalent `--only-gaps` |
| **Plan** | All selected; none selected → error; skip all fillable; unfillable gap in `only` list still `not_fillable`, not `gap_not_selected` |
| **Plan-block arm** | Track mismatch + `--only-gaps 2`: every gap reports the block reason, **none** reports `gap_not_selected` (§5.2) |
| **Selection + coverage** | Gap outside query-reference region: `outside_reference_coverage` beats `gap_not_selected` |
| **Selection + equivalence** | Selected gap with drop verdict + `skip_equivalent_gaps`: `already_matches_reference` beats `gap_not_selected`; with the gate off, the same gap can be selected and planned |
| **Patch** | Subset patch leaves unselected samples identical to input A |
| **Preview** | `--repair-preview --only-gaps 2` shows `gap_not_selected` for others; no write |
| **Output** | Human + JSON `plan_skip_reason`. Display shape is already settled and tested by the index-convention PR — v1 only must not regress it: verbose characterize line `gap #<report> (<k> of <planned> planned)`, retry line identity-only, skip warn line `gap #<report> (<range>)` with **no** total, progress-bar denominator still region count, span carries `region_index` **and** `gap_index` (there is no `report_gap_num` field; an earlier draft invented one) |
| **Filter note** | Fires when filtered; silent when `--only-gaps` names every gap |
| **Regression** | No selection flags ⇒ plan, PCM, JSON, and summary counts byte-identical to pre-change (the `GapSelection::all` default path) |
| **Index base** | `--fingerprint-gap N` and `--only-gaps N` name the same gap |
| **Output on error** | `--format json` + a bad selection ⇒ **stdout empty**, message on stderr, exit 2; human format same run ⇒ gap table still printed, then the error, then exit 2 |

v1.5 cases: [TEMP-gap-selection-ranges-plan.md](TEMP-gap-selection-ranges-plan.md) § 6.

---

## 10. Settled decisions (v1)

Kept because each one has a reason someone would otherwise re-derive wrongly.

1. **Duplicate tokens → error** (2026-07-28). Under an identity reading (§2.1), naming the same gap
   twice cannot mean anything, so it is a typo and should say so.
2. **Token type `Vec<String>` from v1** (2026-07-28), even though v1 parses only integers, so v1.5
   range tokens do not break the TOML key's type (§4).
3. **Where `GapSelectionMode` lives** (2026-07-28, by source audit). The original recommendation was
   not implementable: the sole production `build_gap_fill_plan` caller is inside `PatchAudio::run`,
   which sees only `request`. Mode rides `PatchRequestSettings`; the resolved `GapSelection` is a direct
   field on `PatchAudioRequest`; resolution happens in `run_repair.rs` (§5.6).
4. **Tokens are identities, not counts** (2026-07-28). Forced by v1.5 containment tokens, which would
   otherwise make integers ambiguous inside a mixed list (§2.1).
5. **JSON output on a selection error: suppress** (2026-07-29). [cli-output.md](../cli-output.md)
   already makes it normative; a success-shaped document with exit 2 breaks the contract that makes
   "parse stdout only" safe. Human format keeps the table as a documented exception. Implementation
   must carry an explicit signal rather than matching `Err(RepairError::Config)` (§3).

Settled elsewhere: gap numbering / display split and the all-0-based rejection →
[archive/TEMP-gap-index-convention-plan.md](archive/TEMP-gap-index-convention-plan.md); the JSON
scan-params contract → [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md); range ε and containment →
[TEMP-gap-selection-ranges-plan.md](TEMP-gap-selection-ranges-plan.md).

---

## 11. Promotion / done criteria

- [x] Status **v1 done**; operator contract in [gap-repair-guide.md](../gap-repair-guide.md) §
      Iterative subset patching (identity labels, flags/TOML, empty-selection asymmetry, precedence
      vs fillability/coverage/equivalence, filter note, error shapes). Supporting homes:
      [cli-output.md](../cli-output.md) (filter note + JSON/human selection-error exception),
      [gap-vocabulary.md](gap-vocabulary.md) § Gap numbering (input identity rule),
      [json-output.md](../json-output.md) / [error-mapping.md](../error-mapping.md).
- [x] [pipeline.md](../pipeline.md) fill-plan section links the guide, not this TEMP file.
- [ ] Archive **this** file once v1.5 either ships or is abandoned; the ranges and deferred docs
      stand alone. Design rationale left here until then (§2.1 why containment forced identity,
      §5 wiring, §10 settled decisions).
