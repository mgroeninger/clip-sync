# Gap selection (subset patching) — plan (DRAFT)

Status: **not started**.

Refreshed **2026-07-27** against current source (M-GAPKEY, gap equivalence,
`PatchRequestSettings`, repair-preview, two-pass characterize/execute).
Wiring / validation / index-base sections corrected **2026-07-28** after a source audit
(§5.2, §5.4, §5.6, §4, §12) — the earlier draft named a `build_gap_fill_plan` call site that
does not exist and left index validation homeless.

Companions: [pipeline.md](../pipeline.md) § Orchestration / Fill plan,
[gap-fill-modes.md](../gap-fill-modes.md), [gap-repair-guide.md](../gap-repair-guide.md),
[cli-output.md](../cli-output.md), [json-output.md](../json-output.md),
[gap-vocabulary.md](gap-vocabulary.md) § Silence-character pre-gate.

Motivating use case: after a full repair run, the user wants to **patch only some gaps**
(iterative retry, partial write, or scripting) without re-running alignment/scan with different
inputs. Today every fillable (and non-equivalent) gap in the scan report enters
`GapFillPlan.regions`; there is no way to exclude gaps that are planned but should be left
untouched on A for this invocation.

---

## 1. Problem (one paragraph)

Write / repair-preview always plans and attempts every repairable gap that survives the
fillability, coverage, and (when enabled) equivalence gates. Iterative workflows (“patch
1,2,4,5 first; retry 3 with different flags”) require either editing source video, maintaining
multiple configs, or accepting a full re-patch. A **gap selection** layer at fill-plan time
would let the user name a subset while keeping the **full scan table** for context. Separately,
gap **`#` in the human report is a 1-based index into `GapReport.gaps`** — convenient within one
scan recipe, but **not stable** when `min_gap_ms` or other scan knobs change the detected run
list. Selection must document that contract and offer **time-range** tokens for cross-run
stability without pretending indices are global IDs.

---

## 2. Gap identity contract

| Handle | Stable across rescans? | Meaning |
|--------|------------------------|---------|
| **`#` (table index)** | No (when scan knobs change) | 1-based index into `report.gaps` in chronological order on A; matches stdout `#` column |
| **A time range** `(start_secs, end_secs)` | Yes (same A decode clock, same scan recipe) | Matches `Gap::video_a_start_secs` / `video_a_end_secs` |
| **Internal join key** | Per run | `FillRegion::gap_index` / `GapFillSkipped::gap_index` — **0-based** index into `GapReport.gaps` (M-GAPKEY, 2026-07-27). Prefer this over float timestamps; the old `gap_key` helper is **deleted** |
| **Below `min_gap_ms`** | N/A | Not in the report; no `#` |

**Rules for users and docs:**

- Copy `#` from **the table produced by this run** (or JSON `gaps[]` order).
- If `min_gap_ms`, `silence_hold_ms`, `scan_block_ms`, `silence_peak_fraction`, or
  `absolute_silence_rms` will change before the next patch attempt, record **Range** (or JSON
  `video_a_start_secs` / `video_a_end_secs`) and use a **range token** (v1.5), not a remembered `#`.
- Do **not** silently remap old indices onto a new scan.

**Scan params echo (v1):** JSON `GapScanJson` today carries `scan_block_ms` and
`silence_peak_fraction` but not `min_gap_ms`, `silence_hold_ms`, or `absolute_silence_rms`.
Those knobs live on `RepairConfig` and are used by `ScanGaps`, but are **not** stored on
`GapReport` today — extend the report (or pass them alongside) when adding the JSON fields so
scripts can verify they are on the same scan recipe before reusing a saved gap list.

---

## 3. User-facing semantics

| Rule | Detail |
|------|--------|
| **Patch / preview modes** | Flags apply when a fill plan is built: `--wav` / `--mux` **and** `--repair-preview`. Scan-only runs ignore selection (no fill plan). |
| **Full scan always** | Phases 1–2 unchanged; stdout/JSON still list **all** detected gaps. |
| **Filter at plan time** | Hook: `build_gap_fill_plan` (`domain/gap_fill.rs`). Unselected gaps never enter `regions`. |
| **Audio on A** | Unselected gaps keep **original A** audio in the output (no splice). Preview never splices; status still shows `gap_not_selected`. |
| **Fillability unchanged** | Selection is orthogonal to `fillable` / `unfillable` / `not planned` for track mismatch, B energy, query-reference coverage, and equivalence. |
| **Mutual exclusivity** | `--only-gaps` and `--skip-gaps` cannot both be set (clap `conflicts_with`). |
| **Empty selection** | If resolution yields zero regions and no plan-block reason, exit with a clear error before patch/preview. **Deliberate asymmetry** — see below. |

