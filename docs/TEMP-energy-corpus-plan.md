# Temporary plan: energy signature production corpus (synthetic tuning)

> **Status:** **Phases A–D landed** (2026-06-23). `ProductionScenarioSpec`, F1/F2/F3-long builders, scan helpers, **EC-1–EC-3 domain oracles**, F1-long **scan→patch e2e** (`f1_production_scan_patch_smoke` + oracle control, both `#[ignore]`), F2-long **oracle patch** (`f2_production_oracle_patch_smoke`, energy at pause₁), and ignored mode matrix (F1-long scan-derived + F2-long oracle-injected rows; context 30 on the 120 s fixture). Vocabulary wired into [gap-repair-guide.md](gap-repair-guide.md) and [corpus-validation.md](corpus-validation.md). **Open:** **EC-6** mode discrimination — F2-long matrix (2026-06-23) shows all modes patch at pause₁ (slide 0); discrimination is domain-only, patch path needs a non-zero offset or production fit weights to face the decoy (see Tuning record). Non-ignored CI smoke; Phase F/G.
>
> Archive to `docs/archive/energy-corpus-plan.md` when the production corpus ships and tuning notes are recorded. Update [TEMP-energy-signature-plan.md](TEMP-energy-signature-plan.md) Phase 3 checklist, [corpus-validation.md](corpus-validation.md) § Gap fill, and [gap-repair-guide.md](gap-repair-guide.md) as needed.

**Problem:** Energy signature shipped with **short synthetic oracles** (8 s integration fixtures, tight `fill_border_search_secs`, structure-heavy weights). Phase 3 tuning — compare `bool` / `energy` / `auto`, sweep `gap_signature_context_secs`, retune `min_structure_match_score` — was defined as operator corpus work on long-form pairs. That requires media the project may not commit (copyright). Existing **gap_corpus** chirp WAVs exercise **scan**, not energy discrimination (identical sine seams let waveform dominate).

**Goal:**

1. **F1-long / F2-long** pure-Rust fixtures @ **48 kHz**, **60–120 s**, sized for production defaults (`fill_border_search_secs = 10`, context **3 / 10 / 30**).
2. **Full pipeline path** — `ScanGaps` → `PatchAudio` on written WAVs, not only injected `GapReport`.
3. **Mode matrix runner** — record patched/skipped/marginal, slides, wall time; find bool-skip / auto-patch deltas.
4. **Optional `EnvelopeProfile`** — offline stats from PD/CC (local only) → parameters for generators; **no source audio in repo**.
5. Document tuning outcomes (context guidance, threshold retune or confirm `0.55`).

**Non-goals:**

- Committing non–PD/CC recordings or operator-specific file paths.
- Replacing U1–U8 / I1–I4 short fixtures (keep for fast CI).
- Phase 4 optimizations (FFT xcorr, adaptive context, landmarks) — only if this corpus exposes pain.
- Changing `gap_energy.rs` search math unless long-context bugs appear.

---

## Relationship to parent plan

| Parent ([TEMP-energy-signature-plan.md](TEMP-energy-signature-plan.md)) | This plan |
|-----------------------------------------------------------------------|-----------|
| Phases 0–2 shipped (energy bins, `auto`, U/I acceptance) | Phase 3 **corpus** slice |
| Manual baseline on drift-heavy pair | **Synthetic + optional profile** substitute |
| `min_structure_match_score` retune | Informed by matrix **EC-1–EC-5** |
| README / cli-output structure docs | After matrix results |

---

## Current codebase baseline

