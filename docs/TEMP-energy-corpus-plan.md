# Temporary plan: energy signature production corpus (synthetic tuning)

> **Status:** **Phases A–D landed** (2026-06-23). `ProductionScenarioSpec`, F1/F2/F3-long builders, scan helpers, **EC-1–EC-3 domain oracles**, F1-long **scan→patch e2e** (`f1_production_scan_patch_smoke` + oracle control, both `#[ignore]`), F2-long **oracle patch** (`f2_production_oracle_patch_smoke`, energy at pause₁), and ignored mode matrix (F1-long scan-derived + F2-long oracle-injected rows; context 30 on the 120 s fixture). Vocabulary wired into [gap-repair-guide.md](gap-repair-guide.md) and [corpus-validation.md](corpus-validation.md). **EC-6 met (2026-06-23 d)** via the new `build_f4_decoy_production` fixture: under structure isolation `energy`/`auto` patch the true pause (slide +7 s) while `bool` stays at the decoy (slide 0) — `f4_decoy_patch_discrimination` + `p4_*`. (Production fit weights mask the split — but the shipped **mode-coupled `fill_fit_energy_nominal_bias_scale`**, default 0.25, un-masks it: energy auto-corrects a drifted nominal map — see Tuning record 2026-06-23 f.) Weight tuning resolved (lever is nominal bias, not the weight split / `min_structure_match_score`). Non-ignored CI smoke landed (`corpus_scan_patch_smoke`). **Open:** Phase F (optional profile→synthesize); Phase G archival + cross-links.
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
| **F2 post-seam** | **Done (domain only):** pause₂ placed outside pause₁ post context; B cloned from A with pause₂ silence only; production uses multi-bin post rise + zero fill slack. **Limitation (2026-06-23):** `b = a.clone()` makes pause₁ uniquely waveform-identifiable, so F2 cannot separate `bool` from `energy` at the patch layer — see [Phase E.1](#phase-e1--ec-6-decoy-fixture-redesign-next). |
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
- [x] **CI smoke:** one committed non-ignored patch case (fast budget) — `corpus_scan_patch_smoke` (in `tests/energy_signature_production.rs`, the integration binary, so it does not contend with the lib-suite wall-clock timing tests). Full scan→patch on a 16 kHz / 32 s production-geometry F1 (border 10 s, 50 ms bins); asserts one gap detected and patched. ~5 s, runs on every PR. Guards the e2e path the `#[ignore]`d 48 kHz corpus tests cover only on demand.

### Phase E — Acceptance criteria (production synthetic)

Corpus IDs **EC-1–EC-6** (lib tests: `p1_` / `p2_` / `p3_` where implemented).

| ID | Fixture | Config | Assertion | Status |
|----|---------|--------|-----------|--------|
| **EC-1** | F1-long 60 s | `energy`, context 3, structure-heavy domain weights | **Domain:** unified `start_frame` within ±1 bin of truth (`p1_`). **Patch:** `f1_production_scan_patch_smoke` + oracle control patch with `production_repair_config` | Domain ✅ / Patch ✅ |
| **EC-2** | F2-long | same | **Domain:** energy at pause₁ (`p2_`). **Patch:** `f2_production_oracle_patch_smoke` — energy patches at pause₁ (slide ≈ 0 from A-aligned nominal, not decoy pause₂) | Domain ✅ / Patch ✅ |
| **EC-3** | F3-long drone | `auto` | Resolved `signature_mode=bool` (`p3_`) | ✅ |
| **EC-4** | F1-long | `auto`, context 3 | No regression vs I5-style suite defaults — F1-long auto patches in 2026-06-23 matrix | ✅ |
| **EC-5** | F1-long 120 s | context **30** | Completes within wall budget; no hot-path failure — 120 s fixture patches at context 30 (~43 s, 2026-06-23) | ✅ |
| **EC-6** | F4-decoy | structure-isolated | `energy`/`auto` patch at the true pause (slide +7 s); `bool` stays at the decoy (slide 0) | **Met (2026-06-23 d)** — `f4_decoy_patch_discrimination` (patch) + `p4_*` (domain). Note: production fit weights mask the split (all modes → decoy); see Tuning record + [Phase E.1](#phase-e1--ec-6-decoy-fixture-redesign-next) |

### Phase E.1 — EC-6 decoy fixture redesign (next)

**Intent:** Build the one fixture that actually separates `bool` from `energy` **at the patch layer**. The 2026-06-23 b/c runs proved config can't do it — the gap is fixture geometry.

**Why the current F2 fails (learned):**

| Fact | Consequence |
|------|-------------|
| `b = a.clone()` then zero pause₂ | B's content around **pause₁** matches A exactly → waveform tier alone uniquely identifies pause₁ |
| Decoy pause₂ has **no matching B content** | Never a real competitor once any waveform/energy signal is present |
| Patch haystack centers on **A-aligned** nominal (gap time + zero offset = pause₁) | Decoy pause₂ is off-center; `align_adjustment_secs` reads 0 for every mode |
| Bool runs `snap_fill_to_gap` | Bool snaps to the gap edge (pause₁) regardless of structure ambiguity |

So all three tiers — bool, structure, waveform — independently converge on pause₁. Mode never matters.

**Requirements for an EC-6-capable fixture (all three needed):**

1. **Haystack must include the decoy as a genuine competitor.** Inject a **non-zero alignment offset** mapping A's pause₁ onto B's pause₂, so the B haystack centers on the decoy and every mode must *search away* from center to reach the truth. (Mirrors how `oracle_injected_alignment` is built, but with a real offset.)
2. **bool/structure genuinely ambiguous** between true pause and decoy — two similar-duration silences with near-identical active/silent bin patterns (already true in F2).
3. **Distinct B content at the true pause** so the **waveform tier cannot trivially win**, while the **energy contour still can** — i.e. B's true-pause neighborhood must differ in fine waveform from A (so Pearson is not ~1 there) yet share the loudness envelope. Breaks the `b = a.clone()` shortcut.

**Acceptance shape:** new builder (e.g. `build_f4_decoy_production`) + matrix rows where **`energy`/`auto` patch near the true pause (non-zero slide toward truth) and `bool` lands on the decoy (slide ≈ 0) or skips.** Assert the slide divergence, not just patched counts (the `slide_secs` column already surfaces it).

- [x] Builder with offset injection + distinct true-pause B content — `build_f4_decoy_production` (`test_support/energy_signature_fixtures.rs`). A's gap sits at the decoy time; B carries the decoy (descending outer pre) and the true pause (ascending outer pre, matching A) with an **identical inner border** so the narrow waveform seam stays neutral. Context-3 only (`shift = gap + 2·context ≤ border`).
- [x] Oracle domain test: `energy` at truth, `bool` ties at decoy — `p4_f4_decoy_energy_separates_but_bool_ties` (fast, score-level) + `p4_f4_decoy_unified_search_diverges` (`#[ignore]`, full unified search confirms `prefer_start` keeps bool at the decoy nominal while energy lands on truth).
- [x] Matrix rows (oracle-injected, context 3) showing the bool/energy slide split → records **EC-6**. F4-decoy wired into `energy_signature_mode_matrix` (structure-isolated) + `f4_decoy_patch_discrimination` asserts energy→truth (slide +7 s) / bool→decoy (slide 0). Waveform neutrality holds at the patch layer under structure isolation; production weights mask the split (recorded in Tuning record).

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
- [x] Confirm or retune `min_structure_match_score` (default 0.55) — **no change.** The F4 weight sweep (2026-06-23 e) shows the energy-vs-bool masking lever is `fill_fit_nominal_bias_scale`, not a structure-score floor; `min_structure_match_score` is orthogonal. Corpus matrix uses **0.0** (structure-isolated, like I1); operator defaults unchanged.
- [x] Document **mode-coupled `fill_fit_energy_nominal_bias_scale`** (default 0.25; energy self-corrects a drifted nominal) in [gap-repair-guide.md](gap-repair-guide.md) § Layer 4 + [gap-fill-modes.md](gap-fill-modes.md) (Structure signatures + config table).
- [x] Document recommended `gap_signature_context_secs` (3 vs 10 vs 30) in [gap-repair-guide.md](gap-repair-guide.md) § Layer 4 + [gap-fill-modes.md](gap-fill-modes.md) — caveated: keep the 3 s default; matrix showed no measurable patch benefit from 10 / 30 s; treat as a manual per-gap knob, not a default to raise.
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
- **`slide_secs` column** added to the matrix CSV so this kind of all-modes-agree result is visible at a glance rather than hidden behind equal patched counts.

**Production-weights follow-up (2026-06-23 c)** — `production_fit_weights_config` (`structure 0.35 / waveform 0.65`, production nominal bias) wired as a second F2-long pass (`F2-long-prodw`, `run_oracle_matrix_rows` + `f2_production_weights_diagnostic`).

| Fixture | Config | Mode | Patched | Slide s | Wall s | Result |
|---------|--------|------|---------|---------|--------|--------|
| F2-long | structure-isolated | Bool/Energy/Auto | 1 each | 0.000 | ~12 | placed at pause₁ |
| F2-long-prodw | production fit weights | Bool/Energy/Auto | 1 each | 0.000 | ~46–70 | placed at pause₁ |

- **Production fit weights change nothing** — all modes still patch at **slide 0.000** (pause₁), `High`. Switching from structure isolation to the waveform-driven tier does not create discrimination.
- **Root cause is the fixture, not the config:** F2's `b = a.clone()` with only pause₂ zeroed means pause₁ is **uniquely identifiable by waveform** (B's pre/post around pause₁ matches A exactly), while the decoy pause₂ has no matching B content. So every tier — bool (via `snap_fill_to_gap`), structure, and waveform — independently converges on pause₁. No weight setting can expose a bool/energy gap here.
- **Wall cost:** production weights run ~4–6× slower (~46–70 s vs ~12 s) because the waveform tier does full-PCM Pearson over the haystack.
- **EC-6 needs a redesigned fixture**, not a config knob. Requirements: (1) the patch haystack must *include* the decoy as a genuine competitor (non-zero offset mapping A's pause₁ onto B's pause₂, so the haystack centers on the decoy); (2) bool/structure ambiguous between true and decoy; (3) **B content distinct at pause₁** so waveform alone cannot trivially win while the energy contour still can. The current `b = a.clone()` violates (1) and (3).

**F4-decoy result (2026-06-23 d) — EC-6 MET.** New `build_f4_decoy_production` fixture (Phase E.1) satisfies all three requirements: A's gap sits at the decoy nominal; the true pause is shifted +7 s in B; decoy/truth carry identical bool patterns but anti-correlated outer energy contours; the inner ~200 ms border is identical (waveform-neutral). Oracle-injected patch, context 3.

| Fixture | Config | Mode | Patched | Slide s | Wall s | Result |
|---------|--------|------|---------|---------|--------|--------|
| F4-decoy | structure-isolated | Bool | 1 | **0.000** | ~36 | stays at decoy |
| F4-decoy | structure-isolated | Energy | 1 | **7.000** | ~37 | slides to true pause |
| F4-decoy | structure-isolated | Auto | 1 | **7.000** | ~38 | slides to true pause |
| F4-decoy-prodw | production fit weights | Bool/Energy/Auto | 1 each | 0.000 | ~265–282 | all stay at decoy |

- **Discrimination survives the patch path** under structure isolation: `energy`/`auto` resolve the true pause (slide +7 s), `bool` stays at the decoy (slide 0). Asserted by `f4_decoy_patch_discrimination` (`#[ignore]`); domain oracle by `p4_*`.
- **Production weights mask the split** — with full production fit weights (`structure 0.35 / waveform 0.65`, `nominal_bias 1.0`), all modes stay at the decoy (slide 0). The weight **sweep below** shows the masking lever is `nominal_bias`, not the waveform weight.
- **Wall cost:** production-weights rows ~7× slower (~270 s vs ~37 s) — full-PCM Pearson per candidate.

**Weight tuning — `nominal_bias` is the lever, not the structure/waveform split (2026-06-23 e).** Sweep on F4 (energy mode, decoy 7 s off the nominal map), `f4_decoy_weight_sweep` + `f4_decoy_bias_boundary`:

| structure_w | waveform_w | nominal_bias | energy slide | result |
|-------------|------------|--------------|--------------|--------|
| 1.00 | 0.00 | 0.0 | 7.000 | truth (structure isolation) |
| 0.35 | 0.65 | 0.0 | 7.000 | **truth — full production weights, no bias** |
| 0.35 | 0.65 | 0.10 | 7.000 | truth |
| 0.35 | 0.65 | 0.25 | 7.000 | truth (boundary) |
| 0.35 | 0.65 | 0.35 | 0.000 | decoy (masked) |
| 0.35 | 0.65 | 0.50 | 0.000 | decoy |
| 0.35 | 0.65 | 1.00 | 0.000 | decoy (default) |
| 0.65 | 0.35 | 1.00 | 0.000 | decoy (more structure does not rescue it) |
| 0.50 | 0.50 | 1.00 | 0.000 | decoy |

(`bool` control stays at the decoy in every row, with and without bias.)

**Findings:**

- **The structure/waveform weight split does not mask energy.** At the full production `0.35 / 0.65`, energy still resolves the true pause — provided `nominal_bias ≤ ~0.25`. No need to change the weight split for the energy signature.
- **`fill_fit_nominal_bias_scale` is the lever.** It anchors the search to the alignment-supplied nominal map; at the default `1.0` it pins placement to the (wrong) decoy and a confident energy score cannot override it. Masking begins at bias `0.35` for a 7 s-off nominal; smaller real-world offsets ("hundreds of ms", the plan's motivating case) tolerate more bias, but the direction is unambiguous: **energy mode wants low `nominal_bias`.**
- **`min_structure_match_score` is *not* the relevant knob.** The masking is in the search objective's bias term, not a structure-score floor — retuning the floor would not change this. (Closes the parent plan's "retune `min_structure_match_score`" question: no change needed on this account.)
- **Guarded by** `f4_decoy_energy_recovers_at_low_bias` (energy, `0.35/0.65`, bias `0.25` → slide 7.000).

**Recommendation (no global default change):** keep `nominal_bias = 1.0` as the default — it protects the common case where the alignment map is correct and guards against the search wandering to spurious far matches. Instead:

1. **Operator guidance:** repairing drift-heavy material where the nominal B map is off should pair `gap_signature_mode = energy` (or `auto`) with a reduced `fill_fit_nominal_bias_scale` (≤ 0.25 recovers a 7 s offset at default weights). Document in [gap-repair-guide.md](gap-repair-guide.md) § Layer 4 / [gap-fill-modes.md](gap-fill-modes.md).
2. **Mode-coupled `nominal_bias` — IMPLEMENTED (2026-06-23 f).** New `fill_fit_energy_nominal_bias_scale` config (default **0.25**) applies a lower distance-from-nominal penalty when the resolved signature is **energy**, while bool-resolved gaps keep the base `fill_fit_nominal_bias_scale` (default 1.0). Applied per-resolved-signature in `patch_region.rs`. So at production fit weights, energy automatically un-masks (slide → true pause) without operators touching the base bias. Guarded by `f4_decoy_mode_coupled_bias` (energy → 7.000 s / bool → 0.000 s with base bias 1.0). The penalty scales linearly with distance (`0.02 × scale × bins`), so the lowered energy scale only frees far-off (drifted) candidates — small offsets win under either scale, keeping the change low-risk for the common case.

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
