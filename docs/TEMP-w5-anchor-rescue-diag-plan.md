# W5 anchor-rescue fixture discovery — plan (DRAFT)

Status: **in progress** — routing proven (`route_w5_anchor_rescue_e3`); domain oracle green
(`w5_fixture_throat_symmetric_weak_and_brackets_exist`); pipeline A6 `#[ignore]`. **Phase 0 done**
(`SeamGateConfig`/`SeamGateGeometry` extraction). **Phase 1 done** (`diag_w5_anchor_rescue` /
`w5_anchor_rescue_single_cell`; probe removed). **Phase 2 done** (`coarse_w5_grid` /
`diag_w5_anchor_rescue_coarse_grid`) — **E3 pocket EMPTY in `(offset, search)`** (see §5.2.8 result);
**Phase 3 blocked** pending §8 Q1 (independent B shift). Next actionable: decouple `b_shift_secs`
from `peak_offset_secs` and re-sweep.

**Phase 0 prerequisite (do first):** extract the inline `SeamGateParams` construction
(`patch_audio.rs` ~1577) into reusable builders — `SeamGateConfig::from_repair(...)` (run-constant
tuning + frames) and `derive_seam_gate_geometry(cfg, …, gap_frames, …)` (per-gap window + the two
`gap_frames`-derived counts `seam_gate_frames`/`border_frames`). Behavior-preserving refactor; the
struct is `pub(crate)` and no test references it, so green-before = green-after. This lets the oracle
call the **same** constructors production uses (no field-by-field hand-mirroring, no drift), and
collapses task 1.4 to a thin call. See §5.1b for the revised contract.

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

## 4. Regimes (classifier target — Phase 2)

Phase 1 emits raw scores only; **regime labels are Phase 2**. For reference, each grid cell
`(peak_offset_secs, fill_border_search_secs)` with `search < offset` maps to:

| Regime | Baseline (unified @ throat) | Best passing bracket | Joint pool winner | Routing |
|--------|----------------------------|----------------------|-------------------|---------|
| `Invalid` | — | — | — | `search >= offset` |
| `BaselineHigh` | High (≥ 0.35) | any | baseline | E1 |
| `BaselineMarginalWins` | Marginal (not High) | any or none High | baseline (no `anchor_seam_used`) | E2 |
| `AnchorRescuePossible` | not High | ≥ 1 with `min(pre,post) ≥ 0.35` | anchor bracket (`anchor_seam_used`) | **E3** |
| `AllSkip` | below floors | none pass gate | — | E5 |

Auto mode also needs `min(baseline_pre, baseline_post) < min_fill - margin` (currently 0.25 with
`min_fill=0.35`, `margin=0.10`) for anchor search to run, and nominal throat must stay
symmetric-weak (`min < 0.27`, `|pre-post| ≤ 0.10`) for A6 domain validity.

**Discovery objective (Phase 2):** among valid A6 cells, maximize `max_bracket_min - 0.35` where at
least one bracket passes the gate; prefer cells classified `AnchorRescuePossible`. Refine only
where coarse neighbors **change regime**. See §5.2 for classifier inputs.

---

## 5. Implementation phases

### Phase 0 — Extract `SeamGateConfig` / `SeamGateGeometry` (behavior-preserving) — **do first**

**Acceptance gate for the whole PR:** full suite green, unchanged. `SeamGateParams` is `pub(crate)`
and no test references it, so green-before = green-after is the only contract. Do **not** "improve"
behavior anywhere.

