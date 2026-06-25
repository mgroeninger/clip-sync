# Temporary plan: test tiers (unit / integration / oracle / validation / diagnostic)

> **Status:** Draft (2026-06-25). Motivated by confusion between CI-fast tests, integration
> tests, domain oracles, and validation/contract work (e.g. residual gate C1–C5, floor oracle,
> energy acceptance). Cargo exposes a single `cargo test` surface; `clip-sync-repair --lib`
> already runs ~65s with 8 `#[ignore]` tests while mixing true units with P1/P4 oracles.
> Integration binaries add ~20 min (dominated by `patch_audio_integration`). There is no way to
> run “integration only” or “validation only” without knowing per-file `--test` flags.
>
> Archive to `docs/archive/test-tier-plan.md` when shipped. Until then, update
> [development.md](development.md) only after Phase 1 lands.

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

- Five named **tiers** with documented commands, CI profiles, and conventions for new tests.
- **PR-fast** path that does not run validation or diagnostics.
- **Physical separation** over time so `--lib` returns to seconds-scale true units.
- Optional **cargo-nextest** profiles when script-based filtering becomes painful.

**Non-goals (v1):**

- Replacing `#[test]` / libtest with a custom harness.
- Using `examples/` for contract validation.
- Mandatory validation crate in v1 (defer unless Phase 4 is insufficient).
- Renaming every existing test in one pass (convention applies to new/edited tests;
  migrate opportunistically).
- Workspace-wide rollout to `clip-sync` / `clip-sync-cli` in Phase 1 (repair crate first).

---

## Tier definitions

| Tier | Purpose | Default CI? | Typical location | Typical runtime |
|------|---------|-------------|------------------|-----------------|
| **unit** | Pure logic: policies, config, CLI parse, small fakes | yes | `src/**` `#[test]` | ms–s |
| **integration** | Patch/scan/CLI on synthetic WAV; seam behavior | yes (subset) | `tests/*_integration.rs` | s–min |
| **oracle** | Domain acceptance: U*, P*, F*, score harness rows | optional PR | `tests/oracle_*.rs` (target); today also `--lib` | s–min |
| **validation** | Real codec / corpus / contract (C1–C5, FLOOR_OK, Run B) | no (`#[ignore]`) | `tests/validate_*.rs` (target); today `floor_oracle_integration.rs` | min+; needs ffmpeg/corpus |
| **diagnostic** | CSV dumps, sweeps, golden generators | never | same files as validation/oracle | manual only |

**Mapping to Cargo:**

| Mechanism | Tier use |
|-----------|----------|
| `cargo test --lib` | **unit** (target: unit only) |
| `cargo test --test <file>` | **integration** / **oracle** / **validation** by filename |
| `#[ignore = "tier:…"]` | **validation** + **diagnostic** off default CI |
| `cargo test … -- --ignored` | validation + diagnostic (filter by name) |
| `#[cfg(feature = "…")]` | optional compile-time exclusion (Phase 3) |
| `cargo nextest run --profile …` | orchestration (Phase 4) |

---

## Current baseline (`clip-sync-repair`, 2026-06-25)

| Command | Wall time (debug) | Notes |
|---------|-------------------|-------|
| `cargo test -p clip-sync-repair --lib` | ~65s | 269 tests, 8 ignored; includes P1/P4 oracles, `gap_corpus_committed`, `f1_production_haystack_scan_vs_oracle` |
| `cargo test -p clip-sync-repair --tests` | ~21 min | **Also runs `--lib`**; not integration-only |
| `--test patch_audio_integration` | ~15.5 min | 28 pass; I1/I2/I3 energy fixtures dominate |
| `--test residual_gate_integration` | ~82s | C4 `off_no_regression_baseline` |
| `--test floor_oracle_integration` | &lt;1s default | 7/9 tests `#[ignore]` (validation) |
| `--test seam_residual_corpus` | ~21s | 2 pass; 4 diagnostic `#[ignore]` |

