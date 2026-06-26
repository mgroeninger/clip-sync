# Fit-joint routing extraction — plan (DRAFT)

Status: **steps 1–3 done** (3a delegation + 3b `defer_residual` collapse). Single always-pool path;
the debug-flips-the-path foot-gun is eliminated. lib + characterization + residual-equivalence +
`patch_audio_integration` + `validate_residual_gate` all green. **Steps 4–5 in progress** (number-driven
payoff suite + `CandidateScorer` seam for composed orchestration assertions).

> **As-built note (doc reconciled to the code):** §§2–6 originally sketched a *monolithic*
> `route_fit_joint(baseline, anchors, grid) → Decision`. The landed design is **surgical delegation**:
> the router (`application/fit_routing.rs`) owns the decision *rules* (comparators + predicates); the
> driver (`patch_region`) keeps the *orchestration* (precedence/short-circuit/selection). Lower parity
> risk; the trade is that exit precedence is not a single pure function — step 5 adds the
> `CandidateScorer` seam to make the orchestration number-testable too.

> **Layer note:** the router lands in `application/fit_routing.rs`, not `domain/` as first sketched
> — its skip type (`SeamGateFailure`) and candidate shape are application-coupled. Purity /
> number-testability (the actual goal) is unaffected by the layer; domain would force moving
> `SeamGateFailure` down for no benefit.

Companions: [TEMP-anchor-seam-plan.md](TEMP-anchor-seam-plan.md),
[gap-repair-guide.md](gap-repair-guide.md) § Vocabulary,
[residual-gate-wiring-plan.md](residual-gate-wiring-plan.md).

Motivating finding: the pipeline-level anchor oracles (A2/A5/A5b) patch on the **baseline throat at
~0.99** and short-circuit before anchor search runs — so they never exercise the anchor rescue path,
yet were green. The run-metadata fields `anchor_seam_used` / `anchor_bracket_move_frames`
(`GapPatchStatus::Patched`) now make that visible. This plan makes the *routing decision* testable
with numbers so that blind spot becomes an asserted decision, not a silent coincidence.

---

## 1. Problem (one paragraph, pre-extraction)

The fit-joint routing — baseline short-circuit → anchor search → boundary grid → winner/tier — lived
in `evaluate_seam_gate_fit_joint` and `try_anchor_seam_joint_search` (`application/patch_region.rs`),
**fused with measurement**: each branch condition was computed by `evaluate_seam_gate_fit_candidate`,
which runs the unified structure+waveform search on real PCM. Because decision and measurement were
entangled, the routing could only be tested through full WAV fixtures, and we could not
deterministically hit "baseline weak, anchor strong" (the throat scores whatever the fixture audio
happens to produce). The leaf gate predicates were already pure and tested; the
**orchestration/precedence** was not.

After steps 1–3 the decision *rules* are pure and number-tested (`fit_routing`); step 5 will make the
*orchestration* number-testable too. Precedent for the target shape already existed:
`harness/src/residual_gate.rs` tests residual-gate decisions purely over a synthetic `FloorOracleRun`
struct (numbers), no audio — the same model for fit-joint routing.

---

## 2. The 7 terminal exits (as built)

Strict precedence order. The "decided by" column names the driver helper and the router rule it
delegates to (the orchestration lives in `evaluate_seam_gate_fit_joint` / `try_anchor_seam_joint_search`).

