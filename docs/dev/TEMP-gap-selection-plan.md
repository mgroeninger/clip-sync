# Gap selection (subset patching) — plan (DRAFT)

Status: **§0 implemented (uncommitted); v1 not started**.

### Readiness (2026-07-28)

| Phase | Ready? | Gated on |
|-------|--------|----------|
| **§0 prep PR** | **Yes — implement now.** All file/line references source-audited; no open decision touches it | — |
| **v1** | Yes, once §0 merges — *except* one scope split | §12.3 (JSON scan params: a `GapReport` change with a golden revision, independent of selection — **split into its own PR**); §12.11 (JSON document on a selection error) |
| **v1.5** | No | §12.4 (range ε), §12.7 (containment: full enclosure vs overlap) — both want a real corpus case first |
| **v2** | No | Depends on v1.5 |

Nothing else in §12 is open: 1, 2, 5, 6, 8, 9, 10 are settled and recorded with reasons.

Refreshed **2026-07-27** against current source (M-GAPKEY, gap equivalence,
`PatchRequestSettings`, repair-preview, two-pass characterize/execute).
Wiring / validation / index-base sections corrected **2026-07-28** after a source audit
(§5.2, §5.4, §5.6, §4, §12) — the earlier draft named a `build_gap_fill_plan` call site that
does not exist and left index validation homeless. A second audit pass the same day corrected the
§5.6 blast radius (overstated — one edit, not four crates), named the §5.5 emission site, and added
the §3 error-ordering caveat.

**§0 prep PR added 2026-07-28** — a gap-index convention (plus one live defect fix) that must merge
before v1. It absorbs the index-base and progress/span work formerly scoped into v1.
Its file:line references were re-verified against source **2026-07-28** (third pass): all land; the
pass corrected the `mod.rs:218`→`:285` arm, added the path-prefix convention, recorded the second
`--fingerprint-gap` consumer, and retired the json-output changelog item (field is undocumented).

**§2.1 added 2026-07-28** — selection tokens are identities, never counts. Settled ahead of v1.5
because containment tokens would otherwise make integers ambiguous inside a mixed list.

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

## 0. Prep PR: gap-index convention (land before v1)

Status: **implemented, uncommitted** (2026-07-28). Prerequisite — merge before selection work begins.
See the §10 §0 checklist for what landed and the two deviations from this section as written.

**Path convention in this section:** unqualified paths are relative to `crates/clip-sync-repair/src/`
(so `composition.rs` means `crates/clip-sync-repair/src/composition.rs`, **not** the same-named file in
`clip-sync-cli`); `tests/` is `crates/clip-sync-repair/tests/`. Line numbers re-verified 2026-07-28.

**Why separate.** Selection adds a fifth reason for `plan.regions` to be shorter than `report.gaps`,
which is the exact condition under which the existing gap-index defects become visible. Landing the
convention first keeps two mechanical renames and one value fix out of the selection diff, where a
reviewer could not distinguish a rename from a behavior change.

### 0.1 The rule

> **Data is 0-based and positional; only rendered text and CLI arguments are 1-based.**
> A gap's identity is always its position in `GapReport.gaps`, named `gap_index`. Anything indexing
> `plan.regions` is named `region_*` and is never called a gap. Identity and progress counts are
> never printed as one token.

### 0.2 Current state (source audit, 2026-07-28)

The codebase already follows the data/display split almost everywhere. This is a repair of two
defects, not a redesign.

| Surface | Base | Status |
|---------|------|--------|
| Table `#` column (`cli/output.rs:838`) | 1 | ✅ `i + 1` at the boundary |
| `(review gap #N)` (`cli/output.rs:331-339`, rendered `:360-363`) | 1 | ✅ `i + 1` |
| `skipped … (gap #N at …)` (`cli/output.rs:409`, rendered `:423`) | 1 | ✅ `i + 1` |
| Skip-fill log line (`patch_audio/log.rs:151`) | 1 | ✅ `gap_index + 1` |
| JSON | — | ✅ no gap-number field exists; pure array position |
| Internal structs / maps (`FillRegion`, `GapFillSkipped`, `GapFingerprint::index`, `GapRepairSpec`) | 0 | ✅ uniform |
| `--fingerprint-gap` | 0 | ❌ **defect 2** — the only user-facing gap number not on the table's base |
| `PatchOffsetAnchor::source_gap_index` | 1-rendered | ❌ **defect 1** — wrong *axis*, not wrong base |

