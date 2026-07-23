# Residual gate — validation notes

Epistemic guide + engineering notes for residual/floor gate tests (`SeamResidualVerdict`,
`residual_gate` modes).

| Document | Role |
|----------|------|
| **[`matrix.toml`](matrix.toml)** | Canonical **inventory** of implemented instances (`location`, `config_profile`, `tier`, `assertion`) |
| **This README** | Why tests mean what they claim (config profiles, G5 ladder), **backlog**, **implementation** runbook |

Companions:

- [archive/residual-gate-findings.md](../../../../docs/dev/archive/residual-gate-findings.md) — bugs, G5 Run B, FD-1 (archived ledger)
- [archive/residual-gate-wiring-plan.md](../../../../docs/dev/archive/residual-gate-wiring-plan.md) — production wiring (archived design)
- [BACKLOG.md](../../../../BACKLOG.md) § Residual gate follow-ups — open/deferred items
- [floor_oracle/manifest.toml](../floor_oracle/manifest.toml) — real-media encode cases

Harness extraction and test file migration are **optional hygiene** (G5 is resolved; rescue stays
non-default). Do them when adding backlog tests or when navigation hurts — not prerequisites.

---

## What this suite must establish

The residual/floor gate ships as **default `veto`** (anti-echo), with `veto_rescue` an opt-in
false-skip recovery. This collection of tests establishes the gate's **validity contract** (C1a–C5
below). Each `matrix.toml` row carries a `claims` field naming which clause(s) it serves.

| # | Claim | Why it matters | Status | Instances |
|---|-------|----------------|--------|-----------|
| **C1a** | **Gate composition** — when `informative ∧ headroom > margin ∧ Pearson passes`, `apply_residual_to_confidence` vetoes | Proves the veto rule is wired correctly | **Proved** (score + unit) | `disagreement_oracles`; `f4_decoy_placement_informative_with_high_headroom`; `gap_fill_fit.rs` units |
| **C1b** | **Pipeline veto fires** — `PatchAudio` under `production_fit` skips with `ResidualHeadroomExceeded` on an acoustic echo | Optional end-to-end proof the skip reason surfaces | **Not proved; optional** — **F4 out of scope (M6)** | — (see Backlog if pursued) |
| **C2** | **Abstains out-of-regime** — two-mic / different-capture → floor uninformative → no opinion | Protects non-same-master pairs | Calibration + **`production_fit`** | `gate_real_codec`; `gate_real_codec_production_fit` (two_mic) |
| **C3** | **Never false-vetoes a truth gap** | A veto that harms good fills is worse than no gate | **`production_fit`** + calibration + M5 units | `gate_real_codec_production_fit`; `vorbis_64k_no_false_veto`; `apply_residual_abstains_*` |
| **C4** | **`off` is a true no-op** | Opt-out must be costless and exact | **Proved** | `off_no_regression_baseline` |
| **C5** | **Rescue has real-media value** | Decides whether rescue should be default | **Resolved: no** (G5) | `deadzone_finale_run_b`, `deadzone_punch_assert`, `rescue_bb_synthetic` |

**Reading the status column:** For default `veto`, **C1a + C3 + C4 + C2** are the shipped contract.
**C1b** is not required to ship — no test asserts `ResidualHeadroomExceeded` from a pipeline run
today, and F4 cannot provide it under production nominal-floor anchoring (**M6**). Score-level F4
veto in `disagreement_oracles` proves **C1a** only (harness truth-anchors the floor at the decoy
frame; production anchors at nominal → abstain). `f4_decoy_residual_gate_vetoes_bool` documents
that pipeline behavior (abstain → patch), not C1b.

---

## Config profiles (read this first)

The same manifest case can **pass Pearson under calibration** and sit in the **dead zone under
`production_fit`** (G5). Tag every real-codec row with which profile it actually uses.

| Profile | How to recognize it | Pearson / fit | Used for |
|---------|---------------------|---------------|----------|
| **`calibration`** | `production_repair_config` or `floor_oracle_repair_config` in `validate_floor_oracle.rs` | `min_fill 0`, `fill_absolute_floor −0.05`, `waveform_weight 0` | FLOOR_OK measurement, structure-isolated gate checks |
| **`production_fit`** | `RepairConfig::default()` (optionally override `gap_signature_mode` / `residual_gate`) | Shipped floors (`min_fill 0.35`, etc.) | Run B, anything claiming shipped patch behavior |
| **`production_like_synthetic`** | `RepairConfig { ..Default::default() }` on in-memory fixtures | Production floors + default weights | `broadband_oracle_veto_rescue_patches_marginal` |

