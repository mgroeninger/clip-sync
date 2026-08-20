# `gap_fingerprint/measure.rs` folder split — plan

Status: **done** (2026-08-20, P0–P8 landed; see §3 as-landed DAG correction).
Maintainability-only peel of the remaining
`measure.rs` monolith into `gap_fingerprint/measure/` (folder + submodules).
Byte-preserving moves; never bundled into a behavior-change PR.

This is the **second half** of the 2026-07-23
[`TEMP-gap-fingerprint-module-split-plan.md`](archive/TEMP-gap-fingerprint-module-split-plan.md):
that split extracted `schema` / `project` / `contract` and left **the entire PCM
measurement body in one file**. The file is now **6271 lines** (2026-08-20 recount;
~3427 production + ~2844 colocated tests) — larger than the pre-split
`gap_fingerprint.rs` monolith. The growth since the 5990-line count this plan was
drafted against is the nominal-first envelope-rescue mechanism
([TEMP-lag-nominal-first-plan.md](TEMP-lag-nominal-first-plan.md)), already landed in
source (see `lag.rs` ownership below) even though that plan's own status header still
reads "draft, not started" — it is mid real-media validation, not unimplemented.

Companions: [archive/TEMP-gap-fingerprint-module-split-plan.md](archive/TEMP-gap-fingerprint-module-split-plan.md)
(parent split + `schema ← project ← measure` DAG; **keep the cycle guard**),
[archive/TEMP-policies-module-split-plan.md](archive/TEMP-policies-module-split-plan.md)
(template), [gap-fingerprint.md](gap-fingerprint.md) (product docs; path note only
when the tree lands).

---

## 1. Problem

`application/gap_fingerprint/measure.rs` still mixes every PCM→fingerprint concern
in one file. Cohesion slices already exist as builders and a header comment
(`//! Owns the lag summarize/classify path, the placement / seam-probe / dual-fit /
wide-envelope builders, build_gap_fingerprint, compute_region_measurements,
characterize_gaps*, write_corpus_dir`).

| Concern | Approx. locus today | Role |
|---------|---------------------|------|
| **PCM / level** | `LevelProfileSpan`…`structure_params_for` (~L36–613, with `to_db`/`mono_*` interleaved after config) | Shared RMS / dB / noise-floor probes |
| **Config** | `FingerprintConfig` / `GapInputs` / `DEFAULT_LAG_ENVELOPE_MAX_LAG_MS` (~L205, ~L436–514); `from_request` (~L2072–2109) | Probe knobs. `from_request` currently sits *after* `build_gap_fingerprint` |
| **Lag** | `LAG_EDGE_TOL_MS`…`lag_at_placement` (~L201–270, ~L307–1240) + `seam_local` re-export | Curve summarize/classify, envelope recenter, per-shoulder sweep |
| **Placement** | `PlacementScores`…`place_on_b` (~L615–832); `PROBE_BORDER_STANDOFF_FRAMES`; `impl CorpusProbeRecipe` (~L668–697) | Structure-best `place_on_b` + probe-recipe stamp |
| **Probes** | `seam_probe_*` (~L1242–1451); `splice_dualfit_*` / `splice_summary_from_lag` (~L1453–1616); `wide_envelope_*` (~L1583–1742) | Three independent `*_at_placement` builders |
| **Build** | `classify_bracket_stage` + `build_gap_fingerprint` (~L1744–2070) | One-gap summary fingerprint from decoded windows |
| **Overlay** | `tags_from_measurements` (~L284–301); `stage_of` / `RegionMeasurements` / `compute_region_measurements` (~L2112–2593) | From-decode gate-oracle overlay. `tags_from_measurements` is **physically** next to lag, **logically** next to `RegionMeasurements` |
| **Characterize / write** | `CharacterizeAbPcm`…`characterize_gaps*` (~L2595–3261); `write_corpus_dir` / `entry_stem` (~L3263–3386) | Pair orchestrator + corpus files on disk |

Locus table is orientation only. Extract by the §3 ownership table + DAG, not by
contiguous line ranges. The two worst misplacements (`tags_from_measurements` up
with lag; `FingerprintConfig::from_request` down after the builder) are called
out in §3 contested.