**Empty-plan asymmetry (deliberate).** Today an empty fill plan is *not* an error:
`patch_audio/mod.rs` (`if plan.regions.is_empty()`) prints `No gaps planned for patch; skipping audio
decode.` and returns `Ok` with an all-`NotPlanned` summary. That stays. Selection is different because
an empty result means the **user's own arguments** selected nothing patchable — a silent success there
is indistinguishable from "worked". So: selection-caused emptiness errors (`RepairError::Config`,
exit **2**), everything else keeps the existing `Ok` + phase-line behavior. This check lives in the
post-scan resolve step (§5.6), *before* the plan is built, so the error mentions the selection, not the plan.

**Status column (write / preview):** unselected gaps that would otherwise be repairable show
`not planned: gap not selected` (machine: `gap_not_selected`).

**Summary counts.** Plan-time skips flow into `GapPatchStatus::NotPlanned` and therefore into
`PatchSummary::not_planned_count`, which the human summary prints. `--only-gaps 2` on a 6-gap run
will read `… 5 not planned` next to the §5.5 filter note. That is correct and intended — no special
casing; the filter note is what disambiguates it.

---

## 4. CLI and config (v1)

### Flags

```text
--only-gaps <LIST>   Patch only these gaps (1-based report indices; comma-separated)
--skip-gaps <LIST>   Patch all fillable gaps except these (same index semantics)
```

TOML (`[repair]`):

```toml
only_gaps = ["2", "4", "5"]
# skip_gaps = ["3"]   # mutually exclusive with only_gaps
```

CLI overrides TOML when both present (same pattern as other repair flags in `cli/mod.rs`).

### Token type: strings from day one (not `Vec<usize>`)

v1 accepts **only** integer tokens, but the stored type on both `Args` and `RepairConfig` is
`Option<Vec<String>>`, parsed into indices during resolve. Typing v1 as `Vec<usize>` would force a
breaking type change on the TOML key when v1.5 adds range tokens (`"6128.25-6360.0"`). Cost today is
one `parse::<usize>()` with a friendly error; cost of deferring is a config-compat break. Decided —
see §12.6.

TOML numbers are also accepted for ergonomics (`only_gaps = [2, 4, 5]`) via a serde-untagged
`GapSelectorToken { Index(usize), Token(String) }` if that proves cheap; the string form is normative.

### Index parsing

- Comma-separated positive integers: `3`, `2,4,5`, ` 2 , 4 `.
- Validate: `1 ≤ index ≤ report.gaps.len()`; duplicate tokens → **error** (see §12).
- Out-of-range → fail fast with: `gap index 7 out of range (6 gaps detected)`.
- Empty list is **not** the same as absent: `only_gaps = []` (or `--only-gaps ""`) resolves to
  "nothing selected" → the §3 empty-selection error. Absent / `None` → `GapSelectionMode::All`.

### Index base: 1-based, and `--fingerprint-gap` must be reconciled

`--only-gaps` is **1-based** (matches the stdout `#` column, `output.rs` `"  #{:<3} …"`).

The existing calibration flag `--fingerprint-gap` (`cli/args.rs`) is **0-based** — it is consumed as
`select.contains(i)` against the 0-based `enumerate()` index in
`gap_fingerprint/measure.rs::characterize_gaps` — even though its own doc comment tells the user to
"use the normal repair gap table to pick which gaps", and that table is 1-based. So today
`--fingerprint-gap 3` and the proposed `--only-gaps 3` would name **different gaps**.

**Decision (v1, same PR):** make `--fingerprint-gap` 1-based so every user-facing gap index in the
tool means the table `#`. It is a calibration-only flag behind the `calibration` feature with no
golden-output dependency, so the change is cheap; convert at the boundary in `composition.rs`
(`dump_gap_fingerprints`) rather than inside `characterize_gaps`, keeping the internal `select` slice
0-based. Update the flag's doc comment to say "1-based, as shown in the gap table".