**Floor-oracle gate tests** (`floor_oracle_residual_gate_real_codec`,
`floor_oracle_veto_rescue_real_broadband_codec`, `floor_oracle_vorbis_64k_veto_no_false_veto`) run
**calibration**, not `production_fit`, via `run_built_floor_oracle` → `floor_oracle_repair_config`.
Their “truth patches” / “rescue ≡ veto” claims are **valid under relaxed floors only** — not proof
of shipped gate behavior on real media (see G5).

**Run B** (`source_gap_oracle_transient_csv`) uses `run_built_floor_oracle_cfg` +
`RepairConfig::default()` — the only floor-oracle driver on **`production_fit`** today.

Never infer shipped behavior from `source_gap_oracle_floor_csv` or the calibration gate tests.

---

## Taxonomy (axes for `matrix.toml` rows)

Three axes describe a validation instance:

1. **Layer / placement** — where in the stack; who picks B's alignment.
2. **Gap geometry** — A/B relation and how the gap was built.
3. **Evaluation** — which pipeline stages run; `residual_gate` mode.

### Layers

| Layer | What runs | Can prove search-winner behavior? |
|-------|-----------|-----------------------------------|
| **A — Unit** | `policies.rs` probes | No |
| **B — Score** | `seam_residual_corpus.rs` at a **fixed** B frame | No (P0 only) |
| **C — Pipeline** | `PatchAudio` / floor-oracle runners | Yes (P2+) |

### Placement (abbreviated)

| Mode | Who places B | Epistemic note |
|------|--------------|----------------|
| **P0** truth | Harness | Signal disagreement (H2-B, F4); not field placement |
| **P1** oracle nominal | Manifest gap + refine | Calibration / FLOOR_OK |
| **P2** search winner | Production fit search | Gate tests, Run B |
| **P3+** sweep / grid / field | Varies | Research or production path |

**M5 reach:** `placement_slide > max_lag` → residual abstains; Pearson alone decides. Run B
control row can slide past the seam.

### Gap geometry (manifest / fixtures)

| Axis | Examples |
|------|----------|
| `donor_relation` | `same_master`, `two_mic`, `synthetic_decoy` |
| `encode_geometry` | `inject_then_encode`, `punch_after_encode`, `in_memory` |
| `gap_anchor` | `mid_content`, `finale_transient`, `speech`, `ambient` |
| `b_independence` | `same_bitrate`, `dual_bitrate` |

Manifest knobs: `punch_after_encode`, `gap_anchor_secs`, `bitrate_a`/`bitrate_b` — see
[manifest.toml](../floor_oracle/manifest.toml).

### Pearson vs residual (permanent split, H1)

| Signal | Measures | Broadband same-master at truth |
|--------|----------|--------------------------------|
| **Pearson** | Trimmed-border shape similarity | Often **dead zone** under `production_fit` |
| **Residual** | Raw-window cancellation | Low headroom if same-master |

### Gate composition

| Condition | `veto` | `veto_rescue` |
|-----------|--------|---------------|
| `!informative` or beyond lag reach | Abstain | Abstain |
| Pearson OK, low headroom | Pass | Pass |
| Pearson OK, high headroom | **Veto** | **Veto** |
| Pearson dead zone, low headroom | Skip | **Rescue → Marginal** |
| Pearson dead zone, high headroom | Skip | Skip |

**`rescue_trigger`** (Run B CSV): `pearson_min < min_fill` ∧ informative ∧ `headroom ≤ margin`
at the **decided** placement — not the same as “gap patched.”

### What each layer can prove

| Claim | A | B (P0) | C calibration | C `production_fit` |
|-------|---|--------|---------------|---------------------|
| Headroom / floor math | ✓ | ✓ | ✓ | ✓ |
| C1a composition (veto rule) | ✓ | ✓ (F4 at fixed frame) | — | — |
| C1b pipeline `ResidualHeadroomExceeded` | — | — | — | **not proved** (optional) |
| H2-B rescue mechanism | — | ✓ | ✓ (synthetic) | ✓ (synthetic oracle) |
| C3 truth / gate inert | — | — | ✓ (relaxed floors) | ✓ (`gate_real_codec_production_fit`) |
| C2 two-mic abstain | — | — | ✓ | ✓ |
| C5 rescue real-media value | — | — | — | resolved (Run B + punch assert) |

