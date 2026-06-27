# W5 anchor-rescue fixture discovery — plan (DRAFT)

Status: **in progress** — routing proven (`route_w5_anchor_rescue_e3`); domain oracle green
(`w5_fixture_throat_symmetric_weak_and_brackets_exist`); pipeline A6 `#[ignore]`; probe slated for
replacement by diagnostic tier.

Companions: [TEMP-anchor-seam-plan.md](TEMP-anchor-seam-plan.md) §7 rows **A6/A6b**,
[archive/fit-routing-extraction-plan.md](archive/fit-routing-extraction-plan.md) §8 (fixture-bound
measurement vs `ScriptedFitSource`), [development.md](development.md) § Tier decision rule,
[gap-repair-guide.md](gap-repair-guide.md) § W5 / Editorial anchor seam.

---

## 1. Problem (one paragraph)

Fit-joint **routing** for W5 anchor rescue is proven: `ScriptedFitSource` + `FitJointConfig` drive
`evaluate_seam_gate_fit_joint_core` and assert exit **E3** (`route_w5_anchor_rescue_e3` in
`patch_region.rs`). What remains open is **measurement**: does real PCM, scored through
`AudioFitSource` (`evaluate_seam_gate_fit_candidate`, unified haystack search, anchor brackets),
produce a score table that makes E3 win with `anchor_seam_used=true`?

The synthetic fixture `build_w5_symmetric_weak_throat_anchor_rescue` (F1-style B shift + silent nominal
gap) plus `w5_anchor_rescue_repair` (`fill_border_search_secs` < `peak_offset_secs`) is a controlled
lab for that question. Tuning is a **low-dimensional, non-smooth** search (thresholds at 0.12 / 0.25 /
0.35, discrete unified slides, routing switches) — not a fine linear walk. We need a **diagnostic**
that maps `(peak_offset, fill_border_search)` into **regimes** and refines around behavioral
boundaries, then **locks one cell** in integration (`anchor_seam_oracle` pipeline A6).

---

## 2. What this plan is / is not

| In scope | Out of scope |
|----------|----------------|
| Parameterized W5 fixture + repair helpers | Changing `ScriptedFitSource` / E1–E7 precedence |
| Per-cell seam scores (nominal, baseline, brackets) | Production default `RepairConfig` |
| Diagnostic CSV / stderr regime map | Full `integration_gap_corpus` W5 row asserting `anchor_seam_used` |
| Coarse grid + boundary refine | Gradient descent / closed-form geometry |
| Lock one config → un-ignore A6 pipeline | Replacing scan or anchor candidate algorithms |

**Tier:** **diagnostic** (emit data; no PR gate). **Not validation** — no external ffmpeg/corpus
contract. **Not integration** until a single `(offset, search)` is frozen and asserted.

---

## 3. Layers (do not conflate)

```text
ScriptedFitSource     →  "Given scores, does routing pick anchor?"     DONE (route_w5_*)
AudioFitSource        →  "Does PCM produce those scores?"              THIS PLAN
PatchAudio (A6)       →  "Does full patch set anchor_seam_used?"       After lock-in
```

---

## 4. Regimes (classifier target)

For each grid cell `(peak_offset_secs, fill_border_search_secs)` with `search < offset`:

| Regime | Baseline (unified @ throat) | Best bracket `min(pre,post)` | Routing interest |
|--------|----------------------------|------------------------------|------------------|
| `Invalid` | — | — | `search >= offset` |
| `BaselineHigh` | High (≥ 0.35) | any | E1 — anchor irrelevant |
| `BaselineMarginalWins` | Marginal, wins pool | < 0.35 or loses rank | E2 — patched, no anchor |
| `AnchorRescuePossible` | not High | ≥ 0.35 | **E3 pocket** — integration target |
| `AllSkip` | below floors | < 0.35 | E5 — no patch |

Auto mode also needs `min(baseline_pre, baseline_post) < min_fill - margin` (currently 0.25 with
`min_fill=0.35`, `margin=0.10`) for anchor search to run.

**Search objective (discovery):** maximize `max_bracket_min - 0.35` subject to baseline not High and
nominal symmetric-weak (`min < 0.27`, `|pre-post| ≤ 0.10`). Refine only where coarse neighbors
**change regime**.

---

## 5. Implementation phases

### Phase 1 — Single-cell diagnostic (replaces `probe_w5`)

**Goal:** Same utility as today's `probe_w5_anchor_rescue_scores`, correct tier, reusable API for
Phase 2.

| # | Task | Location |
|---|------|----------|
| 1.1 | `build_w5_symmetric_weak_throat_anchor_rescue(…, peak_offset_secs, fill_border_search_secs)` — sync `structure_params.search_radius_frames` with search | `test_support/energy_signature_fixtures.rs` |
| 1.2 | `w5_anchor_rescue_repair(…, fill_border_search_secs)` or builder that sets repair from cell | `test_support/energy_signature_production.rs` |
| 1.3 | `W5AnchorRescueCellScores` + `score_w5_anchor_rescue_cell(fixture, repair) → { nominal, baseline, brackets[] }` | `test_support/w5_anchor_rescue_diag.rs` (new) |
| 1.4 | Per-bracket scoring via same path as gate (unified match at `bracket.refined` or thin wrapper) | reuse `preview_patch_geometry` / `evaluate_seam_gate_fit_candidate` inputs |
| 1.5 | `tests/diag_w5_anchor_rescue.rs` — `w5_anchor_rescue_single_cell` prints human-readable row | new `[[test]]` + `diagnostic-tests` |
| 1.6 | Remove `probe_w5_anchor_rescue_scores` from `anchor_seam_oracle.rs` | integration binary |
| 1.7 | `Cargo.toml` `[[test]]`, `development.md` matrix row, `test-tier.ps1 -Tier diagnostic` | wiring |

