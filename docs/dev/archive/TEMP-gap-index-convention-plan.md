# Gap-index convention (prep PR for gap selection) — **SHIPPED 2026-07-28**

Status: **done**. Implemented 2026-07-28; the error-wording follow-up landed the same day.
Split out of `TEMP-gap-selection-plan.md` on 2026-07-29 and archived — it was §0 there, and it is
complete, so it no longer belongs in an active plan.

**What survives this document:** exactly one rule, §1 below, now recorded permanently in
[gap-vocabulary.md](../gap-vocabulary.md) § Gap numbering. Read that, not this. Everything here is
provenance: why the rule exists, what two defects it repaired, and what the code looked like before.

**Siblings** (what this constrained):
[TEMP-gap-selection-plan.md](../TEMP-gap-selection-plan.md) — selection adds a fifth reason for
`plan.regions` to be shorter than `report.gaps`, which is the exact condition that made these defects
visible. Landing the convention first kept two mechanical renames and one value fix out of the
selection diff, where a reviewer could not have distinguished a rename from a behavior change.

> ⚠️ **Line numbers in §2–§5 are frozen at audit time (2026-07-28, pre-fix)** and are stale by
> construction. §6 (the checklist) is the record of what actually landed.

---

## 1. The rule

> **Data is 0-based and positional; only rendered text and CLI arguments are 1-based.**
> A gap's identity is always its position in `GapReport.gaps`, named `gap_index`. Anything indexing
> `plan.regions` is named `region_*` and is never called a gap. Identity and progress counts are
> never printed as one token.

**Path convention in this document:** unqualified paths are relative to `crates/clip-sync-repair/src/`
(so `composition.rs` means `crates/clip-sync-repair/src/composition.rs`, **not** the same-named file in
`clip-sync-cli`); `tests/` is `crates/clip-sync-repair/tests/`.

## 2. Current state (source audit, 2026-07-28 — frozen, pre-fix)

The codebase already followed the data/display split almost everywhere. This was a repair of two
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
just unlabeled. The footgun is the bare, axis-ambiguous number, which §3 D fixes directly.
*(Recorded so it is not rediscovered.)*

## 3. Required changes

### A. Fix the axis defect — `source_gap_index` names the wrong gap

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
  `AlreadyMatchesReference`, and after selection v1 `GapNotSelected`). With `skip_equivalent_gaps` on
  by default this fires routinely.
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
  **[json-output.md](../../json-output.md) does not document that field today** and no golden JSON
  contains it (verified 2026-07-28). So there was no changelog entry to write: the value fix is
  invisible to the documented contract. Documenting `patch_anchors_used` is a separate, optional task.

### B. Retire the `gap_index` name collision

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

### C. `--fingerprint-gap` → 1-based

The flag's own doc comment already tells users to pick from the repair gap table, and that table is
1-based; it was consumed 0-based via `select.contains(i)` against `enumerate()` over `report.gaps`
(`gap_fingerprint/measure.rs:2296,2317`).

- Convert at the boundary in `composition.rs:134` (`dump_gap_fingerprints`); the internal `select`
  slice stays 0-based.
- **Reject `0` explicitly** — subtracting 1 from a `usize` underflows. Range-check against
  `report.gaps.len()` too; the report is in scope at that call site (fn param, `composition.rs:93`).
- **Both checks go at `composition.rs:134`, deliberately — not split.** A second consumer of the flag
  exists: `validate_fingerprint_flags` (`infrastructure/cli/mod.rs:262-278`) already returns
  `Result<(), String>` and runs pre-scan, so it *could* host the `0` check and fail before decode.
  It does not, because splitting them puts two "invalid gap number" messages in two files with only
  one able to name the gap count. One validator, one message shape. Leave
  `validate_fingerprint_flags` asserting only flag co-dependence.
- Doc comment (`cli/args.rs:38`) → "1-based, as shown in the gap table". Update
  [gap-fingerprint.md](../gap-fingerprint.md), [cli-output.md](../../cli-output.md), and `README.md`.