**An all-0-based flip was considered and rejected.** It would change five correct display sites to
fix two broken ones, silently re-point every historical `gap #N` in the analysis notes and archived
TEMP docs by one, and would not remove the conversion boundary anyway: progress counts must run
`1..M` (`gap 0/6` first and `5/6` last is worse), so both bases would still meet on the same line —
just unlabeled. The footgun is the bare, axis-ambiguous number, which §0.3 D fixes directly.

### 0.3 Required changes

#### A. Fix the axis defect — `source_gap_index` names the wrong gap

`build_patch_anchor_candidates` (`application/patch_audio/anchor_retry.rs:35-39`) enumerates
`regions.iter().zip(region_results.iter())` and binds the **enumerate ordinal** to a variable named
`gap_index`, while discarding `region_results.0` — which *is* the report index (populated from
`region.gap_index` at `patch_audio/mod.rs:285`, the execute arm; the preview arm at `:218` does the
same but never reaches anchored retry):

```rust
.filter_map(|(gap_index, (region, (_, outcome, _)))| {
//             ^^^^^^^^^ region ordinal, misnamed
//                                    ^ the real report index, discarded
```

That ordinal flows to `PatchAnchorCandidate::gap_index` → `PatchOffsetAnchor::source_gap_index`
(`domain/patch_anchor.rs:77`), which is rendered as a report number and serialized.

- **Fix:** take the report index from `region_results.0` (equivalently `region.gap_index` — the same
  value by construction); the enumerate ordinal has no remaining use in that closure.
- **Failure today:** the value undercounts by the number of gaps plan-skipped at or before the
  anchor — always low, never high. With 6 gaps where `#2` is unfillable and `#5` is outside
  coverage, anchors from report `#3`/`#6` print as `gap #2, gap #4`. The tell: `#2` is the
  *unfillable* gap, which the same stdout reports as `not planned: not fillable` a few lines above —
  it was never attempted and cannot be an anchor donor.
- **Trigger:** any plan-time skip (`NotFillable`, `OutsideReferenceCoverage`,
  `AlreadyMatchesReference`, and after v1 `GapNotSelected`). With `skip_equivalent_gaps` on by
  default this fires routinely.
- **Not a repair-correctness bug.** Anchor math uses `a_secs` / `offset_secs` / `weight` only; every
  consumer of `source_gap_index` is a label or JSON (`patch_anchor.rs:247`, `:297` → `:331`, serde
  `PatchAnchorReport`). Offsets applied to media are unaffected.
- Update the doc comment on `PatchAnchorCandidate::gap_index` (`patch_anchor.rs:47`) to state it is a
  **report** index. The field name is correct *after* the fix and does not change.
- **Fingerprint system is not affected** — `PatchAnchorTable` / `PatchOffsetAnchor` appear nowhere in
  `application/gap_fingerprint/`, and the dump path never runs pass-2 anchored retry. Note the noun
  collision: "anchor" there means an *editorial seam* anchor (`domain::gap_anchor_seam::AnchorSource`,
  `AnchorSet`, anchor brackets) — an unrelated type on an unrelated axis.
- **JSON:** value correction on an existing field, no schema change. `PatchOffsetAnchor` derives
  `Serialize` and is exported as `PatchSummary::patch_anchors_used` (`patch_result.rs:374`), but
  **[json-output.md](../json-output.md) does not document that field today** and no golden JSON
  contains it (verified 2026-07-28). So there is no changelog entry to write: the value fix is
  invisible to the documented contract. Documenting `patch_anchors_used` is a separate, optional task.

#### B. Retire the `gap_index` name collision

| Site | Now | After |
|------|-----|-------|
| `patch_audio/mod.rs:181,245` | `let gap_num = index as u64 + 1` | `let region_num = index as u64 + 1` (stays `u64` — feeds `progress`) |
| `patch_audio/anchor_retry.rs:164` | `let gap_num = index + 1` | `let region_num = index + 1` (`usize` here, unlike `mod.rs`) |
| `patch_audio/mod.rs:249` span | `gap_index = gap_num` | `region_index = region_num`, **plus** `gap_index = region.gap_index` (0-based report) |
| `patch_audio/anchor_retry.rs:183` span | `gap_index = gap_num` | same as above |

