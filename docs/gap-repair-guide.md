# Gap repair guide — classifying gaps and choosing settings

Operational guide for `clip-sync-repair`: what kinds of gaps exist, how they appear in the report, and which profiles and flags are worth trying. This is the **operator-decision lens** on **phase 4 (per-gap patch)** of the [repair pipeline](pipeline.md).

**Normative reference** (flag matrix, pipelines, config defaults): [gap-fill-modes.md](gap-fill-modes.md).  
**Report layout and skip strings**: [cli-output.md](cli-output.md) § Repair gap outcomes.  
**JSON fields**: [json-output.md](json-output.md).

---

## How to use this guide

1. Run repair with **`-v`** on the **original** source for video A (not a previously muxed repair unless you intend a second pass).
2. Read the **gap table** and verbose per-gap lines (`pre`/`post`, `signature_mode`, `fit path`).
3. Match the gap to a **stage** and **shape** below (or read the composed **[Vocabulary](#vocabulary)** tags).
4. Apply the **recommendation** row; use [gap-fill-modes.md](gap-fill-modes.md) for exact flag names and TOML keys.

Thresholds below use **production defaults** unless noted: `min_fill_correlation = 0.35`, `fill_marginal_margin = 0.08`, `fill_absolute_floor = 0.12`, `fill_mode = fit`, `dual_fit = true`, repair profile **`default`** (`fit_boundary_search = baseline_only`, `fill_border_search_secs = 10`).

---

## Pipeline stages

Every gap passes through three layers. The “type” depends on where it stops.

```text
Scan (silence on A)
  → Fill plan (fillable? — `b_has_energy`, coverage, tracks)
    → Patch (structure + waveform placement → dual-fit rescue?)
      → Patched / skipped / not planned
```

| Stage | Question | Outcomes in report |
|-------|----------|-------------------|
| **Scan** | Is A silent for ≥ `min_gap_ms` (default 1 s)? | Omitted if too short |
| **Plan** | Is B mapped and energetic? Tracks OK? In query coverage? | `unfillable`, `not planned: …` |
| **Patch — bracket** | Structure finds B bracket? Waveform tier OK? | `patched`, `skipped: …` |
| **Patch — G6** | Dual-fit rescue (default on) after scored gate skip? | May upgrade skip → `patched` |

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

**Program-quiet at plan time:** when B is also silent at the mapped span (`b_has_energy = false`, **P2**), the gap is **unfillable** — both masters quiet, nothing to copy. The fingerprint analyzer may additionally label skipped gaps as program-quiet for metrics (`donor_interior_nominal`); that label does **not** short-circuit the patch path.

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

### What the skip `pre` / `post` scores mean

Fit mode tries several placements in order: **baseline** (scan throat) → **anchor** brackets (when anchor seam runs) → **boundary grid** cells (when `--full` / `full_grid`). Gate mode may also **extend** the gap edges and re-score.

| Field | Meaning |
|-------|---------|
| **`pre` / `post`** | Waveform Pearson at the **first** placement that recorded the skip (usually baseline throat). Structure-only skips use structure envelope scores instead (`pre=0 post=0` is common). |
| **`min`** | The threshold checked at that failure — `fill_absolute_floor` (**0.12**) for waveform skips, `min_structure_match_score` for structure skips. **Not** `min_fill_correlation` (0.35). |
| **`best pre=… post=… @ …`** | When a later placement scored higher, the **best** `min(pre, post)` seen across baseline / anchor / grid / extension, and which step found it (`baseline`, `anchor`, `grid`, `extension`). Omitted when nothing beat the reported scores. |
| **`[tier · seam]` suffix** | Derived from the reported `pre`/`post` (first failure), not from `best`. |

Tags and tier on a skip row describe the **throat (first) failure**. Use `best` when tuning thresholds or deciding whether `--full` / `--anchor-seam-mode auto` might help.

**Tuning with the scores:** classification always uses `min(pre, post)` on the **winning** candidate (both seams must hold). Three knobs:

| Knob | Default | What it changes |
|------|---------|-----------------|
| `min_fill_correlation` | 0.35 | High tier floor; marginal band top (`min_fill_correlation − fill_marginal_margin` = **0.27**); also caps the effective hard floor |
| `fill_marginal_margin` | 0.08 | Marginal band width (default **0.27–0.35**) |
| `fill_absolute_floor` | 0.12 | Hard skip below this `min(pre, post)` |

Lowering `--min-fill-correlation` only helps skips whose **`best min(pre, post)`** falls in the dead zone or marginal band (e.g. `best` at 0.22 → try ~0.30 to marginal-patch). **Hard skips** (`best` &lt; 0.12) need `--fill-absolute-floor`, better placement (`--full`, anchor seam), or alignment — not `min_fill_correlation` alone.

For per-bracket fall-through detail without re-running repair, use `--gap-fingerprints` (see [gap-fingerprint.md](gap-fingerprint.md)).

### Seam patterns (within fit)

> How `pre`/`post` are built (border templates, channel selection, peak-normalized Pearson, windows): [seam-scoring.md](seam-scoring.md).

| ID | Pattern | Example scores | Tier | Listen risk |
|----|---------|----------------|------|-------------|
| **W1** | Balanced good | pre 0.6, post 0.5 | High | Low |
| **W2** | Balanced marginal | pre 0.30, post 0.32 | Marginal | Medium |
| **W3** | **Asymmetric marginal** | pre 0.28, post 1.00 | Marginal | **Echo / repeat** (C3) |
| **W4** | **Asymmetric dead zone** | pre 0.23, post 1.00 | Dead zone | Skipped (C3) |
| **W5** | Symmetric weak (common with **energy**) | pre 0.14, post 0.14 | Dead zone | Skipped |
| **W6** | Structure fail | — | `skipped: boundary alignment failed` | Skipped (C5, P6) |
| **W7** | **Bracket-exhausted → dual-fit** | Throat skip, but shoulders viable at own lag | Was skip; may become **High** after G6 | Low when rescued |

**Surround (5.1) note:** seam Pearson follows the channel(s) carrying signal (within ~20 dB of the loudest), not a fixed front L/R pair — so a **center-dominant 5.1 mix** is scored on its center channel. Before this, near-silent front channels produced noise-correlation `pre/post ≈ 0` (a false **W5**) and skipped fillable gaps. If you still see persistent symmetric-weak seams on surround content, confirm the content isn't genuinely near-silent across *all* channels (then the skip is correct). See [gap-fill-modes.md](gap-fill-modes.md) § Multichannel seams.

### Editorial anchor seam (W5 rescue)

When the **scan throat** is quiet but salient audio exists nearby (speech peaks, bool onsets in the **flanking context**), throat-only Pearson can land in **W5** (`symmetric_weak`, dead zone) even though a better editorial cut exists. **Editorial anchor seam** searches A-side anchor candidates (energy peaks, bool transitions, scan fallback), pairs feasible brackets, and re-scores waveform matchability at those anchors. Candidate search uses the same pre/post windows as structure signatures — see [gap-fill-modes.md](gap-fill-modes.md#signature-context-and-contour-geometry).

**Not the same as [patch anchors](gap-fill-modes.md#patch-anchors)** (`anchored_retry`): patch anchors fix **clip offset drift** between passes; anchor seam fixes **where on A/B the seam is measured** for one gap.

| `anchor_seam_mode` | Behavior |
|--------------------|----------|
| **`off`** (default) | Throat-only seam scoring; no anchor bracket search |
| **`auto`** | Run when baseline throat `min(pre, post) < min_fill_correlation - fill_marginal_margin` **and** the gap signature has contour in the flanking context halves ([gap-fill-modes.md](gap-fill-modes.md#signature-context-and-contour-geometry) § Signature context and contour geometry) |
| **`force`** | Always try anchor bracket search before the boundary grid; under `baseline_only`, defers accepting a **marginal** baseline (E2) so anchor can run first (diagnostics / oracles) |

CLI: `--anchor-seam-mode auto|force|off`. TOML: `anchor_seam_mode = "auto"` (default). Fit mode only; `-v` emits `repair note: anchor_seam_mode=off: …` when explicitly set to off.

**Outcomes when anchor search wins:**

- `anchor_seam_used = true` on patched gaps (JSON `status` + `tags`; verbose `anchor_seam=true`)
- `anchor_bracket_move_frames` — how far the bracket moved from scan-refined edges (verbose `anchor_move_frames=N`)
- Human status: `patched (anchor pre→post)` (any confidence) or `! patched (anchor …)` when marginal
- `patch_tier = anchor_trusted` when structure is strong at both anchors but throat Pearson is in the marginal band — see § `anchor_trusted` below

Anchor seam does **not** require `--full`; it runs under `baseline_only` when triggered. Tier-2 xcorr (ambiguous Pearson) reuses `residual_lag_secs` for max lag. Residual veto still applies (F4 decoy must skip).

### Dual-fit rescue (W7)

**Dual-fit (G6, W7)** runs when bracket search returns a **scored skip** other than structure alignment
failed. Default **`dual_fit = true`**. The rescue path:

1. Finds seam-local peaks on A pre/post shoulders.
2. Registers each shoulder on B at its own lag (not lag-0 bracket search).
3. Trims the donor interior to reconcile length.
4. Re-validates the assembled fill with the **unchanged** gate floors.

Typical W7 gaps: silence splices where `min(pre, post)` failed at the throat but both shoulders align
strongly at independent lags (see golden dual-fit targets in the status ledger §4). Dual-fit **declines**
donor-broken bridges and program-quiet donors — those remain skips.

| Situation | Action |
|-----------|--------|
| Gap `unfillable` / `b_has_energy = false` | Expected shared pause (P2). Not a patch failure. |
| Gap skips `boundary correlation below threshold` after `--full` | May be W7 candidate already attempted; listen. If still skip, donor may be broken or one-shoulder-dead. |
| Reproduce pre-A3 bracket-only behavior | `--no-dual-fit` |
| Force dual-fit on when disabled in TOML | `--dual-fit` |

Details: [gap-fill-modes.md](gap-fill-modes.md) § Program-quiet (D11) / Dual-fit rescue.

---

## Layer 4 — Structure signature mode (`fit` only)

| Mode | Behavior | When to try |
|------|----------|-------------|
| **`auto`** (default) | Energy when pre/post envelope has contour (&gt;5% peak-normalized range in flanking context); else bool — see [gap-fill-modes.md](gap-fill-modes.md#signature-context-and-contour-geometry) | General long-form without per-gap tuning |
| **`bool`** | Active/silent bins | Talk/pause patterns; force when energy over-slides |
| **`energy`** | Log-RMS envelope Pearson | Contour-rich gaps; ambiguous bool |

Gate legacy path **always** uses bool structure. Signature mode does **not** change scan, profiles, or tier thresholds — only placement and thus `pre`/`post`.

Verbose line: `signature_mode=bool` or `signature_mode=energy` — the **resolved** tier after `auto` selection, not the config value `auto`.

**Mode-coupled nominal bias.** When a gap resolves to **energy**, the search uses a lower distance-from-nominal penalty (`fill_fit_energy_nominal_bias_scale`, default **0.25**) than bool-resolved gaps (`fill_fit_nominal_bias_scale`, default **1.0**). An energy match is the signal that the alignment-supplied nominal B map may be wrong, so a confident energy contour is allowed to slide further off the nominal to the true pause — energy mode **self-corrects a drifted nominal map** without you touching the base bias. The penalty grows with distance, so this only loosens far-off (seconds of drift) candidates; sub-second offsets place the same either way. To restore the old hard anchoring for energy gaps, raise `fill_fit_energy_nominal_bias_scale` toward `1.0`. Both are config-only (no CLI flag).

**Context length (`gap_signature_context_secs`).** Keep the **3 s** default. Raising it (10 / 30 s) widens the envelope/bool window matched on each side of the gap — in principle more disambiguating for hard gaps where 3 s of context aliases across several candidates — but the synthetic corpus matrix (contexts 3 / 10 / 30) showed **no measurable patch benefit**, and a longer context decodes and holds more B audio per gap (slower, more memory). Treat it as a manual knob to try on a specific stubborn drift gap, **not** a default to raise. CLI: `--gap-signature-context-secs`. **Where** contour is measured (pre/post halves, gap interior excluded, 50 ms bins): [gap-fill-modes.md](gap-fill-modes.md#signature-context-and-contour-geometry).

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
| `patch_tier` | `high`, `marginal`, `anchor_trusted`, `dead_zone`, `hard_skip`, `structure_fail`, `not_applicable` | Fit tiers + patch (W, Layer 3) | Gap table ` [tier · seam]` suffix; `patched`, `!`, `patched (anchor …)`, `skipped: …` |
| `seam_shape` | `balanced`, `asymmetric_post`, `asymmetric_pre`, `symmetric_weak`, `not_applicable` | Seam scores (W1–W5) | Gap table suffix (`post-strong`, `weak both sides`, …); `-v` `gap tags:` |
| `content_hint` | `flat`, `contour`, `speech_boundary_suspected`, `long_tail` | Content shape (C1–C5) | Not emitted — guide only |
| `fit_path` | `baseline_only`, `boundary_grid` | Profile (Layer 5) | `-v` `fit path:` |
| `signature_mode` | `bool`, `energy` | Layer 4 (resolved) | `-v` `signature_mode=` |
| `residual_band` | `cancels`, `correlates_only`, `no_floor` | Residual/floor headroom (fit mode) | `-v` `residual_band=`; JSON `tags` |
| `anchor_seam_used` | `true` (omitted when false) | Fit mode: winning placement used an editorial anchor bracket, not scan throat alone | `-v` `anchor_seam=true`; JSON `status.patched` + `tags` |
| `anchor_bracket_move_frames` | integer (omitted when 0) | Total frame displacement of anchor bracket from scan-refined baseline | `-v` `anchor_move_frames=`; JSON `status.patched` + `tags` |
| `patch_skip_reason` | `boundary_alignment_failed`, `correlation_below_threshold`, `b_extract_failed`, `aligned_segment_out_of_range`, `zero_length_gap`, `residual_headroom_exceeded` | Patch skip enum | JSON `reason`; verbose skip line |

`patch_tier` and `seam_shape` apply only when the gap reached patch with `fill_mode = fit`. Plan-only gaps use `patch_tier = not_applicable`.

**Run metadata** (corpus matrix / tuning notes — not emitted as `gap tags:` today):

| Field | Values | Use |
|-------|--------|-----|
| `signature_mode_config` | `bool`, `energy`, `auto` | TOML/CLI request before per-gap resolve |
| `gap_signature_context_secs` | e.g. `3`, `10`, `30` | Matrix column |
| `gap_report_source` | `scan_derived`, `oracle_injected` | How the gap entered patch (see [corpus-validation.md](corpus-validation.md)) |
| `fixture_scenario` | `F1`, `F2`, `F3`, `F1-long`, `F2-long`, `F3-long` | Synthetic oracle ID |
| `structure_trusted` | `true`, `false` | JSON patched outcome; structure accepted without waveform gate (gate mode) |
| `anchor_seam_used` | `true`, `false` | JSON + `-v` tags; editorial anchor bracket won (fit mode) |
| `anchor_bracket_move_frames` | integer | JSON + `-v` tags; bracket displacement from scan-refined baseline |
| `anchor_trusted` | via `patch_tier=anchor_trusted` | Fit mode: strong structure at editorial anchors, throat Pearson below `min_fill_correlation` but patch accepted | Gap table `patched (anchor …)` + ` [anchor trusted · seam]`; JSON `tags.patch_tier` |
| `donor_relation` | `same_master`, `mixed`, `diff_capture` | Run-level: fraction of gaps with informative floors (≥70% → `same_master`) | JSON `patch.donor_relation`; patch summary header |

**Naming:** Guide **P0–P7** = plan-time gap types (Layer 1). Corpus acceptance IDs **EC-1–EC-6** in [energy-corpus-plan.md](archive/energy-corpus-plan.md) are unrelated — always qualify which “P” you mean.

### Corpus fixtures (F1–F3)

Synthetic energy-signature oracles ([corpus-validation.md](corpus-validation.md) § Energy signature). **`fixture_scenario`** + domain oracle define the test; **vocabulary tags** describe a production-default patch run on the same WAVs.

| Scenario | Geometry | Domain oracle | Typical `content_hint` | Expected tags when patched (production default) |
|----------|----------|---------------|------------------------|--------------------------------------------------|
| **F1** / **F1-long** | Decoy dropout within border; nominal B map wrong | Energy/`auto` → true offset; bool → decoy or tie | `contour`, `decoy_duplicate`* | `plan_kind=fillable`, `signature_mode=energy`, slide ≠ 0 |
| **F2** / **F2-long** | Two pauses; nominal → pause₂, truth → pause₁ | Energy/`auto` → pause₁ | `contour`, `dual_pause`* | `plan_kind=fillable`, `signature_mode=energy`, slide ≈ 0 at pause₁ |
| **F3** / **F3-long** | Steady drone | `auto` resolves → bool | `flat` | `signature_mode=bool` (resolved) |

\*Guide-only hints for run notes — not computed by `gap_tags.rs`.

**Structure vs waveform skip:** `skipped: boundary correlation below threshold` covers both structure-below-threshold and waveform-below-threshold. Use verbose scores and JSON `min_correlation` to distinguish; structure-only failures often show `pre=0 post=0` before waveform tier runs. Tag as `patch_tier=structure_fail` only for `boundary alignment failed`; correlation skips use `dead_zone` / `hard_skip` via score bands below. When waveform search tried multiple placements, the skip line may add `best pre=… post=… @ anchor|grid|…` — see Layer 3 § What the skip `pre` / `post` scores mean.

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
| `patched (anchor …)` — editorial anchor seam, strong structure, `min(pre,post) < min_fill_correlation` | `anchor_trusted` | W5 rescue |
| `skipped: boundary correlation below threshold`, `0.12 ≤ min(pre,post) < 0.27` | `dead_zone` | W4, W5 |
| Same skip, `min(pre,post) < 0.12` | `hard_skip` | — |
| `skipped: boundary alignment failed` | `structure_fail` | W6, P6, C5 |

**`anchor_trusted` (fit mode only):** Distinct from gate-mode `structure_trusted`. Applies when `anchor_seam_mode` finds a bracket with strong structure scores at both anchors, B-side anchor matchability passes, and waveform Pearson at the **anchor windows** is in the marginal band (`≥ min_fill_correlation - fill_marginal_margin` but `< min_fill_correlation`) — i.e. a throat-only read would be W5/dead zone, but the editorial cut is accepted. Requires `anchor_seam_used` on the winning candidate. Residual veto still applies; F4 decoy must skip.

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
# Symmetric weak at throat, rescued by anchor seam (energy)
plan_kind=fillable patch_tier=anchor_trusted seam_shape=symmetric_weak
content_hint=contour fit_path=baseline_only signature_mode=energy anchor_seam_mode=auto
→ guide: W5 throat skip avoided; status `patched (anchor 0.15→0.14) [anchor trusted · weak both sides]`; not gate-mode `structure_trusted`

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
| W5 | `patch_tier=dead_zone`, `seam_shape=symmetric_weak` — or `patch_tier=anchor_trusted` when anchor seam rescues |
| W6 | `patch_tier=structure_fail` |
| W7 | Was `patch_tier=dead_zone` or correlation skip; may become `patch_tier=high` after dual-fit (default on) |

Tags are computed at patch time for fillable regions (preserving `fit_path` and `signature_mode`). Plan-only gaps derive tags from `status` only.

### Tool output

With **`-v`**, each fillable gap emits a line after placement:

```text
           gap tags: plan=fillable tier=anchor_trusted seam=symmetric_weak fit_path=baseline_only signature_mode=energy anchor_seam=true anchor_move_frames=1200
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
| Symmetric weak (energy) | W5 | `default`, `-v` (anchor `auto` on by default) | `--full`; tune scan if hole not in report (P7) | Expecting bool-style `post=1.0` fix; anchor rescue needs salient contour in flanking context (default ±3 s from each gap edge, not inside the hole) — [gap-fill-modes.md](gap-fill-modes.md#signature-context-and-contour-geometry) |
| Long tail / huge gap | P6 + C5 + W6 | Expect skip | Manual edit; do not run `--full` on multi-minute gaps | `--full` on 200 s+ gaps (hours) |
| Bracket-exhausted silence splice | W7 | Default run (`dual_fit` on) | `--full` if throat weak but not bracket-exhausted; `--no-dual-fit` only for regression | Lowering floors without listening |
| Shared pause (both masters quiet) | P2 | `unfillable` at plan — expected | Fix alignment / source if misclassified | `--full`, `--min-fill-correlation` |
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
                    min(pre,post) < 0.12     → hard skip (or W7 if dual-fit rescues)
                    0.12 ≤ min < 0.27       → dead zone → --full, auto/energy; dual-fit may rescue (W7)
                  patched !                 → W2/W3 → listen (W3 echo risk)
                  patched (no !)            → W1 or W7 rescue → done
                  patched (anchor …)        → W5 rescue or anchor_trusted → listen
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
| `gap tags:` | Composed vocabulary tags (`plan`, `tier`, `seam`, `fit_path`, `signature_mode`, `anchor_seam`, `anchor_move_frames`) — see [gap-repair-guide.md](gap-repair-guide.md) § Vocabulary |

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
| `anchor_seam_mode` | `auto` | `force` for diagnostics; `off` to disable editorial anchor search |
| `max_anchor_bracket_secs` | 5.0 | Max span between pre/post editorial anchors |
| `anchor_seam_min_match_pearson` | 0.12 | B-side anchor matchability Pearson floor |
| `dual_fit` | `true` | G6 per-shoulder rescue after scored gate skip; `--no-dual-fit` to disable |
| Scan: `silence_fraction`, `absolute_silence_rms` | 0.01, 33 | Affects P5 vs P7 |
| Output bit depth | automatic | `--wav` output is 24-bit int when A's source track is 24/32-bit or float; otherwise 16-bit int. Lossy sources (AAC, AC-3) have no detectable depth → always 16-bit. Shown in track info as `(decodable, 16-bit out)` / `(decodable, 24-bit out)`. See [pipeline.md](pipeline.md) § 5. |

---

## Related documentation

| Doc | Contents |
|-----|----------|
| [gap-fill-modes.md](gap-fill-modes.md) | `fit` vs `gate`, flag × mode matrix, extension, profiles, performance recipes |
| [cli-output.md](cli-output.md) | Progress, gap table, skip reason strings |
| [json-output.md](json-output.md) | `GapPatchStatus`, `confidence`, machine-readable outcomes |
| [corpus-validation.md](corpus-validation.md) | Corpus tiers, energy-signature oracles, vocabulary matrix rows |
| [energy-corpus-plan.md](archive/energy-corpus-plan.md) | F1/F2-long synthetic tuning (EC-* acceptance) |
| [README.md](../README.md) § Gap patching | Short pipeline overview |