### v1.5 extension (same flag, mixed tokens)

Auto-detect per token:

| Token shape | Resolution |
|-------------|------------|
| Integer `N` | Report index `N` |
| `START-END` | Seconds (`6128.25-6360.0`) or `H:MM:SS` / `H:MM:SS.mmm` using existing `format_timestamp` display conventions |

Range match (default **strict**): gap edges within ε (e.g. 50 ms) of parsed start/end.
(`TIME_EPS_SECS = 1e-9` is for wall-clock equality guards only — **not** the product ε for
range tokens.) Unmatched range → error listing detected gaps (no silent skip).

`--only-gaps` and `--skip-gaps` accept the same token grammar; skip resolves ranges to indices first, then subtracts.

#### `START-END` is a gap *identity*, not a *window* — and needs a companion that is

**A `START-END` token selects exactly one gap: the one whose own edges match.** It is a rescan-stable
spelling of a single `#`, which is what §2's identity contract asks for. It does **not** select every
gap falling inside the interval — a token spanning three gaps matches zero and errors.

That is a defensible default but a bad guess at user intent: `--only-gaps 1:42:00-1:50:00` reads as
"patch everything in that stretch" to almost everyone. Both behaviors are wanted, for different jobs,
and they must not share a syntax:

| Token | Semantics | Serves |
|-------|-----------|--------|
| `START-END` | **Strict identity.** Matches the single gap whose `video_a_start_secs` / `video_a_end_secs` are both within ε. No match → **error**. | §2 cross-rescan stable handle. Errors *loudly* when the scan recipe moved the gap — the whole point of preferring ranges over remembered indices |
| `START..END` | **Containment.** Selects every gap whose A window lies entirely within `[START − ε, END + ε]`. Zero matches → **error** (empty selection, §3). | "patch this whole stretch", bulk exclusion via `--skip-gaps` |

Keeping them distinct preserves the §2 rule that stale handles must never silently remap: under
containment, a gap that shifted or split still lands inside a wide window and is quietly selected —
acceptable when the user asked for a region, unacceptable when they meant "that specific gap".

Containment tokens are order-insensitive on overlap (a gap matched by several tokens is selected once)
and compose freely with index and identity tokens in the same list.

**Neither token restricts the scan.** `START..END` selects from gaps that were already detected across
the whole file; it does not stop gaps outside the interval from being *found* and reported. Limiting
detection to a window is a different axis, deliberately out of scope — §7.1.

**Open:** whether containment requires the gap to be *fully* inside the window (recommended — an
unambiguous rule) or merely to *overlap* it. Full containment can surprise at edges; overlap can pull
in a gap that mostly sits outside the requested stretch. Recommend full containment with the ε slack,
and an error message that names any gap that overlapped but was excluded, so the surprise is visible
rather than silent. Deferred to v1.5 (see §12.7).

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

**Index base:** the user-facing tokens are 1-based; `GapSelection` stores **0-based** indices, converted
once during resolve. Every internal gap identity in the crate (`FillRegion::gap_index`,
`GapFillSkipped::gap_index`, `report.gap_equivalence_at(index)`) is 0-based, so a 1-based set would put
an `i + 1` conversion inside the plan loop and invite an off-by-one at each new call site. Convert at
the boundary, once.

Parse CLI/TOML → `GapSelectionMode` in infrastructure; resolve to `GapSelection` once `GapReport`
exists (§5.6).

### 5.2 Fill plan hook

Current signature (2026-07-27):

```rust
pub fn build_gap_fill_plan(
    report: &GapReport,
    crossfade_ms: u64,
    skip_equivalent_gaps: bool,
) -> GapFillPlan
```

Extend — do **not** replace `skip_equivalent_gaps`:

```rust
pub fn build_gap_fill_plan(
    report: &GapReport,
    crossfade_ms: u64,
    skip_equivalent_gaps: bool,
    selection: &GapSelection,
) -> GapFillPlan
```

`FillRegion` / `GapFillSkipped` already carry `gap_index: usize` (0-based report index). After
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

So: selected-but-equivalent → `already_matches_reference`, **not** `gap_not_selected`.
Equivalence beats selection (same rule as archived [TEMP-gap-equivalence-plan.md](archive/TEMP-gap-equivalence-plan.md)).