Do **not** redesign lag / placement / overlay / dump flow. Do **not** reopen
fingerprint feature work (thresholds, new fields, 8g.5).

---

## 2. Non-goals

- **Renaming the public path** — keep `crate::application::gap_fingerprint::*` via
  the existing `pub use measure::*;` on `gap_fingerprint/mod.rs`. Nested files are
  **not** part of the public path. Facade `measure/mod.rs` re-exports the pre-split
  `pub` / `pub(crate)` / `pub(super)` surface so
  `gap_fingerprint::{build_gap_fingerprint, characterize_gaps*, summarize_lag_curve,
  FingerprintConfig, write_corpus_dir, entry_stem, DEFAULT_LAG_ENVELOPE_MAX_LAG_MS,
  SEAM_PROBE_*, PROBE_BORDER_STANDOFF_FRAMES, …}` still resolve.
- **Changing behavior** — pure move/split only (serde field order / JSON shape /
  thresholds / classifiers unchanged).
- **Moving `lag_correlation_curve` ownership** — stays in `domain::seam_local`;
  keep the `pub use` re-export on the **measure facade** (today it lives in
  `measure.rs`).
- **Reopening the parent M-MOD DAG** — `project` must still never import `measure`.
  `tags_from_measurements` stays next to `RegionMeasurements` (cycle-guard option 1
  from the parent plan).
- **Harness `gap_fingerprint_corpus` split** — already done; out of scope.
- **Sibling `region/` split** — separate plan.
- **Perf / algorithm work** — no envelope / dual-fit / overlay retunes.
- **Moving `#[cfg(test)]` into `tests/` or `*_test.rs`**.
- **Rewriting `impl CorpusProbeRecipe` into `schema.rs`** — the impl needs
  `FingerprintConfig` + `probe_border_standoff_secs`; keep it in `placement`.

---

## 3. Final layout

```text
application/gap_fingerprint/measure/
  mod.rs              # thin facade; re-exports the pre-split pub / pub(crate) / pub(super) surface
  pcm.rs              # level_profile, noise_floor_probe*, mono_*, to_db, median, structure_params_for,
                       # probe_border_*, PROBE_BORDER_STANDOFF_FRAMES, fine_rms_envelope
  config.rs           # FingerprintConfig, GapInputs, DEFAULT_LAG_ENVELOPE_MAX_LAG_MS, from_request
  lag.rs              # summarize_lag_curve, sweeps, envelope recenter, lag_at_placement, seam_local re-export
  placement.rs        # place_on_b, impl CorpusProbeRecipe
  probes.rs           # seam_probe_* + splice_dualfit_* + wide_envelope_* + SEAM_PROBE_* consts +
                       # weighted_downmix_rms
  build.rs            # build_gap_fingerprint, classify_bracket_stage
  overlay.rs          # RegionMeasurements, compute_region_measurements, tags_from_measurements, stage_of
  characterize.rs     # characterize_gaps*, CharacterizeAbPcm, write_corpus_dir, entry_stem
```