1. **Define `SeamGateConfig`** (owned, no lifetime, `#[derive(Clone, Copy)]`) — the 41 run-constant
   fields, **plus** the three secs inputs currently living as `patch_audio` locals:
   `normalize_window_secs`, `min_border_discovery_secs`, `fill_seam_search_secs`. *(Gotcha A — without
   these the geometry builder can't compute `correlate_frames`.)*
2. **Define `SeamGateGeometry<'a>`** (`Copy`) — the 8 per-gap fields: `a_pcm`, `b_samples`,
   `b_extract_start_secs`, `refined_b_start_secs`, `refined_b_end_secs`, `seam_gate_frames`,
   `border_frames`, `anchor_search_prior`. *(The catch: `seam_gate_frames`/`border_frames` belong here
   because they ride `gap_frames` through `correlate_frames`.)*
3. **Redefine `SeamGateParams<'a>`** as `{ cfg: &'a SeamGateConfig, geom: SeamGateGeometry<'a> }`.
   *(Gotcha C — `cfg` is a borrow so the composite stays `Copy`; the `..*params` retry sites depend on
   that.)*
4. **`SeamGateConfig::from_repair(request, sample_rate, channels)`** — move the ~32 `request.*` copies
   + run-constant frame derivations (`context_frames`, `bin_frames`, `border_standoff_frames`,
   `search_radius_frames`, `fill_length_slack_frames`, `max_extend_frames`, `step_frames`,
   `residual_max_lag_frames`) here.
5. **`derive_seam_gate_geometry(cfg, a_pcm, b_samples, b_extract_start_secs, refined_b_start_secs,
   refined_b_end_secs, gap_frames, anchor_search_prior)`** — computes `correlate_frames` from `cfg`'s
   secs + `gap_frames`, then `seam_gate_frames`/`border_frames`.
6. **Rewrite the production builder in `patch_audio.rs` (~1577):** hoist
   `let cfg = SeamGateConfig::from_repair(...)` **above the per-gap loop** so the borrow outlives every
   gap's params *(Gotcha C, part 2)*; inside the loop build `geom` via `derive_seam_gate_geometry(...)`
   and assemble `SeamGateParams { cfg: &cfg, geom }`.
7. **Update the two retry struct-update sites (`patch_region.rs:1558`, `:1615`):** rewrite as
   `SeamGateParams { geom: SeamGateGeometry { refined_b_end_secs /* or _start_ */, ..params.geom },
   ..*params }`. Override **only** the one secs field — do **not** recompute
   `seam_gate_frames`/`border_frames` for the grown gap. *(Gotcha B — preserves existing behavior.)*
8. **Migrate field access across `patch_region.rs`** (~18 consumer fns, `FitHaystackCache::build`,
   `AudioFitSource`): `params.x` → `params.cfg.x` or `params.geom.x`. Compiler-driven.
9. **Verify:** `cargo build` clean, then full repair test suite green with zero test-file edits.

**Settled, don't spend time on:** the struct is already `Copy` (proven by `..*params` compiling today,
so `AnchorSearchPrior`/`AnchorMatchabilityParams` are `Copy`); there is **no** `Debug` derive or
whole-struct formatting to preserve (verified).

### Phase 1 — Single-cell diagnostic (replaces `probe_w5`) — **ready to implement**

**Goal:** Strict **superset** of today's `probe_w5_anchor_rescue_scores`: nominal + baseline Pearson,
feasible bracket list, **plus per-bracket gate-path scores**. Correct **diagnostic** tier, reusable
API for Phase 2. No regime labels yet.

**Why probe is insufficient:** `probe_w5` prints bracket frames only. Pipeline A6 fails because
baseline Marginal wins the joint pool while anchor brackets stay below High (~0.26 post). Phase 1 must
score each bracket on the **unified haystack path** (same as `AudioFitSource::score`), not
`anchor_seam_diagnostic` (nominal placement + `matchability_at_anchor` only).

**Score-path note:** with the Phase 0 builders, oracle bracket scores and production share the exact
`SeamGateConfig`/`SeamGateGeometry` constructors, so they agree by construction (no marginal-cell
drift). `oracle_baseline_throat_pearson` (a separate helper) may still differ slightly from full
`PatchAudio`; Phase 1 bracket scores use the gate wrapper below, and Phase 3 pipeline lock-in remains
the final arbiter.

#### 5.1a Cell parameters

```rust
/// One W5 discovery cell (Phase 1+).
pub struct W5AnchorRescueCell {
    pub peak_offset_secs: f64,       // B shift = A peak offset (F1-style)
    pub fill_border_search_secs: f64, // repair + structure search_radius; must be < peak_offset
}
```