**Plan-block arm is not selection-aware — by design.** `build_gap_fill_plan` early-returns before the
per-gap loop when `track_compatibility` is `None` or `Mismatch`, marking every gap with the block reason
(or `NotFillable`). Selection is never consulted there: with no compatible track layout, *nothing* is
patchable, and reporting `gap_not_selected` for gaps the user did select would be a lie. Stated here so
a later reader does not "fix" the early return into a selection-aware one.

### 5.3 New skip reason

`domain/patch_result.rs` (alongside existing `AlreadyMatchesReference`):

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

### 5.4 Progress and logging — remaining report `#` sites

**Already fixed (M-GAPKEY):** skip / marginal warn lines use `region.gap_index` (report position)
via `format_skip_gap_fill_log` — no `gap_key` HashMap needed.

**Still wrong today:** characterize and execute loops in `patch_audio/mod.rs` enumerate
`plan.regions` and set:

```text
gap_num = index + 1   # region ordinal, not report #
```

for `progress("patch-characterize" | "patch-gap", …)`, verbose `gap N/M: A …` lines, and the
tracing span field `gap_index`.

**Fix (v1, same PR):** use `region.gap_index + 1` as the displayed / spanned report `#`.

Recommended verbose shape:

```text
gap 4/6 (1:42:08 – 1:46:00): …   # report gap 4 of 6 detected
```

**Numerator and denominator come from different sources — today they share one variable.** Both loops
currently derive `gap_num` and the `N/M` denominator from the same `region_count` (`plan.regions.len()`).
Under selection that produces nonsense like `gap 4/2`. The rule:

| Surface | Numerator | Denominator |
|---------|-----------|-------------|
| Verbose `gap N/M: A …` | `region.gap_index + 1` (report `#`) | `report.gaps.len()` (**new** — not `region_count`) |
| `progress("patch-characterize" / "patch-gap", …)` bar | loop ordinal `index + 1` (**unchanged**) | `region_count` (**unchanged**) |
| Phase line `Aligning N fill region(s)` | — | planned count (**unchanged**) |

The progress bar must keep counting *work*, not report positions, or it stalls and jumps under selection.
Only the verbose text switches to report identity, so the two must stop sharing `region_count`.

**Do not repurpose the `patch_gap` span field `gap_index`.** It is currently emitted as the region
ordinal (`gap_index = gap_num`). Silently changing its meaning breaks any log query keyed on it while
the field name and type stay identical — the worst kind of break. Instead:

```rust
tracing::info_span!(
    "patch_gap",
    region_index = gap_num,              // renamed: was `gap_index`, same value
    gap_index = region.gap_index,        // NEW: 0-based report index, matches FillRegion::gap_index
    report_gap_num = region.gap_index + 1, // report `#` as displayed
    region_count,
    …
)
```

A rename is a loud break (queries stop matching and the operator investigates); a redefinition is a
quiet one. If a rename is judged too disruptive, the fallback is to leave `gap_index` alone and add
only `report_gap_num` — but then note in the span docs that `gap_index` is a region ordinal, since
that contradicts `FillRegion::gap_index`.

- Do **not** change `PatchAnchorCandidate::gap_index` — that remains a **region** index into
  `plan.regions` (deliberate dual meaning; see §6).

### 5.5 `repairable_count` / scan followup

`GapReport::repairable_count()` stays “all gaps that *could* be patched” (no selection; also
ignores the equivalence drop today — leave that semantics alone). Optional stderr when
selection active:

```text
Gap filter: patching 3 of 6 detected gaps (only-gaps: 2,4,5)
```

Do not change scan-only `repairable_count` semantics.

The note fires only when `selection.is_filtered(report.gaps.len())` — a `--only-gaps` list that happens
to name every gap prints nothing.

### 5.6 Wiring: where selection is stored, resolved, and validated

**The only production `build_gap_fill_plan` call site is inside `PatchAudio::run`**
(`application/patch_audio/mod.rs`, step 1), which has access to `request` and nothing else. There is no
composition-level or orchestration-level call to pass a `GapSelection` into — an earlier draft of this
plan assumed one. Selection must therefore reach the plan builder **on the request**, exactly as
`skip_equivalent_gaps` does.

Three-stage flow:

| Stage | Where | Type | Why here |
|-------|-------|------|----------|
| **1. Parse** | `cli/args.rs` + `cli/mod.rs` → `RepairConfig` | `Option<Vec<String>>` ×2 (`only_gaps`, `skip_gaps`) | Same CLI-overrides-TOML pattern as every other repair knob |
| **2. Carry** | `RepairConfig::patch_settings()` → `PatchRequestSettings.gap_selection: GapSelectionMode` | unresolved | `patch_settings()` is the single "policy moves in whole" boundary; a new knob that skipped it would become a second source of truth |
| **3. Resolve + validate** | `application/run_repair.rs`, after `ScanGaps` returns the report | `GapSelection` on `PatchAudioRequest` | First point where both the intent and the gap count exist, and the last point that can still return `Result` |

**Stage 3 detail.** `PatchRequestSettings::into_request(report)` returns `PatchAudioRequest`, *not*
`Result`, so it cannot host validation. Do not change its signature — it is the deliberate
"policy moves in whole, no per-field copy list" seam. Instead resolve just before it, in the two
`run_repair` arms that already hold the report (`run_preview` and `into_write_request`):

```rust
// run_repair.rs — both arms
let selection = resolve_gap_selection(&patch_settings.gap_selection, &report)
    .map_err(RepairError::Config)?;      // exit 2 via exit_code_for