**Ignored lib tests (8):** `gap_corpus_{generated,external,patch_timing_*,regenerate}`,
`p4_f4_decoy_unified_search_diverges`, `write_full_surface_repair_golden`.

**Catalog (not tests):** `crates/clip-sync-repair/tests/residual_gate/` — matrix + README;
maps scattered tests to C1–C5.

---

## Conventions (all phases)

### `#[ignore]` reason prefix

```rust
#[ignore = "tier:validation — needs ffmpeg + fetch_corpus_sources"]
#[ignore = "tier:diagnostic — CSV export; cargo test diag_seam_residual -- --ignored --nocapture"]
```

Human-readable tail after `tier:<name> —`.

### Test name prefixes (new / renamed tests)

| Prefix | Tier |
|--------|------|
| `unit_` | unit (rare in `src`; modules usually suffice) |
| `integration_` | integration |
| `oracle_` | oracle |
| `validate_` | validation |
| `diag_` | diagnostic |

Existing names (e.g. `seam_residual_disagreement_oracles`) stay until touched.

### Integration test binaries (target layout)

```text
tests/
  common/                          # shared runners (residual_gate_runner, floor_oracle_fixtures)
  patch_audio_integration.rs       # tier:integration
  scan_gaps_integration.rs
  cli_*_integration.rs
  oracle_energy.rs                 # tier:oracle (split from lib acceptance)
  validate_floor_oracle.rs         # tier:validation (from floor_oracle_integration.rs)
  validate_residual_gate.rs        # tier:validation (from residual_gate_integration.rs)
  diag_energy_matrix.rs            # tier:diagnostic (from energy_signature_production ignores)
  residual_gate_catalog/           # rename from residual_gate/ — matrix.toml + README only
```

Rule: **if it is not fast and not testing one module in isolation, it does not belong in
`src/**` `#[test]`.**

---

## Phase 1 — Convention + script + docs (no file moves)

**Deliverables:**

1. `scripts/test-tier.ps1` — tier selector (repair crate first; extend to workspace later).
2. `docs/development.md` — replace four-bucket table with tier table + script examples.
3. Standardize `#[ignore]` on **edited** validation/diagnostic tests (`tier:validation`,
   `tier:diagnostic`).
4. Document **integration-only** command (all `--test` binaries, no `--lib`).

**`scripts/test-tier.ps1` interface:**

```powershell
param(
  [ValidateSet('unit','integration','oracle','validation','diagnostic','pr','pr-repair')] $Tier,
  [switch] $Nocapture
)

# unit        — --lib with skips for known oracles (until Phase 2)
# integration — --test *_integration binaries; --skip i1_/i2_/i3_ heavy rows
# oracle      — seam_residual_disagreement_oracles, p1_/p2_/p4_/u* filters on --lib + oracle binary
# validation  — floor_oracle, source_gap_oracle, gate_real_codec, deadzone — --ignored
# diagnostic  — diag_, *_csv, energy_signature_mode — --ignored --nocapture
# pr-repair   — composed fast gate for clip-sync-repair
```

**`pr-repair` composition (initial):**

```powershell
cargo test -p clip-sync-repair --lib -- --skip p1_ --skip p2_ --skip p4_ --skip integration_ --skip f1_production_haystack
cargo test -p clip-sync-repair gap_corpus_committed
cargo test -p clip-sync-repair seam_residual_disagreement_oracles
cargo test -p clip-sync-repair --test config_roundtrip --test scan_gaps_integration --test cli_mux_integration
# optional: --test patch_audio_integration with --skip i1_ --skip i2_ --skip i3_
```

**Done when:** `.\scripts\test-tier.ps1 -Tier pr-repair` passes locally; `development.md` lists
all tiers with wall-time expectations.

---

## Phase 2 — Physical separation (repair crate)

**Move oracles out of `--lib`:**

