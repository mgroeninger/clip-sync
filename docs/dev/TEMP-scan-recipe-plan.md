# `ScanRecipe` — let a `GapReport` state the recipe that produced it (DRAFT)

Status: **unparked 2026-07-30 — ready to implement.** Consumer: external scripts that must compare
saved scan knobs to the next run; hand-rolled field checks are already incomplete as knobs scale.
Sequencing record (archived):
[archive/TEMP-gap-selection-sequencing-plan.md](archive/TEMP-gap-selection-sequencing-plan.md)
(§3 trigger: same-recipe equality). The deferred `--gaps-from` loader remains a future in-tree
consumer of the same `PartialEq`; it is not required to start.

Split out of `TEMP-gap-selection-plan.md` on 2026-07-29. It arrived there as "echo the scan params in
JSON" but it is not a selection feature: it is a report-provenance fix with its own defect, its own
golden revision, and no dependency on selection. It was briefly sequenced before v1 for diff hygiene
only; that gate was dropped — selection has no code dependency on this type.

**Siblings:**
[archive/TEMP-gap-selection-sequencing-plan.md](archive/TEMP-gap-selection-sequencing-plan.md) (unpark rule),
[archive/TEMP-gap-selection-plan.md](archive/TEMP-gap-selection-plan.md) (v1 — **archived**),
[archive/TEMP-gap-selection-ranges-plan.md](archive/TEMP-gap-selection-ranges-plan.md) (v1.5 — **archived**),
[TEMP-gap-selection-deferred.md](TEMP-gap-selection-deferred.md) (the `--gaps-from` manifest, a
future in-tree consumer of the `PartialEq`),
[archive/TEMP-gap-index-convention-plan.md](archive/TEMP-gap-index-convention-plan.md) (shipped),
[archive/TEMP-silence-floor-findings.md](archive/TEMP-silence-floor-findings.md) (**archived
2026-07-30**; F11 — JSON provenance — is the one item it delegated here, closed by this plan's
`GapScanJson` checklist item).

> **Verification rule for this document.** A `file:line` reference or a claim about current behavior
> belongs **only** in §5 (the checklist), where it is about to be executed and therefore checked. In
> the rationale sections, state the decision and the reason; cite source only where the citation *is*
> the evidence, and re-verify it when you touch that paragraph. This document already produced two
> retracted "bugs" by ignoring that rule — see §3 — and one retraction was later over-applied to shield
> a different, real defect in the same function (§3 / checklist).

---

## 1. Why

Two symptoms, one cause.

**The visible one.** JSON `GapScanJson` carries `scan_block_ms` and `silence_peak_fraction` but not
`min_gap_ms`, `silence_hold_ms`, or `absolute_silence_rms`. A script that saves a gap list cannot
check whether the next run used the same scan recipe before reusing it.

**The one that proves it needs a type.** `ScanRecipe::from_report` in the fingerprint corpus schema
hardcodes `min_gap_ms: None, absolute_silence_rms: None`, with the comment *"What the `GapReport`
reliably carries; the bin path overwrites the rest from config"* — and `composition.rs` then back-fills
both from config. That closure exists **only** because the report cannot answer "what recipe produced
you?". Flat fields fix the JSON symptom and leave the back-fill in place. A type deletes it.

The flat shape has already failed once here. That is the argument for §2.

## 2. The type

The gap-identity contract already defines this in prose — *"if `min_gap_ms`, `silence_hold_ms`,
`scan_block_ms`, `silence_peak_fraction`, or `absolute_silence_rms` will change before the next patch
attempt, record the time range instead of a remembered `#`"*. Make it one thing:

```rust
/// The scan knobs that determine **which gaps are detected**. Equality means "same gap list".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScanRecipe {
    pub min_gap_ms: u64,
    /// **Effective** hold — `silence_hold_blocks * scan_block_ms`, i.e. what the scan applied after
    /// block quantization, not the configured `RepairConfig::silence_hold_ms`. See §3.
    pub silence_hold_ms: u64,
    pub scan_block_ms: u64,
    pub silence_peak_fraction: f32,
    pub absolute_silence_rms: f32,
}
```