let mut request = patch_settings.into_request(report);
request.gap_selection = selection;        // per-run resolved value, like `measure_residual`
```

`gap_selection` on `PatchAudioRequest` follows the `measure_residual` precedent: a per-run field set
directly on the request rather than through `Deref`, defaulting to `GapSelection::all(gap_count)` so
every existing constructor keeps compiling with unchanged behavior.

`resolve_gap_selection` owns all four failures — non-integer token, out of range, duplicate, and
empty result (§3) — and is where the 1-based → 0-based conversion happens. Mutual exclusivity is caught
earlier and twice: clap `conflicts_with` for the CLI, and `RepairConfig::validate` for the TOML path
(clap cannot see config-file keys). Range/count validation **cannot** live in `validate` — it runs
pre-scan, where `report.gaps.len()` does not exist yet.

**Blast radius.** `PatchRequestSettings` gains a field, so every literal construction needs it — four
outside this crate's own module tests:

- `crates/clip-sync-repair-harness/src/patch_audio.rs`
- `crates/clip-sync-repair-fixtures/src/gap_corpus_fixtures.rs`
- `crates/clip-sync-repair-fixtures/src/energy_signature_production.rs` (`patch_request_from_repair`)
- `crates/clip-sync-repair/tests/query_reference_integration.rs`

plus `build_gap_fill_plan`'s new parameter at ~11 call sites (9 in `domain/gap_fill.rs` tests, 2 in
`tests/query_reference_integration.rs`). All mechanical (`GapSelectionMode::All` / `GapSelection::all(n)`),
but it spans four crates — budget for it.

**Diagnostic path is unaffected.** `dump_gap_fingerprints` (`composition.rs`) builds a
`PatchAudioRequest` but never calls `build_gap_fill_plan` — it goes through
`characterize_gaps_from_decode`, which does its own selection via `--fingerprint-gap`. It will inherit
the new field harmlessly. Do **not** wire `--only-gaps` into it: the two flags are separate user
intents (which gaps to *patch* vs which to *characterize*), and §4 already reconciles their index base.

---

## 6. Interactions

### `anchored_retry`

`build_patch_anchor_candidates` still uses `gap_index` as **index into `plan.regions`**, not
report `#` (`patch_audio/anchor_retry.rs`). That is unrelated to `FillRegion::gap_index` (report
index). Pass-2 retry only sees gaps that were **planned and attempted** in pass 1.

| Scenario | Behavior |
|----------|----------|
| Excluded gap was a strong anchor donor in a prior full run | Not available in this run; pass 2 may recover fewer gaps — **document**. |
| User excludes gap that would have been retried | Expected; no special case. |

No v1 change to anchor table indexing; optional v2 note if we expose anchor donor by report `#`.

### Scan-only

Selection flags ignored; no `GapNotSelected` in output.

### `--repair-preview`

Selection **applies** (fill plan is built; outcomes are characterize-only). Status strings use the
same `gap_not_selected` path as write mode.

### Equivalence (`skip_equivalent_gaps`)