| Module | Owns | Notes |
|--------|------|-------|
| `pcm.rs` | `LevelProfileSpan`, `level_profile`, `noise_floor_probe` / `noise_floor_probe_grid`, `mono_slice_rms`, `to_db`, `finite_corr`, `mono_rms`, `mono_peak`, `median`, `flatness`, `structure_params_for`, `PROBE_BORDER_STANDOFF_FRAMES`, `probe_border_standoff_secs`, `probe_border_spec`, `fine_rms_envelope` | Leaf. Uses `schema::SILENCE_FLOOR_DB` / `LevelProfile`. No `FingerprintConfig` param dependency beyond what's passed in. `PROBE_BORDER_STANDOFF_FRAMES` stays `pub(crate)` via facade (`schema.rs` docs) |
| `config.rs` | `DEFAULT_LAG_ENVELOPE_MAX_LAG_MS`, `FingerprintConfig` (+ `Default` + `from_request`), `GapInputs` | Leaf w.r.t. siblings. `from_request` moves *with* the struct, out of the post-builder locus |
| `lag.rs` | `LAG_EDGE_TOL_MS`, `LagSweepParams`, `LagSideSweep`, `seam_prominence`, `summarize_lag_curve`, `secondary_peak`, `classify_lag`, `lag_side_sweep`, `EnvelopeCfg`, `is_recoverable`, `stamp_rescue`, `rescue_shoulder`, `lag_pair`, envelope recenter helpers (`EnvelopePeak`, `envelope_peak_at`, `envelope_shoulder`, `envelope_recenter_frames`), `lag_at_placement`, `pub use domain::seam_local::{…}` | Depends on `pcm` (`finite_corr`, `probe_border_spec`, `fine_rms_envelope`) + `config`. Public: `summarize_lag_curve` + seam_local re-export. `EnvelopeCfg`/`is_recoverable`/`stamp_rescue`/`rescue_shoulder` are the nominal-first triage/rescue helpers (2026-08 addition, ~L894–985 today) — self-contained: they call only `lag_side_sweep` + the existing envelope-recenter helpers, no new DAG edges |
| `placement.rs` | `impl CorpusProbeRecipe`, `PlacementScores`, `PlaceOnBInput`, `place_on_b`, `b_mapped_frame_in_haystack` | Depends on `pcm` + `config`. Calls `pcm::probe_border_standoff_secs` / `pcm::probe_border_spec` (relocated from here — see as-landed note below) |
| `probes.rs` | `weighted_downmix_rms`, `SEAM_PROBE_FINE_LAG_MS` / `SEAM_PROBE_ENV_BIN_MS` (`pub(super)` via facade), `seam_probe_*`, `DUALFIT_SEAM_UNIQ_LAG_MS` / `SEAM_LOCAL_SEARCH_MS`, `splice_dualfit_at`, `mono_lag_side`, `splice_summary_from_lag`, `wide_envelope_*` | Three independent builders in one file so we do not pay three phases for ~150-line slices. Depends on `lag` + `pcm` (`fine_rms_envelope`, `probe_border_spec`) + `config`. Public: `splice_summary_from_lag` |
| `build.rs` | `classify_bracket_stage`, `build_gap_fingerprint` | One-gap summary builder. Depends on pcm/config/lag/placement/probes. **Must not import `overlay` or `characterize`** |
| `overlay.rs` | `tags_from_measurements`, `BracketStageDetail`, `stage_of`, `anchor_params_from_gate`, `RegionMeasurements`, `RegionMeasureInput`, `compute_region_measurements` | Gate-oracle overlay. **Cycle guard:** owns `tags_from_measurements` (consumes `RegionMeasurements`; calls `project::tags_from_fields`). Depends on pcm/config/lag/placement/probes. **Must not import `build` or `characterize`** |
| `characterize.rs` | `refused_channel_mismatch_corpus`, `channel_layout_mismatch`, `CharacterizeAbPcm`, `characterize_gaps_from_decode`, `characterize_gaps`, `detail_tier_str` / `lag_verdict_str` / `hms`, `entry_stem`, `Manifest*`, `write_corpus_dir` | Pair orchestrator + disk writer. Depends on build + overlay + config + placement (`CorpusProbeRecipe::for_corpus`) |
| `mod.rs` | `mod` + re-exports matching today's `pub use measure::*` | No production code; no unit tests |

Eight submodules is the minimum that respects the DAG without leaving a 1.5 kloc
remainder. `probes.rs` is the one deliberate bundle (three small `*_at_placement`
builders that do not call each other). Do **not** split `probes.rs` further in this
plan. Do **not** merge `overlay` into `characterize` — the cycle guard and the
oracle overlay are a distinct cohesion slice.

### Contested / easy-to-misplace helpers