| Area | Path | Current state | Gap |
|------|------|---------------|-----|
| Short oracles | `test_support/energy_signature_fixtures.rs` | F1–F3 unit + `build_f*_integration` @ **8 s** | Too short for 10 s border + 30 s context |
| Integration tests | `tests/patch_audio_integration.rs` | I1–I4 via `energy_sig_patch_options` (3.5 s border, 0.5 s context, `waveform_weight = 0`) | Not production defaults |
| Gap injection | `gap_report_from_energy_fixture` in `test_support/energy_signature_production.rs` | Shared oracle path for I1-style reports | Scan→patch e2e still open |
| Structure params | F1 integration | `search_radius_frames = gap_frames * 2` | Not tied to `fill_border_search_secs` |
| Silence threshold | Fixtures | `absolute_silence_rms: 0.0` in structure params | Production scan uses **33.0** |
| F2 seam | F2 integration | A post-rise vs B hard cut | Needs `fill_absolute_floor = -0.05` in I3; production is **0.12** |
| Gap scan corpus | `tests/gap_corpus/` | Chirp + zeroed gaps | No energy decoy geometry |
| Profiling | — | None | No `EnvelopeProfile` → generator bridge |

### Fixture timeline today

```text
INTEGRATION_TOTAL_SECS = 8.0
  anchor ≈ 30% of file
  context ≈ 0.25 × total (integration) or 50 frames (unit)
  search_radius ≈ gap_frames × 2 (F1 integration)
```

### Production layout required

```text
lead-in ≥ gap_signature_context_secs + fill_border_search_secs + fill_align_margin_secs
gap     ≥ max(min_gap_ms, 2 × gap_signature_bin_ms)
decoy   within fill_border_search_secs of nominal (F1 shift, F2 pause spacing)
```

| Scenario | `total_secs` | Max context tested |
|----------|--------------|-------------------|
| F1-long standard | **60** | 3, 10 |
| F1-long / F2-long stress | **120** | 30 |
| F2-long (two pauses) | **90** | **3 only** in matrix (pause spacing scales with context; pause₂→pause₁ slide must stay inside `fill_border_search_secs = 10`, so context 10/30 overruns the border) |
| CI smoke | **60** | 3 only |

---

## Decisions

| Topic | Decision |
|-------|----------|
| **License** | All committed fixtures **pure-Rust PCM** or profile JSON (stats only). PD/CC audio used **locally** for profiling only. |
| **Sample rate** | **48 kHz** for production corpus; keep 11.025 kHz unit fixtures unchanged. |
| **Channels** | Mono + one **stereo** smoke case (optional). |
| **Scan** | Gap regions **digital zero** (or below `absolute_silence_rms = 33`); duration ≥ **1000 ms**; block-aligned (~250 ms). |
| **Patch config** | `production_repair_config(mode, context_secs)` in `energy_signature_production.rs` (plan name `production_sig_patch_options`); mirrors production defaults (`border = 10`, weights 0.35/0.65, `min_structure_match_score = 0.55`). |
| **Structure isolation** | Domain oracles may use structure-heavy weights; **production matrix** uses default unified weights. |
| **F2 post-seam** | **Done:** pause₂ placed outside pause₁ post context; B cloned from A with pause₂ silence only; production uses multi-bin post rise + zero fill slack. |
| **Matrix** | Modes: `bool`, `energy`, `auto`. Contexts: `3`, `10`, `30` (skip invalid combos per file length). |
| **CI** | One committed smoke: F1-long 60 s, `auto`, context 3, `baseline_only`. Full matrix **`--ignored`**. |
| **Profile format** | JSON under `tests/energy_corpus/profiles/` (committed example only). |

---

## Vocabulary and matrix recording

**Tag definitions:** [gap-repair-guide.md](gap-repair-guide.md) § Vocabulary (`plan_kind`, `patch_tier`, `seam_shape`, `signature_mode`, `patch_skip_reason`, …).

**Naming:** Guide **P0–P7** = plan-time gap types. This plan uses **EC-* (energy corpus)** for acceptance IDs — do not confuse with guide P5 “fillable”.

**Two layers per matrix row:**

1. **Fixture oracle** — `fixture_scenario`, domain outcome (truth frame / slide), `gap_report_source`.
2. **Run tags** — from `-v` `gap tags:` or JSON when `PatchAudio` runs with production defaults.

Example row (see [corpus-validation.md](corpus-validation.md) for full format):