Defaults for existing tests: `peak_offset_secs = 1.0`, `fill_border_search_secs = 0.78`.

#### 5.1b Bracket scoring contract

Add a **crate-internal** oracle wrapper (Phase 1 — reuse the Phase 0 builders, do **not** hand-build
`SeamGateParams` field-by-field):

| Piece | Location | Contract |
|-------|----------|----------|
| `seam_gate_params_from_energy_fixture(fixture, repair) -> SeamGateParams` | `test_support/w5_anchor_rescue_diag.rs` | Build `SeamGateParams` + baseline `RefinedGapFrames` by calling the **Phase 0** constructors: `SeamGateConfig::from_repair(repair, rate, channels)` then `derive_seam_gate_geometry(cfg, a_pcm, b_samples, …, gap_frames, …)`. Derive the per-gap window via `gap_report_from_energy_fixture` / `preview_patch_geometry` / `production_geometry_params`; feed those into the geometry builder rather than reconstructing fields. |
| `oracle_score_fit_candidate(...)` | `application/patch_region.rs` as `pub(crate)` | Thin wrapper: `evaluate_seam_gate_fit_candidate(refined, baseline, params, cache, anchor_seam_bracket)` → `(pre, post, confidence, ranking_score)` or gate failure. |
| `score_w5_bracket_at_gate(...)` | `w5_anchor_rescue_diag.rs` | Call wrapper with `refined = bracket.refined`, `anchor_seam_bracket = true`. Skip brackets where gate returns `Err` (record `passed_gate = false`). |

**Do not use** [`anchor_seam_diagnostic.rs`](../crates/clip-sync-repair/src/test_support/anchor_seam_diagnostic.rs) Pearson columns for discovery — wrong placement (nominal, not unified winner).

Nominal and baseline throat scores **reuse** existing helpers unchanged:
[`oracle_nominal_throat_pearson`](../crates/clip-sync-repair/src/test_support/energy_signature_production.rs),
[`oracle_baseline_throat_pearson`](../crates/clip-sync-repair/src/test_support/energy_signature_production.rs).

#### 5.1c Return types

```rust
pub struct W5BracketGateScore {
    pub pre_frame: usize,
    pub post_frame: usize,
    pub move_frames: usize,
    pub passed_gate: bool,
    pub pre_pearson: Option<f64>,
    pub post_pearson: Option<f64>,
    pub min_pearson: Option<f64>,       // min(pre, post) when passed
    pub confidence: Option<FillConfidence>,
    pub ranking_score: Option<f64>,
}

pub struct W5AnchorRescueCellScores {
    pub cell: W5AnchorRescueCell,
    pub nominal_pre: f64,
    pub nominal_post: f64,
    pub baseline_pre: f64,
    pub baseline_post: f64,
    pub brackets: Vec<W5BracketGateScore>,
    pub wall_ms: u64,
}
```

`score_w5_anchor_rescue_cell(fixture, repair) -> W5AnchorRescueCellScores` builds fixture via
`build_w5_cell(cell)` (see tasks below), scores nominal/baseline, enumerates feasible brackets
(same helpers as `w5_fixture_throat_symmetric_weak_and_brackets_exist`), scores each at gate.

#### 5.1d Tasks