| Symbol | Owner | Why |
|--------|-------|-----|
| `tags_from_measurements` | **`overlay`** | Parent-plan cycle guard: it takes `RegionMeasurements`. Today it sits above the lag section (~L284) — do **not** move it with lag |
| `FingerprintConfig::from_request` | **`config`** | Inherent impl; currently after `build_gap_fingerprint`. Move with the struct |
| `impl CorpusProbeRecipe` | **`placement`** | Needs `FingerprintConfig` + `pcm::probe_border_standoff_secs`. Type stays in `schema.rs` |
| `PROBE_BORDER_STANDOFF_FRAMES` / `probe_border_standoff_secs` / `probe_border_spec` | **`pcm`** (moved from the original plan's `placement`) | As-landed correction: `probe_border_spec` is called directly by `lag_at_placement` (`lag.rs`) and by two `seam_probe_*` builders (`probes.rs`), not only by `placement`. The original DAG text ("`lag` must not import `placement`"; `probes ← lag, pcm, config` with no `placement`) didn't trace this call graph — it already existed in the pre-split monolith. Rather than add `lag → placement` / `probes → placement` edges the DAG forbids, the const + both helper fns move down to `pcm` (a leaf both already depend on, taking no `FingerprintConfig`/`PlacementScores` state), making both call sites legal under the *existing* `← pcm` edge with zero new edges. `impl CorpusProbeRecipe` stays in `placement.rs` (it genuinely needs `FingerprintConfig`) and calls the relocated `pcm::` fns. Facade `pub(crate) use pcm::PROBE_BORDER_STANDOFF_FRAMES;` so `schema.rs`'s `super::measure::PROBE_BORDER_STANDOFF_FRAMES` doc link and any `gap_fingerprint::` consumer keep resolving at the same path |
| `SEAM_PROBE_FINE_LAG_MS` / `SEAM_PROBE_ENV_BIN_MS` | **`probes`** | `contract.rs` tests reach them as `super::super::measure::SEAM_PROBE_*`. Facade **must** `pub(super) use` both or those tests break without an import sweep |
| `fine_rms_envelope` | **`pcm`** (moved from the original plan's `probes`) | As-landed correction: the plan's own note here said "only `seam_probe_side` calls them today; if lag later needs them, lift to `pcm` in a follow-up — not this plan" — but `lag_at_placement`'s envelope-recenter path (the nominal-first rescue mechanism, landed after this plan was drafted) already calls it directly. That follow-up condition was met before this split started, so `lag → probes` would otherwise be a new forbidden edge; moved to `pcm` (leaf) for the same zero-new-edges reason as `probe_border_spec` above |
| `weighted_downmix_rms` | **`probes`** | Only `seam_probe_side` calls it; no cross-module callers found, stays put |
| `seam_prominence` | **`lag`** | Lag-uniqueness helper; dual-fit probe may call it — `probes` may import `lag`, not the reverse |
| `splice_summary_from_lag` | **`probes`** | Public; only needs `LagFingerprint` (schema) + `mono_lag_side`. Keep next to dual-fit splice, not in `lag` |
| `classify_bracket_stage` | **`build`** | Only `build_gap_fingerprint` calls it. Overlay uses `stage_of` (gate failure → fingerprint stage), a different mapper |
| `entry_stem` / `write_corpus_dir` | **`characterize`** | Dump-path I/O. `gap_listen` + `composition.rs` consume them via `gap_fingerprint::{entry_stem, write_corpus_dir}` |

### Path-stability (must not require an import sweep)

These paths are load-bearing and must keep resolving **without** editing callers:

| Path today | After split |
|------------|-------------|
| `crate::application::gap_fingerprint::{build_gap_fingerprint, characterize_gaps, characterize_gaps_from_decode, CharacterizeAbPcm, FingerprintConfig, GapInputs, summarize_lag_curve, splice_summary_from_lag, DEFAULT_LAG_ENVELOPE_MAX_LAG_MS, lag_correlation_curve, …}` | `measure/mod.rs` `pub use` the same names (parent `gap_fingerprint/mod.rs` already `pub use measure::*`) |
| `crate::application::gap_fingerprint::{write_corpus_dir, entry_stem, PROBE_BORDER_STANDOFF_FRAMES}` (`pub(crate)`) | facade `pub(crate) use` |
| `super::super::measure::SEAM_PROBE_{FINE_LAG,ENV_BIN}_MS` from `contract.rs` tests (`pub(super)`) | facade `pub(super) use probes::{…}` |
| `super::measure::PROBE_BORDER_STANDOFF_FRAMES` in `schema.rs` docs | facade `pub(crate) use pcm::PROBE_BORDER_STANDOFF_FRAMES` (as-landed: relocated from `placement` to `pcm`, see §3 contested-helpers) |

### Internal visibility

- Intra-folder items that were private: `pub(in crate::application::gap_fingerprint::measure)`
  so siblings can `use super::pcm::mono_rms` without crate-wide widening.
- Pre-split `pub` / `pub(crate)` / `pub(super)` surface: re-export from `measure/mod.rs`
  at the **same** visibility. Do not `pub` a formerly-`pub(crate)` helper.
- `project.rs` continues to import only `super::schema` + `super::contract` — **zero**
  `measure` symbols.

### Dependency direction (must hold)

**As-landed correction (2026-08-20):** the DAG text below was drafted from the
§3 locus table without tracing `probe_border_spec`'s / `fine_rms_envelope`'s
real call graph, which already crossed these lines in the pre-split monolith
(`lag_at_placement` and two `seam_probe_*` builders call `probe_border_spec`;
`lag_at_placement`'s envelope-recenter path calls `fine_rms_envelope`). Rather
than add the `lag → placement`, `probes → placement`, and `lag → probes` edges
this text forbids, both symbol groups were relocated to `pcm` (see §3
contested-helpers) — a leaf every affected module already depends on — so the
diagram below is satisfied with zero new edges. `characterize → pcm` is added
outright below: `pcm` is a zero-dependency leaf, so any number of modules
depending on it directly can never create a cycle; `characterize_gaps*` calls
`noise_floor_probe` / `noise_floor_probe_grid` directly and always did.

```text
schema, contract, project     (unchanged parent DAG)

pcm            (leaf; schema only)
config         (leaf; schema only)
lag       ←  pcm, config, schema
placement ←  pcm, config, schema
probes    ←  lag, pcm, config, schema
build     ←  pcm, config, lag, placement, probes, schema
overlay   ←  pcm, config, lag, placement, probes, schema, project   # tags_from_fields only
characterize ← build, overlay, pcm, config, placement, schema, project

gap_fingerprint/mod.rs  pub use measure::*
```

**Cycle guards**

- `project` must not import `measure` (parent plan; still the whole point of
  keeping `tags_from_measurements` in overlay).
- `build` must not import `overlay` / `characterize`.
- `overlay` must not import `build` / `characterize`. Overlay calls
  `lag_at_placement` / `seam_probe_at_placement` / `splice_dualfit_at` /
  `wide_envelope_at_placement` **directly**, not via `build_gap_fingerprint`.
- `lag` must not import `probes` / `placement` / `build`.
- `pcm` / `config` must not import any other `measure::*` sibling.

```text
schema  ←  project  ←  overlay / characterize     # tags_from_fields / spec_to_fingerprint_summary
schema  ←  measure/*                              # types only
```

---

## 4. Phase ledger

Extract **one cohesion slice per phase**. Do not combine with behavior PRs.

| Phase | Slice | Status |
|-------|-------|--------|
| **P0** | `git mv` `measure.rs` → `measure/mod.rs` | done |
| **P1** | `pcm.rs` (level / RMS / noise-floor probes) | done — also gained `probe_border_*`/`PROBE_BORDER_STANDOFF_FRAMES` (from placement) and `fine_rms_envelope` (from probes); see as-landed DAG correction in §3 |
| **P2** | `config.rs` (`FingerprintConfig` / `GapInputs` / `DEFAULT_*` / `from_request`) | done |
| **P3** | `lag.rs` (summarize / sweep / envelope / `lag_at_placement` + seam_local re-export) | done |
| **P4** | `placement.rs` (`place_on_b` + probe borders + `CorpusProbeRecipe::for_corpus`) | done — probe-border const/fns relocated to `pcm.rs` (§3 as-landed correction), `impl CorpusProbeRecipe` still here calling them via `pcm::*` |
| **P5** | `probes.rs` (seam-probe + dual-fit splice + wide-envelope) | done — `fine_rms_envelope` relocated to `pcm.rs` (§3 as-landed correction) |
| **P6** | `build.rs` (`build_gap_fingerprint`) | done |
| **P7** | `overlay.rs` (`RegionMeasurements` + `tags_from_measurements` + `compute_region_measurements`) | done |
| **P8** | Remainder → `characterize.rs` + thin `mod.rs` facade | done |

**Suggested order rationale.** PCM first (no sibling deps; noise-floor tests move
cleanly). Config next — lag/placement/probes all take `&FingerprintConfig`, so it
has to exist before those extracts. Lag next (largest leaf after PCM, public
`summarize_lag_curve`, lots of unit tests). Placement and probes are independent
of build/overlay. Build before overlay so the summary path compiles without the
oracle overlay. Overlay before characterize (from-decode dump is the overlay's
only caller besides tests). Characterize last, taking the writer with it.

**P0 notes.** After `git mv`, `mod measure;` in `gap_fingerprint/mod.rs` still
resolves and `pub use measure::*;` is unchanged. Rust forbids a leftover
`measure.rs` beside the folder.

**P1 notes.** Take `to_db` / `mono_rms` / `median` even though they currently sit
*below* `GapInputs`. Leave `tags_from_measurements` behind (P7). Tests: all
`noise_floor_probe_*`, `reduction_*`, `mono_keeps_both_reduction_rows_*`.

**P2 notes.** Move `from_request` with the struct (currently ~800 lines below
`Default`). `DEFAULT_LAG_ENVELOPE_MAX_LAG_MS` is `pub` — facade re-export.
No unit tests are config-owned today.

**P3 notes.** Take the `pub use domain::seam_local::{…}` block with lag (callers
reach it as `gap_fingerprint::lag_correlation_curve`). Also take the nominal-first
rescue helpers (`EnvelopeCfg`, `is_recoverable`, `stamp_rescue`, `rescue_shoulder`) —
they sit between `lag_side_sweep` and `lag_pair` and are pure lag-internal (no
placement/probes deps). Tests: `lag_curve_*`, `summarize_lag_curve_*`, `lag_summary_*`,
`lag_at_placement_*`, `lag_pair_*` (including the nominal-first shapes —
`lag_at_placement_envelope_recenter_is_per_shoulder`,
`lag_at_placement_envelope_shared_far_unlocks_both`,
`lag_at_placement_post_unaffected_by_a_rescued_pre`, and any others
[TEMP-lag-nominal-first-plan.md](TEMP-lag-nominal-first-plan.md)'s exit bar adds).
Leave `tags_from_measurements` in the remainder even though it currently sits
immediately above this block.

**P4 notes.** `PROBE_BORDER_STANDOFF_FRAMES` must be re-exported `pub(crate)` from
whatever file still named `measure` to the parent (during P4 that is still
`measure/mod.rs`). Tests: `probe_recipe_*`.

**P5 notes.** Re-export `SEAM_PROBE_*` as `pub(super)` from the facade **in the
same commit** — `contract.rs` tests will fail otherwise. Public
`splice_summary_from_lag` re-exported `pub`. Tests: `weighted_downmix_*`,
`splice_summary_from_lag_*`, `wide_envelope_*`.

**P6 notes.** `build_gap_fingerprint` only. Confirm zero `RegionMeasurements` /
`characterize_gaps` references in `build.rs`.

**P7 notes.** Move `tags_from_measurements` **with** `RegionMeasurements` (the
point of this phase). Tests: `tags_from_measurements_maps_the_shared_fields`,
`stage_of_keeps_structure_and_residual_channels_separate`. Confirm `project.rs`
still has zero `measure` references.

**P8 notes.** `characterize_gaps*` + `write_corpus_dir` + thin facade. Tests:
`characterize_gaps_*`, `scan_verdict_survives_*`,
`characterize_gaps_from_decode_include_diagnostics_*`,
`write_corpus_library_*`, plus the large from-decode / equivalence overlay
fixtures that drive `characterize_gaps_from_decode`. After this, `mod.rs` is
re-exports only.

---

## 5. Verification (per phase / final gate)

- `cargo build -p clip-sync-repair --all-targets`
- `cargo clippy -p clip-sync-repair --all-targets`
- `.\scripts\test-tier.ps1 -Tier pr-repair`

After **P3**, confirm external paths still compile without import edits:
`domain/seam_local.rs` tests (`gap_fingerprint::summarize_lag_curve`),
`clip-sync-repair-fixtures` (`DEFAULT_LAG_ENVELOPE_MAX_LAG_MS`).

After **P4**, confirm `schema.rs` intra-doc link
`super::measure::PROBE_BORDER_STANDOFF_FRAMES` still resolves.

After **P5**, confirm `contract.rs` `super::super::measure::SEAM_PROBE_*` tests
still compile (facade re-export).

After **P8**, confirm external call sites still compile without import edits:
`composition.rs`, `gap_listen/mod.rs`, `equivalence_calibration`,
`fingerprint_corpus_fixtures.rs`, harness consumers of `gap_fingerprint::*`.
Confirm `-Tier pr-repair` still green including `gap_repair_spec_diff` /
`golden_baseline_invariance` (projection guards) and from-decode dump tests.

---

## 6. Success criteria

- [x] `application/gap_fingerprint/measure/` is a directory (`mod` + eight
      submodules); no leftover sibling `measure.rs`.
- [x] `measure/mod.rs` is a thin facade (re-exports only).
- [x] Public import paths unchanged (`gap_fingerprint::*`, including
      `pub(crate)` `write_corpus_dir` / `entry_stem` / `PROBE_BORDER_STANDOFF_FRAMES`
      and `pub(super)` `SEAM_PROBE_*`). Individually re-verified 2026-08-20 against
      `composition.rs`, `gap_listen/mod.rs`, `equivalence_calibration.rs`,
      `fingerprint_corpus_fixtures.rs`, `contract.rs`, `schema.rs`.
- [x] DAG in §3 holds (no `project → measure`; no `build ↔ overlay`; no
      `lag → probes`) — as landed with the §3 as-landed correction (`probe_border_*`
      and `fine_rms_envelope` relocated to `pcm.rs`, a leaf both `lag` and `probes`
      already imported, closing the forbidden edges with zero new ones;
      `characterize → pcm` accepted as a docs-only addition since `pcm` is a leaf).
- [x] `tags_from_measurements` lives in `overlay.rs` next to `RegionMeasurements`.
- [x] `FingerprintConfig::from_request` lives in `config.rs` next to the struct.
- [x] Intra-folder vis uses `pub(in crate::application::gap_fingerprint::measure)`
      for formerly-private items; no crate-wide widening.
- [x] JSON corpus shape unchanged (round-trip / curated fixtures still green).
- [x] Unit tests colocated per submodule (small pure test helpers — `noise`,
      `base_noise`, `lerp`, `shifted_pair`, `write_speech`, `write_noise`,
      `write_shaped_noise` — duplicated per-file where used cross-module in the
      original monolith, since Rust test modules are private per file).
- [x] Byte-preserving: no intentional behavior / string / threshold / serde changes
      (production code moves are byte-preserving; test-code extraction preserves
      behavior but is not strictly byte-identical, per plan scope).

---

## Related

- [archive/TEMP-gap-fingerprint-module-split-plan.md](archive/TEMP-gap-fingerprint-module-split-plan.md) — parent split (done); this is the leftover `measure.rs` peel
- [gap-fingerprint.md](gap-fingerprint.md) — product docs (add a one-line module-map note when the tree lands)
- [archive/TEMP-fingerprint-field-clarity-plan.md](archive/TEMP-fingerprint-field-clarity-plan.md) — why `SEAM_PROBE_*` are `pub(super)` (contract drift-guards)
- `crates/clip-sync-repair/src/application/gap_fingerprint/measure.rs` — current monolith
- Sibling [TEMP-patch-audio-region-module-split-plan.md](TEMP-patch-audio-region-module-split-plan.md)