```text
F1-long,auto,3,scan_derived,0,1,0,8420,"plan=fillable tier=structure_fail sig=energy","EC-1 domain OK; patch haystack fail"
```

Lib test names (`p1_f1_production_…`, `p2_f2_…`) keep historical prefixes; docs use **EC-1**, **EC-2**, etc.

---

## Phases

### Phase A — Parameterize geometry

**Intent:** Refactor builders; keep 8 s integration tests green.

- [x] Add `ProductionScenarioSpec` (`total_secs`, `gap_signature_context_secs`, `fill_border_search_secs`, `gap_signature_bin_ms`, margins, `min_gap_secs`). `sample_rate` / `channels` remain builder args.
- [x] Add `gap_anchor_secs(spec) -> f64` (lead-in from context + border + margin).
- [x] Derive `StructureMatchParams` from spec:
  - `bin_frames = round(bin_ms × rate)`
  - `search_radius_frames = round(fill_border_search_secs × rate)`
  - `gap_frames ≥ max(min_gap, 2 × bin_frames)`
  - `context_frames = round(context_secs × rate)`
- [x] Refactor `build_f1_integration` / `build_f2_integration` → wrappers on `ProductionScenarioSpec::integration_fast()` (8 s, current behavior).
- [x] Unit test: anchor helper places F1 decoy inside search radius for 60 s / border 10.

### Phase B — F1-long / F2-long builders

**Intent:** Production-scale WAV geometry (F1 decoy, F2 dual-pause).

- [x] `build_f1_production` / `build_f1_production_at(total_secs, …)` — reuse `RampGapFillSpec` / `fill_ramp_gap`; refine guards.
- [x] `build_f2_production(spec) -> EnergySignatureFixture` — pause spacing `≤ 2 × border`; guards at pause edges.
- [x] `build_f3_drone_production(spec)` for `auto` → bool (flat envelope, non-zero level).
- [x] `write_fixture_wavs` unchanged; optional output under `target/energy_corpus/` for manual CLI not implemented.
- [ ] Optional stereo F1-long smoke.

### Phase C — Scan-and-patch path

**Intent:** Exercise real `ScanGaps` + refine, not only injected `GapReport`.

- [x] After `write_fixture_wavs`, run `ScanGaps` with production scan defaults (`min_gap_ms = 1000`, `absolute_silence_rms = 33`, `scan_block_ms = 250`).
- [x] Assert detected gap count and boundaries within tolerance (±0.35 s scan smoke; tighter ± block TBD).
- [x] Build B reference: aligned copy for F1/F2 (same as integration).
- [x] `PatchAudio` on **scan-derived** gaps with production defaults (e2e acceptance) — `f1_production_scan_patch_smoke` (`#[ignore]`); oracle control via `f1_production_oracle_patch_control` / `gap_report_from_energy_fixture`.

### Phase D — Mode matrix runner

**Intent:** Automated Phase 3 comparison without external media.

- [x] `tests/energy_signature_production.rs` (`energy_signature_mode_matrix`, `#[ignore]`).
- [x] Ignored test loops: fixtures × modes × contexts; logs CSV-friendly rows with `slide_secs` + `skip_reason`. F1-long via `run_matrix_rows` (scan-derived); F2-long via `run_oracle_matrix_rows` (oracle-injected — real scan can't detect F2's pause₁ gap since B is silent there).
- [x] Skip invalid context combos (`production_matrix_contexts`: context 30 only on ≥ ~83 s fixtures; 120 s block for EC-5 prep). F2-long pinned to context 3 (pause₂→pause₁ slide must stay inside `fill_border_search_secs = 10`).
- [ ] Optional: subprocess `clip-sync-repair -v` on written WAVs for verbose `signature_mode=` / struct lines.
- [ ] **CI smoke:** one committed non-ignored patch case (fast budget).

### Phase E — Acceptance criteria (production synthetic)

Corpus IDs **EC-1–EC-6** (lib tests: `p1_` / `p2_` / `p3_` where implemented).