F4 pipeline (`f4_decoy_residual_gate_vetoes_bool`): documents **abstain** on F4 under veto — bool
and energy paths patch; **not** C1b. See **M6**.

---

## Dead-zone & rescue (H2-B / G5)

**Product question (G5):** does `veto_rescue` usefully recover a gap on **real** lossy codec noise?
**Answer:** no on current evidence — synthetic-only; see summary below.

### Epistemic map (do not conflate)

```text
synthetic H2-B mechanism ──► broadband_oracle_veto_rescue_patches_marginal (production_like_synthetic)
                             seam_residual_disagreement_oracles (P0 score)
        │
        ▼
real codec under CALIBRATION ──► floor_oracle_* gate tests (relaxed floors; NOT shipped behavior)
        │
        ▼
real codec dead zone, PRODUCTION_FIT ──► source_gap_oracle_transient_csv (Run B, diagnostic)
   inject+encode AND punch-after-encode → floor uninformative where Pearson dies
        │
        ▼
G5 RESOLVED: rescue trigger does not occur on real codec noise (synthetic-only)
   deadzone_punch_assert [landed]; finale_floor_nan_probe [optional, M3-adjacent]
```

| Step | Test / driver | Profile | What it actually shows |
|------|---------------|---------|------------------------|
| Mechanism | `broadband_oracle_veto_rescue_patches_marginal` | `production_like_synthetic` | Rescue upgrades marginal when Pearson dead (synthetic) |
| Score H2-B | `seam_residual_disagreement_oracles` | production Pearson | Dead zone + rescue at P0 truth |
| Calibration gate | `floor_oracle_residual_gate_real_codec`, `floor_oracle_veto_rescue_real_broadband_codec` | **`calibration`** | Gate doesn't break truth under relaxed floors; rescue ≡ veto there — **not** “Pearson passes at truth” in production |
| G5 probe | `source_gap_oracle_transient_csv` | **`production_fit`** | `rescue_trigger` CSV; finale dead zone |
| G5 punch | same driver, punch manifest rows | **`production_fit`** | Native A borders — confound **refuted** |
| G5 assert | `deadzone_punch_assert` | **`production_fit`** | Rescue inert on punch rows (landed) |

### Run B manifest cases

| manifest `id` | `encode_geometry` | `gap_anchor` | role |
|---------------|-------------------|--------------|------|
| `cc_music_gap_oracle_aac_128k` | inject_then_encode | mid_content | Control; search may slide past seam |
| `cc_music_transient_*` | inject_then_encode | finale_transient | Inject+encode finale |
| `cc_music_transient2_aac_dual` | inject_then_encode | ~102 s | Earlier anchor |
| `cc_music_punch_finale_aac_*` | punch_after_encode | finale_transient | **Decisive G5** — native borders; floor *still* uninformative (confound refuted) |

```text
cargo test -p clip-sync-repair --features validation-tests --test validate_floor_oracle source_gap_oracle_transient_csv -- --nocapture
```

### G5 summary (production_fit, finale) — **resolved**

1. Real seams can be in the Pearson dead zone (~0.02) — corrects “real masters pass Pearson at
   truth” (that held only under calibration).
2. Independent AAC encodes: floor **uninformative** where Pearson dies → rescue inert — under both
   inject-then-encode **and** punch-after-encode (native borders), so the NaN floor is **genuine,
   not an oracle artifact** (confound hypothesis refuted).
3. Vorbis same-bitrate flip may be M2 deterministic floor (−120 dB), not codec noise.
4. **Net:** rescue value on real codec noise is **actively unsupported** (not merely unproven) →
   `veto_rescue` is synthetic-only on current evidence, correctly non-default; veto inertness
   reassuring. Remaining sub-question: *why* the floor is NaN (cancellation failure vs probe
   abstention, M3-adjacent) — does not change the conclusion.

Details: [archive/residual-gate-findings.md § Run B](../../../../docs/dev/archive/residual-gate-findings.md).

---

## Catalog (`matrix.toml`)

[`matrix.toml`](matrix.toml) lists **implemented** instances only. When you ship a test, add a row;
when you add a planned test, put it in **Backlog** below (not in the matrix until it exists).

| Field | Purpose |
|-------|---------|
| `id` | Stable name (matches backlog ids when applicable) |
| `config_profile` | `calibration` \| `production_fit` \| `production_like_synthetic` |
| `tier` | `fast` \| `ignored` \| `diagnostic` |
| `assertion` | `hard_assert` \| `diagnostic_csv` \| `floor_calibration` |
| `location` | `tests/<file>.rs::<test_fn>` (see `matrix.toml`) |
| `fixture_ref` | Manifest case id(s) or harness fixture name |
| `proves` | One-line epistemic claim (must match actual `config_profile`) |
| `claims` | Contract clause(s): `C1a`, `C1b`, `C2`–`C5`, or `support` (see § What this suite must establish) |