| # | Exit | Condition | Decided by (driver → router rule) |
|---|------|-----------|-----------------------------------|
| E1 | Baseline High | baseline Pearson `High`, residual-confirmed at finalize | screen → `terminates_high` + `try_finalize_high_joint_candidate` |
| E2 | Baseline accept | `BaselineOnly` ∧ `{High,Marginal}` — **not Marginal under `force`** | `baseline_accept_without_grid` → `baseline_only_accepts` |
| E3 | Anchor High | best Pearson-`High` over the pool is an anchor bracket | `best_high_joint_candidate` (`selection_cmp` + `terminates_high`) + is-anchor |
| E4 | Anchor accept | `BaselineOnly` ∧ best-overall is an anchor & accepts | `best_anchor_joint_candidate` + `baseline_only_accepts` |
| E5 | BaselineOnly winner | no grid: pool winner, else `Skip(best_below_floor)` | `select_joint_fit_winner_with_residual` (`winner_cmp`) |
| E6 | Grid High | **full grid scored**, then best Pearson-`High` by ranking, residual-confirmed | `try_finalize_best_grid_high` → `best_high_joint_candidate` |
| E7 | Grid winner | pool winner, else `Skip(best_below_floor)` | `select_joint_fit_winner_with_residual` |

Anchor gate (whether E3/E4 are reachable) is pure: `should_run_anchor_seam(...)`
(`gap_anchor_seam.rs`). Note E6 is **not** an early-exit on the first grid High — the grid is scored
in full, then the best High wins (see §6).

---

## 3. The seam: driver vs router (as built)

**Surgical delegation**, not a monolithic `route_fit_joint`: the router owns the decision *rules*
(comparators + predicates), the driver keeps the *orchestration* (precedence, selection loop). Each
driver call site swapped an inline expression for a behaviour-identical router call — minimal parity
risk. The cost: exit precedence is not a single pure function (it stays in the driver); step 5 closes
that with a `CandidateScorer` seam.

**Router** (`application/fit_routing.rs`, pure, number-tested):

```rust
struct CandidateScore { confidence: FillConfidence, boundary_move: usize, ranking_score: f64 }

fn terminates_high(confidence: FillConfidence) -> bool;          // E1/E3/E6 screen
fn baseline_only_accepts(search, confidence) -> bool;            // E2/E4
fn selection_cmp(a, b) -> Ordering;  // max_by: rank↑ then move↑  (tie → larger move)
fn winner_cmp(a, b) -> Ordering;     // sort:   rank↓ then move↑  (tie → smaller move)
```

`CandidateScore` is the lean decision projection — identity, `anchor_seam_used`, residual, etc. stay
on the driver's `FitJointCandidate`. `ranking_score` and `confidence` are **Pearson** (residual is
applied later at selection; see §5).

**Driver** (`evaluate_seam_gate_fit_joint`, `try_anchor_seam_joint_search`): projects each
`FitJointCandidate` via `.score()` and drives its `max_by` / `sort_by` / High-screen through the
router rules. Single **always-pool** path; residual applied lazily at selection
(`try_finalize_*` / `select_joint_fit_winner_with_residual`), a no-op when residual is disabled.

---

## 4. Where it lives

Router → `application/fit_routing.rs` (not `domain/`: inputs are application-coupled, e.g.
`SeamGateFailure`; purity/number-testability is unaffected — see the layer note up top).
Orchestration + measurement stay in `application/patch_region.rs`.

---

## 5. Simplification unlocked: collapse `defer_residual`

The `defer_residual` dual path (`record_fit_joint_candidate` eager/`best` vs
`record_fit_joint_candidate_to_pool` deferred/`pool`, threaded through `:840-1024`) exists only to
control *when* residual is measured. The collapse is **always-pool + lazy-finalize-at-selection**:
candidates are scored with **Pearson** confidence/rank only (no residual), and residual is applied at
selection in router order — confirming the High screen ([`terminates_high`]) and walking
[`pool_winner_order`] until a candidate's residual verdict passes (fall-through on veto). When
residual measurement is disabled, finalize is a no-op, so one path serves both.

> **Correction (step-2 review):** an earlier draft said "`confidence` residual-finalized *at scoring
> time*" — that would have measured residual for every grid cell (a cold-path perf regression). The
> real selection (`select_joint_fit_winner_with_residual:472-505`) sorts by *Pearson* rank then
> applies residual lazily in order. The router therefore owns only the **order**; the driver keeps
> the lazy residual loop. This still collapses the pool-vs-best fork (always pool, finalize a no-op
> when residual off) and removes the `_to_pool` twins + `global_best_joint_candidate`.

