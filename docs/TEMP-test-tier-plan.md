# Temporary plan: test tiers (unit / integration / validation / diagnostic)

> **Tier model (v2, 2026-06-25):** four execution tiers — **unit / integration / validation /
> diagnostic**. "**oracle**" is **not a tier**; it is a *label* (the `oracle_` file/test-name prefix)
> for domain-acceptance tests that assert against a computed ground-truth (SD/EC rows). An oracle
> test is scheduled as **integration** (repo-only, deterministic) or **validation** (needs a real
> codec/corpus, or is a slow off-PR contract matrix). Select oracle tests by name filter
> (`cargo test oracle_`) or acceptance ID — not by tier. See [Tier decision rule](#tier-decision-rule).

> **Status:** Phase 1 landed (2026-06-25); Phases 2–5 pending. See [Phase status](#phase-status).
> Motivated by confusion between CI-fast tests, integration
> tests, domain oracles, and validation/contract work (e.g. residual gate **RG01–RG05**, floor
> oracle, energy acceptance **SD** / **EC**). Cargo exposes a single `cargo test` surface;
> `clip-sync-repair --lib` already runs ~65s with 8 `#[ignore]` tests while mixing true units
> with **EC01** / **EC06** domain oracles (legacy `p1_` / `p4_` test names).
> Integration binaries add ~20 min (dominated by `patch_audio_integration`). There is no way to
> run “integration only” or “validation only” without knowing per-file `--test` flags.
>
> Archive to `docs/archive/test-tier-plan.md` when shipped (after Phase 2+ lands or is dropped).
> Phase 1 conventions are already mirrored in [development.md](development.md); keep that doc
> as the living reference and this one as the migration tracker.

## Phase status

| Phase | Scope | Status |
|-------|-------|--------|
| **1** | Conventions + `test-tier.ps1` + docs (no file moves) | **Landed (2026-06-25)** — script, CI `-Tier pr`, `development.md` all in place; see [acceptance criteria](#acceptance-criteria) |
| **2** | Physical separation, repair crate (`--lib` < 15s) | **Landed (2026-06-25)** — see [acceptance criteria](#acceptance-criteria) |
| **2b** | Physical separation, `clip-sync` (`tests/` binaries) | Pending (stubs error with “Phase 2b” message) |
| **2c** | `align_videos` integration move | Deferred |
| **3** | Feature-gated tiers (`autotests = false`, `required-features`) + `tests/common/` per-binary includes (Clippy) | **Landed (2026-06-25)** |
| **4** | cargo-nextest profiles | Optional / pending |
| **5** | `clip-sync-repair-validate` crate | Deferred |

> **Note:** the Phase 1 `test-tier.ps1` implements the four execution tiers (unit / integration /
> validation / diagnostic) plus an `oracle` convenience selector and the `pr` / `pr-repair` /
> `pr-repair-extended` / `pr-align` composites and `validation-align` / `diagnostic-align` Phase 2b
> stubs — slightly ahead of the minimal Phase 1 deliverable list below.

**Problem:**

1. **No taxonomy in tooling.** Everything is `#[test]`; validation looks like integration looks
   like unit tests.
2. **`--lib` is overloaded.** `test_support/energy_signature_acceptance.rs` and
   `gap_corpus_committed` live beside `policies.rs` unit tests.
3. **`cargo test --tests` still runs `--lib`.** There is no `--no-lib`; integration-only requires
   listing every `--test <file>`.
4. **`#[ignore]` is the only slow-tier gate**, but ignored tests mix validation (floor oracle,
   Run B) with diagnostics (CSV sweeps) with no filter distinction.
5. **New validation work** (residual gate catalog, floor oracle, energy matrix) will keep landing
   as “more tests” unless tiers are explicit.

**Goal:**

- Four named **tiers** with documented commands, CI profiles, and conventions for new tests
  (`oracle` is a label, not a tier — see [v2 note](#temporary-plan-test-tiers-unit--integration--validation--diagnostic)).
- **PR-fast** path that does not run validation or diagnostics.
- **Physical separation** over time so `--lib` returns to seconds-scale true units.
- Optional **cargo-nextest** profiles when script-based filtering becomes painful.

**Non-goals (v1):**

- Replacing `#[test]` / libtest with a custom harness.
- Using `examples/` for contract validation.
- Mandatory validation crate in v1 (defer unless Phase 5 is insufficient).
- Renaming every existing test in one pass — new/edited tests follow
  [test-acceptance-glossary.md](test-acceptance-glossary.md); full SD/SP/EC/RG rename is not a
  tier-plan phase.
- Workspace-wide rollout to `clip-sync` / `clip-sync-cli` in Phase 1 (repair crate first).

---

## Tier definitions

| Tier | Purpose | Default CI? | Typical location | Typical runtime |
|------|---------|-------------|------------------|-----------------|
| **unit** | Pure logic: policies, config, CLI parse, small fakes | yes | `src/**` `#[test]` | ms–s |
| **integration** | Patch/scan/CLI on synthetic WAV; seam behavior; **domain-acceptance oracles** (SD/EC, F* fixtures, score harness) that are repo-only and deterministic | yes (fast subset on PR; slow rows via extended/name filter) | `tests/*_integration.rs`, `tests/oracle_*.rs` (target); today also `--lib` | s–min |
| **validation** | Needs an external resource (real codec / ffmpeg / downloaded corpus / env), **or** a slow exhaustive off-PR contract matrix (**RG01–RG05**, EC6, FLOOR_OK, Run B) | no (`#[ignore]` / feature-gated) | `tests/validate_*.rs` (target); today `floor_oracle_integration.rs` | min+; needs ffmpeg/corpus |
| **diagnostic** | CSV dumps, sweeps, golden generators (emit data, no pass/fail assertion) | never | `tests/diag_*.rs` (target); today mixed in validation files | manual only |

### Tier decision rule

Pick a tier by asking these questions **in order**; the first match wins. This is the source of
truth — it replaces the old habit of asking "is this an oracle or a validation test?" (two
overlapping questions that produced ambiguous "oracle / validation" bins).

1. **Does it emit data for humans** (CSV, sweep, golden generator) with no meaningful pass/fail
   assertion? → **diagnostic**.
2. **Does it need an external resource** (ffmpeg binary, downloaded/external corpus, real codec,
   env var) **or** is it a slow exhaustive acceptance/contract matrix run off-PR (RG catalog,
   EC6 sweeps)? → **validation**.
3. **Does it exercise a single module in isolation**, repo-only, deterministic, fast? → **unit**.
4. **Otherwise** (repo-only, deterministic, asserts pass/fail across modules/binaries) →
   **integration**. This includes **oracle** tests — those keep the `oracle_` name prefix as a
   selection label but schedule as integration.

**Speed is not a tier.** A slow repo-only integration test (e.g. `patch_audio_integration` SP rows)
stays *integration*; it is kept off the default PR via script selection (`pr-repair-extended`, name
filters), not by relabeling it. Only external-dependency or exhaustive-contract work becomes
*validation*.

**Mapping to Cargo:**

| Mechanism | Tier use |
|-----------|----------|
| `cargo test --lib` | **unit** (target: unit only) |
| `cargo test --test <file>` | **integration** / **validation** by filename (`oracle_*` files are integration) |
| `cargo test oracle_` | select the **oracle label** (domain-acceptance) across integration binaries |
| `#[ignore = "tier:…"]` | **validation** + **diagnostic** off default CI |
| `cargo test … -- --ignored` | validation + diagnostic (filter by name) |
| `[[test]]` + `required-features` | compile-time tier gates (Phase 3; preferred over `#![cfg(feature)]`) |
| `cargo nextest run --profile …` | orchestration (Phase 4) |

---

## Current baseline (`clip-sync-repair`, 2026-06-25)

| Command | Wall time (debug) | Notes |
|---------|-------------------|-------|
| `cargo test -p clip-sync-repair --lib` | ~65s | 269 tests, 8 ignored; includes EC01/EC06 oracles (`p1_`/`p4_`), `gap_corpus_committed`, `f1_production_haystack_scan_vs_oracle` |
| `cargo test -p clip-sync-repair --tests` | ~21 min | **Also runs `--lib`**; not integration-only |
| `--test patch_audio_integration` | ~15.5 min | 28 pass; SP01/SP02/SP03 rows (`i1_`/`i2_`/`i3_`) dominate |
| `--test residual_gate_integration` | ~82s | RG04 `off_no_regression_baseline` |
| `--test floor_oracle_integration` | &lt;1s default | 7/9 tests `#[ignore]` (validation) |
| `--test seam_residual_corpus` | ~21s | 2 pass; 4 diagnostic `#[ignore]` |

**Ignored lib tests (8):** `gap_corpus_{generated,external,patch_timing_*,regenerate}`,
`p4_f4_decoy_unified_search_diverges`, `write_full_surface_repair_golden`.

**Catalog (not tests):** `crates/clip-sync-repair/tests/residual_gate/` — matrix + README;
maps scattered tests to **RG01–RG05** (legacy `C1–C5` in `claims` until matrix is edited).

**Acceptance IDs:** [test-acceptance-glossary.md](test-acceptance-glossary.md) — SD/SP/EC/RG/PL/GK/CS;
tiers (this doc) vs behavioral families (glossary).

---

## Conventions (all phases)

### `#[ignore]` reason prefix

```rust
#[ignore = "tier:validation — needs ffmpeg + fetch_corpus_sources"]
#[ignore = "tier:diagnostic — CSV export; cargo test diag_seam_residual -- --ignored --nocapture"]
```

Human-readable tail after `tier:<name> —`.

### Test name prefixes (new / renamed tests)

| Prefix | Tier | Notes |
|--------|------|-------|
| `unit_` | unit | rare in `src`; modules usually suffice |
| `integration_` | integration | |
| `oracle_` | integration | **label, not a tier** — domain-acceptance assertions; scheduled as integration. Use `oracle_` files/names to *select* acceptance tests |
| `validate_` | validation | external dep or exhaustive off-PR contract |
| `diag_` | diagnostic | |

Existing names (e.g. `seam_residual_disagreement_oracles`) stay until touched.

### Integration test binaries (target layout)

```text
tests/
  common/                          # shared runners (residual_gate_runner, floor_oracle_fixtures)
  patch_audio_integration.rs       # tier:integration
  scan_gaps_integration.rs
  cli_*_integration.rs
  oracle_energy.rs                 # integration tier, oracle label (split from lib acceptance)
  validate_floor_oracle.rs         # tier:validation (from floor_oracle_integration.rs)
  validate_residual_gate.rs        # tier:validation (from residual_gate_integration.rs)
  diag_energy_matrix.rs            # tier:diagnostic (from energy_signature_production ignores)
  residual_gate_catalog/           # rename from residual_gate/ — matrix.toml + README only
```

Rule: **if it is not fast and not testing one module in isolation, it does not belong in
`src/**` `#[test]`.**

### Harness organization (fixtures, runners, catalogs)

Tiers say **when** tests run; harness layout says **where code lives**. This is not a custom
libtest harness — it is fixture builders, shared runners, data catalogs, and thin `#[test]` fns.

```text
clip-sync (feature test-utils)
  └── application/testing/          cross-crate fakes, alignment corpus helpers

clip-sync-repair lib
  └── test_support/                 F* builders, production helpers — no acceptance/oracle/integration #[test]
  └── application/testing/          gap_corpus manifest + scan runner (#[cfg(test)] today)

tests/  (integration binaries only)
  ├── common/                       runners shared by 2+ binaries (not a Cargo target)
  ├── floor_oracle/, gap_corpus/    manifest.toml + README + committed/generated WAV
  ├── residual_gate_catalog/        matrix.toml + README + baseline CSV (not tests)
  ├── fixtures/                     JSON/TOML for CLI/config roundtrips
  └── <tier>_*.rs                   thin #[test] fns: call runner, assert SD/EC/RG/SP row
```

| Kind | Location | Compiles | Contains |
|------|----------|----------|----------|
| **Domain fixtures** | `src/test_support/` | always (lib) | F* builders, WAV writers, oracle math — **no acceptance/oracle/integration `#[test]`** (fixture-builder unit tests OK) |
| **Corpus data** | `tests/<corpus>/` | N/A | `manifest.toml`, README, `wav/` |
| **Contract catalog** | `tests/residual_gate_catalog/` | N/A | `matrix.toml`, README, baseline CSV |
| **Integration runners** | `tests/common/` | with parent `[[test]]` binary | `residual_gate_runner`, encode/build pairs, matrix loops, CSV printers |
| **Assertions** | `tests/<tier>_*.rs` | per tier / `required-features` | Thin wrappers around runners |
| **Unit assertions** | `src/**` `#[cfg(test)]` | `--lib` | Single-module logic only |
| **Cross-crate fakes** | `clip-sync` `test-utils` | dev-dep | Shared progress fakes, alignment corpus |

**Rules:**

1. **`tests/common/`** — shared runner sources (not a `[[test]]` target). **Phase 2:** `mod common;`
   from each consumer (compiles all submodules per binary). **Phase 3:** drop monolithic
   `mod common`; each `tests/<tier>_*.rs` binary includes only the units it needs — see
   [Phase 3 — `tests/common/` and Clippy](#phase-3--testscommon-and-clippy).
2. **`test_support`** stays on the repair lib so integration binaries can call builders. Do not add
   new acceptance/oracle/integration `#[test]` modules there (fixture-builder unit tests OK); optional later: `test-support` feature or Phase 5 validate crate
   for heavy runners.
3. **New RG row** — add `matrix.toml` entry **and** reuse `common/residual_gate_runner` (or extend
   it). Do not copy `run_built_floor_oracle` / patch loops into a test file.
4. **Catalog ≠ runner ≠ test** — `matrix.toml` inventories instances; runners live in `common/` or
   `test_support/`; `#[test]` fns only assert. Matrix-driven execution stays deferred (Phase 5).
5. **Runner extraction** — follow [residual_gate_catalog/README.md](../crates/clip-sync-repair/tests/residual_gate_catalog/README.md) § Implementation when splitting floor/residual binaries; tier plan does not duplicate that runbook.

**Phase hooks:**

| Phase | Harness work |
|-------|----------------|
| 1 | Document layers (this section); no refactors required for `test-tier.ps1` |
| 2 | No new acceptance/oracle/integration `#[test]` in `test_support/` (fixture-builder unit tests OK); extract shared runners to `common/`; finish runner use in `validate_*` splits; rename catalog folder; update `test-tier.ps1` |
| 2 follow-up (repair) | `integration_gap_corpus.rs` binary; move `gap_corpus_committed` out of lib `application/testing/` — **required** if `--lib` still exceeds 15s after main splits |
| 2b | `clip-sync` corpus + symphonia regressions → `tests/` binaries; `pr-align` script tier — see [Phase 2b](#phase-2b--physical-separation-clip-sync) |
| 3 | `autotests = false`; per-binary `tests/common/` includes; tier `required-features`; Clippy clean on default features — see [Phase 3](#phase-3--feature-gated-tiers-optional) |
| 5 | Optional matrix driver + `clip-sync-repair-validate` owns validate-tier runners |

### Target `Cargo.toml` layout (`[[test]]`)

After Phase 2 file moves, disable autotest discovery and declare every integration binary
explicitly. Prefer `required-features` on `[[test]]` over `#![cfg(feature)]` at the top of test
files — Cargo skips compilation entirely when the feature is off, and error messages name the
missing feature.

**Sketch** (`crates/clip-sync-repair/Cargo.toml`, target end state):

```toml
[package]
name = "clip-sync-repair"
# …
autotests = false   # require explicit [[test]] below; tests/common/ is not a binary

[features]
default = []
ffmpeg-mux = []
he-aac = ["clip-sync/he-aac"]
ac3 = ["clip-sync/ac3"]
ffmpeg-tests = ["clip-sync/ffmpeg-tests"]
# Tier compile gates (Phase 3)
# (no oracle-tests feature: oracle_* binaries are integration tier — always compile)
validation-tests = []    # validate_* binaries (ffmpeg + corpus)
diagnostic-tests = []    # diag_* binaries (CSV / sweeps; never CI)

# ── tier: integration (default compile; PR runs subset via script) ─────────────

[[test]]
name = "config_roundtrip"
path = "tests/config_roundtrip.rs"

[[test]]
name = "scan_gaps_integration"
path = "tests/scan_gaps_integration.rs"

[[test]]
name = "patch_audio_integration"
path = "tests/patch_audio_integration.rs"

[[test]]
name = "query_reference_integration"
path = "tests/query_reference_integration.rs"

[[test]]
name = "cli_wav_integration"
path = "tests/cli_wav_integration.rs"

[[test]]
name = "wav_bit_depth_integration"
path = "tests/wav_bit_depth_integration.rs"

[[test]]
name = "integration_floor_oracle_smoke"
path = "tests/integration_floor_oracle_smoke.rs"   # manifest_loads, gap_frames smoke (from floor_oracle_integration.rs)

[[test]]
name = "integration_energy_smoke"
path = "tests/integration_energy_smoke.rs"           # corpus_scan_patch_smoke + lib scan/domain smokes (PR)

[[test]]
name = "integration_residual_gate_smoke"
path = "tests/integration_residual_gate_smoke.rs"   # RG04 off_no_regression_baseline (PR)

[[test]]
name = "cli_mux_integration"
path = "tests/cli_mux_integration.rs"
required-features = ["ffmpeg-mux"]

# ── integration tier, oracle label (always compile; PR runs subset) ────────────

[[test]]
name = "oracle_energy"
path = "tests/oracle_energy.rs"                    # SD/EC acceptance rows (from lib energy_signature_acceptance)

[[test]]
name = "seam_residual_oracle"
path = "tests/seam_residual_oracle.rs"

[[test]]
name = "seam_residual_corpus"
path = "tests/seam_residual_corpus.rs"             # non-ignored acceptance rows; diag fns move out or stay #[ignore]

# ── tier: validation (off default compile) ───────────────────────────────────

[[test]]
name = "validate_floor_oracle"
path = "tests/validate_floor_oracle.rs"
required-features = ["validation-tests"]

[[test]]
name = "validate_residual_gate"
path = "tests/validate_residual_gate.rs"              # f1_production_scan_patch_smoke + EC6 f4_decoy_* + future RG rows
required-features = ["validation-tests"]

# ── tier: diagnostic (off default compile) ─────────────────────────────────────

[[test]]
name = "diag_energy_matrix"
path = "tests/diag_energy_matrix.rs"
required-features = ["diagnostic-tests"]

[[test]]
name = "diag_seam_residual"
path = "tests/diag_seam_residual.rs"               # *_csv rows split from seam_residual_corpus.rs
required-features = ["diagnostic-tests"]
```

**Tier → `cargo test` commands** (after Phase 3):

| Tier | Compile | Run |
|------|---------|-----|
| **unit** | always | `cargo test -p clip-sync-repair --lib` |
| **integration** | always | `cargo test -p clip-sync-repair --test patch_audio_integration` (etc.; script lists all default binaries, no `--lib`) |
| ↳ *oracle label* | always (integration) | `cargo test -p clip-sync-repair --test oracle_energy --test seam_residual_oracle` (or `cargo test -p clip-sync-repair oracle_`) |
| **validation** | `validation-tests` | `cargo test -p clip-sync-repair --features validation-tests --test validate_floor_oracle` |
| **diagnostic** | `diagnostic-tests` | `cargo test -p clip-sync-repair --features diagnostic-tests --test diag_energy_matrix -- --nocapture` |

**Notes:**

- `tests/common/` stays **source files** under `tests/common/*.rs` (not `[[test]]` targets). Phase 3
  stops using a shared `tests/common/mod.rs` pulled in via `mod common;` — each consumer binary
  includes only the units it needs (see [harness + Clippy](#phase-3--testscommon-and-clippy)).
- `autotests = false` prevents Cargo from auto-discovering a stray `tests/foo.rs` without an
  explicit entry — renames and splits must update `Cargo.toml` in the same PR.
- `cli_mux_integration` already needs `ffmpeg-mux`; `required-features` replaces scattered
  `#[cfg(feature = "ffmpeg-mux")]` wrappers inside the file over time.
- **Lib corpus tests** (`gap_corpus_committed` in `gap_corpus_fixtures.rs`) stay in `--lib` until
  moved to `tests/integration_gap_corpus.rs` (optional Phase 2 follow-up); not shown above.

**Migration from today (autotests default `true`):**

| Current binary | Target `[[test]]` name | Tier | `required-features` |
|----------------|------------------------|------|---------------------|
| `config_roundtrip.rs` | `config_roundtrip` | integration | — |
| `scan_gaps_integration.rs` | `scan_gaps_integration` | integration | — |
| `patch_audio_integration.rs` | `patch_audio_integration` | integration | — |
| `query_reference_integration.rs` | `query_reference_integration` | integration | — |
| `cli_wav_integration.rs` | `cli_wav_integration` | integration | — |
| `wav_bit_depth_integration.rs` | `wav_bit_depth_integration` | integration | — |
| `cli_mux_integration.rs` | `cli_mux_integration` | integration | `ffmpeg-mux` |
| `energy_signature_production.rs` | split → `integration_energy_smoke` + `oracle_energy` (integration) + `diag_energy_matrix` + `f1_production_scan_patch_smoke` + EC6 in `validate_residual_gate` | integration / diagnostic / validation | diag: `diagnostic-tests`; validation: `validation-tests` |
| `floor_oracle_integration.rs` | split → `integration_floor_oracle_smoke` + `validate_floor_oracle` | integration / validation | validation: `validation-tests` |
| `residual_gate_integration.rs` | split → `integration_residual_gate_smoke` + `validate_residual_gate` | integration / validation | validation: `validation-tests` |
| `seam_residual_oracle.rs` | `seam_residual_oracle` | integration (oracle label) | — (see [Resolved decisions](#resolved-decisions)) |
| `seam_residual_corpus.rs` | `seam_residual_corpus` + `diag_seam_residual` | integration / diagnostic | diag: `diagnostic-tests` |

---

## Workspace scope

| Crate | Phases | Notes |
|-------|--------|-------|
| **`clip-sync-repair`** | 1–5 (primary) | Baseline metrics, file moves, `[[test]]` sketch, `test-tier.ps1` repair paths |
| **`clip-sync`** | **2b** (+ optional **2b-3**) | First `tests/` binaries for this crate; corpus + symphonia tier splits — [Phase 2b](#phase-2b--physical-separation-clip-sync). `align_videos.rs` inline integration bulk **deferred** to 2c |
| **`clip-sync-cli`** | **out of scope v1** | Two small integration binaries; piggybacks on workspace `cargo test`; revisit only if CLI tests grow |

CI **PR workspace** row already combines `clip-sync corpus_committed` (unchanged) with
`clip-sync-repair pr-repair` (new script). Tier machinery is repair-first; `clip-sync` keeps its
existing corpus filters until Phase 2b.

---

## PR contract (required CI gates)

Explicit list of what **must pass on every PR**. Wall times are debug builds on a typical dev
machine (2026-06-25); treat as budgets, not SLAs. See [CI migration](#ci-migration) for when each
row lands in GitHub Actions.

### Composite `pr` tier

`test-tier.ps1 -Tier pr` runs **`pr-align` + `pr-repair` + `clip-sync-cli`** in sequence; fail fast
on first non-zero exit. Individual crate tiers remain callable alone.

### `clip-sync-repair` — `pr-repair`

| Phase | What runs | Wall budget (debug) |
|-------|-----------|---------------------|
| **1 (interim)** | lib (oracle skips) + `gap_corpus_committed` + fast integration binaries (`config_roundtrip`, `scan_gaps`, `cli_wav`, `query_reference`, `residual_gate`, `floor_oracle`, `seam_residual_corpus`, `wav_bit_depth_integration`, `corpus_scan_patch_smoke`); optional `cli_mux` if ffmpeg; **excludes** full `patch_audio_integration` | ~4–6 min |
| **2 (target)** | `--lib` (units only, &lt;15s) + binaries below | ~3–5 min |
| **3 (target)** | Same as Phase 2; validation/diagnostic binaries not compiled unless features passed | ~3–5 min |

**Required test fns / binaries (target, post–Phase 2):**

| Kind | Name | Tier | Notes |
|------|------|------|-------|
| lib filter | all unit modules + `gap_corpus_committed` | unit / integration | oracle rows moved out of lib |
| `--test` | `config_roundtrip` | integration | |
| `--test` | `scan_gaps_integration` | integration | |
| `--test` | `cli_wav_integration` | integration | |
| `--test` | `wav_bit_depth_integration` | integration | source-driven WAV bit depth |
| `--test` | `query_reference_integration` | integration | query-mode repair smoke |
| `--test` | `integration_floor_oracle_smoke` | integration | manifest + gap_frames |
| `--test` | `integration_energy_smoke` | integration | `corpus_scan_patch_smoke` + lib scan/domain smokes (see [Resolved decisions](#resolved-decisions)) |
| `--test` | `integration_residual_gate_smoke` | integration | `off_no_regression_baseline` (RG04 only; see [Resolved decisions](#resolved-decisions)) |
| `--test` | `integration_gap_corpus` | integration | `gap_corpus_committed` (Phase 2 follow-up) |
| `--test` | `oracle_energy` | integration (oracle label) | SD U1–U8 + EC `p3_`/`p4_` (skips slow `p1_`/`p2_`/haystack/patch smokes on PR) |
| `--test` | `seam_residual_corpus` | integration (oracle label) | `f4_decoy_placement_informative_with_high_headroom`, `seam_residual_disagreement_oracles` |

`gap_corpus_committed` runs via `--lib` only (no duplicate name filter on PR).

**Explicitly not on default PR** (nightly / manual / extended):

| Name | Tier | Run via |
|------|------|---------|
| `off_no_regression_baseline` siblings / future RG rows | validation | `validation-tests` + `validate_residual_gate` |
| `patch_audio_integration` (full) | integration | `pr-repair-extended` or `-Tier integration` |
| `i1_` / `i2_` / `i3_` / `i4_` (SP01–SP04) | integration | optional in extended profile (~15 min) |
| `floor_oracle_*` ignored, `source_gap_oracle_*` | validation | nightly |
| `energy_signature_mode_matrix`, EC6 smokes, F4 sweeps | diagnostic / validation | `--features diagnostic-tests` or manual |
| `seam_residual_*_csv`, `diag_*` | diagnostic | manual |

**`pr-repair-extended` (optional, path filters):** `pr-repair` + `patch_audio_integration` with
`--skip i1_ --skip i2_ --skip i3_` (sine seam grid only, ~minutes not ~15 min).

### `clip-sync` — `pr-align`

| Phase | What runs | Wall budget (debug) |
|-------|-----------|---------------------|
| **Until 2b** | `cargo test -p clip-sync corpus_committed` (name filter on lib) | ~10–30s |
| **2b (target)** | `--lib` + `integration_corpus` + `integration_extract_window` + `integration_media_reader` + `oracle_anchored_end` | ~1–3 min (dominated by `--lib` until Phase 2c) |
| **2c note** | `align_videos` still in `--lib` until moved | budget may stay high |

**Required (target, post–2b):**

| Name | Tier |
|------|------|
| `corpus_committed_cases` | integration (`integration_corpus`) |
| `corpus_manifest_loads`, `corpus_verify_offset_pass`, `corpus_verify_option_a_false_pass_probe`, ambiguity-flag probes, `corpus_query_reference_b_longer_fast` | integration |
| WAV rows in `integration_extract_window`, `integration_media_reader` | integration |
| `oracle_anchored_end` rows | integration (oracle label) |

### `clip-sync-cli`

| Name | Tier |
|------|------|
| `cargo test -p clip-sync-cli` | integration (adapter; 2 binaries today) |

Included in composite `-Tier pr`.

---

## Per-file test inventory (repair binaries)

Target tier for each `#[test]` when splitting. Acceptance IDs from
[test-acceptance-glossary.md](test-acceptance-glossary.md).

### `energy_signature_production.rs` → split across four binaries

| Test fn | Today | Target binary | Tier | `#[ignore]` after split? |
|---------|-------|---------------|------|--------------------------|
| `corpus_scan_patch_smoke` | default CI | `integration_energy_smoke.rs` | integration | no |
| `energy_signature_mode_matrix` | ignore | `diag_energy_matrix.rs` | diagnostic | no (binary gated) |
| `f1_production_scan_patch_smoke` | ignore | `validate_residual_gate.rs` | validation | no (feature-gated) |
| `f1_production_oracle_patch_control` | ignore | `oracle_energy.rs` | integration (oracle label) | no |
| `f2_production_oracle_patch_smoke` | ignore | `oracle_energy.rs` | integration (oracle label) | no |
| `f2_production_weights_diagnostic` | ignore | `diag_energy_matrix.rs` | diagnostic | no |
| `f4_decoy_residual_gate_vetoes_bool` | ignore (ec6) | `validate_residual_gate.rs` | validation | no (feature-gated) |
| `f4_decoy_patch_discrimination` | ignore (ec6) | `validate_residual_gate.rs` | validation | no (feature-gated) |
| `f4_decoy_mode_coupled_bias` | ignore (ec6) | `validate_residual_gate.rs` | validation | no (feature-gated) |
| `f4_decoy_energy_recovers_at_low_bias` | ignore (ec6) | `validate_residual_gate.rs` | validation | no (feature-gated) |
| `f4_decoy_weight_sweep` | ignore | `diag_energy_matrix.rs` | diagnostic | no |
| `f4_decoy_bias_boundary` | ignore | `diag_energy_matrix.rs` | diagnostic | no |
| `f4_decoy_patch_diagnostic` | ignore | `diag_energy_matrix.rs` | diagnostic | no |

**Decision:** validation production smokes (`f1_production_scan_patch_smoke` + EC6 `f4_decoy_*` gate
tests) land in **`validate_residual_gate.rs`** behind `validation-tests` (no separate
`validate_energy_corpus` binary); oracle smokes (`f1/f2_production_oracle_patch_*`) in
**`oracle_energy.rs`**.

Add `integration_energy_smoke.rs` to repair `[[test]]` sketch (not in original list).

### `residual_gate_integration.rs`

| Test fn | Today | Target binary | Tier | PR? |
|---------|-------|---------------|------|-----|
| `off_no_regression_baseline` | default CI (~82s) | `integration_residual_gate_smoke.rs` | integration | **yes** |
| (future RG01–RG03, RG05 rows) | — | `validate_residual_gate.rs` | validation | no |
| `f1_production_scan_patch_smoke` (from `energy_signature_production.rs`) | ignore | `validate_residual_gate.rs` | validation | no |

`residual_gate_integration.rs` is **renamed/split**, not moved wholesale behind `validation-tests`.

### `patch_audio_integration.rs`

| Group | Tests | Tier | PR default? |
|-------|-------|------|-------------|
| Sine seam / fit / anchored-retry grid | `patch_audio_fills_gap_*` … `patch_audio_anchored_retry_*` (22 tests) | integration | extended only |
| SP01–SP04 energy signature | `i1_f1_*`, `i2_f1_*`, `i3_f2_*`, `i4_f3_*` (4 tests) | integration (SP) | extended only (heavy) |
| Production smoke | `patch_audio_fit_production_defaults_smoke` | validation | no (`#[ignore]` → validation or manual) |
| CSV / debug dumps | `i1_f1_patch_diagnostic`, `i3_f2_patch_diagnostic` | diagnostic | no → `diag_patch_audio.rs` (Phase 2 follow-up) |

Mixed-tier tests may stay in one binary short term; **diagnostic fns move to `diag_patch_audio.rs`**
when touched (same rule as `diag_seam_residual`).

### `query_reference_integration.rs`

| Test fn | Tier | PR? |
|---------|------|-----|
| `repair_auto_no_clip_count_mismatch_error` | integration | yes |
| `repair_query_gap_inside_region_fillable` | integration | yes |
| `repair_b_longer_query_gap_inside_region_patched_with_donor_audio` | integration | yes |
| `repair_query_gap_outside_region_skipped` | integration | yes |

Stays one binary: `query_reference_integration` (integration tier). No split planned.

### `wav_bit_depth_integration.rs`

| Test fn | Tier | PR? |
|---------|------|-----|
| `wav_writer_24bit_int_source_produces_24bit_output` | integration | yes |
| `wav_writer_float32_source_produces_24bit_int_output` | integration | yes |
| `wav_writer_int32_source_produces_24bit_int_output` | integration | yes |
| `wav_writer_lossy_source_stays_16bit` | integration | yes |
| `wav_writer_16bit_source_stays_16bit` | integration | yes |
| `wav_writer_24bit_sample_values_round_trip` | integration | yes |

All fast (`WavSpec` assertions, no ffmpeg/corpus, no `#[ignore]`). Pure integration tier; stays one
binary `wav_bit_depth_integration`. No split planned. Included in `pr-repair` via `--test wav_bit_depth_integration`.

### `seam_residual_corpus.rs` → integration (oracle label) + diagnostic

| Test fn | Today | Target binary | Tier | PR? |
|---------|-------|---------------|------|-----|
| `f4_decoy_placement_informative_with_high_headroom` | default CI | `seam_residual_corpus.rs` | integration (oracle label) | yes |
| `seam_residual_disagreement_oracles` | default CI | `seam_residual_corpus.rs` | integration (oracle label) | yes |
| `seam_residual_broadband_csv` | ignore | `diag_seam_residual.rs` | diagnostic | no |
| `seam_residual_alignment_sweep_csv` | ignore | `diag_seam_residual.rs` | diagnostic | no |
| `seam_residual_truth_decoy_csv` | ignore | `diag_seam_residual.rs` | diagnostic | no |
| `seam_residual_disagreement_csv` | ignore | `diag_seam_residual.rs` | diagnostic | no |

### Lib modules (repair) — Phase 2 / follow-up

| Module | Tests to move | Target |
|--------|---------------|--------|
| `test_support/energy_signature_acceptance.rs` | all `#[test]` | `oracle_energy.rs` |
| `test_support/energy_signature_production.rs` | `f1_production_haystack_scan_vs_oracle` | `oracle_energy.rs` |
| same | `scan_detects_f1_production_gap`, `f1_production_scan_and_domain_smoke` | `integration_energy_smoke.rs` |
| same | `production_matrix_contexts_skip_thirty_on_sixty_sec_fixture` | **stay in lib** (unit) |
| `test_support/energy_signature_fixtures.rs` | `production_spec_tests` (4 fns) | **stay in lib** (`test_support/` fixture-builder units — see [Resolved decisions](#resolved-decisions)) |
| `application/testing/gap_corpus_fixtures.rs` | `gap_corpus_committed` | `integration_gap_corpus.rs` |
| same | generated/external/patch_timing/regenerate | validation / diagnostic binaries or stay ignored in gap binary |
| `infrastructure/cli/output.rs` | `write_full_surface_repair_golden` | `diag_repair_golden.rs` (diagnostic; defer) |

---

## Phase 1 — Convention + script + docs (no file moves)

**Deliverables:**

1. `scripts/test-tier.ps1` — tier selector (repair crate first; extend to workspace later).
2. `docs/development.md` — replace four-bucket table with tier table + script examples.
3. `.github/workflows/ci.yml` — PR gate becomes `.\scripts\test-tier.ps1 -Tier pr` (not
   `cargo test --workspace`).
4. Standardize `#[ignore]` on **edited** validation/diagnostic tests (`tier:validation`,
   `tier:diagnostic`); bulk rename not required.
5. Document **integration-only** command (all repair `--test` binaries, no `--lib`).

**Out of scope (Phase 1):** file moves / binary splits, `autotests = false`, `[[test]]` entries,
`residual_gate/` → `residual_gate_catalog/` rename, nightly validation workflow, cargo-nextest,
`validation-align` / `diagnostic-align` implementation (stub with clear message).

### `pr-repair` scope

Phase 1 CI must not regress default repair coverage except where intentional:

| Included on PR | Excluded from PR (by design) |
|----------------|------------------------------|
| Lib units (`--skip` legacy SD rows in lib) | Lib oracles: `p1_`, `p2_`, `p4_`, `f1_production_haystack` |
| `gap_corpus_committed` | Full `patch_audio_integration` (~15 min) |
| `config_roundtrip`, `scan_gaps_integration` | `i1_` / `i2_` / `i3_` / `i4_` SP rows → `pr-repair-extended` or manual |
| `cli_wav_integration`, `query_reference_integration` | All `#[ignore]` validation / diagnostic rows |
| `residual_gate_integration` (RG04 `off_no_regression_baseline`, ~82s) | |
| `floor_oracle_integration` (2 fast smokes) | |
| `seam_residual_corpus` (2 default oracle rows) | |
| `corpus_scan_patch_smoke` (`energy_signature_production` binary) | |
| `wav_bit_depth_integration` | |
| `cli_mux_integration` only when ffmpeg on PATH | |

See [PR contract](#pr-contract-required-ci-gates) — `pr-repair` Phase 1 column.

**`scripts/test-tier.ps1` interface:**

```powershell
param(
  [ValidateSet(
    'unit','integration','oracle','validation','diagnostic',
    'pr','pr-repair','pr-repair-extended','pr-align',
    'validation-align','diagnostic-align'
  )] $Tier,
  [ValidateSet('clip-sync-repair','clip-sync','clip-sync-cli','workspace')] $Package = 'workspace',
  [switch] $Nocapture
)

# Fail fast: exit non-zero on first failing cargo test invocation.
# unit / integration / validation / diagnostic — execution tiers, per [PR contract](#pr-contract-required-ci-gates)
# oracle              — convenience selector for the oracle *label* (domain-acceptance rows that
#                       schedule as integration); not a separate tier — see [decision rule](#tier-decision-rule)
# pr                  — pr-align + pr-repair + clip-sync-cli
# pr-repair           — repair PR contract (see below)
# pr-repair-extended  — pr-repair + patch_audio sine grid (--skip i1_/i2_/i3_)
# pr-align            — align PR contract (corpus_committed until 2b)
# validation-align / diagnostic-align — Phase 2b; stub in Phase 1
```

**`pr-repair` composition:**

```powershell
# Lib: units only until Phase 2 moves SD acceptance out of --lib
cargo test -p clip-sync-repair --lib -- --skip p1_ --skip p2_ --skip p4_ --skip integration_ --skip f1_production_haystack
cargo test -p clip-sync-repair gap_corpus_committed

# Integration + oracle smokes (today’s binaries; no file splits yet)
cargo test -p clip-sync-repair --test config_roundtrip --test scan_gaps_integration `
  --test cli_wav_integration --test query_reference_integration `
  --test residual_gate_integration --test floor_oracle_integration --test seam_residual_corpus `
  --test wav_bit_depth_integration
cargo test -p clip-sync-repair corpus_scan_patch_smoke

# Optional when ffmpeg on PATH (mux tests are mostly #[ignore] today)
# cargo test -p clip-sync-repair --features ffmpeg-mux --test cli_mux_integration
```

**`pr-repair-extended`:** `pr-repair` then:

```powershell
cargo test -p clip-sync-repair --test patch_audio_integration -- --skip i1_ --skip i2_ --skip i3_
```

**Integration-only** (`-Tier integration -Package clip-sync-repair`) — explicit `--test` list, **no
`--lib`** (`cargo test --tests` still runs `--lib`; do not use it):

```powershell
cargo test -p clip-sync-repair `
  --test config_roundtrip --test scan_gaps_integration --test patch_audio_integration `
  --test query_reference_integration --test cli_wav_integration --test cli_mux_integration `
  --test energy_signature_production --test floor_oracle_integration `
  --test residual_gate_integration --test seam_residual_oracle --test seam_residual_corpus `
  --test wav_bit_depth_integration
```

**Validation / diagnostic tiers (Phase 1):** use `--ignored` with legacy substring filters until
`tier:` prefixes are widespread (`floor_oracle_`, `diagnostic:`, `matrix:`, `gap_corpus_generated`,
etc.). Document ffmpeg requirement in `development.md`.

### Acceptance criteria

- [x] `scripts/test-tier.ps1` exists; `-Tier pr`, `pr-repair`, `pr-align`, `unit`, `integration`
      (repair) invoke the commands above. *(four execution tiers + oracle selector + composites implemented)*
- [x] `.\scripts\test-tier.ps1 -Tier pr` passes locally without ffmpeg. *(ffmpeg-gated
      `cli_mux_integration` is skipped when ffmpeg is absent)*
- [x] `.\scripts\test-tier.ps1 -Tier pr-repair` passes locally without ffmpeg (~4–6 min debug).
- [x] CI (`.github/workflows/ci.yml`) runs `-Tier pr`, not `cargo test --workspace`.
- [x] `development.md` documents the tiers, script examples, wall-time budgets, and that
      `cargo test --workspace` is dev convenience only.
- [x] `development.md` documents integration-only command and warns against `cargo test --tests`.
- [x] `validation-align` / `diagnostic-align` exit with a clear “Phase 2b” message (not silent no-op).
- [x] No test file moves; no `Cargo.toml` `[[test]]` / `autotests` changes.

**Done:** all acceptance criteria met (2026-06-25). Phase 1 conventions live in
[development.md](development.md); proceed to Phase 2.

---

## Phase 2 — Physical separation (repair crate)

**Move oracles out of `--lib`:**

| From | To |
|------|-----|
| `src/test_support/energy_signature_acceptance.rs` `#[test]` fns | `tests/oracle_energy.rs` |
| `src/test_support/energy_signature_production.rs` | `f1_production_haystack_scan_vs_oracle` → `tests/oracle_energy.rs` |
| Keep fixture builders in `test_support/` | no acceptance/oracle/integration tests in acceptance module |

**Move lib scan smokes to integration binary:**

| From | To |
|------|-----|
| `energy_signature_production.rs` (lib) | `scan_detects_f1_production_gap`, `f1_production_scan_and_domain_smoke` → `tests/integration_energy_smoke.rs` |
| same (lib) | `production_matrix_contexts_skip_thirty_on_sixty_sec_fixture` → **stay in lib** (unit) |

**Split validation binaries:**

| From | To |
|------|-----|
| `floor_oracle_integration.rs` (ignored gate/CSV tests) | `tests/validate_floor_oracle.rs` |
| `residual_gate_integration.rs` (future RG rows) + `energy_signature_production.rs` | `tests/validate_residual_gate.rs` (`f1_production_scan_patch_smoke` + EC6 `f4_decoy_*`; no `validate_energy_corpus` binary) |
| Leave in `floor_oracle_integration.rs` or move | fast smokes: `floor_oracle_manifest_loads`, `floor_oracle_gap_frames_use_production_anchor` → `integration_floor_oracle_smoke.rs` |

**Split diagnostics:**

| From | To |
|------|-----|
| `energy_signature_production.rs` ignored matrix/sweeps | `tests/diag_energy_matrix.rs` |
| `energy_signature_production.rs` | `tests/integration_energy_smoke.rs` (`corpus_scan_patch_smoke` + lib scan/domain smokes) |
| `residual_gate_integration.rs` | `tests/integration_residual_gate_smoke.rs` (RG04 only) + `tests/validate_residual_gate.rs` (validation rows) |
| `seam_residual_corpus.rs` `*_csv` tests | `tests/diag_seam_residual.rs` (see [Resolved decisions](#resolved-decisions)) |
| `patch_audio_integration.rs` `i1_/i3_` diagnostic | defer `tests/diag_patch_audio.rs` |

**Rename catalog folder:** `tests/residual_gate/` → `tests/residual_gate_catalog/`; update
references in split binaries, findings doc.

**Harness (same phase):**

- Delete or empty `energy_signature_acceptance.rs` tests after `oracle_energy.rs` split; leave
  builders in `test_support/`.
- **`test_support/` policy:** no new acceptance/oracle/integration `#[test]` modules; existing
  fixture-builder unit tests (`energy_signature_fixtures.rs` `production_spec_tests`) may stay.
- Extract shared loop helpers from `energy_signature_production.rs` (`run_oracle_matrix_rows`,
  `run_matrix_rows`, etc.) to `tests/common/` **before** splitting binaries.
- `validate_floor_oracle.rs` / `validate_residual_gate.rs` import `common::residual_gate_runner`
  and `common::floor_oracle_fixtures` — no duplicated pipeline helpers in the test file.
- `diag_energy_matrix.rs` owns matrix loop helpers or imports shared runner from `common/` if
  shared with oracle tier.

**`test-tier.ps1` (same phase, same PR as renames):**

- `pr-repair`: swap legacy binaries for `integration_energy_smoke`, `integration_floor_oracle_smoke`,
  `integration_residual_gate_smoke`; drop lib `--skip p1_/p2_/p4_/f1_production_haystack` once oracles
  move to `oracle_energy`.
- `-Tier oracle`: `--test oracle_energy` (and existing seam oracle rows) instead of lib name filters.
- `-Tier integration` / `validation` / `diagnostic`: update `--test` lists and filters for split binaries.

**Migration order:** create new binaries → update script → delete or empty old binaries (`energy_signature_production.rs`,
`floor_oracle_integration.rs`, `residual_gate_integration.rs`) in the same PR. With `autotests = true`
(Phase 3 deferred), Cargo auto-discovers new `tests/*.rs` until old files are removed.

### Acceptance criteria

- [x] `cargo test -p clip-sync-repair --lib` &lt; 15s debug (units + `gap_corpus_committed` + fixture-builder units only).
- [x] No acceptance/oracle/integration `#[test]` in `src/test_support/` (fixture-builder units OK).
- [x] `tests/oracle_energy.rs`, `integration_energy_smoke.rs`, smoke/validate/diag splits land per [inventory](#per-file-test-inventory-repair-binaries).
- [x] `tests/residual_gate/` renamed to `tests/residual_gate_catalog/`.
- [x] `.\scripts\test-tier.ps1 -Tier pr-repair` and `-Tier pr` pass without ffmpeg.
- [x] [development.md](development.md) updated if PR commands change materially (no change — still `test-tier.ps1 -Tier pr-repair`).

**Done when:** acceptance criteria above are met.

**Phase 2 follow-up (repair):** move `gap_corpus_committed` (+ generated/external runners) from
`src/application/testing/gap_corpus_fixtures.rs` to `tests/integration_gap_corpus.rs`; add `[[test]]`
entry. **Required** (not optional) if `--lib` still exceeds 15s after the main splits. Same pattern
as corpus split in Phase 2b.

---

## Phase 2b — Physical separation (`clip-sync`)

**Prerequisite:** repair Phase 2 landed (conventions + `test-tier.ps1` patterns proven).

**Why this phase exists:** `clip-sync` has **no** `tests/*.rs` integration binaries today — unlike
`clip-sync-repair`, everything runs under `cargo test -p clip-sync --lib`. That mixes fast domain
units with manifest E2E (`corpus_committed_cases`), symphonia adapter matrices
(`media_reader_tests`, `extract_window_regression`), and slow/generated tiers behind a flat
`#[ignore]` bucket. [corpus-validation.md](corpus-validation.md) documents **data tiers**
(Committed / Generated / External); this phase maps them onto **execution tiers** (integration /
validation / diagnostic) with explicit Cargo targets.

### Two “tier” vocabularies (do not conflate)

| Vocabulary | What it names | Where defined |
|------------|---------------|---------------|
| **Corpus data tier** | Fixture source: committed WAVs vs generated-at-test-time vs external env | `tests/corpus/manifest.toml` `tier = …`, [corpus-validation.md](corpus-validation.md) |
| **Execution tier** | When CI runs the test: unit / integration / validation / diagnostic (oracle = label) | This plan |

| Corpus data tier | Execution tier | Default `cargo test`? | Today’s entry point |
|------------------|----------------|----------------------|---------------------|
| Committed | **integration** | yes | `corpus_committed_cases` in `corpus_fixtures.rs` |
| Generated | **validation** | no (`#[ignore]`) | `corpus_generated_cases`, per-case ignores (`corpus_mkv_tail_*`, query-reference) |
| External | **validation** | no (`#[ignore]` + `CLIP_SYNC_CORPUS`) | `corpus_external_cases` |
| — (manifest probes) | **integration** | yes | `corpus_verify_offset_pass`, `corpus_verify_option_a_false_pass_probe`, ambiguity-flag cases |
| — (fixture regen) | **diagnostic** | no | `regenerate_committed_wav_fixtures` |

### Current baseline (`clip-sync`, measure at 2b kickoff)

| Command | Notes (2026-06-25, approximate) |
|---------|--------------------------------|
| `cargo test -p clip-sync --lib` | All lib `#[test]`; includes committed corpus + symphonia WAV rows + large `align_videos` module |
| `cargo test -p clip-sync corpus_committed` | PR gate today (~few s); filter on `corpus_committed_cases` |
| `cargo test -p clip-sync --features he-aac,test-utils corpus_ -- --ignored` | Generated + external corpus |
| `cargo test -p clip-sync --features ffmpeg-tests extract_window_regression` | Encoded-container extract rows |
| `cargo test -p clip-sync --features ffmpeg-tests,ac3 ac3_corpus_chirp` | AC-3 characterization |

**Lib modules to split (priority order):**

| Source module | Today | Target |
|---------------|-------|--------|
| `application/testing/corpus_fixtures.rs` `mod tests` | committed + generated + external + probes + regen | Runners stay in lib (`test-utils`); `#[test]` → `tests/integration_corpus.rs` + `tests/validate_alignment_corpus.rs` + `tests/diag_regenerate_corpus.rs` |
| `infrastructure/symphonia/extract_window_regression.rs` | WAV in default CI; encoded behind `ffmpeg-tests` | `tests/integration_extract_window.rs` (WAV) + `tests/validate_extract_window.rs` (`required-features = ["ffmpeg-tests"]`) **or** single binary with feature-gated cases |
| `infrastructure/symphonia/media_reader_tests.rs` | mix of WAV + `#[cfg(feature = "ffmpeg-tests")]` | `tests/integration_media_reader.rs` + `tests/validate_media_reader.rs` (`ffmpeg-tests`) |
| `application/testing/anchored_end_oracles.rs` `mod tests` | window oracle math | `tests/oracle_anchored_end.rs` |
| `application/offset_refinement.rs` slow `#[ignore]` | pcm discover/refine 60–120 s | `tests/diag_pcm_refinement.rs` |
| `application/locate_query_spike.rs` | Q0 spike ignores | `tests/diag_locate_query_spike.rs` |
| `infrastructure/symphonia/ac3_oxideav_characterization_tests.rs` | `ac3` + `ffmpeg-tests` | `tests/validate_ac3_characterization.rs` |
| `application/align_videos.rs` `#[cfg(test)]` (~30 tests) | real-pipeline integration smoke | **Defer Phase 2c** — large module; stays in `--lib` for 2b |

**Harness rules (same as repair):**

- `application/testing/corpus_fixtures.rs` keeps `run_manifest_cases`, `run_corpus_case`, manifest
  types — **no `#[test]`** after split.
- Corpus **data** stays at workspace `tests/corpus/` (manifest, WAV, sources.toml).
- Integration binaries use `clip_sync::testing::…` → requires `test-utils` on the library under
  test (`required-features = ["test-utils"]` on corpus-related `[[test]]` entries).

### Target `tests/` layout (`clip-sync`)

```text
crates/clip-sync/tests/
  integration_corpus.rs           # tier:integration — committed manifest E2E + fast probes
  validate_alignment_corpus.rs    # tier:validation — generated, external, source_cases, long query-reference
  integration_extract_window.rs # tier:integration — WAV rows only
  validate_extract_window.rs      # tier:validation — MP4/MKV/MP3 rows (ffmpeg-tests)
  integration_media_reader.rs   # tier:integration — WAV / default adapter smoke
  validate_media_reader.rs        # tier:validation — container round-trips (ffmpeg-tests)
  oracle_anchored_end.rs          # integration tier, oracle label — anchored-end window oracles
  diag_pcm_refinement.rs          # tier:diagnostic — pcm_discover / refine_recovers / diagnose_*
  diag_locate_query_spike.rs      # tier:diagnostic — Q0 spikes
  diag_regenerate_corpus.rs       # tier:diagnostic — regenerate_committed_wav_fixtures
  validate_ac3_characterization.rs  # tier:validation — ac3_corpus_chirp (ac3 + ffmpeg-tests)
```

### Target `Cargo.toml` layout (`clip-sync`)

`clip-sync` has no `tests/` binaries today; Phase 2b **introduces** `autotests = false` and explicit
`[[test]]` entries (same pattern as repair Phase 3 — can land in one PR or split 2b moves / 2b-3
gates).

```toml
[package]
name = "clip-sync"
autotests = false

[features]
default = ["default-tracing"]
# … existing he-aac, ac3, ffmpeg-tests, test-utils …
validation-tests = []    # validate_* binaries
diagnostic-tests = []    # diag_* binaries

# ── tier: integration ────────────────────────────────────────────────────────

[[test]]
name = "integration_corpus"
path = "tests/integration_corpus.rs"
required-features = ["test-utils"]

[[test]]
name = "integration_extract_window"
path = "tests/integration_extract_window.rs"

[[test]]
name = "integration_media_reader"
path = "tests/integration_media_reader.rs"

# ── integration tier, oracle label ─────────────────────────────────────────────

[[test]]
name = "oracle_anchored_end"
path = "tests/oracle_anchored_end.rs"
required-features = ["test-utils"]

# ── tier: validation ───────────────────────────────────────────────────────────

[[test]]
name = "validate_alignment_corpus"
path = "tests/validate_alignment_corpus.rs"
required-features = ["test-utils", "validation-tests"]

[[test]]
name = "validate_extract_window"
path = "tests/validate_extract_window.rs"
required-features = ["ffmpeg-tests", "validation-tests"]

[[test]]
name = "validate_media_reader"
path = "tests/validate_media_reader.rs"
required-features = ["ffmpeg-tests", "validation-tests"]

[[test]]
name = "validate_ac3_characterization"
path = "tests/validate_ac3_characterization.rs"
required-features = ["ac3", "ffmpeg-tests", "validation-tests"]

# ── tier: diagnostic ───────────────────────────────────────────────────────────

[[test]]
name = "diag_pcm_refinement"
path = "tests/diag_pcm_refinement.rs"
required-features = ["diagnostic-tests"]

[[test]]
name = "diag_locate_query_spike"
path = "tests/diag_locate_query_spike.rs"
required-features = ["diagnostic-tests"]

[[test]]
name = "diag_regenerate_corpus"
path = "tests/diag_regenerate_corpus.rs"
required-features = ["test-utils", "diagnostic-tests"]
```

**`integration_corpus.rs` test inventory (from `corpus_fixtures.rs`):**

| Test fn | Stays / moves | Notes |
|---------|---------------|-------|
| `corpus_manifest_loads` | integration | fast smoke |
| `corpus_committed_cases` | integration | **PR gate** (`pr-align`) |
| `corpus_verify_offset_pass` | integration | generated-at-test-time single case |
| `corpus_verify_option_a_false_pass_probe` | integration | verification contract |
| `corpus_looped_discovery_alias_sets_ambiguity_flag` | integration | |
| `corpus_repeated_segment_sets_ambiguity_flag` | integration | |
| `corpus_query_reference_b_longer_fast` | integration | fast query-reference smoke |
| `corpus_generated_cases` | **validate_alignment_corpus** | `#[ignore]` → validation binary |
| `corpus_external_cases` | **validate_alignment_corpus** | |
| `corpus_source_cases` | **validate_alignment_corpus** | |
| `corpus_mkv_tail_decodable_extent_gap` | **validate_alignment_corpus** | |
| `corpus_query_reference_45min_anchor` | **validate_alignment_corpus** | |
| `corpus_query_reference_b_longer_anchor` | **validate_alignment_corpus** | |
| `regenerate_committed_wav_fixtures` | **diag_regenerate_corpus** | |

### `test-tier.ps1` extensions (Phase 2b)

Add `-Package clip-sync` (or dedicated tiers) and **`pr-align`** workspace profile:

```powershell
# pr-align — replaces bare `cargo test -p clip-sync corpus_committed`
cargo test -p clip-sync --lib
cargo test -p clip-sync --test integration_corpus
cargo test -p clip-sync --test integration_extract_window
cargo test -p clip-sync --test integration_media_reader
cargo test -p clip-sync --test oracle_anchored_end

# validation — nightly; needs ffmpeg, optional he-aac / corpus env
cargo test -p clip-sync --features test-utils,validation-tests,he-aac --test validate_alignment_corpus

# diagnostic
cargo test -p clip-sync --features test-utils,diagnostic-tests --test diag_regenerate_corpus -- --nocapture
```

Update **PR workspace** CI profile to `pr-align` + `pr-repair` once both scripts exist.

### Phase 2b deliverables

1. Create `crates/clip-sync/tests/` binaries per layout above (incremental PRs OK: corpus first,
   then symphonia, then diag).
2. Strip `#[test]` from split modules; leave runners/fixtures in `application/testing/` and
   `test_support/`.
3. Standardize `#[ignore]` → `tier:validation` / `tier:diagnostic` on any tests left in `--lib`
   until 2c moves `align_videos` tests.
4. Extend `scripts/test-tier.ps1` with `pr-align`, `validation-align`, `diagnostic-align`.
5. Update [development.md](development.md) and [corpus-validation.md](corpus-validation.md) quick
   start to reference tier script (not only `corpus_` name filters).

### Phase 2b-3 — Feature gates (`clip-sync`, optional)

Land `autotests = false` + `validation-tests` / `diagnostic-tests` + `[[test]]` table above.
Default `cargo test -p clip-sync` compiles `--lib` + integration binaries only (oracle_* are
integration).

**Done when:**

- `cargo test -p clip-sync --lib` no longer runs `corpus_committed_cases` (moved to
  `integration_corpus`).
- `.\scripts\test-tier.ps1 -Tier pr-align` passes locally without `--ignored`.
- [corpus-validation.md](corpus-validation.md) quick start lists `test-tier.ps1` commands alongside
  legacy `corpus_` filters.

### Phase 2c — `align_videos` integration (defer)

Move `align_videos.rs` `#[cfg(test)]` module (~30 real-pipeline tests) to
`tests/integration_align_videos.rs` when `--lib` wall time or PR confusion warrants it. Not required
for `pr-align` if name filters or module-level organization suffice short term.

---

## Phase 3 — Feature-gated tiers (optional)

Wire the [target `[[test]]` layout](#target-cargotoml-layout-test) into
`clip-sync-repair/Cargo.toml`:

1. Set `autotests = false`.
2. Add `validation-tests`, `diagnostic-tests` features (see sketch). No `oracle-tests` feature —
   `oracle_*` binaries are integration tier and always compile.
3. Declare every binary with `[[test]]`; put `required-features` on validation, diagnostic, and
   existing `ffmpeg-mux` targets.
4. Do **not** use `#![cfg(feature)]` at file tops — `required-features` on `[[test]]` is
   sufficient for skipping whole binaries.
5. Refactor `tests/common/` consumers per [harness + Clippy](#phase-3--testscommon-and-clippy) (same
   PR or immediately after step 1–3).

**CI fast path:** default `cargo test -p clip-sync-repair` compiles unit + integration binaries
only (oracle_* are integration; no `validation-tests` / `diagnostic-tests`).

**Nightly / local:**

```powershell
cargo test -p clip-sync-repair --features validation-tests --test validate_floor_oracle --test validate_residual_gate
cargo test -p clip-sync-repair --features diagnostic-tests --test diag_energy_matrix --test diag_seam_residual -- --nocapture
```

Update `scripts/test-tier.ps1` `Invoke-RepairValidation` / `Invoke-RepairDiagnostic` to pass
`--features validation-tests` / `--features diagnostic-tests` and drop `--ignored` for binaries
that are feature-gated only.

(No `--ignored` once tests live only in feature-gated binaries.)

### Phase 3 — `tests/common/` and Clippy

**Problem (2026-06):** `cargo clippy --workspace --all-targets -- -D warnings` fails on
`clip-sync-repair` integration binaries. Each `tests/*.rs` file is a **separate crate root**; Phase
2's `mod common;` compiles all four runner modules in every consumer, but each binary uses only one
or two — rustc reports `dead_code` / `unused_imports` on the rest. This is a false positive caused
by integration-test layout, not unused harness code.

**Units (already the right split — do not re-merge):**

| Module | Depends on | Consumers |
|--------|------------|-----------|
| `floor_oracle_fixtures` | — | `integration_floor_oracle_smoke`, `validate_floor_oracle` |
| `residual_gate_runner` | `floor_oracle_fixtures` | `validate_floor_oracle` only |
| `seam_residual_scoring` | — | `seam_residual_corpus`, `diag_seam_residual` |
| `energy_signature_matrix` | — | `diag_energy_matrix` only |

**What tier features fix:** `required-features` on `validate_*` / `diag_*` binaries stops compiling
validation/diagnostic **binaries** (and their runner modules) on PR / default `cargo clippy`. That
removes roughly half of today's Clippy noise.

**What tier features do *not* fix:** features are **crate-wide**. On PR, two integration binaries
still overlap — `integration_floor_oracle_smoke` needs `floor_oracle_fixtures` while
`seam_residual_corpus` needs `seam_residual_scoring`, but `mod common` would still compile both in
each binary. No `validation-tests` / `diagnostic-tests` boundary separates always-on integration
consumers.

**Decision (Phase 3 harness):**

1. **Drop `mod common` and `tests/common/mod.rs`.** Each consumer declares only the modules it
   needs, sibling to the test root:

   ```rust
   #[allow(dead_code)] // harness pub items shared across binaries; each consumer uses a subset
   mod floor_oracle_fixtures {
       include!("common/floor_oracle_fixtures.rs");
   }
   ```

   Refactor `residual_gate_runner.rs`: `super::floor_oracle_fixtures` → `crate::floor_oracle_fixtures`.
   Use `//` (not `//!`) for file headers inside `tests/common/*.rs` — inner doc comments break
   under `include!`.

2. **Do not use `#[path = "..."]`** — same brittleness as `include!`, but `include!` is the
   documented pattern for shared integration sources when `autotests = false` and there is no
   `tests/common/mod.rs` entry.

3. **Do not rely on `#[cfg(feature)]` inside a shared `common/mod.rs`** to subset modules — it does
   not give per-binary dependency sets and still leaves integration↔integration overlap on PR.

4. **Workspace micro-crates** (`clip-sync-repair-test-floor-oracle`, etc.) — defer unless Phase 2b
   align harness growth makes `include!` repetition painful; not required for repair Phase 3.

5. **`#[allow(dead_code)]` on the `mod { include!(…) }` wrapper** — required when a harness file
   exposes items for multiple consumers (e.g. `seam_residual_scoring` serves both corpus and diag);
   not a substitute for monolithic `mod common`.

**Clippy expectations after Phase 3:**

| Command | Compiles |
|---------|----------|
| `cargo clippy -p clip-sync-repair --all-targets -- -D warnings` (PR) | lib + integration `[[test]]` binaries only |
| `+ --features validation-tests` | + `validate_*` |
| `+ --features diagnostic-tests` | + `diag_*` |
| Full local / nightly | both tier features (+ `ffmpeg-mux` / `ffmpeg-tests` as needed) |

Document in `development.md`; optional CI nightly row runs validation tier features before merge
to main.

**Done when:** default `cargo test -p clip-sync-repair` does not compile validation or diagnostic
binaries; `cargo clippy -p clip-sync-repair --all-targets -- -D warnings` passes without tier
features; `development.md` documents features, explicit `--test` names, and the Clippy command
variants above.

---

## Phase 4 — cargo-nextest profiles (optional)

Add `.config/nextest.toml`:

```toml
[profile.pr-repair]
# unit + fast integration + disagreement_oracles
default-filter = '…'

[profile.validation]
default-filter = 'test(validate_) or test(floor_oracle) or test(source_gap_oracle)'
run-ignored = true
```

```powershell
cargo nextest run --profile pr-repair -p clip-sync-repair
cargo nextest run --profile validation -p clip-sync-repair
```

**Done when:** CI or local docs prefer nextest for tier runs; script remains fallback.

---

## Phase 5 — Validation crate (defer)

`clip-sync-repair-validate` workspace member:

- Depends on `clip-sync-repair`, `clip-sync` (`test-utils`).
- Owns `validate_*` tests, matrix driver, optional CLI.
- `cargo test -p clip-sync-repair` = product tests only.

**Trigger:** matrix driver ships, or validation compile time materially slows default builds.

---

## CI profiles (target)

| Profile | Tiers | When |
|---------|-------|------|
| **PR workspace** | `pr` (= `pr-align` + `pr-repair` + `clip-sync-cli`) | every PR |
| **PR repair extended** | `pr-repair-extended` | optional / path filters on `clip-sync-repair/**` |
| **Nightly** | `validation` + `validation-align` for both crates | scheduled; ffmpeg on runner |
| **Manual** | `diagnostic` + `diagnostic-align` | tuning / CSV baselines |

See [PR contract](#pr-contract-required-ci-gates) for per-fn detail and wall budgets.

---

## CI migration

How GitHub Actions evolves as phases land. Current (Phase 1, live):

```yaml
# .github/workflows/ci.yml (current — Phase 1 landed)
- run: ./scripts/test-tier.ps1 -Tier pr
```

| Stage | Trigger | Workflow change | Notes |
|-------|---------|-----------------|-------|
| **0 — was** | — | `cargo test --workspace` | Runs full lib + all integration binaries; no ffmpeg; ~65s repair lib + integration compile cost |
| **1 — Phase 1 (landed 2026-06-25)** | `test-tier.ps1` exists | ✅ `run: ./scripts/test-tier.ps1 -Tier pr` | Live in `ci.yml`; replaced workspace blanket test; Windows runner; no ffmpeg |
| **2 — Phase 2 lands** | repair lib &lt;15s | keep `-Tier pr`; update `pr-repair` binary list + drop lib oracle `--skip`; document extended profile optional | `integration_*_smoke`, `oracle_energy`; script changes in same PR as file splits |
| **3 — repair Phase 3** | feature gates | `pr` does **not** pass `validation-tests` / `diagnostic-tests` | Nightly job added (below) |
| **4 — Phase 2b** | align `tests/` binaries | `pr-align` in script; `pr` uses it instead of `corpus_` filter | align lib still heavy until 2c |
| **5 — optional nextest** | script pain | `cargo nextest run --profile pr` | script remains fallback |

### Nightly job (target, post–Phase 3 + 2b)

Separate workflow or `workflow_dispatch` / `schedule` job:

```yaml
# sketch — not shipped with Phase 1
jobs:
  nightly-validation:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
      - run: choco install ffmpeg -y   # or equivalent; document in development.md
      - run: ./scripts/test-tier.ps1 -Tier validation -Package clip-sync-repair
      - run: ./scripts/test-tier.ps1 -Tier validation-align -Package clip-sync
```

Env (optional): `CLIP_SYNC_CORPUS`, `CLIP_SYNC_GAP_CORPUS` as repo/org variables for external
tiers — skip step when unset (tests already no-op).

### `cargo test --workspace` after tiers ship

**Decision:** not the PR gate. Document in `development.md` as “full local compile check” only.
Default `cargo test -p <crate>` per crate still runs that crate’s lib + default integration
binaries (respecting `autotests = false` + no validation features).

---

## Resolved decisions

Closes gaps called out in plan review (2026-06-25).

| Topic | Decision |
|-------|----------|
| **RG04 on PR** | `off_no_regression_baseline` stays on PR in **`integration_residual_gate_smoke.rs`** (integration tier). Do not move the only default-CI RG test behind `validation-tests`. |
| **`corpus_scan_patch_smoke` on PR** | Moves to **`integration_energy_smoke.rs`** (integration), not diagnostic. |
| **`seam_residual_corpus` CSV tests** | Split to **`diag_seam_residual.rs`** with `diagnostic-tests`; the `seam_residual_corpus` binary (integration, oracle label) keeps score/acceptance fns only. Replaces earlier “keep file” wording. |
| **`#[ignore]` after feature-gated binaries** | **Remove** `#[ignore]` when the test lives in a `required-features` binary (validation/diagnostic). Keep `tier:` in comments or module docs if useful. Use `#[ignore]` only for tests still in a shared binary that mixes default + manual rows. |
| **oracle is a label, not a tier** | Domain-acceptance (`oracle_*`) tests schedule as **integration** (repo-only) or **validation** (external dep / exhaustive contract). No `oracle-tests` feature; `oracle_*` binaries always compile. Select via `cargo test oracle_` or acceptance ID. |
| **Composite `pr` tier** | `pr-align` + `pr-repair` + `cargo test -p clip-sync-cli`. |
| **Validation command `--ignored`** | Required only while tests remain in lib/default binaries. After Phase 3 / 2b-3, run `cargo test --features validation-tests --test validate_*` **without** `--ignored`. |
| **Non-goals Phase 5 typo** | Validation crate deferral refers to **Phase 5**, not Phase 4 (nextest). |
| **`f1_production_scan_patch_smoke`** | Lands in **`validate_residual_gate.rs`** with EC6 `f4_decoy_*` rows — no separate `validate_energy_corpus` binary. |
| **Lib `energy_signature_production` tests** | `f1_production_haystack_scan_vs_oracle` → **`oracle_energy.rs`**; `scan_detects_f1_production_gap` + `f1_production_scan_and_domain_smoke` → **`integration_energy_smoke.rs`**; `production_matrix_contexts_skip_thirty_on_sixty_sec_fixture` → **stay in lib** (unit). |
| **`test_support/` `#[test]` policy** | No acceptance/oracle/integration tests in `test_support/` after Phase 2. Fixture-builder unit tests (`energy_signature_fixtures.rs` `production_spec_tests`) **may stay** — they are not tier violations. |

---

## Decisions

| Topic | Decision |
|-------|----------|
| Tier count | Four: unit, integration, validation, diagnostic. **oracle = label** (`oracle_` prefix), scheduled as integration/validation — see [v2 note](#temporary-plan-test-tiers-unit--integration--validation--diagnostic) and [decision rule](#tier-decision-rule) |
| Primary gate | `#[ignore]` + naming + script; not a new crate in v1 |
| `examples/` | Not used for validation |
| Residual gate catalog | Stays data (`matrix.toml`); rename to `residual_gate_catalog/` |
| `cargo test --tests` | Document as **misleading** (includes `--lib`); use tier script |
| Workspace scope | Phase 1–2 repair-first; **Phase 2b** = `clip-sync` corpus + symphonia splits; **Phase 2c** = `align_videos` defer |
| libtest `--skip` | Acceptable in Phase 1; reduce reliance after Phase 2 moves oracles |
| Harness layout | Fixtures in `test_support/`; runners in `tests/common/`; catalogs data-only; thin `#[test]` in tier binaries — see [Harness organization](#harness-organization-fixtures-runners-catalogs) |
| PR contract | [PR contract](#pr-contract-required-ci-gates) is source of truth for CI; per-file splits in [inventory](#per-file-test-inventory-repair-binaries) |
| Oracle (label) on PR | **Yes** for `seam_residual_disagreement_oracles` + `oracle_energy` SD rows (integration tier); not all `oracle_*` rows |
| RG04 / energy smoke on PR | **integration** smokes (`integration_residual_gate_smoke`, `integration_energy_smoke`), not validation feature |
| `#[ignore]` vs feature gates | Remove ignore when binary is feature-gated; see [Resolved decisions](#resolved-decisions) |
| `tests/common/` harness (Phase 3) | Drop `mod common`; per-binary `include!("common/….rs")`; tier features gate binaries not integration overlap — see [Phase 3 — `tests/common/` and Clippy](#phase-3--testscommon-and-clippy) |
| `cargo test --workspace` | Dev convenience only after Phase 1; not CI PR gate |
| `test_support/` tests | Builders only for acceptance/oracle/integration; fixture-builder unit tests OK — see [Resolved decisions](#resolved-decisions) |
| `validate_residual_gate` scope | `f1_production_scan_patch_smoke` + EC6 `f4_decoy_*` + future RG rows; no `validate_energy_corpus` crate/binary |

---

## What not to do

| Approach | Why skip |
|----------|----------|
| Custom libtest harness | Fragile, non-standard |
| `tests/validation/` folder without file split | Cargo compiles `tests/*.rs` as binaries; folder alone does not create tiers |
| Move catalog to `docs/` only | `matrix.toml` + baseline CSV are test-adjacent artifacts |
| Full rename of every test in Phase 1 | High churn; prefix on new/edited only |

---

## Verification

```powershell
# After Phase 1
.\scripts\test-tier.ps1 -Tier pr                    # full workspace PR gate
.\scripts\test-tier.ps1 -Tier pr-repair
.\scripts\test-tier.ps1 -Tier unit -Package clip-sync-repair
.\scripts\test-tier.ps1 -Tier validation -Package clip-sync-repair   # needs ffmpeg + corpus

# After Phase 2
cargo test -p clip-sync-repair --lib          # should be much faster
cargo test -p clip-sync-repair --test oracle_energy
cargo test -p clip-sync-repair --test integration_energy_smoke
cargo test -p clip-sync-repair --test integration_residual_gate_smoke

# After Phase 3 (validation compile-gated; no --ignored)
cargo test -p clip-sync-repair --features validation-tests --test validate_floor_oracle

# After Phase 2b
.\scripts\test-tier.ps1 -Tier pr-align
cargo test -p clip-sync --test integration_corpus

# After Phase 2b-3
cargo test -p clip-sync --features validation-tests --test validate_alignment_corpus
```

---

## Related docs

- [test-acceptance-glossary.md](test-acceptance-glossary.md) — SD/SP/EC/RG/PL/GK/CS IDs (permanent)
- [development.md](development.md) — build/test commands (update in Phase 1)
- [corpus-validation.md](corpus-validation.md) — alignment corpus tiers (parallel pattern)
- [crates/clip-sync-repair/tests/residual_gate_catalog/README.md](../crates/clip-sync-repair/tests/residual_gate_catalog/README.md) — RG contract catalog (legacy C1–C5)
- [residual-gate-findings.md](residual-gate-findings.md) — shipped gate findings
