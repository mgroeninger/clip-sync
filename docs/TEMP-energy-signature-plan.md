# Temporary plan: energy-envelope gap structure matching

> **Status:** Phase 0–2 **complete** (2026-06-21). Energy bins, `GapSignature` enum, `gap_signature_mode` config (**`auto` default**), fit-path search + flat-envelope fallback, shared fixtures (`test_support/`), acceptance **U1–U8** (+ **U5b** F2 integration domain), **I1–I5**, **P2-1–P2-2** green. **I1** and **I3** assert domain + haystack + full patch (not domain-only). Phase 3 corpus tuning / docs remain open.
>
> Archive to `docs/archive/energy-signature-plan.md` when Phase 3 ships.

**Problem:** Structure match today collapses each time bin to a single boolean (“mostly silent” vs “mostly active”). Two regions can share the same talk/pause **pattern** but differ in loudness dynamics (breath level, room tone, musical swells, encode AGC). When the nominal B map is off by hundreds of ms, or multiple pauses sit near the gap, bool structure is ambiguous and repair compensates with more waveform slide / A-boundary extension — higher CPU, same marginal placement.

**Goal:** Upgrade the **structure tier** inside the existing per-gap patch pipeline to a **gated log-RMS (or quantized energy) envelope** over configurable context (default 3 s; up to ~30 s for hard gaps). Match pre-gap and post-gap halves on B with normalized correlation (or FFT cross-correlation). Keep waveform Pearson at ~250 ms borders as the fine placement layer. Preserve bool structure as a **fallback** when the envelope is flat (near-silence, steady drone).

**Non-goals (v1):** Changing global alignment (chromaprint / query-reference); gap **scan** detection; replacing waveform seams; onset/landmark constellation maps (defer); spectrogram features; runtime mode switching per gap without config.

---

## Current codebase baseline

| Area | Path | Current state | First phase touched |
|------|------|---------------|---------------------|
| Per-gap orchestration | `application/patch_audio.rs` | B haystack sized by `gap_signature_context_secs` + `fill_border_search_secs` | 1 |
| Seam gate | `application/patch_region.rs` | `build_gap_context_signature` → `match_gap_fill_unified_in_b` | 1, 2 |
| Structure search | `domain/gap_structure.rs` | `ActivityTimeline` (`Vec<bool>`), `bin_similarity`, coarse + fine polish | 1, 2 |
| Unified fit | `domain/gap_fill_fit.rs` | Joint structure + waveform scoring (`fill_fit_*_weight`) | 2 |
| Seam scoring | `domain/policies.rs` | `fill_seam_correlations`, `trim_low_energy_*` (local gate analogue) | — (unchanged) |
| Config | `infrastructure/config.rs` | `gap_signature_context_secs`, `gap_signature_bin_ms`, silence thresholds | 1 |
| Integration tests | `tests/patch_audio_integration.rs` | Energy acceptance I1–I4, diagnostics, `assert_energy_integration_patch` | 0, 2 |
| Fill fitting | `docs/TEMP-fill-fitting-plan.md` | Phases A–C shipped (`fill_mode = fit` default) | 2 (scores into unified fit) |

### Pipeline fit (unchanged outer stages)

```text
align → scan gaps → fill plan (offset map)
    → per gap: refine_gap_frames (A)
    → slice B haystack (context + border search + margin)
    → evaluate_seam_gate
         ├─ [NEW] energy envelope match on B     ← replaces bool-only structure
         ├─ unified fit (structure + waveform)    ← structure term becomes energy score
         └─ waveform Pearson at borders           ← unchanged fine tier
    → splice + crossfade + normalize
```