**The defining property is `PartialEq` ⇔ same gap list.** Everything below follows from protecting it.
Equality is **bitwise** (including the two `f32` fields) and **intentional**: values are copied from
config, never recomputed, and the `--gaps-from` manifest round-trips them through JSON. Do not replace
this with an epsilon comparison later — that would quietly break the property the loader depends on.

- **`decode_chunk_secs` is deliberately excluded.** It is a decode-throughput knob; it cannot move a
  gap boundary. Keeping it out is what makes recipe equality *mean* "same gap list" — the property the
  `--gaps-from` loader and the gap-identity stability contract both depend on. It stays a flat field on
  the report.
- **Canonical on `ScanGapsRequest`; derived at the scanner boundary.** Do **not** park the recipe next
  to the existing scan-param flats — that is two sources of truth for one fact, which is how the
  back-fill defect arose. **Replace** `min_gap_secs`, `scan_block_secs`, `silence_peak_fraction`, and
  `absolute_silence_rms` with `recipe: ScanRecipe`. Derive the secs forms the `SilenceRunScanner`
  needs from `recipe.min_gap_ms` / `recipe.scan_block_ms` at the scanner construction sites.
- **Whole-millisecond narrowing is intentional.** The request currently carries `min_gap_secs` /
  `scan_block_secs` as `f64`; the recipe stores `u64` ms. Every production and fixture value today is
  whole-ms (e.g. `0.25` → 250). Sub-millisecond request knobs are not supported after this change —
  record that rather than rediscovering it when a fixture wants `0.001` secs.
- **`silence_hold_blocks` stays on the request** as its own canonical form (§3). Construct
  `recipe.silence_hold_ms` as `silence_hold_blocks * recipe.scan_block_ms` — **never**
  `RepairConfig::silence_hold_ms` (configured, pre-quantization).
- **JSON stays flat.** `GapScanJson` reads `report.recipe.*` and emits the same five keys at top level.
  The contract change is purely additive; no nesting.

## 3. Effective vs configured hold — and a retracted bug

An earlier draft called `format_scan_summary` buggy for printing `hold_ms = silence_hold_blocks *
block_ms` instead of the configured `silence_hold_ms`. **That was wrong, and the correction changes
what the recipe stores.** (It is recorded here rather than deleted because the wrong version is the
tempting one.)

`SilenceRunScanner` is constructed with `silence_hold_blocks` and nothing else — for both the A-side
and B-side scanners. It never sees a millisecond hold. So `blocks × block_ms` **is** the threshold the
scan applied: `silence_hold_ms = 450` at `scan_block_ms = 100` really does hold for 500 ms. The summary
line reports the effective value, which is more accurate than the configured one, and
`format_scan_summary_includes_thresholds_and_count` asserts exactly that on purpose (`blocks: 2` at
250 ms → `"hold 500ms"`). **The hold rendering is not a bug.**

The consequence for the recipe: two configs with `silence_hold_ms` 450 and 500 at a 100 ms block
produce **identical** gap lists, so they must compare **equal**. Therefore:

- The recipe's `silence_hold_ms` is defined as `silence_hold_blocks * scan_block_ms` — the effective,
  post-quantization hold. [json-output.md](../json-output.md) and the user docs must say *effective*,
  so a script comparing the echo against its own TOML `silence_hold_ms` is not surprised by 450 → 500.
- **`ScanGapsRequest` therefore needs no new field.** `silence_hold_blocks` is already canonical for
  the only property that matters; the ms value is a derived display/echo form. This supersedes the
  earlier "add `silence_hold_ms` to the request" step — the lossy-storage problem *dissolves* rather
  than being fixed, and the PR gets smaller.
- Storing the *configured* value instead would make recipe equality strictly finer than gap-list
  equality, which is the one thing the `--gaps-from` loader must not do: it would reject a saved gap
  list as stale after a config edit that provably cannot change the list.