Both `patch_gap` spans change together — leaving one behind means a single span name carrying two
definitions of `gap_index`, which is worse than the status quo. The rename is a *loud* break (log
queries stop matching and the operator investigates); silently redefining the field would be a quiet
one.

#### C. `--fingerprint-gap` → 1-based

The flag's own doc comment already tells users to pick from the repair gap table, and that table is
1-based; today it is consumed 0-based via `select.contains(i)` against `enumerate()` over
`report.gaps` (`gap_fingerprint/measure.rs:2296,2317`).

- Convert at the boundary in `composition.rs:134` (`dump_gap_fingerprints`); the internal `select`
  slice stays 0-based.
- **Reject `0` explicitly** — subtracting 1 from a `usize` underflows. Same message shape as the v1
  selection validator: `gap index 0 is invalid (gap indices are 1-based)`. Range-check against
  `report.gaps.len()` too; the report is in scope at that call site (fn param, `composition.rs:93`).
- **Both checks go at `composition.rs:134`, deliberately — not split.** A second consumer of the flag
  exists: `validate_fingerprint_flags` (`infrastructure/cli/mod.rs:262-278`) already returns
  `Result<(), String>` and runs pre-scan, so it *could* host the `0` check and fail before decode.
  It does not, because splitting them puts two "invalid gap number" messages in two files with only
  one able to name the gap count. One validator, one message shape. Leave
  `validate_fingerprint_flags` asserting only flag co-dependence.
- Doc comment (`cli/args.rs:38`) → "1-based, as shown in the gap table". Update
  [gap-fingerprint.md](gap-fingerprint.md), [cli-output.md](../cli-output.md), and `README.md`.
- **Corpus emission is unchanged:** `GapFingerprint::index` and the per-gap filename `g{:03}`
  (`measure.rs:2407`) stay 0-based array positions. No golden churn, no invalidation of existing
  corpus dirs, `equivalence-calibration` / `gap-fingerprint-stats` joins keep working.
- The resulting `--fingerprint-gap 3` → `…_g002_….json` asymmetry is documented, not fixed: the
  filename already carries the A-timeline timestamp (`…_t01-42-08_g002_…`, `measure.rs:2387,2407`),
  so files are located by time — which is the stable handle §2 recommends anyway — not by counting.

#### D. Never print identity and count as one token

```text
  gap 4/6: A 1:42:08 – 1:46:00        # now — is 4 a gap or a position? is 6 gaps or regions?
  gap #4 (3 of 6 planned): A 1:42:08 – 1:46:00     # after
```

- Identity is `#`-prefixed and always the report position (`region.gap_index + 1`).
- Counts are spelled `N of M` and always describe *work* (loop ordinal over planned region count).
- `progress(...)` bar calls (`mod.rs:183,246`) are pure work counters — **unchanged**, and they keep
  `region_count` as denominator so the bar never stalls or jumps.

**The `N/M` shape is already overloaded today — this is not a selection-induced problem.** Two sites
print it with **different meanings for `M`**:

| Site | Now | `M` means | After |
|------|-----|-----------|-------|
| `patch_audio/mod.rs:185` | `gap 4/6: A …` | planned region count | `gap #4 (3 of 6 planned): A …` |
| `patch_audio/anchor_retry.rs:178` | `anchored retry gap 4: A …` | — (identity only, no `/M`) | `anchored retry gap #4: A …` |
| `patch_audio/log.rs:150` | `gap 2/2 (…): <reason>` | **report total** — identity over total, not a count | `gap #2 (…): <reason>` |

`log.rs:150` is the one that proves the rule is needed: it looks like a progress count and is not one.
Drop `/total` there entirely — a skip warning gains nothing from the detected-gap total, and removing
it also retires the out-of-range fallback branch (`log.rs:148,153-155`) that exists only to avoid
mislabelling `M`.

Churn: the `format_skip_gap_fill_log` doc comment (`log.rs:135-138`) and one exact-string assertion,
`skip_gap_fill_log_matches_stdout_gap_number` (`log.rs:301-331`, expects `"gap 2/2 (1:42:08 –
1:46:00): structure alignment failed"`). No other test asserts these strings — audited 2026-07-28.

