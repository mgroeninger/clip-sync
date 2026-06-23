# Temporary plan: energy signature production corpus (synthetic tuning)

> **Status:** **Phases A–D in progress** (2026-06-22). `ProductionScenarioSpec`, F1/F2/F3-long builders, scan helpers, **EC-1–EC-3** acceptance, and ignored mode matrix landed. Vocabulary wired into [gap-repair-guide.md](gap-repair-guide.md) § Vocabulary and [corpus-validation.md](corpus-validation.md) § Energy signature. F1-long end-to-end patch on scan-derived gaps remains open (domain + scan smoke pass; use I1-style oracle `GapReport` for patch until refine/scan alignment is tightened).
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
| Gap injection | `gap_report_from_energy_fixture` | Skips real scan | Need scan-and-patch path |
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
| F2-long (two pauses) | **90** | 3, 10, 30 (e.g. pauses ~25 s and ~45 s) |
| CI smoke | **60** | 3 only |

---

## Decisions

| Topic | Decision |
|-------|----------|
| **License** | All committed fixtures **pure-Rust PCM** or profile JSON (stats only). PD/CC audio used **locally** for profiling only. |
| **Sample rate** | **48 kHz** for production corpus; keep 11.025 kHz unit fixtures unchanged. |
| **Channels** | Mono + one **stereo** smoke case (optional). |
| **Scan** | Gap regions **digital zero** (or below `absolute_silence_rms = 33`); duration ≥ **1000 ms**; block-aligned (~250 ms). |
| **Patch config** | New `production_sig_patch_options(mode, context_secs)` mirroring `RepairConfig` defaults (`border = 10`, weights 0.35/0.65, `min_structure_match_score = 0.55`). |
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

- [ ] Add `ProductionScenarioSpec` (`total_secs`, `sample_rate`, `channels`, `gap_signature_context_secs`, `fill_border_search_secs`, `gap_signature_bin_ms`, margins, `min_gap_secs`).
- [ ] Add `gap_anchor_secs(spec) -> f64` (lead-in from context + border + margin).
- [ ] Derive `StructureMatchParams` from spec:
  - `bin_frames = round(bin_ms × rate)`
  - `search_radius_frames = round(fill_border_search_secs × rate)`
  - `gap_frames ≥ max(min_gap, 2 × bin_frames)`
  - `context_frames = round(context_secs × rate)`
- [ ] Refactor `build_f1_integration` / `build_f2_integration` → wrappers on `ProductionScenarioSpec::integration_fast()` (8 s, current behavior).
- [ ] Unit test: anchor helper places F1 decoy inside search radius for 60 s / border 10.

### Phase B — F1-long / F2-long builders

**Intent:** Production-scale WAV geometry (F1 decoy, F2 dual-pause).

- [ ] `build_f1_production(spec) -> EnergySignatureFixture` — reuse `RampGapFillSpec` / `fill_ramp_gap`; keep refine guards (`silence_start - 1`, B guard frame).
- [ ] `build_f2_production(spec) -> EnergySignatureFixture` — pause spacing `≤ 2 × border`; guards at pause edges.
- [ ] `build_f3_drone_production(spec)` for `auto` → bool (flat envelope, non-zero level).
- [ ] `write_fixture_wavs` unchanged; optional output under `target/energy_corpus/` for manual CLI.

### Phase C — Scan-and-patch path

**Intent:** Exercise real `ScanGaps` + refine, not only injected `GapReport`.

- [ ] After `write_fixture_wavs`, run `ScanGaps` with production scan defaults (`min_gap_ms = 1000`, `absolute_silence_rms = 33`, `scan_block_ms = 250`).
- [ ] Assert detected gap count and boundaries within tolerance (± block or ±1 bin).
- [ ] Build B reference: aligned copy for F1/F2 (same as integration) or `write_clean_*` only where geometry allows.
- [ ] `PatchAudio` with `production_sig_patch_options(mode, context_secs)`.

### Phase D — Mode matrix runner

**Intent:** Automated Phase 3 comparison without external media.

- [ ] New module: `test_support/energy_signature_production_corpus.rs` or `tests/energy_signature_matrix.rs`.
- [ ] Ignored test loops: fixtures × modes × contexts; logs CSV-friendly rows (fixture, mode, context, patched, skipped, marginal, wall_ms, notes).
- [ ] Optional: subprocess `clip-sync-repair -v` on written WAVs for verbose `signature_mode=` / struct lines.
- [ ] **CI smoke:** one case committed (fast budget, e.g. 90 s wall in manifest).

### Phase E — Acceptance criteria (production synthetic)

Corpus IDs **EC-1–EC-6** (lib tests: `p1_` / `p2_` / `p3_` where implemented).