| ID | Fixture | Config | Assertion | Status |
|----|---------|--------|-----------|--------|
| **EC-1** | F1-long 60 s | `energy`, context 3, structure-heavy domain weights | **Domain:** unified `start_frame` within ±1 bin of truth (`p1_`). **Patch:** `f1_production_scan_patch_smoke` + oracle control patch with `production_repair_config` | Domain ✅ / Patch ✅ |
| **EC-2** | F2-long | same | **Domain:** energy at pause₁ (`p2_`). **Patch:** `f2_production_oracle_patch_smoke` — energy patches at pause₁ (slide ≈ 0 from A-aligned nominal, not decoy pause₂) | Domain ✅ / Patch ✅ |
| **EC-3** | F3-long drone | `auto` | Resolved `signature_mode=bool` (`p3_`) | ✅ |
| **EC-4** | F1-long | `auto`, context 3 | No regression vs I5-style suite defaults — F1-long auto patches in 2026-06-23 matrix | ✅ |
| **EC-5** | F1-long 120 s | context **30** | Completes within wall budget; no hot-path failure — 120 s fixture patches at context 30 (~43 s, 2026-06-23) | ✅ |
| **EC-6** | Matrix | all | Record sheet: cases where `auto`/`energy` patches and `bool` skips (or reverse); include vocabulary tags | **Not met** — F2-long matrix (2026-06-23 b) shows all modes patch at pause₁ (slide 0). Discrimination lives in the domain oracle, not the patch path; needs non-zero offset or production fit weights (see Tuning record) |

### Phase F — Profile → synthesize (optional)

**Intent:** Loosely match real pause/level stats without committing audio.

- [ ] `EnvelopeProfile` struct: `pause_duration_secs`, `pre_gap_ramp_secs`, `post_gap_level_db`, `inter_pause_secs`, `envelope_flat`, `suggested_ramp_step`.
- [ ] Committed example: `tests/energy_corpus/profiles/synthetic_example.json` (hand-authored stats, `"source": "synthetic"`).
- [ ] `build_from_profile(profile, scenario: F1|F2|F3) -> EnergySignatureFixture`.
- [ ] Doc + script: `scripts/profile_envelope.ps1` (ffmpeg `silencedetect` / `astats` on **local** PD/CC file → operator fills JSON template).
- [ ] One acceptance test: example profile produces finite scores and EC-1-like discrimination when mapped to F1-long geometry.

### Phase G — Tuning outcomes & docs

**Intent:** Close Phase 3 from parent plan.