This is what makes the convention self-enforcing: a new call site cannot blend the two, and an
operator cannot misread which is which.

### 0.4 Explicitly unchanged

- All internal indices stay 0-based; all human display stays 1-based with `#`.
- No JSON schema change; no golden JSON revision (`source_gap_index` is a value fix on an existing
  field).
- `GapFingerprint::index`, corpus filenames, `gap_equivalence` array parallelism: untouched.
- `PatchAnchorCandidate::gap_index` keeps its name — it becomes accurate rather than misleading.

### 0.5 Tests

| Case | Assert |
|------|--------|
| Anchor donor after a plan-time skip | `summary.gaps[anchor.source_gap_index]` is `Patched`, and the rendered `gap #N` equals that gap's table `#`. **This is the regression guard** — `tests/patch_audio_integration.rs:538-549` passes today only because its fixture plans every gap, so ordinal and report index coincide; it also indexes report-ordered `summary.gaps` with the ordinal, encoding the same conflation |
| `--fingerprint-gap 0` | Error, no underflow |
| `--fingerprint-gap N` out of range | Error naming the detected gap count |
| `--fingerprint-gap N` (valid) | Selects the same gap the table shows as `#N`; emitted corpus `index` is `N - 1` |
| Verbose patch line | Matches `gap #<report> (<k> of <planned> planned)`; progress-bar args unchanged |
| Skip warn line | `format_skip_gap_fill_log` → `gap #<report> (<range>): <reason>`; update `skip_gap_fill_log_matches_stdout_gap_number` (`log.rs:301`) and confirm the out-of-range branch is gone |
| Span fields | `region_index` (ordinal) and `gap_index` (0-based report) both present on both `patch_gap` spans |

### 0.6 What this removes from v1

Landing §0 first collapses several open threads in this plan: §5.4's span rename/redefinition
negotiation and its numerator/denominator rule table, §4's `--fingerprint-gap` reconciliation and its
0-token hazard, and §6's "deliberate dual meaning" note on `PatchAnchorCandidate::gap_index`. After
§0, selection only has to say *which* gaps enter `regions` — never how a gap is numbered.

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

### 2.1 Selection tokens are **identities**, never counts — settled 2026-07-28

This is the same distinction §0.3 D draws in output, applied to input. In output, `#4` is a label and
`3 of 6 planned` is a count. On input, **`--only-gaps` / `--skip-gaps` take labels only.**

> **Every token names a gap. No token is a position within a derived subset.**
> Each token resolves independently against the whole `GapReport.gaps` as printed by this run;
> the results are **unioned**. A token can never narrow the resolution domain of another token.

`--only-gaps 4` means "the gap the table calls #4". It does **not** mean the 4th fillable gap, the 4th
gap in some window, the 4th planned region, or the 4th of those already selected.

**Consequences (all v1, all testable):**

| Property | Because |
|----------|---------|
| Order-insensitive: `5,2` ≡ `2,5` | A set of labels has no sequence |
| Duplicates are typos → **error**, not "twice" | Naming the same gap twice cannot mean anything under an identity reading. Settles §12.2 |
| Mixed token kinds compose by **union**, never as a pipeline | `--only-gaps 2,1:42:00..1:50:00` = `{#2} ∪ {gaps in window}`, resolved against the same report |
| `--skip-gaps` = report set **minus** the union | Same resolution domain as `--only-gaps`; the two differ only in polarity |
| Validation is `1 ≤ n ≤ report.gaps.len()` | Bounds come from the report, never from a filtered count |
| `GapSelection` is a `HashSet<usize>` (§5.1) | The data structure already commits: no order, no multiplicity |

**Why this must be settled before v1.5.** The containment token `START..END` turns an interval into a
*set of gaps* — which creates a second enumeration and, under a count reading, a second meaning for
integers. `--only-gaps 1:42:00..1:50:00,2` would become ambiguous: gap #2, or the 2nd gap inside the
window? The identity rule kills the question before it is asked, and it kills it in exactly the same
way §7.1 keeps a hypothetical `--scan-window` from competing with selection: **there is one gap list,
the report, and every handle points into it.**

