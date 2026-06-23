# Gap repair guide — classifying gaps and choosing settings

Operational guide for `clip-sync-repair`: what kinds of gaps exist, how they appear in the report, and which profiles and flags are worth trying.

**Normative reference** (flag matrix, pipelines, config defaults): [gap-fill-modes.md](gap-fill-modes.md).  
**Report layout and skip strings**: [cli-output.md](cli-output.md) § Repair gap outcomes.  
**JSON fields**: [json-output.md](json-output.md).

---

## How to use this guide

1. Run repair with **`-v`** on the **original** source for video A (not a previously muxed repair unless you intend a second pass).
2. Read the **gap table** and verbose per-gap lines (`pre`/`post`, `signature_mode`, `fit path`).
3. Match the gap to a **stage** and **shape** below (or read the composed **[Vocabulary](#vocabulary)** tags).
4. Apply the **recommendation** row; use [gap-fill-modes.md](gap-fill-modes.md) for exact flag names and TOML keys.

Thresholds below use **production defaults** unless noted: `min_fill_correlation = 0.35`, `fill_marginal_margin = 0.08`, `fill_absolute_floor = 0.12`, `fill_mode = fit`, repair profile **`default`** (`fit_boundary_search = baseline_only`, `fill_border_search_secs = 10`).

---

## Pipeline stages

Every gap passes through three layers. The “type” depends on where it stops.

```text
Scan (silence on A)
  → Fill plan (fillable?)
    → Patch (structure + waveform placement)
      → Patched / skipped / not planned
```

| Stage | Question | Outcomes in report |
|-------|----------|-------------------|
| **Scan** | Is A silent for ≥ `min_gap_ms` (default 1 s)? | Omitted if too short |
| **Plan** | Is B mapped and energetic? Tracks OK? In query coverage? | `unfillable`, `not planned: …` |
| **Patch** | Structure finds B bracket? Waveform tier OK? | `patched`, `skipped: …` |

---

## Layer 1 — Plan-time types (scan / fill plan)

These gaps never enter structure match, or are excluded before patch.

| ID | Type | How it is detected | Typical duration | Report status |
|----|------|--------------------|------------------|---------------|
| **P0** | Below scan floor | &lt; `min_gap_ms` | sub-second | Not listed |
| **P1** | **Unfillable** — no B overlap | A silent before shared A∩B overlap | any | `unfillable` |
| **P2** | **Unfillable** — B dry | `b_has_energy = false` | any | `unfillable` |
| **P3** | **Not planned** — outside coverage | Query-reference: gap off B map | any | `not planned: outside_reference_coverage` |
| **P4** | **Not planned** — tracks | Layout mismatch / no compatibility | any | `not planned: …` |
| **P5** | **Fillable** | Silent on A, B has energy in map | **~1–30 s** common | Enters patch (`repairable` in scan-only runs) |
| **P6** | **Fillable** long / tail | Same as P5 but spans file end or very long silence | **30 s – minutes** | Often patch **skip** (structure) |
| **P7** | **Audible hole, not scanned** | Dropout remains audible but re-encode or bed noise prevents silence detection | ~1 s | **Absent from report** — tune scan or fix source |

**P7** matters when a second run on a repaired/muxed file finds fewer gaps than expected: the tool only repairs what the scanner classifies as silence.

---

## Layer 2 — Content shape (acoustic / editorial)

Independent of report labels — helps predict seam behavior. All assume **P5** (fillable).

| ID | Shape | Structure signal | Common seam pattern after fit |
|----|-------|------------------|----------------------------|
| **C1** | Pure silence / room tone | Flat bool; flat energy (`auto` → bool) | Balanced weak or balanced OK |
| **C2** | Music / ambience dropout | Contour on bool and energy | Variable; may be marginal |
| **C3** | **Boundary gap** — music (or pause) → speech | Strong post seam (onset on B) | **Asymmetric**: low `pre`, high `post` |
| **C4** | Speech / dialog dropout | Talk–pause bool pattern | Asymmetric or balanced |
| **C5** | Long tail / end-of-file silence | Flat envelope; weak structure | **Structure alignment failed** |

**C3** is the highest **echo / repeat** risk when patched: post seam locks on speech; fill tail can overlap A’s border; crossfade blends like a double.

---

## Layer 3 — Fit-mode waveform tiers (default `fill_mode`)

Fit classifies the winning candidate using `min(pre, post)` after unified structure+waveform search. Gate mode uses different rules — see [gap-fill-modes.md](gap-fill-modes.md) § `fill_mode = gate`.

| Tier | Condition on `min(pre, post)` | Human report | Patches? |
|------|------------------------------|--------------|----------|
| **High** | ≥ `min_fill_correlation` (0.35) | `patched (pre=… post=…)` | Yes |
| **Marginal** | ≥ `min_fill_correlation - fill_marginal_margin` (**0.27**) and &lt; 0.35 | `! patched` | Yes (warning) |
| **Dead zone** | ≥ `fill_absolute_floor` (**0.12**) and &lt; **0.27** | `skipped: boundary correlation below threshold` | No |
| **Hard skip** | &lt; `fill_absolute_floor` (0.12) | Same skip string (`min=0.12` in message) | No |

The skip line always shows `min=0.12` in the status column; that is the **absolute floor** label, not the reason a score of 0.23 failed (dead zone vs hard skip).

### Seam patterns (within fit)

| ID | Pattern | Example scores | Tier | Listen risk |
|----|---------|----------------|------|-------------|
| **W1** | Balanced good | pre 0.6, post 0.5 | High | Low |
| **W2** | Balanced marginal | pre 0.30, post 0.32 | Marginal | Medium |
| **W3** | **Asymmetric marginal** | pre 0.28, post 1.00 | Marginal | **Echo / repeat** (C3) |
| **W4** | **Asymmetric dead zone** | pre 0.23, post 1.00 | Dead zone | Skipped (C3) |
| **W5** | Symmetric weak (common with **energy**) | pre 0.14, post 0.14 | Dead zone | Skipped |
| **W6** | Structure fail | — | `skipped: boundary alignment failed` | Skipped (C5, P6) |

---

## Layer 4 — Structure signature mode (`fit` only)

| Mode | Behavior | When to try |
|------|----------|-------------|
| **`auto`** (default) | Energy when pre/post envelope has contour (&gt;5% range); else bool | General long-form without per-gap tuning |
| **`bool`** | Active/silent bins | Talk/pause patterns; force when energy over-slides |
| **`energy`** | Log-RMS envelope Pearson | Contour-rich gaps; ambiguous bool |

Gate legacy path **always** uses bool structure. Signature mode does **not** change scan, profiles, or tier thresholds — only placement and thus `pre`/`post`.

Verbose line: `signature_mode=bool` or `signature_mode=energy` — the **resolved** tier after `auto` selection, not the config value `auto`.

---

## Layer 5 — Repair profiles and search depth

Profiles bundle haystack size, extension flags, and whether the **boundary grid** runs. Explicit CLI/TOML flags override individual fields. See [gap-fill-modes.md](gap-fill-modes.md) and [archive/repair-profiles-plan.md](archive/repair-profiles-plan.md).

**Profile flag precedence:** `--quick` and `--full` win over `--profile <name>` when combined (e.g. `--quick --profile full` → **quick**). `--quick` and `--full` are mutually exclusive. Order: TOML load → TOML `profile` bundle → CLI `--quick` / `--full` / `--profile` → per-field overrides.

| Profile | CLI | Boundary grid | `fill_border_search_secs` | Typical use |
|---------|-----|---------------|---------------------------|-------------|
| **default** | *(none)* | Off (`baseline_only`) | 10 | Interactive repair; accepts marginal baseline |
| **quick** | `--quick` | Off | 5 | Draft mux; faster; smaller B window |
| **full** | `--full` | On (`full_grid`) | 10 | Quality pass; may shift A bracket on hard gaps |

Under **`baseline_only`**, `gap_end_extend_*` flags and `gap_end_extend_max_ms` do **not** run the grid or add B haystack slack until `--full` (or `fit_boundary_search = full_grid`). `-v` may emit `repair note:` when those settings are stored but inactive.

Verbose: `fit path: baseline only` vs `fit path: boundary grid`.

---

## Vocabulary

Canonical **tag names** for gaps. Use these when writing run notes, scripts, or future tool output — they are **orthogonal** (several tags per gap), not a single “gap type” enum.

### Fact vs hint

| Kind | Meaning | Examples |
|------|---------|----------|
| **Fact** | Computed from scan, plan, patch, or seam scores | `plan_kind`, `patch_tier`, `seam_shape`, `fit_path` |
| **Hint** | Editorial guess from duration + scores + listen context | `content_hint` — never drives skip/patch by itself |
| **External** | Not observable in one repair run | P7 (`audible_not_scanned`) — compare listen vs gap table |

Prefer **facts** in automation. Treat **hints** as shorthand for the C-layer shapes in this guide.

### Tag axes

| Tag | Values | Source layer | In report today |
|-----|--------|--------------|-----------------|
| `plan_kind` | `below_scan_floor`, `unfillable`, `not_planned`, `fillable` | Plan (P0–P5) | Status column / omitted |
| `plan_skip_reason` | `not_fillable`, `outside_reference_coverage`, `track_layout_mismatch`, `track_compatibility_unavailable` | Plan (P1–P4) | `unfillable`, `not planned: …` |
| `patch_tier` | `high`, `marginal`, `dead_zone`, `hard_skip`, `structure_fail`, `not_applicable` | Fit tiers + patch (W, Layer 3) | `patched`, `!`, `skipped: …` |
| `seam_shape` | `balanced`, `asymmetric_post`, `asymmetric_pre`, `symmetric_weak`, `not_applicable` | Seam scores (W1–W5) | Derive from verbose `pre`/`post` |
| `content_hint` | `flat`, `contour`, `speech_boundary_suspected`, `long_tail` | Content shape (C1–C5) | Not emitted — guide only |
| `fit_path` | `baseline_only`, `boundary_grid` | Profile (Layer 5) | `-v` `fit path:` |
| `signature_mode` | `bool`, `energy` | Layer 4 (resolved) | `-v` `signature_mode=` |
| `patch_skip_reason` | `boundary_alignment_failed`, `correlation_below_threshold`, `b_extract_failed`, `aligned_segment_out_of_range`, `zero_length_gap` | Patch skip enum | JSON `reason`; verbose skip line |

`patch_tier` and `seam_shape` apply only when the gap reached patch with `fill_mode = fit`. Plan-only gaps use `patch_tier = not_applicable`.

**Run metadata** (corpus matrix / tuning notes — not emitted as `gap tags:` today):

| Field | Values | Use |
|-------|--------|-----|
| `signature_mode_config` | `bool`, `energy`, `auto` | TOML/CLI request before per-gap resolve |
| `gap_signature_context_secs` | e.g. `3`, `10`, `30` | Matrix column |
| `gap_report_source` | `scan_derived`, `oracle_injected` | How the gap entered patch (see [corpus-validation.md](corpus-validation.md)) |
| `fixture_scenario` | `F1`, `F2`, `F3`, `F1-long`, `F2-long`, `F3-long` | Synthetic oracle ID |
| `structure_trusted` | `true`, `false` | JSON patched outcome; structure accepted without waveform gate |

**Naming:** Guide **P0–P7** = plan-time gap types (Layer 1). Corpus acceptance IDs **EC-1–EC-6** in [TEMP-energy-corpus-plan.md](TEMP-energy-corpus-plan.md) are unrelated — always qualify which “P” you mean.

### Corpus fixtures (F1–F3)

Synthetic energy-signature oracles ([corpus-validation.md](corpus-validation.md) § Energy signature). **`fixture_scenario`** + domain oracle define the test; **vocabulary tags** describe a production-default patch run on the same WAVs.

| Scenario | Geometry | Domain oracle | Typical `content_hint` | Expected tags when patched (production default) |
|----------|----------|---------------|------------------------|--------------------------------------------------|
| **F1** / **F1-long** | Decoy dropout within border; nominal B map wrong | Energy/`auto` → true offset; bool → decoy or tie | `contour`, `decoy_duplicate`* | `plan_kind=fillable`, `signature_mode=energy`, slide ≠ 0 |
| **F2** / **F2-long** | Two pauses; nominal → pause₂, truth → pause₁ | Energy/`auto` → pause₁ | `contour`, `dual_pause`* | `plan_kind=fillable`, `signature_mode=energy`, slide ≈ 0 at pause₁ |
| **F3** / **F3-long** | Steady drone | `auto` resolves → bool | `flat` | `signature_mode=bool` (resolved) |

\*Guide-only hints for run notes — not computed by `gap_tags.rs`.

**Structure vs waveform skip:** `skipped: boundary correlation below threshold` covers both structure-below-threshold and waveform-below-threshold. Use verbose scores and JSON `min_correlation` to distinguish; structure-only failures often show `pre=0 post=0` before waveform tier runs. Tag as `patch_tier=structure_fail` only for `boundary alignment failed`; correlation skips use `dead_zone` / `hard_skip` via score bands below.

### Deriving tags from a run

**`plan_kind`**

| Condition | Tag |
|-----------|-----|
| Gap not in table, duration &lt; `min_gap_ms` | `below_scan_floor` (P0) |
| `unfillable` | `unfillable` + `plan_skip_reason` (P1–P2) |
| `not planned: …` | `not_planned` + `plan_skip_reason` (P3–P4) |
| Enters patch or `repairable` in scan-only | `fillable` (P5–P6) |
| Audible hole, no table row | `audible_not_scanned` (P7, external) |

**`patch_tier`** (after fit placement)

| Condition | Tag | Guide IDs |
|-----------|-----|-----------|
| `patched` (no `!`) | `high` | W1 |
| `! patched` | `marginal` | W2, W3 |
| `skipped: boundary correlation below threshold`, `0.12 ≤ min(pre,post) < 0.27` | `dead_zone` | W4, W5 |
| Same skip, `min(pre,post) < 0.12` | `hard_skip` | — |
| `skipped: boundary alignment failed` | `structure_fail` | W6, P6, C5 |

The skip string always shows `min=0.12`; use the **score** in verbose or JSON to separate `dead_zone` from `hard_skip`.

**`seam_shape`** (from `pre` and `post` at the winning candidate; thresholds are heuristics)

| Condition | Tag | Guide IDs |
|-----------|-----|-----------|
| Both ≥ 0.27 and \|pre − post\| ≤ 0.15 | `balanced` | W1, W2 |
| post ≥ 0.85 and post − pre ≥ 0.35 | `asymmetric_post` | W3, W4, C3 |
| pre ≥ 0.85 and pre − post ≥ 0.35 | `asymmetric_pre` | — |
| Both &lt; 0.27 and \|pre − post\| ≤ 0.10 | `symmetric_weak` | W5 |
| Structure fail or no scores | `not_applicable` | W6 |

**`content_hint`** (optional, for notes only)

| Signals | Hint | Guide IDs |
|---------|------|-----------|
| Flat bool / low contour in verbose | `flat` | C1 |
| Contour on bool or energy | `contour` | C2 |
| `asymmetric_post` + fillable duration ~1–5 s | `speech_boundary_suspected` | C3 |
| Dialog-shaped bool pattern | `contour` or `speech_boundary_suspected` | C4 |
| Very long gap or file tail + `structure_fail` | `long_tail` | C5, P6 |

### Composed examples

Short tags you can paste into run notes:

```text
# Boundary gap skipped on default profile
plan_kind=fillable patch_tier=dead_zone seam_shape=asymmetric_post
content_hint=speech_boundary_suspected fit_path=baseline_only signature_mode=bool
→ guide: P5 + C3 + W4; try --full --gap-signature-mode auto

# Marginal patch with echo risk
plan_kind=fillable patch_tier=marginal seam_shape=asymmetric_post
content_hint=speech_boundary_suspected fit_path=baseline_only
→ guide: P5 + C3 + W3; listen; consider --full

# Energy mode symmetric weak skip
plan_kind=fillable patch_tier=dead_zone seam_shape=symmetric_weak
signature_mode=energy fit_path=baseline_only
→ guide: W5; --full or scan tuning if hole missing (P7)

# Plan-time only
plan_kind=unfillable plan_skip_reason=not_fillable
patch_tier=not_applicable seam_shape=not_applicable
→ guide: P1 or P2

# Energy corpus F1-long — domain OK, patch may skip on haystack (record both)
fixture_scenario=F1-long signature_mode_config=auto gap_signature_context_secs=3 gap_report_source=scan_derived
domain=energy_finds_truth tags=plan_kind=fillable patch_tier=structure_fail patch_skip_reason=correlation_below_threshold
→ EC-1; compare with oracle_injected path (I1 pattern)
```

### ID → tag quick map

| Guide ID | Primary tags |
|----------|----------------|
| P0 | `plan_kind=below_scan_floor` |
| P1–P2 | `plan_kind=unfillable`, `plan_skip_reason=not_fillable` |
| P3 | `plan_kind=not_planned`, `plan_skip_reason=outside_reference_coverage` |
| P4 | `plan_kind=not_planned`, `plan_skip_reason=track_*` |
| P5 | `plan_kind=fillable` |
| P6 | `plan_kind=fillable`, `content_hint=long_tail`, often `patch_tier=structure_fail` |
| P7 | `audible_not_scanned` (external) |
| C1 | `content_hint=flat` |
| C2 | `content_hint=contour` |
| C3 | `content_hint=speech_boundary_suspected`, often `seam_shape=asymmetric_post` |
| C4 | `content_hint=contour` |
| C5 | `content_hint=long_tail`, `patch_tier=structure_fail` |
| W1 | `patch_tier=high`, `seam_shape=balanced` |
| W2 | `patch_tier=marginal`, `seam_shape=balanced` |
| W3 | `patch_tier=marginal`, `seam_shape=asymmetric_post` |
| W4 | `patch_tier=dead_zone`, `seam_shape=asymmetric_post` |
| W5 | `patch_tier=dead_zone`, `seam_shape=symmetric_weak` |
| W6 | `patch_tier=structure_fail` |

Tags are computed at patch time for fillable regions (preserving `fit_path` and `signature_mode`). Plan-only gaps derive tags from `status` only.

### Tool output

With **`-v`**, each fillable gap emits a line after placement:

```text
           gap tags: plan=fillable tier=dead_zone seam=asymmetric_post fit_path=baseline_only signature_mode=bool
```

Tag names and derivation rules are defined in this section; the implementation lives in `domain/gap_tags.rs`. The same `tags` object is emitted on each `GapPatchOutcome` in `--format json` output. Corpus matrix rows and tuning records: [corpus-validation.md](corpus-validation.md) § Energy signature production corpus.

---

## Recommendation matrix

Map **shape + outcome** to the next run. Start from **original** video A unless doing a deliberate second pass.

| Situation | IDs | First run | If skip or bad audio | Avoid |
|-----------|-----|-----------|----------------------|-------|
| Routine fillable gaps | P5 + W1 | `default`, `-v` | `--full` on remaining skips | — |
| Short marginal seams | P5 + W2 | `default` | Listen; `--full` if placement wrong | Lowering thresholds without listening |
| **Boundary** gap (music→speech) | C3 + W3 | `default`, `-v` | `--full`; `--gap-signature-mode auto`; ↑ `fill_repeat_penalty_weight` | `--quick` if true match is near haystack edge |
| Boundary gap, skipped | C3 + W4 | `default` | **`--full --gap-signature-mode auto`** | Patching MP4 re-scan only; widening marginal band without cause |
| Symmetric weak (energy) | W5 | `--gap-signature-mode auto` | `--full`; tune scan if hole not in report (P7) | Expecting bool-style `post=1.0` fix |
| Long tail / huge gap | P6 + C5 + W6 | Expect skip | Manual edit; do not run `--full` on multi-minute gaps | `--full` on 200 s+ gaps (hours) |
| Pre-overlap on A | P1 | Ignore | — | Patching |
| Clip drift on long form | P5 (many) | `fill_offset=interpolated` if drift ≥ ~0.05 s | `anchored-retry` after some High patches | `interpolated` when drift tiny |
| Offset map wrong near gap edge | P5, high slide in verbose | `anchored-retry` | Pass 2 after easy gaps patch High | — |
| Second pass on repaired file | P7 risk | Only if intentional | Re-scan with `-v`; compare gap count | Treating as same as first pass |
| Legacy strict gating | — | `--fill-mode gate` | Extension retries; structure trust options | Expecting fit tiering |

---

## Decision flow (`fill_mode = fit`)

```text
In gap table?
  no  → P0 or P7 (scan tuning / source)
  yes → not planned / unfillable?
          yes → P1–P4 (fix input or alignment)
          no  → patch result:
                  structure alignment failed → P6 / C5 / W6
                  skipped correlation:
                    min(pre,post) < 0.12     → hard skip
                    0.12 ≤ min < 0.27       → dead zone → --full, auto/energy
                  patched !                 → W2/W3 → listen (W3 echo risk)
                  patched (no !)            → W1 → done
```

---

## Reading verbose output

| Line | Meaning |
|------|---------|
| `repair profile: …` | Effective profile and `fit_boundary_search` |
| `repair note: …` | Flags stored but inactive this run (see [gap-fill-modes.md](gap-fill-modes.md)) |
| `signature_mode=` | Effective structure tier (`bool` / `energy`) |
| `B search window:` | B haystack; width ∝ `fill_border_search_secs` + context/margins |
| `structure slide` / `waveform slide` | B placement vs nominal map |
| `fit path:` | `baseline only` (default/quick) vs `boundary grid` (`--full`) |
| `gap tags:` | Composed vocabulary tags (`plan`, `tier`, `seam`, `fit_path`, `signature_mode`) — see [gap-repair-guide.md](gap-repair-guide.md) § Vocabulary |

Full column semantics: [cli-output.md](cli-output.md).

---

## Tuning knobs (quality tradeoffs)

Use only when the recommendation matrix is insufficient. Lower floors accept weaker seams.

| Knob | Default | Effect |
|------|---------|--------|
| `min_fill_correlation` | 0.35 | High tier floor; also caps effective absolute floor |
| `fill_marginal_margin` | 0.08 | Width of marginal band (default 0.27–0.35) |
| `fill_absolute_floor` | 0.12 | Hard skip below this `min(pre, post)` |
| `fill_repeat_penalty_weight` | 0.4 | Down-rank repeat-at-border when seams weak (fit) |
| `fill_border_search_secs` | 10 | B slide radius — larger = more CPU, helps edge-clamped matches |
| `gap_signature_context_secs` | 3.0 | Structure context; raise for ambiguous long gaps |
| Scan: `silence_fraction`, `absolute_silence_rms` | 0.01, 33 | Affects P5 vs P7 |

---

## Related documentation

| Doc | Contents |
|-----|----------|
| [gap-fill-modes.md](gap-fill-modes.md) | `fit` vs `gate`, flag × mode matrix, extension, profiles, performance recipes |
| [cli-output.md](cli-output.md) | Progress, gap table, skip reason strings |
| [json-output.md](json-output.md) | `GapPatchStatus`, `confidence`, machine-readable outcomes |
| [corpus-validation.md](corpus-validation.md) | Corpus tiers, energy-signature oracles, vocabulary matrix rows |
| [TEMP-energy-corpus-plan.md](TEMP-energy-corpus-plan.md) | F1/F2-long synthetic tuning (EC-* acceptance) |
| [README.md](../README.md) § Gap patching | Short pipeline overview |