B extract geometry already includes signature context on both sides (`patch_audio.rs` `b_extract_start_secs` / `b_extract_end_secs`). Raising `gap_signature_context_secs` does not require a new decode path — only more audio per gap in memory.

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Representation** | Per bin: `log1p(max(rms, floor))` after mono downmix (match scan silence policy). Optional 4-level quantize for diagnostics only — store `f32` internally. |
| **Noise gate** | Reuse `silence_peak_fraction` + `absolute_silence_rms` from gap scan report (same thresholds gaps were detected with). Values below gate → `0.0` in envelope (not bool collapse). |
| **Normalization** | Peak-normalize each pre/post signature half before correlation (encoding level mismatch). Same philosophy as `seam_pearson` in `policies.rs`. |
| **Similarity** | Replace `bin_similarity` for energy mode with **Pearson** on aligned bin vectors (or dot product after L2 norm — equivalent). Keep `bin_similarity` for bool fallback. |
| **Timeline** | `EnergyTimeline` parallel to `ActivityTimeline`: precompute `Vec<f32>` bins once per B haystack; `bins_for_frames` slice helper mirrors bool path. |
| **Signature type** | Extend `GapContextSignature` with `pre_energy: Vec<f32>`, `post_energy: Vec<f32>` **or** enum `GapSignature { Bool(...), Energy(...) }` — prefer enum to avoid dual truth. |
| **Mode switch** | `gap_signature_mode: bool \| energy \| auto` on `[repair]`. **`auto`** (default): use energy when **both** pre/post halves have contour (peak-normalized envelope range **> 5%**); else bool (silence, steady drone, or other flat envelopes). |
| **Context length** | Single `gap_signature_context_secs` for both modes. Document recommended 3 s (fast) vs 10–30 s (ambiguous / long gaps). No separate “extended context” knob in v1 — users raise one field. |
| **Search geometry** | Reuse `search_best_fill_start/end`, `fine_polish_structure_start`, `search_coarse_step`, `structure_fine_polish_frames` — only scoring function changes. |
| **Unified fit** | Energy pre/post scores feed `structure_pre` / `structure_post` in `unified_fit_score`; weights unchanged (`fill_fit_structure_weight` / `fill_fit_waveform_weight`). |
| **Thresholds** | Keep `min_structure_match_score` name; retune default if energy scores distribute differently (Phase 3 corpus). |
| **Structure trust** | Gate-mode trust flags apply to energy scores same as bool (`strong_structure_trust`, etc.). Fit mode already always measures waveform. |
| **Performance** | Prefer FFT cross-correlation only if profiling shows Pearson slide over 600-bin vectors hot; not required for v1. Cap work: existing `search_coarse_step` + `structure_fine_polish_frames` bounds. |
| **Layering** | Core math in `domain/gap_structure.rs` (or new `domain/gap_energy.rs` if file grows); no changes to `clip-sync` analyzer crate. |

---

## Phases

### Phase 0 — Characterization fixtures

**Intent:** Prove bool structure fails and energy wins on controlled synthetic gaps before changing production defaults.