**Deliberate non-capability:** there is no way to say "the Nth gap of a subset", in v1 or v1.5. If that
is ever wanted, it needs its own syntax and its own justification — not a reinterpretation of integers.

**Stability is a separate axis from identity.** Both token kinds are identities; they differ in how long
they stay valid. Index tokens are **run-scoped** labels (invalidated by a scan-recipe change, §2 table);
range tokens are **recipe-stable** identities. Neither is ever a count.

**Vocabulary:** user-facing docs and error messages say *gap number* (matching the `#` column), not
"gap index". Reserve "index" for the 0-based internal `gap_index` (§5.1).

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

**Ordering caveat — the report still prints first.** "Before patch/preview" is true of the *plan*, not
of stdout. The resolve error lands in `RepairRunOutcome::patch_result`, and `print_repair_outcome`
(`composition.rs:265-311`) prints the full report — including a complete JSON document under
`--format json` — and only then returns the error. So the user sees a normal-looking scan report,
then the error, then exit 2. That is acceptable for the human format (the table is genuinely useful
context for fixing the selection) but questionable for JSON, where a well-formed success document
accompanied by a nonzero exit invites scripts to parse it and proceed. **Open:** suppress the JSON
document on a selection error, or leave it and document that JSON consumers must check the exit code.
Recommend suppressing — see §12.11.

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

### Token parsing

Tokens are **gap numbers** (labels), not positions in a filtered list — §2.1.

- Comma-separated positive integers: `3`, `2,4,5`, ` 2 , 4 `.
- Validate: `1 ≤ n ≤ report.gaps.len()`; duplicate tokens → **error** (§2.1, §12.2).
- Out-of-range → fail fast with: `gap number 7 out of range (6 gaps detected)`.
- `0` → `gap number 0 is invalid (gap numbers are 1-based)` — same message shape as §0.3 C.
- Resolution is order-insensitive and unions across tokens.
- Empty list is **not** the same as absent: `only_gaps = []` (or `--only-gaps ""`) resolves to
  "nothing selected" → the §3 empty-selection error. Absent / `None` → `GapSelectionMode::All`.

### Index base: 1-based

`--only-gaps` is **1-based** (matches the stdout `#` column, `output.rs:838`), per the §0.1 rule:
data is 0-based and positional, CLI arguments and rendered text are 1-based.

Reconciling `--fingerprint-gap` (0-based today) moved to the **§0.3 C prep PR**, together with the
`0`-token underflow hazard its conversion introduces. By the time v1 lands, every user-facing gap
number in the tool already means the table `#`.

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

**Fixed by the §0 prep PR, not by v1.** §0.3 B renames the ordinal (`gap_num` → `region_num`, span
`gap_index` → `region_index` on both `patch_gap` spans, plus a real `gap_index = region.gap_index`),
and §0.3 D splits identity from count in the verbose text:

```text
gap #4 (3 of 6 planned): A 1:42:08 – 1:46:00
```

Identity is the report `#`; the count describes *work* (loop ordinal over planned region count), so
the progress bar and the verbose line no longer disagree and the old `gap 4/2` hazard cannot occur.
`progress("patch-characterize" / "patch-gap", …)` args and the `Aligning N fill region(s)` phase line
are unchanged.

**v1 owes nothing here** beyond not regressing it: selection changes which gaps reach `regions`, and
the display already reports report identity correctly once §0 lands.

### 5.5 `repairable_count` / scan followup

`GapReport::repairable_count()` stays “all gaps that *could* be patched” (no selection; also
ignores the equivalence drop today — leave that semantics alone). Optional stderr when
selection active:

```text
Gap filter: patching 3 of 6 detected gaps (only-gaps: 2,4,5)
```

**Emission site:** `run_repair.rs`, immediately after `resolve_gap_selection` succeeds (§5.6) — the
`progress` handle is already in scope there. Use `progress.phase(...)`, matching the sibling
`format_scan_fillable_followup` line (`scan_gaps.rs:317-318`): this is an unconditional stderr note,
not `phase_verbose`. Build the string in `domain/gap_fill.rs` next to that sibling formatter so both
scan-followup lines live together.

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