## 5b. Touch points (as built)

`FitJointCandidate { outcome, ranking_score, boundary_move }` projects via `.score()` to
`CandidateScore { confidence: outcome.confidence (Pearson), boundary_move, ranking_score }`.

| Driver helper | Router rule used | Residual step (driver) |
|---------------|------------------|------------------------|
| `joint_candidate_ranking_cmp` | `selection_cmp` | — |
| winner sort in `select_joint_fit_winner_with_residual` | `winner_cmp` | lazy walk + `best_below_floor` |
| `accepts_baseline_without_boundary_grid` / `baseline_accept_without_grid` | `baseline_only_accepts` | — |
| baseline / anchor / grid High screens | `terminates_high` | `try_finalize_high_joint_candidate` confirm |

**Two driver changes the collapse depended on:**

1. ✅ `evaluate_seam_gate_fit_candidate` now emits **Pearson** confidence unconditionally with
   `residual = None` at scoring; the winner's final `confidence` comes from
   `finalize_fit_outcome_residual` at selection. (Faithful: the old non-defer branch also yielded
   `None` residual there, since non-defer ⟺ residual-not-wanted.)
2. ⚠️ **Not done — deliberately.** Setting `anchor_seam_used` at construction to retire
   `mark_anchor_outcome` isn't clean: `mark_anchor_outcome` also stamps `anchor_bracket_move_frames`,
   which is only known in the anchor-search loop (`bracket.move_frames` vs the scan hole), not at
   scoring time. So `mark_anchor_outcome` stays, with its pool-append guard (the §6.3 footgun is
   *contained* by the guard, not removed). Revisit only if `move_frames` is ever plumbed to scoring.

`best_below_floor` stays driver state: gate failures recorded at scoring, residual vetoes during the
`winner_cmp` walk.

---

## 6. Parity risks (outcomes)