**Do not over-apply that retraction — but the defect it shielded is now fixed elsewhere.** The same
summary helper had a *different*, live defect: the RMS floor branch formatted the normalized
`absolute_silence_rms` (`33.0 / 32767.0` ≈ `0.001007`) with `{:.0}`, printing `rms floor 0` on every
production scan, with a covering test that constructed the old 0–32767 scale and asserted
`"rms floor 33"`. That was diagnosed and fixed **2026-07-29** outside this plan as F3/F4/F5 of
[archive/TEMP-silence-floor-findings.md](archive/TEMP-silence-floor-findings.md): the CLI now normalizes the
documented 0–32767 input, and the header prints `rms floor 33 (at -60 dBFS)`. **Nothing is left for
this plan to fix here** — the checklist step below is now a pure re-pointing of the reads at
`recipe.*`. Kept as history because the retraction above made this defect easy to wave away twice.

### Carrier note (audited 2026-07-29)

"Round-trip from the `ScanGapsRequest` that produced the report" does not work off the *derived* forms
the request stores today (`min_gap_secs: f64`, `scan_block_secs: f64`, `silence_hold_blocks: u32`,
`absolute_silence_rms: f32`):

| Field | Recoverable from the request as it stands today? |
|-------|--------------------------------------------------|
| `absolute_silence_rms` | Yes — stored verbatim |
| `min_gap_ms` | Yes — `(min_gap_secs * 1000.0).round()`, exactly what the corpus recipe already does |
| `silence_hold_ms` | **Not as configured** — `silence_hold_blocks = ceil(silence_hold_ms / scan_block_ms)` quantizes it. The *effective* hold is recoverable exactly: `blocks × scan_block_ms` |

That last row is not a lossy-storage defect — per §3 the blocks *are* the threshold the scan applied.
Making the recipe canonical on the request removes the derivation question entirely.

## 4. Naming: the corpus DTO is renamed

`gap_fingerprint::schema::ScanRecipe` would otherwise collide with the domain type and imports would
lie about which axis they are on. Rename the DTO **`CorpusScanRecipe`**. A Rust type rename is
invisible to serde for a plain struct — the JSON carries field names, not the type name — so this is a
small rename with **no** golden impact. Land it as its own commit, before anything substantive.

The DTO keeps its `Option` fields (backward compat for corpora written before each field existed) and
gains the missing fifth knob, `silence_hold_ms`.

## 5. Checklist

Ordered — each step depends on the one above.

**Line refs re-verified 2026-07-30** against current source (verification rule). Sites the prior
draft under-counted are marked **[+]**; stale line numbers are corrected in place.

- [ ] **Rename first, alone:** `gap_fingerprint::schema::ScanRecipe` → `CorpusScanRecipe`
  (`schema.rs:34` + `Default`/`PartialEq`/`Serialize`/`Deserialize` derives; `impl` at `:91`;
  `measure.rs:2360` `from_report`, `:2433` `Manifest` field type, `:3336` `::default()`). Type
  rename only — serde emits field names, not the type name, so **no golden churn**. Its own
  commit, keeping the name collision out of the substantive diff
- [ ] **New domain type** `ScanRecipe` (`domain/gap.rs`): `min_gap_ms: u64`, `silence_hold_ms: u64`,
  `scan_block_ms: u64`, `silence_peak_fraction: f32`, `absolute_silence_rms: f32`;
  `#[derive(Debug, Clone, Copy, PartialEq)]` — `PartialEq` is **bitwise** and intended (§2). Types
  match `RepairConfig` (`config.rs:40,43,46,53,57`). `decode_chunk_secs` is **not** a member.
  **[+]** Re-export from `domain/mod.rs` (`pub use gap::{…, ScanRecipe}`). Serde on the domain type
  is **not** required for this deliverable (JSON stays flat on `GapScanJson`); add later when
  `--gaps-from` embeds `domain::ScanRecipe` in a manifest