**Blast radius is smaller than it looks — corrected 2026-07-28 by source audit.** Adding a field to
`PatchRequestSettings` costs **one edit**: `RepairConfig::patch_settings()` (`infrastructure/config.rs:610`).
Nothing else constructs one exhaustively. Both literals in the workspace end in a spread seed —
`..RepairConfig::default().patch_settings()` (`clip-sync-repair-harness/src/patch_audio.rs:280`,
`tests/query_reference_integration.rs:174`, both by deliberate design of the config-bundles refactor) —
and the fixtures crates never build a literal at all; they call `patch_settings()` and attach a report
(`energy_signature_production.rs:248`, `gap_corpus_fixtures.rs:818`, `fingerprint_corpus_fixtures.rs:136`).
A new field is inherited everywhere.

The real cost is `build_gap_fill_plan`'s new parameter: ~11 call sites (9 in `domain/gap_fill.rs` tests,
2 in `tests/query_reference_integration.rs`) plus the production caller. All mechanical
(`GapSelection::all(n)`), single-crate, no cross-crate sweep.

**Diagnostic path is unaffected.** `dump_gap_fingerprints` (`composition.rs`) builds a
`PatchAudioRequest` but never calls `build_gap_fill_plan` — it goes through
`characterize_gaps_from_decode`, which does its own selection via `--fingerprint-gap`. It will inherit
the new field harmlessly. Do **not** wire `--only-gaps` into it: the two flags are separate user
intents (which gaps to *patch* vs which to *characterize*), and §4 already reconciles their index base.

---

## 6. Interactions

### `anchored_retry`

After the §0 prep PR, `PatchAnchorCandidate::gap_index` and `PatchOffsetAnchor::source_gap_index` are
**report** indices (§0.3 A fixes the ordinal that was flowing there); the region ordinal is renamed
`region_num`. Pass-2 retry still only sees gaps that were **planned and attempted** in pass 1, so
selection narrows the donor pool.

| Scenario | Behavior |
|----------|----------|
| Excluded gap was a strong anchor donor in a prior full run | Not available in this run; pass 2 may recover fewer gaps — **document**. |
| User excludes gap that would have been retried | Expected; no special case. |

No v1 change to anchor table indexing — §0 already put it on the report axis.

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
| **§0 prep** | Gap-index convention: `source_gap_index` axis fix; `gap_num`/span renames; `--fingerprint-gap` → 1-based; identity-vs-count display split. **Merge before v1** |
| **v1** | `--only-gaps` / `--skip-gaps` (indices only, string-typed tokens); `GapNotSelected`; scan params in JSON; user docs; repair-preview applies selection |
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

## 10. Implementation checklist

### §0 prep PR (merge first) — **implemented** (see "deviations" below)

- [x] `build_patch_anchor_candidates` (`patch_audio/anchor_retry.rs:35-39`): take the report index from `region_results.0` (populated at `mod.rs:285`), drop the misnamed enumerate ordinal; update the `PatchAnchorCandidate::gap_index` doc comment (`domain/patch_anchor.rs:47`) to say **report** index
- [x] Regression test: anchor donor after a plan-time skip — `summary.gaps[anchor.source_gap_index]` is `Patched` and the anchor's `a_secs` falls inside *its own* gap's A window. Added as `patch_anchor_source_gap_index_is_report_index_after_plan_time_skip`. **`tests/patch_audio_integration.rs:538-549` needed no fix** — `assert_patch_anchors_exclude_structure_trusted` already indexes `summary.gaps` by `source_gap_index`, which is correct once the axis is; what was missing was a fixture where the two axes diverge
- [x] `gap_num` → `region_num` (`patch_audio/mod.rs:181,245`, `anchor_retry.rs:164`)
- [x] Both `patch_gap` spans: `region_index = region_num` + `gap_index = region.gap_index`. Built via a single shared constructor `log::new_patch_gap_span` so the field set cannot drift and a third call site inherits it by construction
- [x] Verbose characterize line → `gap #<report> (<k> of <planned> planned)`; `progress(...)` bar args unchanged. Extracted to `log::format_patch_characterize_verbose_line` + unit test
- [x] Verbose retry line → `anchored <label> gap #<report>` — **identity only, no count**: pass 2 iterates a filtered retry subset, so `k of M` there would count within a set the user never sees (§0.3 D table). Extracted to `log::format_anchored_retry_verbose_line` + unit test asserting the absence of a count
- [x] `format_skip_gap_fill_log` (`log.rs:139-156`) → `gap #<report> (<range>)`; drop `/total` and the out-of-range branch; update the doc comment and the assertion
- [x] `--fingerprint-gap` → 1-based: convert at `composition.rs:134` via `resolve_fingerprint_gap_select`, keep `select` 0-based, reject `0`, range-check against `report.gaps.len()` (both checks here, not in `validate_fingerprint_flags`); doc comment (`cli/args.rs:33-35`, field at `:38`, `value_name = "N"`) + [gap-fingerprint.md](gap-fingerprint.md) + [cli-output.md](../cli-output.md) + `README.md`
- [x] ~~json-output.md changelog~~ — **not needed**: `patch_anchors_used` / `source_gap_index` is undocumented and absent from every golden (§0.3 A)
- [x] Confirm no golden churn — corpus `index` / `g{:03}` filenames unchanged