**In matrix today:** layer B disagreement oracles (C1a); off regression (C4); production_fit gate
(C2/C3); G5 diagnostic + punch assert (C5); calibration gate rows; floor calibration CSV. **Not in
matrix** (by design): layer A units, layer B CSV diagnostics, `f4_decoy_residual_gate_vetoes_bool`
(F4 pipeline abstain — documents M6, not C1b), manifest smokes — see matrix footer.

---

## Implementation

Runbook for extracting shared runners and optionally re-homing layer C gate tests. Read **Config
profiles** first.

### Are we migrating everything in `matrix.toml`?

**No.** The matrix is an **inventory**, not a migration checklist.

| Matrix row | Re-home under `integration_residual_gate_smoke.rs` / `validate_*`? | Reason |
|------------|-----------------------------------------------|--------|
| `disagreement_oracles` | **No** | Layer B — stays in `seam_residual_corpus.rs` |
| `floor_calibration_csv` | **No** | Calibration home — stays in `validate_floor_oracle.rs` |
| `deadzone_finale_run_b` | **Optional** | Good candidate — uses production_fit runner |
| `gate_real_codec`, `rescue_real_mid_safety`, `vorbis_64k_no_false_veto` | **Optional** | Gate tests; share runner with Run B |
| `rescue_bb_synthetic` | **Optional** | Synthetic pipeline — could move from `seam_residual_oracle.rs` |

**Never migrate:** `seam_residual_corpus.rs`, `source_gap_oracle_floor_csv`, `policies.rs` units,
F4 decoy tuning tests in `validate_residual_gate.rs` / `diag_energy_matrix.rs`.

**Eventually:** a matrix-driven runner may *execute* rows by `id` without moving every test file —
only layer C pipeline drivers benefit from a shared harness.

### Cargo layout (important)

Integration tests are **`tests/*.rs` at the crate root**, not files under `tests/residual_gate_catalog/`.
This directory holds **docs + `matrix.toml` only** until you add:

```text
crates/clip-sync-repair-harness/src/
  floor_oracle.rs              # floor-oracle pair builder (manifest + encode)
  residual_gate.rs             # floor-oracle pipeline runner + assertions
  seam_residual.rs             # seam residual score harness
  energy_matrix.rs             # energy signature matrix row runners
tests/
  integration_floor_oracle_smoke.rs
  integration_residual_gate_smoke.rs
  validate_floor_oracle.rs
  validate_residual_gate.rs
  residual_gate_catalog/
    README.md
    matrix.toml
```

### Phase 1 — extract runner (no test moves)

Runner symbols now live in `clip-sync-repair-harness::residual_gate` (formerly
`tests/common/residual_gate_runner.rs`):

| Symbol | Role |
|--------|------|
| `FloorOracleRun` | Outcome bundle: status, skip_reason, residual, seam Pearson, confidence, align slide |
| `run_built_floor_oracle_cfg` | **Core API:** `BuiltFloorOracle` + `&RepairConfig` → `FloorOracleRun` |
| `seam_pre_post`, `gap_status_label`, `skip_reason_label` | Status helpers |
| `assert_truth_patches`, `assert_veto_rescue_matches_veto_on_truth` | Shared assertions (calibration) |

Add two explicit config builders (replace implicit `floor_oracle_repair_config` naming):

```rust
/// calibration — structure isolation, relaxed Pearson (FLOOR_OK + calibration gate tests)
pub fn calibration_repair_config(gate: ResidualGateMode) -> RepairConfig { ... }

/// production_fit — RepairConfig::default() + energy signature + gate mode (Run B, G5 asserts)
pub fn production_fit_repair_config(gate: ResidualGateMode) -> RepairConfig {
    RepairConfig {
        gap_signature_mode: GapSignatureMode::Energy,
        gap_signature_context_secs: 3.0,
        residual_gate: gate,
        ..RepairConfig::default()
    }
}
```

`validate_floor_oracle.rs` imports these; behavior unchanged.

**Runner contract:** one gap per `BuiltFloorOracle`, `measure_residual = true`, decode A to mono for
`gap_report_from_floor_oracle` (see existing `run_built_floor_oracle_cfg`).