| From | To |
|------|-----|
| `src/test_support/energy_signature_acceptance.rs` `#[test]` fns | `tests/oracle_energy.rs` |
| Keep fixture builders in `test_support/` | no tests in acceptance module |

**Split validation binaries:**

| From | To |
|------|-----|
| `floor_oracle_integration.rs` (ignored gate/CSV tests) | `tests/validate_floor_oracle.rs` |
| `residual_gate_integration.rs` | `tests/validate_residual_gate.rs` |
| Leave in `floor_oracle_integration.rs` | fast smokes: `floor_oracle_manifest_loads`, `floor_oracle_gap_frames_use_production_anchor` → or move to `integration_` smoke file |

**Split diagnostics:**

| From | To |
|------|-----|
| `energy_signature_production.rs` ignored matrix/sweeps | `tests/diag_energy_matrix.rs` |
| `seam_residual_corpus.rs` `*_csv` tests | keep file; ensure `diag_` prefix + `tier:diagnostic` |

**Rename catalog folder:** `tests/residual_gate/` → `tests/residual_gate_catalog/`; update
references in `residual_gate_integration.rs`, `floor_oracle_integration.rs`, findings doc.

**Done when:** `cargo test -p clip-sync-repair --lib` &lt; 15s debug; tier script still green.

---

## Phase 3 — Feature-gated tiers (optional)

Add to `clip-sync-repair/Cargo.toml`:

```toml
[features]
default = []
oracle-tests = []       # compiles tests/oracle_*.rs
validation-tests = []   # compiles tests/validate_*.rs
```

Top of gated files:

```rust
#![cfg(feature = "validation-tests")]
```

**CI fast path:** no features → validation binaries do not compile.

**Nightly / local:**

```powershell
cargo test -p clip-sync-repair --features validation-tests -- --ignored
```

**Done when:** default `cargo test -p clip-sync-repair` does not compile validation binaries;
document feature flags in `development.md`.

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
| **PR workspace** | `clip-sync corpus_committed` + `clip-sync-repair pr-repair` | every PR |
| **PR repair extended** | + `integration` (full `patch_audio` sine tests) | optional / path filters |
| **Nightly** | + `validation` (`--ignored`, ffmpeg + corpus fetch) | scheduled |
| **Manual** | `diagnostic` | tuning / CSV baselines |

---

## Decisions

| Topic | Decision |
|-------|----------|
| Tier count | Five: unit, integration, oracle, validation, diagnostic |
| Primary gate | `#[ignore]` + naming + script; not a new crate in v1 |
| `examples/` | Not used for validation |
| Residual gate catalog | Stays data (`matrix.toml`); rename to `residual_gate_catalog/` |
| `cargo test --tests` | Document as **misleading** (includes `--lib`); use tier script |
| Workspace scope | Phase 1–2 repair-first; align `clip-sync` corpus tiers in Phase 2b |
| libtest `--skip` | Acceptable in Phase 1; reduce reliance after Phase 2 moves oracles |

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
.\scripts\test-tier.ps1 -Tier unit
.\scripts\test-tier.ps1 -Tier integration
.\scripts\test-tier.ps1 -Tier oracle
.\scripts\test-tier.ps1 -Tier validation    # needs ffmpeg + corpus
.\scripts\test-tier.ps1 -Tier pr-repair

# After Phase 2
cargo test -p clip-sync-repair --lib          # should be much faster
cargo test -p clip-sync-repair --test validate_floor_oracle -- --ignored
cargo test -p clip-sync-repair --test oracle_energy
```

---

## Related docs

- [development.md](development.md) — build/test commands (update in Phase 1)
- [corpus-validation.md](corpus-validation.md) — alignment corpus tiers (parallel pattern)
- [crates/clip-sync-repair/tests/residual_gate/README.md](../crates/clip-sync-repair/tests/residual_gate/README.md) — C1–C5 contract catalog
- [residual-gate-findings.md](residual-gate-findings.md) — shipped gate findings