- **Corpus emission is unchanged:** `GapFingerprint::index` and the per-gap filename `g{:03}`
  (`measure.rs:2407`) stay 0-based array positions. No golden churn, no invalidation of existing
  corpus dirs, `equivalence-calibration` / `gap-fingerprint-stats` joins keep working.
- The resulting `--fingerprint-gap 3` → `…_g002_….json` asymmetry is documented, not fixed: the
  filename already carries the A-timeline timestamp (`…_t01-42-08_g002_…`, `measure.rs:2387,2407`),
  so files are located by time — the stable handle the identity contract recommends anyway — not by
  counting.

### D. Never print identity and count as one token

```text
  gap 4/6: A 1:42:08 – 1:46:00        # now — is 4 a gap or a position? is 6 gaps or regions?
  gap #4 (3 of 6 planned): A 1:42:08 – 1:46:00     # after
```

- Identity is `#`-prefixed and always the report position (`region.gap_index + 1`).
- Counts are spelled `N of M` and always describe *work* (loop ordinal over planned region count).
- `progress(...)` bar calls (`mod.rs:183,246`) are pure work counters — **unchanged**, and they keep
  `region_count` as denominator so the bar never stalls or jumps.

**The `N/M` shape was already overloaded — this was not a selection-induced problem.** Two sites
printed it with **different meanings for `M`**:

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
`skip_gap_fill_log_matches_stdout_gap_number` (`log.rs:301-331`). No other test asserted these
strings — audited 2026-07-28.

This is what makes the convention self-enforcing: a new call site cannot blend the two, and an
operator cannot misread which is which.

## 4. Explicitly unchanged

- All internal indices stay 0-based; all human display stays 1-based with `#`.
- No JSON schema change; no golden JSON revision (`source_gap_index` is a value fix on an existing
  field).
- `GapFingerprint::index`, corpus filenames, `gap_equivalence` array parallelism: untouched.
- `PatchAnchorCandidate::gap_index` keeps its name — it becomes accurate rather than misleading.

## 5. Tests (as planned)

| Case | Assert |
|------|--------|
| Anchor donor after a plan-time skip | `summary.gaps[anchor.source_gap_index]` is `Patched`, and the anchor's `a_secs` lies inside *that* gap's A window. **This is the regression guard** — `tests/patch_audio_integration.rs:538-549` passed only because its fixture plans every gap, so ordinal and report index coincide. ⚠️ **Correction (implementation):** that helper needed no change — it indexes `summary.gaps` by `source_gap_index`, which is correct once the axis is. The gap was the *fixture*, not the assertion |
| `--fingerprint-gap 0` | Error, no underflow |
| `--fingerprint-gap N` out of range | Error naming the detected gap count |
| `--fingerprint-gap N` (valid) | Selects the same gap the table shows as `#N`; emitted corpus `index` is `N - 1` |
| Verbose patch line | Matches `gap #<report> (<k> of <planned> planned)`; progress-bar args unchanged. **Shipped** as unit tests on the extracted `log::format_patch_characterize_verbose_line` / `format_anchored_retry_verbose_line` (the latter also asserts *no* count appears) |
| Skip warn line | `format_skip_gap_fill_log` → `gap #<report> (<range>): <reason>`; update `skip_gap_fill_log_matches_stdout_gap_number` and confirm the out-of-range branch is gone |
| Span fields | `region_index` (ordinal) and `gap_index` (0-based report) both present on both `patch_gap` spans. ⚠️ **Not shipped as a test** — see deviation 1 |

## 6. What landed (checklist — implemented 2026-07-28)