### Phase 2 — migrate gate tests (landed)

Split binaries (see [development.md](../../../../docs/dev/development.md)):

- `integration_floor_oracle_smoke.rs` — manifest + gap_frames PR smokes
- `integration_residual_gate_smoke.rs` — `off_no_regression_baseline` (RG04 PR)
- `validate_floor_oracle.rs` — ffmpeg/corpus gate rows + calibration CSV
- `validate_residual_gate.rs` — `f1_production_scan_patch_smoke` + EC6 `f4_decoy_*`

**Leave in** `validate_floor_oracle.rs`: `source_gap_oracle_floor_csv` (calibration CSV).
PR smokes: `integration_floor_oracle_smoke.rs`.

From `seam_residual_oracle.rs` (diagnostic tier, `diagnostic-tests` feature): `broadband_oracle_veto_rescue_patches_marginal` (`#[ignore]`, ~100s).

Update `matrix.toml` `location` fields when fns move.

### Phase 3 — matrix driver (optional, later)

`validate_residual_gate.rs::residual_gate_matrix` (future) reads `tests/residual_gate_catalog/matrix.toml`,
filters by `tier` / `assertion`, runs `run_built_floor_oracle_cfg` with the profile matching
`config_profile`. **Do not build** until needed — not required for shipped contract closure.

### Verify

```text
cargo test -p clip-sync-repair --test seam_residual_corpus seam_residual_disagreement_oracles
cargo test -p clip-sync-repair --features validation-tests --test validate_floor_oracle source_gap_oracle_transient_csv -- --nocapture
cargo test -p clip-sync-repair --features validation-tests --test validate_floor_oracle floor_oracle_residual_gate_real_codec -- --nocapture
```

---

## Backlog

### Planned tests

| id | Claim | Profile | Notes |
|----|-------|---------|-------|
| `c1b_acoustic_echo_pipeline_veto` | **C1b** (optional) | `production_fit` | *Needs design.* Pipeline `ResidualHeadroomExceeded` on an **acoustic echo** fixture — Pearson passes shipped floors, informative nominal floor, headroom > margin, slide ≤ `max_lag`. **Not F4** (M6). Candidate homes: `validate_residual_gate.rs` or `patch_audio_integration.rs` (sine seam distortion). |
| `finale_floor_nan_probe` | G5 (sub) | — | *Needs design.* Explain *why* the finale floor is NaN (M3-adjacent). Primary: `policies.rs` unit repro; optional diagnostic CSV in `diag_seam_residual.rs`. |
| `p2_search_winner_bounds` | C3 (field) | `production_fit` | *Needs design.* Bound headroom on the **search winner** (not truth placement). |

**Landed** (in `matrix.toml`): `off_no_regression_baseline` (C4), `gate_real_codec_production_fit`
(C2/C3), `deadzone_punch_assert` (C5 lock).

**Removed / not planned:** `f4_decoy_veto_production_fit` — conflicts with **M6** and production
nominal-floor anchoring; F4 score veto is **C1a** only.

Planned rows graduate to [`matrix.toml`](matrix.toml) when the test lands (see **Catalog**).

### Migrate later (optional — see Implementation)

| Current location | Tests |
|------------------|-------|
| `seam_residual_oracle.rs` | `broadband_oracle_veto_rescue_patches_marginal`, oracle CSVs |
| `validate_floor_oracle.rs` | gate / veto_rescue / transient CSV |
| `validate_residual_gate.rs` | `f1_production_scan_patch_smoke`, EC6 `f4_decoy_*` |

Stay outside: `seam_residual_corpus.rs` (layer B), `source_gap_oracle_floor_csv` (calibration),
other F4 decoy tuning tests, `policies.rs` unit tests.

---

## Quick commands

```text
cargo test -p clip-sync-repair --test integration_residual_gate_smoke off_no_regression_baseline
cargo test -p clip-sync-repair --test seam_residual_corpus seam_residual_disagreement_oracles
cargo test -p clip-sync-repair --features validation-tests --test validate_floor_oracle source_gap_oracle_floor_csv -- --nocapture
cargo test -p clip-sync-repair --features validation-tests --test validate_floor_oracle source_gap_oracle_transient_csv -- --nocapture
cargo test -p clip-sync-repair --features validation-tests --test validate_floor_oracle floor_oracle_residual_gate_real_codec floor_oracle_veto_rescue_real_broadband_codec -- --nocapture
```

Or: `.\scripts\test-tier.ps1 -Tier validation -Package clip-sync-repair`
