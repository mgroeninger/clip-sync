# Temporary plan: energy-envelope gap structure matching

> **Status:** Draft (2026-06-20). Motivated by gap-fill placement that still depends on **1-bit** active/silent structure bins (`gap_signature_bin_ms` @ 50 ms, `gap_signature_context_secs` @ 3 s) while waveform refinement pushes finer PCM loops for precision. A **gated loudness envelope** over a longer context should discriminate dropout edges more cheaply than sliding full PCM, then hand off to the existing waveform seam tier.
>
> Archive to `docs/archive/energy-signature-plan.md` when shipped.

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
| Integration tests | `tests/patch_audio_integration.rs` | Bool-structure fixtures | 0, 3 |
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
| **Mode switch** | `gap_signature_mode: bool \| energy \| auto` on `[repair]`. **`auto`**: use energy when `max(pre_energy) > gate` on both halves; else bool. Default **`energy`** after Phase 3 tuning; **`bool`** during Phase 1–2 for regression safety. |
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

- [ ] Add unit fixtures in `gap_structure` tests (or `tests/patch_audio_integration.rs`):
  - **Same pause pattern, different levels:** A and B share active/silent bool pattern but B dropout offset by N bins; energy differs in pre-gap loudness ramp.
  - **Multiple pauses:** Two similar pauses within `fill_border_search_secs`; only one aligns with A’s pre-gap energy contour.
  - **Flat envelope:** Steady speech/drone — energy and bool should agree; document `auto` fallback to bool.
- [ ] Record baseline: run existing `patch_audio_integration` with verbose; note structure pre/post on a drift-heavy long-form pair if available (manual checklist row in plan appendix).
- [ ] Define acceptance deltas: energy mode locates correct offset in synthetic cases where bool picks wrong candidate.

### Phase 1 — Energy bins + timeline

**Intent:** Build representation and timeline; no change to patch outcomes yet (behind `gap_signature_mode = bool` default).

- [ ] `domain/gap_structure.rs` (or `gap_energy.rs`):
  - `energy_bins(samples, channels, start, end, bin_frames, gate…) -> Vec<f32>`
  - `EnergyTimeline::build` / `bins_for_frames`
  - `build_gap_energy_signature` (mirror `build_gap_context_signature`)
  - `energy_similarity(expected: &[f32], observed: &[f32]) -> f64` (Pearson; length mismatch → `NEG_INFINITY`)
- [ ] `GapSignature` enum wrapping bool vs energy signatures; `build_gap_signature(...)` dispatcher.
- [ ] `score_pre_match` / `score_post_match` dispatch on signature variant.
- [ ] Config: `gap_signature_mode` enum, default `bool`; validation in `config.rs`.
- [ ] Wire `gap_signature_mode` through `PatchAudioRequest` → `SeamGateParams` (field only; still bool path).
- [ ] Unit tests: energy_bins gate behavior; similarity 1.0 on identical vectors; 0.0 on uncorrelated.

### Phase 2 — Search integration + `auto` fallback

**Intent:** Energy mode participates in structure search and unified fit; bool remains default.

- [ ] `match_gap_structure_in_b` / `match_gap_fill_unified_in_b`: accept `GapSignature`; build `EnergyTimeline` when needed.
- [ ] `evaluate_seam_gate` (`patch_region.rs`): call `build_gap_signature` instead of `build_gap_context_signature`.
- [ ] Implement `auto`: if either half’s gated max ≈ 0, build bool signature instead (log at debug).
- [ ] Integration tests under `gap_signature_mode = energy` and `auto`:
  - Port Phase 0 synthetic cases through `prepare_region_patch` or `match_gap_fill_unified_in_b`.
  - Regression: all existing `patch_audio_integration` tests still pass with `bool`.
- [ ] Verbose diagnostics: log `signature_mode=energy|bool` on gap fill plan lines (`patch_audio.rs` / `format_gap_fill_plan_lines`).

### Phase 3 — Context tuning + default flip

**Intent:** Tune thresholds on real material; make energy the default; document context guidance.

- [ ] Corpus / manual pass: compare skip counts and structure pre/post on repair matrix with `gap_signature_context_secs` ∈ {3, 10, 30}.
- [ ] Adjust `min_structure_match_score` default if energy score distribution shifts (document old vs new).
- [ ] Default `gap_signature_mode` → `energy` (keep `bool` for tests via fixture TOML).
- [ ] README § Gap patching — new subsection “Structure signatures” (energy vs bool, context length).
- [ ] `docs/cli-output.md` — verbose `signature_mode` if exposed.
- [ ] Example `[repair]` block in README with optional `gap_signature_context_secs = 15.0` for hard gaps.

### Phase 4 — Optional optimizations (defer if Phase 3 ships clean)

- [ ] FFT cross-correlation for pre/post slide when `context_secs * 1000 / bin_ms > 1000` (profile-driven).
- [ ] **Adaptive context:** only widen context when bool/energy score at nominal map < floor (saves decode on easy gaps) — requires second pass or lazy extend; backlog if not needed.
- [ ] Peak-picked sparse landmarks as third `GapSignature` variant — only if envelope still ambiguous on speech-heavy corpus.

---

## Config surface (cumulative)

| Key | Phase | Default | Notes |
|-----|-------|---------|-------|
| `gap_signature_mode` | 1 | `bool` → `energy` (Phase 3) | `bool` \| `energy` \| `auto` |
| `gap_signature_context_secs` | — | `3.0` | Existing; document 10–30 s for hard gaps |
| `gap_signature_bin_ms` | — | `50` | Existing; 20–50 ms reasonable for energy |
| `silence_peak_fraction` | — | (scan) | Reused as envelope gate |
| `absolute_silence_rms` | — | (scan) | Reused as envelope floor |
| `min_structure_match_score` | 3 | `0.55` (retune?) | Same semantic, new distribution |

No new CLI flags required for v1 (config-only, matching `gap_signature_*` today). Optional later: `--gap-signature-mode energy`.

Existing keys unchanged: `fill_mode`, `fill_fit_*_weight`, `min_fill_correlation`, `fill_border_search_secs`, `fill_seam_search_secs`, gap extension flags.

---

## Testing strategy

| Layer | What |
|-------|------|
| Unit | `energy_bins` gate + RMS; `energy_similarity`; `auto` flat → bool; enum dispatch |
| Unit | `gap_structure` search finds offset with energy where bool fails (Phase 0 fixtures) |
| Lib | `match_gap_fill_unified_in_b` energy path; `structure_fine_polish_frames` unchanged |
| Integration | `patch_audio_integration.rs` — duplicate critical cases under `gap_signature_mode = energy` |
| Regression | Full integration suite with `bool` (CI default in `repair.toml` until Phase 3) |
| Manual | Drift-heavy long-form pair: `-v` compare structure pre/post, skip count vs bool baseline |

**CI:** Run patch integration with `gap_signature_mode = bool` through Phase 2; add parallel `energy` job or matrix row in Phase 2.

---

## Rollout

1. **Phase 0** — synthetic fixtures + baseline notes.
2. **Phase 1** — energy types behind config; default `bool`; no behavior change.
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
| Flat / steady content — energy uninformative | `auto` falls back to bool |
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