- [x] `build_patch_anchor_candidates` (`anchor_retry.rs:35-39`): take the report index from `region_results.0` (populated at `mod.rs:285`), drop the misnamed enumerate ordinal; update the `PatchAnchorCandidate::gap_index` doc comment (`domain/patch_anchor.rs:47`) to say **report** index
- [x] Regression test: anchor donor after a plan-time skip — `summary.gaps[anchor.source_gap_index]` is `Patched` and the anchor's `a_secs` falls inside *its own* gap's A window. Added as `patch_anchor_source_gap_index_is_report_index_after_plan_time_skip`. **`tests/patch_audio_integration.rs:538-549` needed no fix** — `assert_patch_anchors_exclude_structure_trusted` already indexes `summary.gaps` by `source_gap_index`, which is correct once the axis is; what was missing was a fixture where the two axes diverge
- [x] `gap_num` → `region_num` (`patch_audio/mod.rs:181,245`, `anchor_retry.rs:164`)
- [x] Both `patch_gap` spans: `region_index = region_num` + `gap_index = region.gap_index`. Built via a single shared constructor `log::new_patch_gap_span` so the field set cannot drift and a third call site inherits it by construction
- [x] Verbose characterize line → `gap #<report> (<k> of <planned> planned)`; `progress(...)` bar args unchanged. Extracted to `log::format_patch_characterize_verbose_line` + unit test
- [x] Verbose retry line → `anchored <label> gap #<report>` — **identity only, no count**: pass 2 iterates a filtered retry subset, so `k of M` there would count within a set the user never sees (§3 D table). Extracted to `log::format_anchored_retry_verbose_line` + unit test asserting the absence of a count
- [x] `format_skip_gap_fill_log` (`log.rs:139-156`) → `gap #<report> (<range>)`; drop `/total` and the out-of-range branch; update the doc comment and the assertion
- [x] `--fingerprint-gap` → 1-based: convert at `composition.rs:134` via `resolve_fingerprint_gap_select`, keep `select` 0-based, reject `0`, range-check against `report.gaps.len()` (both checks here, not in `validate_fingerprint_flags`); doc comment (`cli/args.rs:33-35`, field at `:38`, `value_name = "N"`) + [gap-fingerprint.md](../gap-fingerprint.md) + [cli-output.md](../../cli-output.md) + `README.md`
- [x] ~~json-output.md changelog~~ — **not needed**: `patch_anchors_used` / `source_gap_index` is undocumented and absent from every golden (§3 A)
- [x] Confirm no golden churn — corpus `index` / `g{:03}` filenames unchanged

**Deviations from the plan as written:**

1. **Verbose lines and the span are extracted, not edited in place.** §5 asked for verbose-line and
   span-field assertions. The verbose lines got exactly that (two `log.rs` unit tests, matching the
   existing `format_skip_gap_fill_log` pattern). The span did **not**: asserting recorded span fields
   needs a subscriber, and `clip-sync-repair` has no `tracing-subscriber` dev-dependency. Rather than
   add one, both spans now come from one constructor — a structural guarantee instead of a test.
   Revisit if a third pass ever needs a *different* field set.
2. **`crates/clip-sync/src/test_support/mod.rs` cfg gate** (`ac3_pcm_analysis`) — out of scope; four
   pre-existing dead-code warnings caused by a provider/consumer `cfg(test)` mismatch. Fixed here
   because the verification runs surfaced them.
3. **Follow-up (2026-07-28, after the PR landed): error wording corrected to "gap number."** The PR
   first shipped `gap index 0 is invalid (gap indices are 1-based)` / `gap index {n} out of range …`.
   That was drift, not a decision: it put the word this convention reserves for the **0-based** axis
   (`FillRegion::gap_index`) into a message asserting indices are 1-based — reintroducing, in the one
   surface a confused user is certain to read, exactly the axis ambiguity the PR existed to remove.
   The `#` prefix does not disambiguate: these messages carry a bare number. **Fixed in
   `composition.rs::resolve_fingerprint_gap_select`** (both messages, the doc comment, and the two
   string assertions). Shipped wording:

   ```text
   gap number 0 is invalid (gap numbers are 1-based)
   gap number 7 out of range (6 gaps detected)
   ```

   Selection's `resolve_gap_selection` must reuse these **verbatim** — one message shape across both
   surfaces; if either is reworded, change both together.

## 7. What this removed from selection v1

Landing this first collapsed several open threads in the selection plan: the span rename/redefinition
negotiation and its numerator/denominator rule table, the `--fingerprint-gap` reconciliation and its
0-token hazard, and the "deliberate dual meaning" note on `PatchAnchorCandidate::gap_index`. After
this PR, selection only has to say *which* gaps enter `regions` — never how a gap is numbered.