Orthogonal user intent: automatic drop of mutual/ambient silence vs manual subset. Precedence
in §5.2. Selecting an equivalent gap does not force a patch when the gate is on.

### Profiles / `fill_mode`

Selection is independent of `repair_profile`, `fill_mode`, dual-fit, anchor/residual flags.

---

## 7. Non-goals

- **Cross-rescan index preservation** without time ranges or a manifest file.
- **Silent remapping** of stale indices onto a new gap list.
- **Partial scan** (only decode regions around selected gaps) — always full `ScanGaps`. See §7.1.
- **Restricting *where gaps are detected*** to a time window — a distinct axis from selection; §7.1.
- **Replacing** `limit_fill_to_mapped_region`, track-compatibility, or equivalence gates.
- **Reintroducing float `gap_key` joins** — use `gap_index` on plan structs.
- **v2 `--gaps-from` manifest** in v1 (see §9).

### 7.1 Selection vs. identification — two axes, one of them out of scope

A recurring design question: should there be a *second* time-window argument that limits **where gaps
are looked for**, alongside the selection flags that limit **which detected gaps are patched**? They
are genuinely different operations and should never share a flag:

| Axis | What it does | Effect on gap identity |
|------|--------------|------------------------|
| **Selection** (`--only-gaps` / `--skip-gaps`, this plan) | Filters an existing report at fill-plan time | **None.** The report is unchanged; every `#` still means what the table says |
| **Identification window** (hypothetical `--scan-window`, §7.2) | Restricts `ScanGaps` to an interval, so gaps outside it are never detected | **Changes the report.** It is a scan knob in the same family as `min_gap_ms` — every `#` shifts |

**They are not mutually exclusive, and cannot produce conflicting lists.** They compose in sequence:

```text
scan window  →  GapReport (the detected list)  →  selection  →  GapFillPlan.regions
```

The only rule this needs: **selection indices always refer to the post-window report.** A window is
applied first and produces the list; selection then filters that list. `--scan-window 1:00:00-1:30:00
--only-gaps 2` means "the second gap found inside that window", never "report gap 2, if it happens to
fall in the window". The risk of two competing gap lists is a documentation problem, not a semantic
conflict — provided a window, if ever added, joins the scan-params echo (§2) and the v2 manifest's
`scan` recipe (§9) so a saved gap list can be validated against the recipe that produced it.

**Why the window is not in this plan** — beyond scope discipline, its motivating benefit does not
survive measurement; see §7.2.

### 7.2 Deferred sketch: `--scan-window` (not planned — recorded so it is not rediscovered)

Shape, if it were ever built:

```text
--scan-window <START-END>   Detect gaps only within this A-timeline interval
```

- A **scan** knob, not a repair knob: lives on `ScanGapsRequest` next to `min_gap_secs` /
  `silence_hold_blocks`, set from `RepairConfig` in `composition.rs::repair_run_input`.
- Must be echoed in `GapScanJson` alongside the §2 scan-params work, and embedded in the §9 manifest
  `scan` block — otherwise a saved gap list cannot be checked against its recipe.
- Named `--scan-window`, **never** as a mode of `--only-gaps`. Sharing a flag between "which gaps
  exist" and "which gaps to patch" is exactly the conflation that makes the two-lists confusion real.
- Alignment is unaffected: it needs broad coverage regardless, so the window applies to gap detection,
  not to `Aligner`.

**Why it is deferred: the perf argument is weaker than it looks.** From
[repair-perf.md](repair-perf.md) §1c (17-pair, post-lever-1b(b), root span `patch_audio`):

| Cost | Share of `patch_audio` | Sensitive to gap selection? |
|------|------------------------|------------------------------|
| `char_gate_search` (inclusive) | 73.8% | **Yes** — per gap. Selection already removes it for unselected gaps |
| `patch_decode_a` + `patch_decode_b` | ~25–35% | **No** — 17 calls for 17 pairs. Per *file*, not per gap |

So there are two different features hiding here:

1. **A window that narrows only *detection*** (decode everything as usual, just don't report gaps
   outside the interval) reaches **none** of the decode cost. It saves the silence-scan pass over A and
   nothing else — while adding a scan knob that destabilizes every `#` precisely where selection is
   trying to make indices dependable. Bad trade; this is the version to refuse.