| # | Task | Location | Done when |
|---|------|----------|-----------|
| 0.1 | **Phase 0:** extract `SeamGateConfig::from_repair` + `derive_seam_gate_geometry`; switch production (`patch_audio.rs` ~1577 + the two retry struct-update sites in `patch_region.rs`) to them | `patch_region.rs`, `patch_audio.rs` | Behavior-preserving; full suite green unchanged. Per-gap bucket = `{a_pcm, b_samples, b_extract_start_secs, refined_b_start_secs, refined_b_end_secs, seam_gate_frames, border_frames, anchor_search_prior}`; everything else run-constant in `cfg`. Retry sites override only `refined_b_*` — must **not** recompute `seam_gate_frames`/`border_frames` |
| 1.0 | `W5AnchorRescueCell` + `build_w5_cell(cell) -> (fixture, repair)` | `w5_anchor_rescue_diag.rs` | One call produces paired fixture + `w5_anchor_rescue_repair(Auto, search)` |
| 1.1 | `build_w5_symmetric_weak_throat_anchor_rescue(…, peak_offset_secs, fill_border_search_secs)` | `energy_signature_fixtures.rs` | `b_dropout_shift_frames = peak_offset`; `structure_params.search_radius_frames = secs_to_frames(fill_border_search_secs)`; add `fill_border_search_secs` as a 4th positional arg (Rust has no default args) — update the 4 call sites in `anchor_seam_oracle.rs` to pass `0.78` |
| 1.2 | `w5_anchor_rescue_repair(mode, fill_border_search_secs)` | `energy_signature_production.rs` | adds `fill_border_search_secs` param — update existing call sites to pass `0.78`, **or** keep `w5_anchor_rescue_repair(mode)` as a thin wrapper forwarding `0.78` (pick one; positional arg means call sites cannot stay literally unchanged) |
| 1.3 | `pub(crate) oracle_score_fit_candidate` | `patch_region.rs` | Returns gate Pearson + confidence for one `(refined, anchor_seam_bracket)` |
| 1.4 | `seam_gate_params_from_energy_fixture` (thin call to Phase 0 builders) + `score_w5_anchor_rescue_cell` | `w5_anchor_rescue_diag.rs` | Full `W5AnchorRescueCellScores`; export via `test_support/mod.rs`. Builds `SeamGateParams` via `SeamGateConfig::from_repair` + `derive_seam_gate_geometry` — no field-by-field reconstruction |
| 1.5 | `tests/diag_w5_anchor_rescue.rs` | new binary | `w5_anchor_rescue_single_cell` prints CSV row + human summary; default cell `(1.0, 0.78)` |
| 1.6 | Remove `probe_w5_anchor_rescue_scores` | `anchor_seam_oracle.rs` | Probe gone; domain + pipeline tests use parameterized builder where needed |
| 1.7 | Wiring | `Cargo.toml`, `mod.rs`, `development.md`, `test-tier.ps1` | See §5.1e |

#### 5.1e Wiring (exact)

`crates/clip-sync-repair/Cargo.toml` — after `diag_anchor_seam`:

```toml
[[test]]
name = "diag_w5_anchor_rescue"
path = "tests/diag_w5_anchor_rescue.rs"
required-features = ["diagnostic-tests"]
```

`test_support/mod.rs`: `pub mod w5_anchor_rescue_diag;`

`scripts/test-tier.ps1` `Invoke-RepairDiagnostic`: add `'--test', 'diag_w5_anchor_rescue'`.

`development.md`: replace `probe_w5_anchor_rescue_scores` row with `diag_w5_anchor_rescue` /
`w5_anchor_rescue_single_cell`.

#### 5.1f Diagnostic output (Phase 1)

CSV header (stderr or stdout via `println!`, match `diag_anchor_seam` style):

```text
peak_offset_secs,fill_border_search_secs,nominal_pre,nominal_post,baseline_pre,baseline_post,\
bracket_pre,bracket_post,bracket_move,passed_gate,pre_pearson,post_pearson,min_pearson,confidence,ranking_score,wall_ms
```

One summary row for nominal/baseline (`bracket_*` empty); one row per bracket.

**Run:**

```powershell
cargo test -p clip-sync-repair --features diagnostic-tests --test diag_w5_anchor_rescue -- --nocapture
```

**Phase 1 exit criteria:** single-cell test runs in &lt;15 s; default cell reproduces known probe
numbers (nominal ~0, baseline pre ~0.81); at least one bracket row with `passed_gate` and Pearson
columns populated.

### Phase 2 — Regime map (coarse + boundary refine) — **spec below; implement after Phase 1**

Phase 2 consumes `score_w5_anchor_rescue_cell` and adds regime classification + grid sweep. See
**§5.2** for full requirements (not started until Phase 1 exit criteria pass).