**Deviations from the plan as written:**

1. **Verbose lines and the span are extracted, not edited in place.** §0.5 asked for verbose-line and span-field
   assertions. The verbose lines got exactly that (two `log.rs` unit tests, matching the existing
   `format_skip_gap_fill_log` pattern). The span did **not**: asserting recorded span fields needs a
   subscriber, and `clip-sync-repair` has no `tracing-subscriber` dev-dependency. Rather than add one,
   both spans now come from one constructor — a structural guarantee instead of a test. Revisit if a
   third pass ever needs a *different* field set.
2. **`crates/clip-sync/src/test_support/mod.rs` cfg gate** (`ac3_pcm_analysis`) — out of §0 scope; four
   pre-existing dead-code warnings caused by a provider/consumer `cfg(test)` mismatch. Fixed here
   because the §0 verification runs surfaced them.

### v1

- [ ] `RepairConfig`: `only_gaps: Option<Vec<String>>`, `skip_gaps: Option<Vec<String>>` (string tokens, §4); validate mutual exclusivity in `RepairConfig::validate` (TOML path — clap cannot see config keys)
- [ ] `Args`: `--only-gaps`, `--skip-gaps` (comma-separated, `conflicts_with` each other); wire in `cli/mod.rs`
- [ ] `GapSelection` (0-based) / `GapSelectionMode` + `resolve_gap_selection(mode, report)` returning `Result<_, String>`; unit tests (out of range, non-integer token, empty list vs absent, skip vs only, duplicates → error)
- [ ] `GapFillSkipReason::GapNotSelected` + all formatters / `gap_tags` mapping (`PlanKind::NotPlanned`) — three sites: `gap_tags::format_plan_skip_reason`, `gap_tags::derive_gap_tags_from_status` match arm, `cli/output::format_fill_skip_reason`
- [ ] `build_gap_fill_plan(..., skip_equivalent_gaps, selection)` + domain tests (incl. precedence vs equivalence / coverage); ~11 call sites updated (§5.6)
- [ ] **Wiring per §5.6:** `GapSelectionMode` on `PatchRequestSettings` (via `patch_settings()`); resolve in **`run_repair.rs`** both arms; resolved `GapSelection` as a direct field on `PatchAudioRequest` (`measure_residual` precedent). Not composition — the only `build_gap_fill_plan` caller is inside `PatchAudio::run`
- [ ] Empty-selection error → `RepairError::Config` (exit 2); confirm the non-selection empty plan keeps its current `Ok` + "No gaps planned" behavior (§3)
- [ ] `PatchRequestSettings` new field — **`config.rs:610` only**; all other constructions inherit it via spread seed or `patch_settings()` (§5.6 blast radius)
- [ ] `GapReport` (+ `GapScanJson`): add `min_gap_ms`, `silence_hold_ms`, `absolute_silence_rms`; populate from scan request / config
- [ ] `format_unified_gap_report` / patch summary: `not planned: gap not selected`
- [ ] Golden JSON fixture update per [json-output.md](../json-output.md) revision rules
- [ ] Docs: [gap-repair-guide.md](../gap-repair-guide.md) workflow, [cli-output.md](../cli-output.md) flags, [pipeline.md](../pipeline.md) fill-plan paragraph, [gap-fill-modes.md](../gap-fill-modes.md) cross-link
- [ ] Integration test: 3-gap fixture, `--only-gaps 2`, assert gap 1 and 3 unchanged on A, gap 2 patched, status strings correct