- [x] Implement the three **fixture geometries** below — shared module `crates/clip-sync-repair/src/test_support/energy_signature_fixtures.rs` + `energy_signature_acceptance.rs` (U1–U8, U5b/U5c, `f2_integration_energy_scores_are_finite`) + `tests/patch_audio_integration.rs` (I1–I4, diagnostics, `assert_energy_integration_patch`).
- [x] Meet all **automated acceptance** rows in [Phase 0/2 acceptance criteria](#phase-02-acceptance-criteria).
- [ ] Record baseline (manual): run existing `patch_audio_integration` with verbose; note structure pre/post on a drift-heavy long-form pair if available — see [Manual baseline](#manual-baseline-optional).

### Phase 0/2 acceptance criteria

Automated Phase 0/2 is **done** when every row in the tables below passes in CI. Fixtures are **synthetic only** (pure-Rust `i16` timelines or in-test WAV writers — no committed external media).

**Shipped (2026-06-21):** all rows below pass except optional manual baseline and P2-3 CI matrix.

#### Shared test constraints

| Constraint | Value | Why |
|------------|-------|-----|
| `fill_mode` | `fit` | Energy signature is wired on the fit seam gate only (`patch_region.rs`); gate legacy path stays bool. |
| `gap_signature_bin_ms` | `50` | Match production default; 20 ms bins in unit tests when timeline is short. |
| `absolute_silence_rms` | `0.0` | Synthetic gaps are exact zeros unless noted. |
| `silence_peak_fraction` | `0.01` | Match scan / repair defaults. |
| `min_structure_match_score` | `0.0` in `energy_sig_patch_options()` | Acceptance tests disable structure floor; production default `0.55` until Phase 3 retune. |
| `fill_absolute_floor` | `-0.05` in `energy_sig_patch_options()` | F2 post-seam at pause₁ is intentionally asymmetric (A ramp vs B hard cut); allows Marginal confidence. Production default unchanged. |
| `short_gap_one_strong_seam_fallback` | `true` in `energy_sig_patch_options()` | Matches production `repair.toml` default. |
| Extension grid | off in fast fixtures | `gap_*_extend_on_*_seam_fail = false`, small `gap_end_extend_max_ms` — keeps CI fast; optional slow fixture with extension on. |
| Waveform design | ramps / level steps on A | Pure sine at two decoys can let **waveform** decide; structure discrimination must not be masked by identical Pearson seams. |

**Tolerance:** `start_frame` within **±1 `bin_frames`** of ground truth unless a test documents intentional slack.

#### Fixture geometries

| ID | Scenario | A (around gap) | B haystack | Nominal B map | Decoy |
|----|----------|----------------|------------|---------------|-------|
| **F1** | Same pause pattern, different levels | Linear amp ramp → silence (gap) → steady post level | Same ramp at **true** offset; **shifted** copy at `+Δ` frames (`Δ` ≥ 2 bins, ≤ `search_radius`) | Points at **decoy** (shifted dropout) | Bool-active/silent **pattern** matches at both sites; energy contour matches only at true site |
| **F2** | Multiple pauses | Single gap with distinctive pre-gap contour (ramp into silence) | Two pauses within `fill_border_search_secs`: **pause₁** (ramp into silence), **pause₂** (hard cut); similar duration | Points at **pause₂** | **pause₁** is true alignment; bool scores within **ε** at both pauses; energy `structure_pre` at pause₁ exceeds pause₂ by **≥ 0.15** |

**Integration variants** (`build_*_integration` at 48 kHz, 8 s timeline):

| ID | Builder | Notes |
|----|---------|-------|
| F1 | `build_f1_integration` | Silence lead before reported gap; guard at `silence_start - 1` blocks refine into ramp tail. |
| F2 | `build_f2_integration` | Unit F2 spacing at ~2.35 s (room for `gap_signature_context_secs`); gap ≥ 2×50 ms bins; scaled domain bins (U5 parity); guards at pause edges. A uses ramp + post-rise; B has hard cut at pause₂. |
| F3 | `build_f3_drone_integration` | Scaled drone + pad to 8 s. |
| **F3** | Flat envelope | Gap in steady non-silent level (drone / constant amp, not digital zero) | Steady level throughout context | Nominal map at gap | N/A — `auto` should not use energy |

Reference implementation sketch for **F1**: `gap_energy.rs` test `energy_finds_offset_when_bool_pattern_ambiguous` (scoring only); extend to full search + bool comparison.

#### Automated acceptance — unit / lib (`gap_energy.rs`, `gap_signature.rs`, `gap_fill_fit.rs`)

| ID | Fixture | Mode | Assertion |
|----|---------|------|-----------|
| U1 | F1 | `energy` | `score_pre_energy_match` at true offset **>** at decoy offset (strict inequality). |
| U2 | F1 | `bool` | `score_pre_match` at true vs decoy: **\|Δ score\| ≤ ε`** (tie or decoy wins — bool ambiguous). **ε = `BOOL_AMBIGUITY_EPS` = 0.45** in fixtures (plan originally 0.08; widened so F1 bool pre scores at true/decoy are treated as ambiguous on synthetic ramps). |
| U3 | F1 | `energy` | `match_gap_fill_unified_in_b`: `alignment.start_frame` within ±1 bin of **true** offset. |
| U4 | F1 | `bool` | `match_gap_fill_unified_in_b`: `start_frame` at **decoy** **or** `\|start − true\| > \|start_energy − true\|` (energy strictly closer). |
| U5 | F2 | `energy` | Unified search: `start_frame` within ±1 bin of **pause₁**. |
| U5b | F2_integration | `energy` | Same as U5 on `build_f2_integration(48_000, …)` — domain oracle before patch path. |
| U5c | F2 @ 48 kHz (scaled unit) | `energy` | `build_f2_at_rate` — confirms scaled unit geometry at patch rate without 8 s pad. |
| U6 | F2 | `bool` | Unified search: `start_frame` at **pause₂** (nominal) **or** bool `structure_pre` within ε at both pauses. |
| U7 | F3 | `auto` | `build_gap_signature(...)` → `GapSignature::Bool(_)`. |
| U8 | F3 | `energy` vs `bool` | At nominal map, unified `structure_pre` and `structure_post` differ by **≤ 0.08** between modes. |

#### Automated acceptance — integration (`tests/patch_audio_integration.rs`)

Use `energy_sig_patch_options()` for I1–I4 (`fill_mode = fit`, `fill_border_search_secs = 3.5`, `gap_signature_context_secs = 0.5`, extensions off, structure-heavy weights with `waveform_weight = 0`, bias scales 0, `min_structure_match_score = 0.0`, `fill_absolute_floor = -0.05`, `short_gap_one_strong_seam_fallback = true`). `gap_signature_mode` on `PatchTestOptions`.

Shared helper `assert_energy_integration_patch(fixture, report, options, test_id, expected_slide_secs)` runs domain oracle, haystack oracle (`preview_patch_geometry` + `unified_match_on_haystack`), full `PatchAudio`, requires `patched_count == 1`, and checks `align_adjustment_secs` within ±1 bin of `expected_slide_secs` (or `structure_slide_secs(fixture, true_fill_start)` when `None`).

| ID | Fixture | Request | Assertion | Status |
|----|---------|---------|-----------|--------|
| I1 | F1 | `gap_signature_mode = energy`, `fill_mode = fit` | `assert_energy_integration_patch(…, None)` — slide vs `structure_slide_secs(true_fill)`. | ✅ |
| I2 | F1 | same geometry, `gap_signature_mode = bool` | **Domain:** bool `start_frame` at decoy **or** farther from truth than energy unified match. | ✅ |
| I3 | F2 | `energy` | `assert_energy_integration_patch(…, Some(0.0))` — A/B aligned at pause₁; slide **0** (not pause₂ nominal offset). | ✅ |
| I4 | F3 | `auto` | `build_gap_signature(Auto)` → `Bool`; domain `unified_match(auto)` same `start_frame` as `bool`. (Full patch outcome equivalence deferred — drone fixture rarely patches through full pipeline.) | ✅ |
| I5 | — | all existing tests, default `auto` | No regressions; suite green (27 passed, 1 ignored smoke). | ✅ |

#### Domain oracle vs patch path

Three layers can disagree; I1/I3 require all three to agree within tolerance:

| Layer | API | B PCM scope |
|-------|-----|-------------|
| **Domain** | `EnergySignatureFixture::unified_match` | Full-track B PCM |
| **Haystack** | `PatchGeometryPreview::unified_match_on_haystack` | Sliced B (patch geometry) |
| **Patch** | `run_patch` → `PatchAudio` | Production pipeline + seam gates |

**Domain oracle** uses fixture `gap_start`/`gap_end` for the A signature and searches full B with `nominal_fill_start` at the decoy (F1) or pause₂ (F2). **Do not** set fixture `gap_start` to refined frames — that desyncs energy signatures (e.g. `-inf` scores).

**Patch path** uses `gap_report_times(fixture)` for `GapReport` A/B times: `refine_gap_frames` on A at pause₁ and separately on B at `nominal_fill_start`/`nominal_fill_end` (pause₂ for F2). When global alignment offset is zero, structure search still centers on pause₁ on B; the B report times encode the wrong nominal map (pause₂).

**Common patch-path failures (fixed in integration fixtures):**

| Skip reason | Cause | Mitigation |
|-------------|-------|------------|
| `BoundaryAlignmentFailed` | `refine_gap_frames` walks into quiet ramp → haystack signature ≠ domain; or gap shorter than one 50 ms energy bin | F1/F2: guard sample at `gap_start - 1`; F2: place pauses ~2.35 s in, gap ≥ `2 × patch_bin_frames` |
| `CorrelationBelowThreshold` / `WaveformBelowThreshold` | Structure or waveform seam gate after placement | F2: intentional A post-rise vs B hard cut — `fill_absolute_floor = -0.05` + `short_gap_one_strong_seam_fallback` in test options |

**Diagnostic tests** (ignored; `--ignored --nocapture`) print fixture vs refined frames, haystack bounds, domain vs haystack match, and patch outcome via `energy_sig_patch_diagnostic`:

```powershell
cargo test -p clip-sync-repair i1_f1_patch_diagnostic -- --ignored --nocapture
cargo test -p clip-sync-repair i3_f2_patch_diagnostic -- --ignored --nocapture
```

Helper: `test_support/patch_geometry_preview.rs` (`preview_patch_geometry`, `unified_match_on_haystack`, `format_diagnostic`).

**F1 integration alignment:** `build_f1_integration` keeps scan-reported `gap_start`/`gap_end` for the domain oracle; `gap_report_times` applies refine for patch reports only. Guard at `silence_start - 1` on A and matching B frame.

**F2 integration alignment:** `build_f2_integration` uses unit-like A (ramp into pause₁, no silence lead), pause₂ as nominal map, gap sized for 50 ms patch bins. I3 expects `align_adjustment_secs ≈ 0` when tracks are aligned at pause₁ (not `structure_slide_secs` relative to pause₂).

**Phase 3 follow-up (optional):** align A/B post-seam at pause₁ in F2 fixture so acceptance tests can use production `fill_absolute_floor` (0.12) instead of `-0.05`.


| ID | Item | Assertion | Status |
|----|------|-----------|--------|
| P2-1 | Verbose fill plan | `-v` gap plan line includes `signature_mode=energy` or `signature_mode=bool` matching resolved signature (`GapSignature::mode_label()`). Unit: `patch_audio.rs` `format_gap_fill_plan_lines` test. | ✅ |
| P2-2 | Port | I1–I4 use the same geometries as U1–U8 (shared fixture helpers in `test_support/`). | ✅ |
| P2-3 | Regression | I5; optional CI matrix row `gap_signature_mode = energy` running I1–I4 only. | I5 ✅; matrix optional |

#### Manual baseline (optional)

Not required for Phase 0/2 **done**; supports Phase 3 tuning.

| Field | Record |
|-------|--------|
| Pair | Long-form A/B with clip offset drift (operator-owned or `CLIP_SYNC_GAP_CORPUS` external tier). |
| Config | `fill_mode = fit`, `gap_signature_mode = auto` then `energy` or `bool`, same other knobs. |
| Metrics | Per gap: skip vs patch, `struct pre` / `struct post` (verbose), `structure_slide`, wall time. |
| Delta | Note cases where energy patches and bool skips (candidate for F1/F2-like synthetic follow-up). |

#### Fixture pitfalls (test authors)

- **Identical sine seams** at decoy and truth → waveform tier dominates; use ramps or level steps on A borders.
- **Decoy outside `search_radius`** → neither mode can win; keep decoy inside `fill_border_search_secs` + margin.
- **Gate mode** → energy not exercised; F1–F3 integration tests must use **fit**.
- **All-silence F3** → only exercises silence; include **steady non-zero** drone for realistic `auto` fallback.
- **F2 gap shorter than 50 ms bin** → haystack energy search returns `None` (`BoundaryAlignmentFailed`); integration gap must be ≥ `2 × patch_bin_frames`.
- **F2 pauses at start of 8 s file** → `context_frames` larger than `gap_start` breaks domain signature; place pauses ~2.35 s in (same anchor as F1 integration).
- **Scaled F2 + pad only** → refine walks into ramp; use `build_f2_integration` native timeline, not `build_f2_scaled` + tail pad alone.
- **F2 slide assertion** → when A/B are drift-aligned at pause₁, expect `align_adjustment_secs ≈ 0`, not slide relative to pause₂ nominal map.

### Phase 1 — Energy bins + timeline

**Intent:** Build representation and timeline; no change to patch outcomes until mode is switched from default `auto`.

- [x] `domain/gap_energy.rs`: `energy_bins`, `EnergyTimeline`, `build_gap_energy_signature`, `energy_similarity` (Pearson).
- [x] `GapSignature` enum + `build_gap_signature` dispatcher (`domain/gap_signature.rs`).
- [x] `score_pre_match` / `score_post_match` dispatch on signature variant.
- [x] Config: `gap_signature_mode` enum, default `auto`; validation in `config.rs`.
- [x] Wire `gap_signature_mode` through `PatchAudioRequest` → `SeamGateParams`.
- [x] Unit tests: energy_bins gate behavior; similarity; signature dispatch.

### Phase 2 — Search integration + `auto` fallback

**Intent:** Energy mode participates in structure search and unified fit; `auto` is default.

- [x] `match_gap_structure_in_b` / `match_gap_fill_unified_in_b`: accept `GapSignature`; build `EnergyTimeline` when needed.
- [x] `evaluate_seam_gate` (`patch_region.rs`): call `build_gap_signature`.
- [x] Implement `auto`: flat envelope on either half → bool (`energy_envelope_is_flat`, peak-normalized range ≤ 5%); debug log.
- [x] Integration tests: **I1–I5** and **P2-1–P2-2** (see acceptance tables).
- [x] Verbose diagnostics: `signature_mode=energy|bool` on gap fill plan lines — **P2-1**.

### Phase 3 — Context tuning + default flip

**Intent:** Tune thresholds on real material; make energy the default; document context guidance.

> **Corpus plan:** [TEMP-energy-corpus-plan.md](TEMP-energy-corpus-plan.md) — long synthetic F1/F2 @ production geometry, mode matrix, optional PD/CC profile → regenerate (no copyrighted media in repo).

- [ ] Corpus / manual pass: compare skip counts and structure pre/post on repair matrix with `gap_signature_context_secs` ∈ {3, 10, 30}. Record rows with vocabulary tags ([gap-repair-guide.md](gap-repair-guide.md) § Vocabulary; acceptance **EC-1–EC-6** in [TEMP-energy-corpus-plan.md](TEMP-energy-corpus-plan.md)).
- [ ] Adjust `min_structure_match_score` default if energy score distribution shifts (document old vs new).
- [x] Default `gap_signature_mode` → `auto`.
- [ ] README § Gap patching — new subsection “Structure signatures” (energy vs bool, context length).
- [ ] `docs/cli-output.md` — verbose `signature_mode` if exposed.
- [ ] Example `[repair]` block in README with optional `gap_signature_context_secs = 15.0` for hard gaps.
- [x] (Optional) F2 integration fixture: align post-seam at pause₁ — done for **F2-long** production geometry; I3 still uses integration floor `-0.05`.

### Phase 4 — Optional optimizations (defer if Phase 3 ships clean)

- [ ] FFT cross-correlation for pre/post slide when `context_secs * 1000 / bin_ms > 1000` (profile-driven).
- [ ] **Adaptive context:** only widen context when bool/energy score at nominal map < floor (saves decode on easy gaps) — requires second pass or lazy extend; backlog if not needed.
- [ ] Peak-picked sparse landmarks as third `GapSignature` variant — only if envelope still ambiguous on speech-heavy corpus.

---

## Config surface (cumulative)

| Key | Phase | Default | Notes |
|-----|-------|---------|-------|
| `gap_signature_mode` | 1 | `auto` | `bool` \| `energy` \| `auto` |
| `gap_signature_context_secs` | — | `3.0` | Existing; document 10–30 s for hard gaps |
| `gap_signature_bin_ms` | — | `50` | Existing; 20–50 ms reasonable for energy |
| `silence_peak_fraction` | — | (scan) | Reused as envelope gate |
| `absolute_silence_rms` | — | (scan) | Reused as envelope floor |
| `min_structure_match_score` | 3 | `0.55` (retune?) | Same semantic, new distribution |

No new CLI flags required for v1 (config-only, matching `gap_signature_*` today). Optional later: `--gap-signature-mode energy`.

Existing keys unchanged: `fill_mode`, `fill_fit_*_weight`, `min_fill_correlation`, `fill_border_search_secs`, `fill_seam_search_secs`, gap extension flags.

---

## Testing strategy

Executable oracles: [Phase 0/2 acceptance criteria](#phase-02-acceptance-criteria) (**U1–U8**, **I1–I5**, **P2-1–P2-3**).

| Layer | What |
|-------|------|
| Unit | `energy_bins` gate + RMS; `energy_similarity`; **U7** (`auto` flat → bool) |
| Lib | **U1–U6**, **U8**, **U5b**/**U5c** — `match_gap_fill_unified_in_b` energy vs bool on **F1–F3** (+ integration domain) |
| Integration | **I1–I4** — full `PatchAudio` on **F1–F3** under `energy` / `bool` / `auto` |
| Regression | **I5** — full suite with production defaults (`auto`) |
| Manual | [Manual baseline](#manual-baseline-optional) — drift-heavy pair for Phase 3 notes |

**CI:** **I5** on every PR; optional job running **I1–I4** with `gap_signature_mode = energy` (**P2-3**).

---

## Rollout

1. **Phase 0** — synthetic fixtures + baseline notes.
2. **Phase 1** — energy types behind config; default `auto`.
3. **Phase 2** — energy search wired; integration tests; `auto` fallback.
4. **Phase 3** — tune, default `energy`, docs.
5. **Phase 4** — optimizations only if profiling demands.
6. Archive this doc; update `PLAN.md` repair § structure match; add `BACKLOG.md` completed row.

---

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| Energy scores not comparable to bool thresholds | Phase 3 corpus retune; keep `bool` mode |
| Long context increases B decode per gap | Default stays 3 s; user opts into 30 s; watch `BACKLOG` streaming item for extreme cases |
| Flat / steady content — energy uninformative | `auto` falls back to bool via `energy_envelope_is_flat` (not only all-silence) |
| Periodic content (chants, loops) | Same ambiguity as bool; waveform seam + unified fit still gate; no worse than today |
| Encode AGC mismatch | Peak-normalize per half; log-RMS not raw peak |
| Slower search | Same candidate count as bool; float Pearson on ~60–600 bins is cheap vs waveform PCM |

---

## Relationship to fill-fitting plan

| Fill-fitting phase | Interaction |
|--------------------|-------------|
| A–C (shipped) | Unified fit consumes structure scores — energy replaces bool at source |
| D (open) | Repeat penalty / dual anchor orthogonal; may share `trim_low_energy` patterns |

Do not block energy signature on Phase D. Ship energy through structure tier independently.

---

## Related reading

- [TEMP-fill-fitting-plan.md](TEMP-fill-fitting-plan.md) — waveform slide + unified fit (shipped A–C)
- [README.md](../README.md) § Gap patching pipeline
- [docs/cli-output.md](cli-output.md) § Gap patch outcomes
- `domain/gap_structure.rs` — `ActivityTimeline`, `build_gap_context_signature`
- `application/patch_audio.rs` — B haystack extraction

---

## Open questions

1. **Enum vs parallel fields:** `GapSignature` enum vs extending `GapContextSignature` with optional `Vec<f32>` — enum preferred for invariant clarity?
2. **Default context after flip:** Keep 3 s default or bump to 5–10 s when `energy` becomes default?
3. **Band-limit:** Speech band (300 Hz–3 kHz) before RMS — worth Phase 1 or only if HVAC rumble false-matches appear?
4. **Expose mode on CLI** for debugging (`--gap-signature-mode`) in Phase 2 or config-only through Phase 3?
5. **Score naming in JSON/verbose:** Keep `struct pre=` label for energy scores or add `energy pre=` alias for clarity?