| # | Task | Location |
|---|------|----------|
| 2.1 | `W5AnchorRescueRegime` + `classify_w5_cell(scores, joint_winner) -> …` | `test_support/w5_anchor_rescue_diag.rs` |
| 2.2 | `coarse_w5_grid(…) -> Vec<W5SweepCell>` | `clip-sync-repair-harness/src/w5_anchor_rescue_sweep.rs` |
| 2.3 | `refine_w5_boundaries(cells) -> Vec<W5SweepCell>` | same |
| 2.4 | `write_w5_sweep_csv(cells, path)` | same |
| 2.5 | `diag_w5_anchor_rescue_coarse_grid`, `diag_w5_anchor_rescue_refine_boundaries` | `tests/diag_w5_anchor_rescue.rs` |

Default coarse suggestion: `peak_offset ∈ [0.6, 1.1]` step 0.05 s; `search ∈ [0.65, 0.85]` step 0.02 s;
skip invalid half-plane (`search >= offset`).

**Performance:** ~5–10 s per cell (baseline unified dominates). Coarse grid ~50–100 valid cells →
~10–15 min local; refine only on boundaries.

#### 5.2 Phase 2 requirements (detailed — for implementer)

Phase 1 answers “what are the scores at one cell?” Phase 2 answers “where is the E3 pocket?”

**5.2.1 `W5SweepCell` schema** (harness; wraps Phase 1 output + classification):

```rust
pub struct W5SweepCell {
    pub scores: W5AnchorRescueCellScores,
    pub regime: W5AnchorRescueRegime,
    pub joint_winner: W5JointWinner,           // Baseline | AnchorBracket(usize) | None
    pub max_bracket_min: Option<f64>,          // best min(pre,post) among passing brackets
    pub anchor_seam_would_run: bool,           // should_run_anchor_seam on baseline scores
}
```

**5.2.2 Joint pool winner (required for `BaselineMarginalWins` vs `AnchorRescuePossible`)**

Scoring brackets alone is not enough: A6 fails when baseline Marginal **outranks** every anchor
bracket in the joint pool. Phase 2 must simulate the **minimal joint ranking** used by
`evaluate_seam_gate_fit_joint_core`:

1. Score baseline at throat (`anchor_seam_bracket = false`) → baseline candidate + `ranking_score`.
2. For each feasible bracket with `passed_gate`, score at gate (`anchor_seam_bracket = true`).
3. Pick global best by `ranking_score` (same as `global_best_joint_candidate`).
4. Set `joint_winner` and `anchor_seam_used` flag (winner is anchor bracket with `move_frames > 0`).

Implementation options (pick one in Phase 2 PR):

- **A (preferred):** `pub(crate) oracle_evaluate_fit_joint_cell(fixture, repair) -> W5JointOutcome`
  in `patch_region.rs` — runs `evaluate_seam_gate_fit_joint` on oracle-built `SeamGateParams` (no
  `PatchAudio`), returns winner + pool snapshot.
- **B:** Reimplement pool ranking in `w5_anchor_rescue_diag.rs` using Phase 1 per-candidate scores
  only (must mirror `fit_anchor_candidate_ranking_score` / E3 early exit rules).

Classifier **must** use joint winner, not `max_bracket_min` alone.

**5.2.3 `classify_w5_cell` rules**

| Regime | Conditions |
|--------|------------|
| `Invalid` | `search >= offset` |
| `AllSkip` | baseline gate fails; no bracket passes |
| `BaselineHigh` | baseline `min(pre,post) ≥ 0.35` |
| `AnchorRescuePossible` | not `BaselineHigh`; ∃ bracket with `min ≥ 0.35` and `passed_gate`; `joint_winner` is anchor |
| `BaselineMarginalWins` | else; baseline wins pool without anchor |

Also compute `anchor_seam_would_run` via `should_run_anchor_seam` (auto contour + score floor) for CSV.

**5.2.4 Boundary refine algorithm**

After coarse grid:

1. Label each cell with `regime`.
2. For each axis (`peak_offset`, `fill_border_search`), find edges where regime ≠ neighbor (4-neighbor
   on valid cells).
3. Insert midpoint cells on those edges (bisect step once per edge; optional second pass if still
   ambiguous).