1. **Grid selection (E6)** — *resolved by design choice, not by early-exit.* The grid is now scored
   in **full**, then the best Pearson-`High` wins by ranking (`try_finalize_best_grid_high`). This
   replaced the old first-High-in-scan-order early-exit; the perf cost of scoring all cells is
   accepted (the user's E6 design choice — more principled than arbitrary scan order). Verified by
   the full suite.
2. **Residual can downgrade High** — "High" is provisional until residual confirms. Handled:
   `try_finalize_high_joint_candidate` re-checks `confidence == High` *after* finalize and falls
   through if downgraded.
3. **Anchor marking** — `mark_anchor_outcome` (`anchor_seam_used` + `anchor_bracket_move_frames`)
   stays post-hoc, **contained** by the pool-append guard (see §5b #2). Footgun mitigated, not
   removed.
4. **`best_below_floor` precedence** — preserved exactly: gate failures at scoring, residual vetoes
   during the `winner_cmp` walk; `StructureAlignmentFailed` as the last-resort skip.
5. **`BaselineOnly` fall-through divergence** — the collapse exposed that defer/non-defer already
   disagreed here (`force`+Marginal: patch vs skip). Resolved to the defer/production behaviour; see
   step 3b note. This was the live foot-gun the whole effort was meant to prevent.

---

## 7. Sequenced steps

- [x] **1. Characterization lock.** `anchor_seam_oracle.rs` now asserts full `RoutingFacts` (exit +
  confidence + tier + seam + `anchor_seam_used` + `fit_path`) on A2/A5/A5b/F4. Locked actuals:
  A5/A5b = baseline-throat `High`/`Balanced`, `anchor_seam_used=false`; A2 = baseline-throat
  `Marginal`/`SymmetricWeak` (anchor not engaged); F4 = correlation `HardSkip`/`AsymmetricPost`
  (skip surfaces as hard-skip, not residual `not_applicable` — name predates the tier). Existing
  leaf-predicate unit tests cover the atoms. 9/9 green.
- [x] **2. Introduce `application/fit_routing.rs`** — `CandidateScore` + pure fns
  (`terminates_high`, `baseline_only_accepts`, `best_high`, `best_by_ranking`, `pool_winner_order`,
  `select_pool_winner`), 10 number-driven unit tests. Faithfully encodes the selection-vs-winner
  tie-break asymmetry. **Step-2 review fixes:** added `pool_winner_order` for the lazy residual
  fall-through (single-winner was insufficient); dropped unused `pre`/`post` fields; corrected the
  "residual-finalized" doc to "Pearson screen, driver applies residual in order" (see §5/§5b). Carries
  a temporary `#![allow(dead_code)]` (removed in step 3). No wiring.
- [x] **3a. Delegate decision logic** — `patch_region` now routes its decisions through
  `fit_routing`: `joint_candidate_ranking_cmp → selection_cmp`, the winner sort → `winner_cmp`,
  `accepts_baseline_without_boundary_grid → baseline_only_accepts`, and all six `confidence == High`
  screens → `terminates_high`, via a `FitJointCandidate::score()` projection. Behaviour-identical
  swaps (each delegate has the same body), so the control flow + `defer_residual` structure are
  untouched. Router pared to the comparators + predicates the driver actually uses (no dead code; no
  map-back). Parity: lib 283/0, characterization oracle 9/0. Integration (residual gate,
  patch_audio) verifying.
- [x] **3b. Collapse `defer_residual`** — single **always-pool** path. Removed the eager-`best` twin
  `record_fit_joint_candidate`, dropped `defer_fit_residual_measurement`, and de-forked
  `evaluate_seam_gate_fit_joint`, `try_anchor_seam_joint_search` (state/ctx no longer carry
  `best`/`defer_residual`), `try_finalize_best_grid_high`, `global_best_joint_candidate`,
  `best_anchor_joint_candidate`. `evaluate_seam_gate_fit_candidate` lost its `defer_residual` param —
  now unconditional Pearson confidence + `residual = None` at scoring (faithful: non-defer always
  returned `None` there too, since non-defer ⟺ residual-not-wanted). Residual stays lazy at selection.
  Build clean (no warnings); lib 284/0, characterization 9/0, `integration_residual_gate_smoke` 1/0
  (residual-on/off equivalence). `patch_audio_integration` + `validate_residual_gate` verifying.

  > **Divergence resolved:** the pre-collapse `BaselineOnly` fall-through already differed between
  > paths — defer returned `select_winner(pool)` (could patch a `force`+Marginal baseline), non-defer
  > returned `Err` (skip). A live foot-gun instance (debug logging flipped which one ran). Collapsed
  > to the **defer/production** behavior (locked by characterization F4, used under residual-gating);
  > full suite confirms nothing relied on the non-defer skip.
- [~] **4. Payoff suite** (number-driven routing tests). Rule-level assertions already exist (step 2's
  8 `fit_routing` tests). The *composed orchestration* assertions — exit precedence, "anchor never
  invoked", the `force` fall-through — need step 5's seam (the driver, not a pure fn, owns precedence);
  tier classification is already in `gap_tags`. **The concrete test list is the gap-type → script
  matrix in §10.**
- [ ] **5. `FitCandidateSource` seam** (wanted) — put the audio-touching operations behind a trait so
  `evaluate_seam_gate_fit_joint` runs against a fake with scripted scores/brackets. **Design in §9.**
  Enables the §10 matrix as fast deterministic number tests. Moderate refactor (inject the source into
  the orchestration; production wires the real audio-backed source).

## 8. What stays fixture-bound

Only the **audio→score mapping**: "a symmetric-weak throat with a salient nearby anchor physically
scores baseline-dead-zone + anchor-bracket-high." Step 4/5 own *what the pipeline decides given
scores*; **one** anchor-rescue fixture (`build_w5_symmetric_weak_throat_anchor_rescue`) owns that the
scores are physically realizable — together they finally pin the anchor-rescue path the original
blind-spot finding exposed (A2/A5 patch the baseline throat, never the anchor).

**A6 status (2026-06):** Domain oracle **done** (`w5_fixture_throat_symmetric_weak_and_brackets_exist`).
Pipeline oracles (`w5_anchor_rescue_pipeline_engages_anchor_seam_{auto,force}`) remain `#[ignore]` until
the winning anchor bracket reaches Pearson **High** (routing E3); manual tuning:
`cargo test -p clip-sync-repair --test anchor_seam_oracle probe_w5_anchor_rescue_scores -- --ignored --nocapture`.

---

## 9. Step 5 — `FitCandidateSource` seam (design)

The orchestration touches audio in three spots; the trait abstracts exactly those so a fake can drive
the **real** precedence loop with scripted numbers (no audio, no windows):

```rust
trait FitCandidateSource {
    // quality + B-side placement for one A-side seam bracket (or a gate failure)
    fn score(&mut self, refined: RefinedGapFrames, anchor_seam_bracket: bool)
        -> Result<(SeamGateOutcome, f64), SeamGateFailure>;
    // anchor brackets to try (empty = gate closed / none feasible)
    fn anchor_brackets(&self, baseline_pre: f64, baseline_post: f64) -> Vec<AnchorBracket>;
    // residual probe at selection (identity when residual off; may Err on veto)
    fn finalize_residual(&self, outcome: SeamGateOutcome) -> Result<SeamGateOutcome, SeamGateFailure>;
}
```

- **Real impl** holds `&params` + `cache` + `baseline`; wraps `evaluate_seam_gate_fit_candidate`, the
  anchor enumeration (`build_gap_signature` / `list_anchor_candidates_a` / `list_feasible_anchor_brackets`),
  and `finalize_fit_outcome_residual`. Production behaviour is unchanged (parity via the full suite).
- **Grid geometry stays in the orchestration** — pure frame arithmetic (`start_min..end_max`/`step`),
  not audio — so the fake scripts only the three methods; the grid loop calls `score()` per
  moved-edge cell.
- **Fake** = scripted `refined → result` map + scripted brackets + a **call counter**. The counter is
  what makes "`anchor_brackets` never called / grid never scored" assertable — control-flow facts no
  other layer can express.
- Routing reads only `confidence` / `ranking_score` / `boundary_move` / `anchor_seam_used` from a
  scored outcome, so the fake builds `SeamGateOutcome`s via a helper that sets those and leaves
  `alignment` / structure fields as harmless defaults (a routing test never inspects B-extraction).

**Windows are mocked away.** The ~250 ms seam-scoring window (`seam_gate_frames`) and the anchor
neighbourhood (`context_frames`) are properties of the *real* scorer; step-5 tests script
positions + scores directly, so they exercise the *decision* over edge-pushed candidates regardless of
windows. Windows only govern the audio→score *derivation* (the fixture, §8).

**Edge-push coverage (what step 5 exercises).** Three distinct edge-pushing paths exist:

| Path | Pushes edges | Step-5 coverage |
|------|--------------|-----------------|
| Anchor brackets (fit) — `try_anchor_seam_joint_search` | outward to editorial boundaries (contain the scan hole) | *decision* ✅ (fake supplies brackets); *derivation* ❌ (domain tests + fixture) |
| Boundary grid (fit) — grid loop | blind `±step` outward nudge | ✅ geometry in orchestration; fake scores each cell |
| Seam-extension retry — `retry_waveform_seam_extensions` | outward on seam fail, re-invokes the gate | ❌ **gate-mode only** (`patch_audio.rs` guard `fill_mode == Gate`), above `fit_joint`; own predicate tests |

So step 5 covers the *decisions* over grid- and anchor-pushed candidates, not the anchor *derivation*
(which boundary to push to — domain `gap_anchor_seam` tests + the §8 fixture) nor the gate-mode retry.

## 10. Step 4 — gap-type → `FitCandidateSource` script matrix

One trait, one fake, configured per row (a table of scripted responses, not N impls). Routing asserts
**exit + confidence + `anchor_seam_used` + patched/skip**; **tier** (`DeadZone`/`HardSkip`/
`AnchorTrusted`) is derived downstream by `gap_tags` (a follow-on assertion, not the router's output).

| Gap type (guide) | `score(baseline)` | `anchor_brackets()` → `score(anchor)` | grid | mode | Exit → routing outcome |
|---|---|---|---|---|---|
| **W1** balanced good | `Ok` High (0.6/0.5) | — | — | any | **E1** → Patched High; `anchor_brackets()` not called |
| **W2** balanced marginal | `Ok` Marginal (0.30/0.32) | none | — | BaselineOnly, ¬force | **E2** → Patched Marginal |
| **W3** asym marginal (C3) | `Ok` Marginal (0.28/1.0) | none | — | BaselineOnly | **E2** → Patched Marginal (→ `AsymmetricPost`) |
| **W4** asym dead zone | `Err` Waveform (0.23/1.0) | none | — | BaselineOnly | **E5** → Skip (→ `DeadZone`) |
| **W5** symmetric weak | `Err` Waveform (0.14/0.14) | none | — | BaselineOnly | **E5** → Skip (→ `DeadZone`) |
| **W5 + anchor rescue** | `Err` Waveform (0.14/0.14) | `[move=400]` → `Ok` High (0.55), `anchor_seam_used` | — | auto/force | **E3** → Patched, `anchor_seam_used`, `move>0` (→ `AnchorTrusted` if structure-trusted) |
| **anchor marginal rescue** | `Err` (0.14/0.14) | `[move=400]` → `Ok` Marginal (0.30) | — | BaselineOnly+auto | **E4** → Patched Marginal, `anchor_seam_used` |
| **hard skip** | `Err` Waveform (0.05/0.04) | none | — | any | **E5/E7** → Skip (→ `HardSkip`) |
| **W6** structure fail | `Err` StructureAlignmentFailed | — | — | any | Skip (→ `StructureFail`) |
| **force fall-through** (3b) | `Ok` Marginal (0.30) | none feasible | — | **force**+BaselineOnly | **E5** → Patched Marginal *(divergence resolved to defer behaviour)* |
| **grid rescue** | `Err`/Marginal | none | one cell → `Ok` High | FullGrid | **E6** → Patched High (→ `fit_path=BoundaryGrid`) |
| **baseline-High short-circuit** | `Ok` High (0.99) | *(scripted but)* | — | force | **E1** → Patched; **assert `anchor_brackets()` call count == 0** |

Notes:
- The **W5-skip vs W5-rescue** pair share the throat script (`Err 0.14/0.14`); the *only* difference is
  whether `anchor_brackets()` yields a strong bracket — the one variable that flips skip→patch. That
  pair is the original blind spot as a two-line diff.
- **W4/W5 skip via E5** because a below-marginal baseline returns `Err` from `score()` → never enters
  the pool → `select_winner` over an empty pool surfaces `best_below_floor`.
- The **call-counter** rows (W1, baseline-High) assert the short-circuit *control flow*, not just the
  outcome.

---

## Decision log

- (resolved) Fixture vs number suite → **both, complementary**: the number suite (step 4) + the
  `CandidateScorer` seam (step 5) own routing; one anchor-rescue fixture owns the audio→score seam (§8).
- (resolved) Surgical delegation over a monolithic `route_fit_joint` (lower parity risk; orchestration
  stays in the driver, made number-testable via step 5).
- (resolved) `BaselineOnly` `force`+Marginal divergence → defer/production behaviour (step 3b note).
- (open) `mark_anchor_outcome` retirement — blocked on plumbing `anchor_bracket_move_frames` to
  scoring time; left post-hoc with its guard (§5b #2).