2. **A window that narrows *decode*** is the one that would pay, because decode is the only large cost
   selection cannot touch. But that is the "partial scan" non-goal above — a perf project, not a CLI
   flag. It is tractable (the B-side haystack for a gap at 1:42 in A sits near 1:42 in B via the
   offset) but carries real correctness risk at the alignment boundary, and the **scan phase's own cost
   is not in the profile tables at all** — the measured root is `patch_audio`. That gap in the data
   must be closed before anyone commits to the work.

**If this is ever revisited, measure first:** instrument `ScanGaps` as a sibling root to `patch_audio`
and record scan-vs-patch share in [repair-perf.md](repair-perf.md). Without that number the payoff is
unknown — and the standing rule in that doc is that the *numbers* decide, not projections.

**Note for the iterative workflow that motivates this plan (§1):** every invocation re-runs the full
scan, so `--only-gaps` does not spare that cost. The lever for *that* is the v2 `--gaps-from` manifest
(reuse a prior gap list instead of re-deriving it), not a scan window.

---

## 8. Phased delivery

| Phase | Scope |
|-------|-------|
| **v1** | `--only-gaps` / `--skip-gaps` (indices only, string-typed tokens); `GapNotSelected`; report `#` on remaining progress/span sites; `--fingerprint-gap` → 1-based; scan params in JSON; user docs; repair-preview applies selection |
| **v1.5** | Mixed tokens on same flags: `START-END` identity + `START..END` containment; range parser, strict-match and containment tests |
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
entries resolve by matching `video_a_*_secs` (within ε) onto report gaps / their `gap_index`.
Accept full `RepairJsonOutput` as a convenience alias.

---

## 10. Implementation checklist (v1)

- [ ] `RepairConfig`: `only_gaps: Option<Vec<String>>`, `skip_gaps: Option<Vec<String>>` (string tokens, §4); validate mutual exclusivity in `RepairConfig::validate` (TOML path — clap cannot see config keys)
- [ ] `Args`: `--only-gaps`, `--skip-gaps` (comma-separated, `conflicts_with` each other); wire in `cli/mod.rs`
- [ ] `GapSelection` (0-based) / `GapSelectionMode` + `resolve_gap_selection(mode, report)` returning `Result<_, String>`; unit tests (out of range, non-integer token, empty list vs absent, skip vs only, duplicates → error)
- [ ] `GapFillSkipReason::GapNotSelected` + all formatters / `gap_tags` mapping (`PlanKind::NotPlanned`) — three sites: `gap_tags::format_plan_skip_reason`, `gap_tags::derive_gap_tags_from_status` match arm, `cli/output::format_fill_skip_reason`
- [ ] `build_gap_fill_plan(..., skip_equivalent_gaps, selection)` + domain tests (incl. precedence vs equivalence / coverage); ~11 call sites updated (§5.6)
- [ ] **Wiring per §5.6:** `GapSelectionMode` on `PatchRequestSettings` (via `patch_settings()`); resolve in **`run_repair.rs`** both arms; resolved `GapSelection` as a direct field on `PatchAudioRequest` (`measure_residual` precedent). Not composition — the only `build_gap_fill_plan` caller is inside `PatchAudio::run`
- [ ] Empty-selection error → `RepairError::Config` (exit 2); confirm the non-selection empty plan keeps its current `Ok` + "No gaps planned" behavior (§3)
- [ ] `PatchRequestSettings` new field in 4 external constructors: repair-harness, both fixtures crates, `query_reference_integration.rs` (§5.6 blast radius)
- [ ] Report `#` on characterize / execute verbose lines with **`report.gaps.len()` denominator**; progress-bar numerator/denominator unchanged (§5.4 table)
- [ ] Tracing span: rename `gap_index` → `region_index`, add `gap_index` (0-based report) + `report_gap_num`; leave `PatchAnchorCandidate::gap_index` as region ordinal
- [ ] `--fingerprint-gap` → 1-based; convert at the `composition.rs` boundary, keep internal `select` 0-based; update its doc comment (§4)
- [ ] `GapReport` (+ `GapScanJson`): add `min_gap_ms`, `silence_hold_ms`, `absolute_silence_rms`; populate from scan request / config
- [ ] `format_unified_gap_report` / patch summary: `not planned: gap not selected`
- [ ] Golden JSON fixture update per [json-output.md](../json-output.md) revision rules
- [ ] Docs: [gap-repair-guide.md](../gap-repair-guide.md) workflow, [cli-output.md](../cli-output.md) flags, [pipeline.md](../pipeline.md) fill-plan paragraph, [gap-fill-modes.md](../gap-fill-modes.md) cross-link
- [ ] Integration test: 3-gap fixture, `--only-gaps 2`, assert gap 1 and 3 unchanged on A, gap 2 patched, status strings correct