4. Re-score only new cells; merge into result set.
5. Stop when no new regime boundaries appear or step &lt; 0.01 s on search / 0.025 s on offset.

**5.2.5 CSV sweep columns** (one row per cell):

```text
peak_offset_secs,fill_border_search_secs,regime,joint_winner,nominal_min,baseline_min,max_bracket_min,\
anchor_seam_would_run,bracket_count,passing_bracket_count,wall_ms
```

Optional: write under `target/w5_anchor_rescue_sweep.csv` when env `W5_SWEEP_CSV=1`.

**5.2.6 Module split**

| Module | Owns |
|--------|------|
| `test_support/w5_anchor_rescue_diag.rs` | Types, `score_w5_anchor_rescue_cell`, `classify_w5_cell`, `build_w5_cell` |
| `clip-sync-repair-harness/w5_anchor_rescue_sweep.rs` | Grid generation, refine, CSV I/O, `run_w5_sweep` loop |
| `tests/diag_w5_anchor_rescue.rs` | Thin tests calling harness |

Export harness module from `harness/src/lib.rs`.

**5.2.7 Phase 2 exit criteria**

- Coarse grid completes locally in &lt;20 min.
- CSV contains ≥1 `AnchorRescuePossible` cell **or** documents empty pocket + escalates §8 open
  questions (independent B shift, chirp bed).
- Refined boundaries bracket at least one `AnchorRescuePossible` cell if any exist in coarse pass.

**5.2.8 If no E3 pocket exists**

Do not proceed to Phase 3 lock-in. Revisit §8 (geometry axes beyond `(offset, search)`), then re-run
Phase 2 with expanded grid or fixture variant.

**Phase 2 result (2026-06-27): E3 pocket is EMPTY.** Coarse grid `offset ∈ [0.6,1.1]` step 0.05,
`search ∈ [0.65,0.85]` step 0.02 → **81 cells**: 28 `BaselineHigh`, 35 `AllSkip`, 18
`BaselineMarginalWins`, **0 `AnchorRescuePossible`**; joint winners 46 `Baseline`, 35 `Skip`, **0
`Anchor`**. Root cause — **baseline and bracket Pearson are coupled along this plane**: the search
radius that lets an anchor bracket reach the shifted fill *also* lets the baseline unified search
reach it, so wherever `max_bracket_min` is High the baseline is High too (e.g. `offset=0.80,
search=0.79`: `max_bracket_min=0.977`, `baseline_min=0.969` → `BaselineHigh`, baseline wins E1). No
cell has "bracket High while baseline weak", which is the precondition for E3. This confirms §8 Q1:
the `(offset, search)` axes cannot produce the pocket because `b_shift = peak_offset` ties the fill's
B location to the same radius both candidates search. **Next:** decouple the B shift from the search
radius (§8 Q1 — independent `b_shift_secs`) so the post seam can stay weak at the baseline placement
while an anchor bracket still reaches the fill, then re-run Phase 2. Reproduce:
`cargo test -p clip-sync-repair --release --features diagnostic-tests --test diag_w5_anchor_rescue
diag_w5_anchor_rescue_coarse_grid -- --nocapture` (CSV under `target/w5_anchor_rescue_sweep.csv` with
`W5_SWEEP_CSV=1`).

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
crates/clip-sync-repair/src/application/
  patch_region.rs                    # Phase 1: pub(crate) oracle_score_fit_candidate
                                     # Phase 2: pub(crate) oracle_evaluate_fit_joint_cell (optional)

crates/clip-sync-repair-harness/src/
  w5_anchor_rescue_sweep.rs          # Phase 2: grid, refine, CSV
  lib.rs                             # pub mod w5_anchor_rescue_sweep

crates/clip-sync-repair/src/test_support/
  mod.rs                             # pub mod w5_anchor_rescue_diag
  w5_anchor_rescue_diag.rs           # Phase 1–2: types, score_w5_cell, classify
  energy_signature_fixtures.rs       # parameterized build_w5_…
  energy_signature_production.rs     # w5_anchor_rescue_repair(search_secs)

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