- [x] Run matrix (ignored); record outcomes in this doc § [Tuning record](#tuning-record) (2026-06-22: all-skip; 2026-06-23: all patch after corpus config fix).
- [ ] Confirm or retune `min_structure_match_score` (default 0.55) — corpus matrix uses **0.0** (structure-isolated, like I1); operator defaults unchanged.
- [ ] Document recommended `gap_signature_context_secs` (3 vs 10 vs 30) in [gap-repair-guide.md](gap-repair-guide.md) / [gap-fill-modes.md](gap-fill-modes.md).
- [x] Wire vocabulary into [gap-repair-guide.md](gap-repair-guide.md) and [corpus-validation.md](corpus-validation.md) (fixture table, matrix row format, EC-* naming).
- [ ] Cross-link from [TEMP-energy-signature-plan.md](TEMP-energy-signature-plan.md); archive parent Phase 3 when done.

---

## Code change checklist

| File | Changes |
|------|---------|
| `test_support/energy_signature_fixtures.rs` | `ProductionScenarioSpec`, anchor math, `build_f1/f2/f3_production`, refactor integration wrappers |
| `test_support/energy_signature_acceptance.rs` | Optional U9–U11 domain oracles for long fixtures |
| `tests/patch_audio_integration.rs` | I1–I4 use shared `gap_report_from_energy_fixture`; production patch smoke landed in `tests/energy_signature_production.rs` (`f1_production_scan_patch_smoke`, `f1_production_oracle_patch_control`, `f2_production_oracle_patch_smoke`) |
| `test_support/patch_geometry_preview.rs` | Haystack oracle accepts production spec |
| `tests/energy_corpus/` (new) | `profiles/*.json`, optional `manifest.toml` wall budgets |
| `scripts/profile_envelope.ps1` | Phase F optional |
| `docs/corpus-validation.md` | § Energy signature production corpus + matrix row format |

**No change** expected to `gap_energy.rs`, `gap_signature.rs`, or search unless P5 finds bugs.

---

## What stays unchanged

- **U1–U8** unit/lib acceptance on short F1–F3 (11.025 Hz / small frames).
- **I1–I4** + `energy_sig_patch_options` (fast, structure-isolated integration).
- **gap_corpus** chirp fixtures (scan detection; not energy-discriminative).

---

## Mode matrix (operator / ignored CI)

Fixed other knobs: `fill_mode = fit`, `fill_border_search_secs = 10`, `repair profile = default` (`baseline_only`), corpus structure isolation via `production_repair_config` (see tuning record), `dry_run = true` for timing unless listen pass.

| Run | `gap_signature_mode` | `gap_signature_context_secs` |
|-----|------------------------|------------------------------|
| 1–3 | `bool`, `energy`, `auto` | 3 |
| 4–6 | same | 10 |
| 7–9 | same | 30 (requires 120 s fixture) |

**Metrics per run:** `patched_count`, `skipped_count`, `patched_marginal_count`, wall time, per-gap verbose (`signature_mode=`, struct pre/post, slide, skip reason), **vocabulary tags** ([gap-repair-guide.md](gap-repair-guide.md) § Vocabulary).

**Minimal pass (3 runs):** `bool` / `energy` / `auto` @ context 3 on F1-long 60 s only.

---

## Tuning record

**Matrix run 2026-06-22** — F1-long 60 s, `scan_derived`, production defaults (`fill_border_search_secs = 10`, `baseline_only`, default weights). Total wall **~972 s** (~108 s/run avg).

| Date | Fixture | Mode | Context | Source | Patched | Skipped | Marginal | Wall s | Tags | Notes |
|------|---------|------|---------|--------|---------|---------|----------|--------|------|-------|
| 2026-06-22 | F1-long | bool | 3 | scan_derived | 0 | 1 | 0 | 112 | `plan=fillable tier=structure_fail seam=n/a` | EC-1 domain OK; patch haystack fail |
| 2026-06-22 | F1-long | bool | 10 | scan_derived | 0 | 1 | 0 | 114 | same | context change no effect |
| 2026-06-22 | F1-long | bool | 30 | scan_derived | — | — | — | — | — | **Invalid on 60 s** — matrix now skips (use 120 s fixture) |
| 2026-06-22 | F1-long | energy | 3 | scan_derived | 0 | 1 | 0 | 104 | same | energy domain finds truth on full B |
| 2026-06-22 | F1-long | energy | 10 | scan_derived | 0 | 1 | 0 | 100 | same | |
| 2026-06-22 | F1-long | energy | 30 | scan_derived | — | — | — | — | — | **Invalid on 60 s** — matrix now skips |
| 2026-06-22 | F1-long | auto | 3 | scan_derived | 0 | 1 | 0 | 99 | same | |
| 2026-06-22 | F1-long | auto | 10 | scan_derived | 0 | 1 | 0 | 99 | same | |
| 2026-06-22 | F1-long | auto | 30 | scan_derived | — | — | — | — | — | **Invalid on 60 s** — matrix now skips |

**Findings:**

- **No mode/context delta on patch** — bool, energy, and auto all skip; context 3 / 10 / 30 does not change outcome. Signature mode affects placement on full B (EC-1 domain passes) but not scan→patch on this fixture yet.
- **Likely skip:** structure gate `correlation_below_threshold` with `pre=0 post=0` (structure search on sliced B haystack fails before waveform tier). Re-run with updated matrix test to confirm exact `min=` in skip column.
- **EC-6 goal not met** — no case where `auto`/`energy` patches and `bool` skips (all skip equally).
- **Performance:** ~100 s/patch on 60 s mono F1-long @ 48 kHz stereo with 10 s border — budget accordingly for F2-long / 120 s matrix expansion.

**Threshold decision:** `min_structure_match_score` = **unchanged (0.55)** — matrix did not reach waveform tier; retune deferred until haystack placement works.

**Context guidance:** default **3 s** retained; raising to 10 / 30 s showed **no patch benefit** on F1-long scan path (domain-only discrimination unchanged in prior EC-1 runs).

---

**Matrix run 2026-06-23** — after scan→patch fixes (`inject_oracle_alignment`, `fill_fit_nominal_bias_scale` wired through `RepairConfig`, corpus `production_repair_config` with structure isolation matching I1). Total wall **~695 s** (~43 s/run avg; 16 rows including oracle controls).

| Date | Fixture | Mode | Context | Source | Patched | Skipped | Marginal | Wall s | Notes |
|------|---------|------|---------|--------|---------|---------|----------|--------|-------|
| 2026-06-23 | F1-long | Energy | 3 | oracle_injected | 1 | 0 | 0 | 44 | control row |
| 2026-06-23 | F1-long | Bool | 3 | scan_derived | 1 | 0 | 0 | 43 | slide 0.5 s |
| 2026-06-23 | F1-long | Bool | 10 | scan_derived | 1 | 0 | 0 | 37 | |
| 2026-06-23 | F1-long | Energy | 3 | scan_derived | 1 | 0 | 0 | 39 | |
| 2026-06-23 | F1-long | Energy | 10 | scan_derived | 1 | 0 | 0 | 38 | |
| 2026-06-23 | F1-long | Auto | 3 | scan_derived | 1 | 0 | 0 | 38 | |
| 2026-06-23 | F1-long | Auto | 10 | scan_derived | 1 | 0 | 0 | 38 | |
| 2026-06-23 | F1-long-120s | Energy | 30 | oracle_injected | 1 | 0 | 0 | 43 | control row |
| 2026-06-23 | F1-long-120s | Bool/Energy/Auto | 3/10/30 | scan_derived | 1 each | 0 | 0 | 39–42 | 9 rows, all `patched(High)` |

**Findings (2026-06-23):**

- **Root cause of 2026-06-22 all-skip:** not haystack geometry (scan and oracle previews both had `true_within_search_radius=true`). Blockers were (1) `patch_settings()` hardcoded `fill_fit_nominal_bias_scale = 1.0`, anchoring search to F1 decoy nominal; (2) production waveform gate (`min_fill_correlation = 0.35`) rejecting synthetic seams with ~0 Pearson at the chosen site.
- **Corpus config:** `production_repair_config` uses production geometry (`fill_border_search_secs = 10`, context from matrix) plus I1-style structure isolation (`structure_weight = 1`, `waveform_weight = 0`, `nominal_bias = 0`, `min_fill_correlation = 0`, `fill_absolute_floor = -0.05`). Operator-facing defaults unchanged.
- **Scan path:** `scan_gaps_for_fixture` now injects I1-style oracle alignment (start + end clips) after scan; gap times remain scan-derived.
- **EC-6 not met on F1-long:** bool, energy, and auto all patch — mode discrimination requires F2-long or restoring production fit weights for the matrix.
- **Performance:** ~40 s/patch after config fix (down from ~100 s when all-skip at structure gate).

---

**Matrix run 2026-06-23 (b)** — F2-long wired into the matrix (`run_oracle_matrix_rows`, oracle-injected, context 3, all three modes). Full matrix re-run: 17 F1 rows + 3 F2-long rows, all `patched(High)`. F2-long at ~11 s/run (90 s mono+stereo, shorter B slide than F1's 0.5 s).

| Date | Fixture | Mode | Context | Source | Patched | Skipped | Marginal | Wall s | Slide s | Notes |
|------|---------|------|---------|--------|---------|---------|----------|--------|---------|-------|
| 2026-06-23 | F2-long | Bool | 3 | oracle_injected | 1 | 0 | 0 | 11 | 0.000 | placed at pause₁ |
| 2026-06-23 | F2-long | Energy | 3 | oracle_injected | 1 | 0 | 0 | 11 | 0.000 | placed at pause₁ |
| 2026-06-23 | F2-long | Auto | 3 | oracle_injected | 1 | 0 | 0 | 11 | 0.000 | placed at pause₁ |

**Findings (F2-long):**

- **EC-6 still not met — now on F2-long too.** All three modes patch at **slide 0.000** (pause₁), `High`. Bool is **not** pulled to the decoy pause₂ in the patch path.
- **Why the domain/patch split:** the domain oracle (`p2_`) centers its search on the decoy nominal (`nominal_fill_start = pause₂`) and energy must slide back, so bool stays ambiguous there. The **patch path** centers the B haystack on the **A-aligned** nominal (A gap time + zero offset = pause₁), so the decoy never enters the search window — `align_adjustment_secs` is measured from pause₁ and reads 0 for every mode. Bool additionally `snap_fill_to_gap`s to the gap edge. The injected `GapReport.video_b_start_secs = pause₂` does **not** re-center the haystack.
- **To make EC-6 reproduce at the patch layer**, the patch must *face* the decoy. Options: (1) inject a non-zero alignment offset that maps A's pause₁ onto B's pause₂ (haystack then centers on pause₂; energy slides back, bool ambiguous); (2) run the matrix with **production fit weights** (`structure 0.35 / waveform 0.65`) so the waveform tier — not `snap_fill_to_gap` — drives placement. Either is a follow-up, not done here.
- **`slide_secs` column** added to the matrix CSV so this kind of all-modes-agree result is visible at a glance rather than hidden behind equal patched counts.

---

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| 60 s still too short for context 30 | Use 120 s fixture for context 30 only (EC-5) |
| Scan doesn't detect synthetic gap | Zero gap ≥ 1 s; silence lead; assert in Phase C before patch |
| F2-long fails at `fill_absolute_floor = 0.12` | Phase B post-seam alignment — **fixed** for domain EC-2; oracle patch (`f2_production_oracle_patch_smoke`) lands at corpus floor `-0.05`. Production floor `0.12` still pending post-seam alignment (parent plan Phase 3 follow-up). |
| Matrix all-skip on F1-long scan path | **Fixed 2026-06-23** — wire nominal bias through config; corpus structure isolation; oracle alignment after scan |
| Matrix runtime (~100 s/run × 9 ≈ 16 min) | Ignored by default; CI smoke only one case; add `skip_reason` column logged |
| Profile drift from real speech | Profile is optional; synthetic F1/F2 remain canonical oracles |

---

## Related reading

- [TEMP-energy-signature-plan.md](TEMP-energy-signature-plan.md) — parent feature, U/I acceptance, Phase 4 deferrals
- [gap-repair-guide.md](gap-repair-guide.md) — operational signature mode guidance; **§ Vocabulary**
- [gap-fill-modes.md](gap-fill-modes.md) — config defaults, performance
- [corpus-validation.md](corpus-validation.md) — corpus tiers; **§ Energy signature production corpus**
- [tests/gap_corpus/README.md](../crates/clip-sync-repair/tests/gap_corpus/README.md) — scan corpus (orthogonal)
- `test_support/energy_signature_fixtures.rs` — F1–F3 builders, `write_fixture_wavs`

---

## Open questions

1. **Stereo F1-long in CI?** Adds size; catch channel-downmix issues vs mono-only.
2. **Subprocess CLI vs in-process `PatchAudio`?** In-process faster; CLI validates real operator path.
3. **Commit 60 s WAVs?** Prefer generate-at-test-time (like gap_corpus generated tier) to keep repo small.
4. **Include `--full` profile row?** Second matrix dimension if boundary grid changes mode outcomes.