---

## 11. Test plan

| Layer | Cases |
|-------|-------|
| **Parse** | `only` / `skip` mutual exclusion (both CLI and TOML paths); out-of-range; `0`; non-integer token; duplicates → error; whitespace; `only_gaps = []` errors while absent selects all |
| **Identity semantics (§2.1)** | `--only-gaps 5,2` ≡ `2,5` (order-insensitive); a token names the same gap regardless of what else is in the list; bounds validate against `report.gaps.len()`, never against a filtered count; `--skip-gaps` selects exactly the report-set complement of the equivalent `--only-gaps` |
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
| **v1.5 identity (§2.1)** | In a mixed list, an integer token still means the report `#` — **never** a position within a containment token's result set (`--only-gaps 1:42:00..1:50:00,2` selects the window's gaps plus report `#2`); each token resolves against the full report independently of the others |

---

## 12. Open decisions

1. ~~**Progress denominator**~~ — **settled 2026-07-28, then moved to §0.** Verbose lines print identity and count separately (`gap #4 (3 of 6 planned)`); the progress bar keeps loop ordinal over region count. Delivered by the §0 prep PR (§0.3 D), not v1.
2. ~~**Duplicate indices in CLI**~~ — **settled 2026-07-28 by §2.1: error.** Under an identity reading, naming the same gap twice cannot mean anything, so it is a typo and should say so.
3. **JSON contract revision:** adding `min_gap_ms` / `silence_hold_ms` / `absolute_silence_rms` to `scan` is additive; bump [json-output.md](../json-output.md) changelog. Requires extending `GapReport` (or an adjacent carrier) — those fields are not on the report today.
4. **Range ε:** 50 ms default strict match — confirm against corpus edge refine behavior; do not reuse `TIME_EPS_SECS`.
5. ~~**Where `GapSelectionMode` lives**~~ — **settled 2026-07-28 by source audit.** The old recommendation was not implementable: the sole production `build_gap_fill_plan` caller is inside `PatchAudio::run`, which sees only `request`. Mode rides `PatchRequestSettings`; resolved `GapSelection` is a direct field on `PatchAudioRequest`; resolution happens in `run_repair.rs`. Full flow in §5.6.
6. ~~**Token type**~~ — **settled 2026-07-28.** `Vec<String>` on both `Args` and `RepairConfig` from v1, even though v1 parses only integers, so v1.5 range tokens do not break the TOML key's type (§4).
7. **Range token semantics (v1.5):** `START-END` = strict single-gap identity, `START..END` = containment. Open sub-question: containment requires *full* enclosure (recommended) vs mere *overlap*. Decide with a real corpus case before implementing; see §4.
8. ~~**`--fingerprint-gap` 1-based conversion**~~ — **settled 2026-07-28: moved to the §0 prep PR** (§0.3 C), along with the `0`-token underflow it introduces and the deliberate flag-vs-filename asymmetry (corpus stays 0-based; files are located by the timestamp already in the name).
9. **All-0-based alternative** — **rejected 2026-07-28** (§0.2): five correct display sites would change to fix two defects, historical `gap #N` references would silently re-point, and the conversion boundary would survive anyway in the progress counter. Recorded so it is not rediscovered.
10. ~~**Identity vs count**~~ — **settled 2026-07-28: identity** (§2.1). Tokens name gaps; they are never positions in a derived subset. Forced by v1.5 containment tokens, which would otherwise make integers ambiguous inside a mixed list.
11. **JSON output on a selection error:** suppress the document, or emit it and rely on the exit code? Recommend **suppress** — a well-formed success document with exit 2 invites scripts to parse and proceed (§3 ordering caveat). Human format keeps printing the table, which is useful context for fixing the selection.

---

## 13. Promotion / done criteria

When v1 ships:

- Mark status **v1 done**; move user-facing contract from this file into [gap-repair-guide.md](../gap-repair-guide.md) and [cli-output.md](../cli-output.md).
- Keep v1.5/v2 sections here until implemented or moved to archive.
- Link from [pipeline.md](../pipeline.md) fill-plan section to the promoted user doc (not this TEMP file).