---

## 11. Test plan

| Layer | Cases |
|-------|-------|
| **Parse** | `only` / `skip` mutual exclusion (both CLI and TOML paths); out-of-range; non-integer token; duplicates → error; whitespace; `only_gaps = []` errors while absent selects all |
| **Plan** | All selected; none selected → error; skip all fillable; unfillable gap in `only` list still `not_fillable` not `gap_not_selected` |
| **Plan-block arm** | Track mismatch + `--only-gaps 2`: every gap reports the block reason, **none** reports `gap_not_selected` (§5.2) |
| **Selection + coverage** | Gap outside query-reference region: `outside_reference_coverage` beats `gap_not_selected` |
| **Selection + equivalence** | Selected gap with drop verdict + `skip_equivalent_gaps`: `already_matches_reference` beats `gap_not_selected`; with gate off, same gap can be selected and planned |
| **Patch** | Subset patch leaves unselected samples identical to input A |
| **Preview** | `--repair-preview --only-gaps 2` shows `gap_not_selected` for others; no write |
| **Output** | Human + JSON `plan_skip_reason`; verbose `gap N/M` uses report `#` over **report total**; progress-bar denominator still region count; span carries `region_index` + `gap_index` + `report_gap_num` |
| **Regression** | No selection flags ⇒ plan, PCM, JSON, and summary counts byte-identical to pre-change (the `GapSelection::all` default path) |
| **Index base** | `--fingerprint-gap N` and `--only-gaps N` name the same gap after the 1-based fix (§4) |
| **v1.5** | `START-END` strict identity: matches one gap, spanning range errors; `START..END` containment selects every fully-enclosed gap; no match → error; mixed token list; overlapping containment tokens select once |

---

## 12. Open decisions

1. ~~**Progress denominator**~~ — **settled 2026-07-28.** Verbose lines: report `#` over report total; progress bar keeps loop ordinal over region count. The two currently share `region_count`; they must stop. Rule table in §5.4.
2. **Duplicate indices in CLI:** error vs last-wins — recommend **error**.
3. **JSON contract revision:** adding `min_gap_ms` / `silence_hold_ms` / `absolute_silence_rms` to `scan` is additive; bump [json-output.md](../json-output.md) changelog. Requires extending `GapReport` (or an adjacent carrier) — those fields are not on the report today.
4. **Range ε:** 50 ms default strict match — confirm against corpus edge refine behavior; do not reuse `TIME_EPS_SECS`.
5. ~~**Where `GapSelectionMode` lives**~~ — **settled 2026-07-28 by source audit.** The old recommendation was not implementable: the sole production `build_gap_fill_plan` caller is inside `PatchAudio::run`, which sees only `request`. Mode rides `PatchRequestSettings`; resolved `GapSelection` is a direct field on `PatchAudioRequest`; resolution happens in `run_repair.rs`. Full flow in §5.6.
6. ~~**Token type**~~ — **settled 2026-07-28.** `Vec<String>` on both `Args` and `RepairConfig` from v1, even though v1 parses only integers, so v1.5 range tokens do not break the TOML key's type (§4).
7. **Range token semantics (v1.5):** `START-END` = strict single-gap identity, `START..END` = containment. Open sub-question: containment requires *full* enclosure (recommended) vs mere *overlap*. Decide with a real corpus case before implementing; see §4.
8. **`--fingerprint-gap` 1-based conversion:** recommended for v1 (§4) so no two gap-index flags disagree. If the calibration workflow has scripted 0-based callers, the alternative is to leave it and document the split loudly — but that is a standing trap.

---

## 13. Promotion / done criteria

When v1 ships:

- Mark status **v1 done**; move user-facing contract from this file into [gap-repair-guide.md](../gap-repair-guide.md) and [cli-output.md](../cli-output.md).
- Keep v1.5/v2 sections here until implemented or moved to archive.
- Link from [pipeline.md](../pipeline.md) fill-plan section to the promoted user doc (not this TEMP file).