| ID | Fixture | Config | Assertion |
|----|---------|--------|-----------|
| **EC-1** | F1-long 60 s | `energy` or `auto`, context 3, production patch opts | Unified/patch `start_frame` within ±1 bin of truth; `bool` at decoy or strictly farther |
| **EC-2** | F2-long | same | Slide ≈ 0 at pause₁ (aligned A/B), not pause₂ nominal |
| **EC-3** | F3-long drone | `auto` | Resolved `signature_mode=bool` |
| **EC-4** | F1-long | `auto`, context 3 | No regression vs I5-style suite defaults |
| **EC-5** | F1-long 120 s | context **30** | Completes within wall budget; no hot-path failure |
| **EC-6** | Matrix | all | Record sheet: cases where `auto`/`energy` patches and `bool` skips (or reverse); include vocabulary tags |

### Phase F — Profile → synthesize (optional)

**Intent:** Loosely match real pause/level stats without committing audio.

- [ ] `EnvelopeProfile` struct: `pause_duration_secs`, `pre_gap_ramp_secs`, `post_gap_level_db`, `inter_pause_secs`, `envelope_flat`, `suggested_ramp_step`.
- [ ] Committed example: `tests/energy_corpus/profiles/synthetic_example.json` (hand-authored stats, `"source": "synthetic"`).
- [ ] `build_from_profile(profile, scenario: F1|F2|F3) -> EnergySignatureFixture`.
- [ ] Doc + script: `scripts/profile_envelope.ps1` (ffmpeg `silencedetect` / `astats` on **local** PD/CC file → operator fills JSON template).
- [ ] One acceptance test: example profile produces finite scores and EC-1-like discrimination when mapped to F1-long geometry.

### Phase G — Tuning outcomes & docs

**Intent:** Close Phase 3 from parent plan.

- [ ] Run matrix (ignored); record outcomes in this doc § [Tuning record](#tuning-record) or `corpus-validation.md`.
- [ ] Confirm or retune `min_structure_match_score` (default 0.55).
- [ ] Document recommended `gap_signature_context_secs` (3 vs 10 vs 30) in [gap-repair-guide.md](gap-repair-guide.md) / [gap-fill-modes.md](gap-fill-modes.md).
- [x] Wire vocabulary into [gap-repair-guide.md](gap-repair-guide.md) and [corpus-validation.md](corpus-validation.md) (fixture table, matrix row format, EC-* naming).
- [ ] Cross-link from [TEMP-energy-signature-plan.md](TEMP-energy-signature-plan.md); archive parent Phase 3 when done.

---

## Code change checklist

| File | Changes |
|------|---------|
| `test_support/energy_signature_fixtures.rs` | `ProductionScenarioSpec`, anchor math, `build_f1/f2/f3_production`, refactor integration wrappers |
| `test_support/energy_signature_acceptance.rs` | Optional U9–U11 domain oracles for long fixtures |
| `tests/patch_audio_integration.rs` | `production_sig_patch_options`, P1–P4 smoke, delegate to matrix module |
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

Fixed other knobs: `fill_mode = fit`, `fill_border_search_secs = 10`, `repair profile = default` (`baseline_only`), `dry_run = true` for timing unless listen pass.

| Run | `gap_signature_mode` | `gap_signature_context_secs` |
|-----|------------------------|------------------------------|
| 1–3 | `bool`, `energy`, `auto` | 3 |
| 4–6 | same | 10 |
| 7–9 | same | 30 (requires 120 s fixture) |

**Metrics per run:** `patched_count`, `skipped_count`, `patched_marginal_count`, wall time, per-gap verbose (`signature_mode=`, struct pre/post, slide, skip reason), **vocabulary tags** ([gap-repair-guide.md](gap-repair-guide.md) § Vocabulary).

**Minimal pass (3 runs):** `bool` / `energy` / `auto` @ context 3 on F1-long 60 s only.

---

## Tuning record

*(Fill after Phase D/G matrix run.)*

| Date | Fixture | Mode | Context | Source | Patched | Skipped | Marginal | Wall s | Tags | Notes |
|------|---------|------|---------|--------|---------|---------|----------|--------|------|-------|
| | | | | | | | | | | |

`Source` = `scan_derived` | `oracle_injected`. `Tags` = `-v` `gap tags:` line or composed equivalents.

**Threshold decision:** `min_structure_match_score` = ___ (was 0.55).

**Context guidance:** default 3 s; raise to ___ s when ___.

---

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| 60 s still too short for context 30 | Use 120 s fixture for context 30 only (EC-5) |
| Scan doesn't detect synthetic gap | Zero gap ≥ 1 s; silence lead; assert in Phase C before patch |
| F2-long fails at `fill_absolute_floor = 0.12` | Phase B post-seam alignment; or split structure-only vs full-production tests |
| Matrix runtime | Ignored by default; CI smoke only one case |
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