- [ ] **`ScanGapsRequest` (`application/scan_gaps.rs:24-46`): recipe becomes canonical.** Replace
  the four flats `min_gap_secs`, `scan_block_secs`, `silence_peak_fraction`, and
  `absolute_silence_rms` with `recipe: ScanRecipe`. **Do not** leave peak/rms as request flats
  beside the recipe — that is the dual-source-of-truth shape this is fixing. Derive
  `scan_block_secs` / `min_gap_secs` for `SilenceRunScanner` at the A-side and B-side construction
  sites (`scan_gaps.rs:173-178`, `:225-230`), not in the summary helper. **`silence_hold_blocks`
  stays** as its own field (what the scanner consumes). Build the recipe at
  `composition.rs:191-204` with `silence_hold_ms: silence_hold_blocks as u64 * scan_block_ms` —
  **never** `config.repair.silence_hold_ms`. Whole-ms narrowing is intentional (§2).
  **[+]** Post-construction mutations of the abs floor become `request.recipe.absolute_silence_rms`
  (`scan_gaps.rs:1327`, `:1353` — production-floor unit tests)
- [ ] **Update the 10 `ScanGapsRequest` literal sites** — production `composition.rs:191`; test
  helpers `scan_gaps.rs:826` (`scan_request`), `query_reference_integration.rs:112`
  (`scan_request`), `clip-sync-repair-fixtures` `gap_corpus_fixtures.rs:697`
  (`build_scan_request`); direct literals `scan_gaps.rs:1267` (the `format_scan_summary` test),
  `patch_audio_integration.rs:1226`, `scan_gaps_integration.rs:82,143,205`,
  `energy_signature_production.rs:226`. Four route through a helper, so the effective edit count
  is smaller than the site count. Mechanical: four flats collapse into one `recipe:` initializer
  (hold_ms from blocks × block_ms). **[+]** `gap_corpus_fixtures` / `energy_signature_production`
  live in `crates/clip-sync-repair-fixtures`, not under `clip-sync-repair/tests`
- [ ] **`format_scan_summary` — re-point only; both renderings are already correct (§3).** Point
  the hold / block / min-gap / peak / rms-floor reads at `recipe.*` (same numbers by construction).
  The hold rendering was never a bug, and the RMS floor branch was fixed under F3/F4 — it now
  prints `rms floor {i16-scale} (at {dBFS})`, covered by
  `format_scan_summary_includes_thresholds_and_count` (`scan_gaps.rs:1266`). Do **not** re-open
  either; assert the existing printed form still holds after the re-point
- [ ] **`GapReport`: `recipe: ScanRecipe`**, replacing the flat `scan_block_ms` /
  `silence_peak_fraction` and sourced from the request that produced it
  (`scan_gaps.rs:356-370` → `recipe: request.recipe`). Full literals to update (**16**, not ~15):
  production path above; unit/helpers —
  `scan_gaps.rs:1029`, `gap_fill.rs:749` (`base_report`), `cli/output.rs:995,1124,1366`,
  `patch_audio/region.rs:2415,2602`, `measure.rs:3152`,
  `tests/patch_audio_integration.rs:727,746` (`make_report` helpers);
  **[+]** also `clip-sync-repair-fixtures`
  `energy_signature_production.rs:46,108`, `fingerprint_corpus_fixtures.rs:92`, and
  **[+]** `clip-sync-repair-harness` `patch_audio.rs:205` (`make_report`). The ~12 spread-update
  sites in `patch_audio_integration.rs` (`..report` / `..make_report(...)`) inherit it. Read sites
  re-point to `report.recipe.*`: `measure.rs:2108,2123`, `patch_audio/mod.rs:167`,
  `cli/output.rs:727-728`, **[+]** `w5_anchor_rescue_diag.rs:320` (in fixtures crate)
- [ ] Delete the back-fill: `complete_recipe` (`composition.rs:136-140`, called at `:144`) and the
  two hardcoded `None`s + the "the bin path overwrites the rest from config" comment in
  `from_report` (`schema.rs:91-100`). `CorpusScanRecipe` is now populated from `report.recipe`
  (all five knobs, including `silence_hold_ms`)
- [ ] `CorpusScanRecipe`: add the missing fifth knob `silence_hold_ms: Option<u64>` (same
  `skip_serializing_if` / `default` treatment as its siblings). **Options stay** — backward compat
  for corpora written before each field existed; new dumps fill all five. Confirmed no golden
  churn: curated fixtures are *deserialized* (`clip-sync-repair-fixtures`
  `gap_cell_fixtures.rs:187` → `GapCorpus`), never byte-compared against a fresh dump, so absent
  fields keep defaulting. (The plan previously cited `tests/gap_cell_fixtures.rs:28` — that file
  only *calls* the loader.)