**Run:**

```powershell
cargo test -p clip-sync-repair --features diagnostic-tests --test diag_w5_anchor_rescue -- --nocapture
```

### Phase 2 — Regime map (coarse + boundary refine)

| # | Task | Location |
|---|------|----------|
| 2.1 | `enum W5AnchorRescueRegime` + `classify_w5_cell(scores) -> W5AnchorRescueRegime` | harness or `test_support` |
| 2.2 | `coarse_w5_grid(offset_range, search_range, step) -> Vec<W5SweepCell>` | `clip-sync-repair-harness/src/w5_anchor_rescue_sweep.rs` (new) |
| 2.3 | `refine_w5_boundaries(cells) -> Vec<W5SweepCell>` — subdivide only where regime ≠ neighbor | same |
| 2.4 | `write_w5_sweep_csv(cells, path)` optional | same |
| 2.5 | `diag_w5_anchor_rescue_coarse_grid`, `diag_w5_anchor_rescue_refine_boundaries` tests | `tests/diag_w5_anchor_rescue.rs` |

Default coarse suggestion: `peak_offset ∈ [0.6, 1.1]` step 0.05 s; `search ∈ [0.65, 0.85]` step 0.02 s;
skip invalid half-plane.

**Performance:** ~5–10 s per cell (baseline unified dominates). Coarse grid ~50–100 valid cells →
~10–15 min local; refine only on boundaries.

### Phase 3 — Lock integration

| # | Task | Location |
|---|------|----------|
| 3.1 | Pick cell from Phase 2 CSV (`AnchorRescuePossible`, best `max_bracket_min`) | human |
| 3.2 | Freeze defaults in fixture builder + `w5_anchor_rescue_repair` | `test_support` |
| 3.3 | Un-ignore `w5_anchor_rescue_pipeline_engages_anchor_seam_{auto,force}` | `anchor_seam_oracle.rs` |
| 3.4 | Update [TEMP-anchor-seam-plan.md](TEMP-anchor-seam-plan.md) §7 A6 row; archive or mark this plan **done** | docs |

Optional: one pipeline run per finalist only (not per grid cell).

---

## 6. File layout (target)

```text
crates/clip-sync-repair-harness/src/
  w5_anchor_rescue_sweep.rs          # Phase 2: grid, regime, CSV

crates/clip-sync-repair/src/test_support/
  w5_anchor_rescue_diag.rs           # Phase 1–2: score_w5_cell, types
  energy_signature_fixtures.rs       # parameterized build_w5_…
  energy_signature_production.rs       # repair helper per cell

crates/clip-sync-repair/tests/
  diag_w5_anchor_rescue.rs           # diagnostic binary
  anchor_seam_oracle.rs              # domain + locked pipeline only

docs/
  TEMP-w5-anchor-rescue-diag-plan.md # this file
```

Pattern to copy: [diag_anchor_seam.rs](../crates/clip-sync-repair/tests/diag_anchor_seam.rs),
[diag_energy_matrix.rs](../crates/clip-sync-repair/tests/diag_energy_matrix.rs) (`f4_decoy_weight_sweep`).

---

## 7. Validation checklist (when Phase 3 lands)

| Check | Expect |
|-------|--------|
| `w5_fixture_throat_symmetric_weak_and_brackets_exist` | nominal symmetric-weak; movable brackets |
| `w5_anchor_rescue_pipeline_engages_anchor_seam_auto` | `patched`, `anchor_seam_used`, `anchor_move_nonzero` |
| `w5_anchor_rescue_pipeline_engages_anchor_seam_force` | same |
| `route_w5_anchor_rescue_e3` | unchanged — routing still independent |
| `gap_corpus_w5_anchor_seam` | still patched (may remain `anchor_seam_used=false` until corpus geometry updated) |

Document locked `(peak_offset, fill_border_search)` in fixture module doc comment.

---

## 8. Open questions

1. **Independent B shift** — should `b_shift_secs` diverge from A `peak_offset_secs` if post seam
   stays weak at anchor bracket?
2. **Chirp bed on silence** — W5 corpus uses quiet chirp for scan; add to A6 fixture for structure
   without helping baseline High?
3. **Soft CI gate** — optional `#[ignore]` test “coarse grid reports ≥1 E3 pocket” (diagnostic only,
   never PR)?

---

## 9. Related reading

| Doc | Contents |
|-----|----------|
| [TEMP-anchor-seam-plan.md](TEMP-anchor-seam-plan.md) | A6/A6b validation rows, fixture id |
| [archive/fit-routing-extraction-plan.md](archive/fit-routing-extraction-plan.md) | §8 fixture-bound; §10 W5+anchor matrix |
| [development.md](development.md) | Diagnostic tier, `diag_*` binaries |
| [test-acceptance-glossary.md](test-acceptance-glossary.md) | Acceptance IDs (A6 not yet listed) |