- [ ] `GapScanJson` (`infrastructure/cli/output.rs:688-713`): emit all five **flat**, reading
  `report.recipe.*` in `from_parts` (`:716+`). No nesting in the JSON contract — the change stays
  purely additive per [json-output.md](../json-output.md)
- [ ] Golden JSON re-baseline: `tests/fixtures/full_surface_repair.json` only (today
  `:106-108` has `decode_chunk_secs` / `scan_block_ms` / `silence_peak_fraction`; add
  `min_gap_ms`, `silence_hold_ms`, `absolute_silence_rms`) + [json-output.md](../json-output.md)
  GapReport table (`:175-177`) and additive-revision list at `:3`, documenting `silence_hold_ms`
  as the **effective** hold and `absolute_silence_rms` as **normalized** (`f32`, default ≈
  `0.001007`, not the legacy 0–32767 display scale). **[+]** One sentence in
  [gap-scan.md](../gap-scan.md) (or the JSON section of the operator docs) that the JSON echo's
  `silence_hold_ms` is effective (`blocks × block_ms`), not the TOML `silence_hold_ms` — §3
  requires user-facing docs to say this so scripts are not surprised by 450 → 500
- [ ] Sanity: values round-trip from the scan request that produced the report, not from a re-read
  of config
- [ ] **Not in scope:** `limit_fill_to_mapped_region` is a *fill* policy living on a scan report
  (wrong home), and `GapReport` gets no `Default` — zero scan params are a meaningless report, and
  the spread-update sites make one unnecessary. Both recorded so they are not rediscovered as bugs.
  Flat-echo interim (sequencing §3) is also obsolete now that this plan is unparked

### Audit notes (2026-07-30) — integration inventory

| Axis | What changes | Out of scope / unchanged |
|------|--------------|--------------------------|
| Domain | New `ScanRecipe`; `GapReport.recipe` replaces two flats | `decode_chunk_secs`, `limit_fill_to_mapped_region` stay flat on the report |
| Request | Four flats → `recipe`; `silence_hold_blocks` stays | `RepairConfig` / CLI args / TOML knobs unchanged (still configured hold) |
| Scanner boundary | Derive secs from `recipe.*` at two `SilenceRunScanner::new` sites | Scanner API unchanged |
| Composition | Build recipe (effective hold); delete `complete_recipe` | Config → request mapping otherwise same |
| Corpus DTO | Rename + `silence_hold_ms: Option`; `from_report` fills from `report.recipe` | Curated JSON fixtures need no rewrite |
| JSON contract | Three additive keys on `GapScanJson` (flat) | No nesting; human `format_scan_summary` re-point only |
| Call sites | 10 request literals; 16 report full literals; 4 named read sites; 2 field mutations | Patch/fingerprint thresholds that take `silence_peak_fraction` as a bare `f32` stay bare `f32` — only the *report/request carrier* changes |

## 6. Downstream

- **Gap-selection v1** gained nothing from this directly and was **not** blocked on it (thin v1
  shipped first; this plan was parked until the script consumer above).
- **External scripts (unpark trigger, 2026-07-30)** compare saved scan knobs to the next run via
  recipe equality — the same property `--gaps-from` will use. Prefer `report.recipe` / the five
  flat JSON keys over hand-rolled field lists.
- **`--gaps-from` (v2, deferred)** remains a future in-tree consumer: its staleness check becomes
  `manifest.scan != report.recipe` — one bitwise `PartialEq`, not a hand-rolled five-field
  comparison that drifts the first time a knob is added. See
  [TEMP-gap-selection-deferred.md](TEMP-gap-selection-deferred.md) § `--gaps-from`.
- **If a `--scan-window` is ever added** (also deferred), it joins the recipe — it is a knob that
  changes which gaps are detected, which is exactly the membership test.
