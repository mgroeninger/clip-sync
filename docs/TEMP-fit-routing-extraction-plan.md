# Fit-joint routing extraction — plan (DRAFT)

Status: **steps 1–2 done**, **step 3a done** (decision logic delegated to `fit_routing`; lib +
characterization green, integration verifying). **Step 3b** (collapse `defer_residual`) deferred as a
scoped, higher-risk follow-up.

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

## 1. Problem (one paragraph)

The fit-joint routing — baseline short-circuit → anchor search → boundary grid → winner/tier — lives
in `evaluate_seam_gate_fit_joint` (`application/patch_region.rs:815-1046`) and
`try_anchor_seam_joint_search` (`:663-813`), **fused with measurement**: each branch condition is
computed by `evaluate_seam_gate_fit_candidate`, which runs the unified structure+waveform search on
real PCM. Because decision and measurement are entangled, the routing can only be tested through full
WAV fixtures, and we cannot deterministically hit "baseline weak, anchor strong" (the throat scores
whatever the fixture audio happens to produce). The leaf gate predicates are already pure and tested;
the **orchestration/precedence** is not extractable today.

Precedent for the target shape already exists: `harness/src/residual_gate.rs` tests residual-gate
decisions purely over a synthetic `FloorOracleRun` struct (numbers), no audio. We want the same for
fit-joint routing.

---

## 2. Definition: the router contract (7 terminal exits)

Strict precedence order, each a pure function of candidate scores + thresholds:

| # | Exit | Condition | Current lines |
|---|------|-----------|---------------|
| E1 | Baseline High | baseline `confidence==High` **after residual finalize** | 862-886 |
| E2 | Baseline accept | `BaselineOnly` ∧ baseline `∈{High,Marginal}` — **not Marginal when `anchor_seam_mode = force`** | 888-913 |
| E3 | Anchor High | anchor gate open ∧ best anchor cand `High` | 759-787 |
| E4 | Anchor accept | `BaselineOnly` ∧ best anchor cand `Marginal` | 789-810 |
| E5 | BaselineOnly winner | no grid: best of pool, else `Skip(best_below_floor)` | 939-952 |
| E6 | Grid High | full grid done; **best** Pearson `High` by ranking (+ residual finalize) | after grid loop |
| E7 | Grid winner | best of pool, else `Skip(best_below_floor)` | 1026-1045 |

Anchor gate (whether E3/E4 are reachable) is already pure:
`should_run_anchor_seam(...)` (`gap_anchor_seam.rs:135`) — the template for the rest.

---

## 3. The seam: driver vs router

- **Driver** (stays in `application`): scores a bracket, finalizes residual, owns *short-circuit
  timing* (stop scoring grid cells at first High). Audio-touching.
- **Router** (`domain/fit_routing.rs`, new): pure functions over `CandidateScore` numbers that decide
  *which exit* and *which winner*. Number-testable.

```rust
struct CandidateScore {
    refined: RefinedGapFrames,
    pre: f64, post: f64,
    structure_pre: f64, structure_post: f64,
    confidence: FillConfidence,    // ALREADY residual-finalized (see §5)
    boundary_move: usize,
    anchor_seam_used: bool,
    anchor_trusted: bool,
    skip: Option<SeamGateFailure>, // None if it passed gates
}

enum Decision { Patched(CandidateScore), Skipped(SeamGateFailure) }

fn baseline_terminal(b: &CandidateScore, p: &RoutingParams) -> Option<Decision>;          // E1,E2
fn anchor_terminal(anchors: &[CandidateScore], p: &RoutingParams) -> Option<Decision>;     // E3,E4
fn is_terminal_high(c: &CandidateScore, p: &RoutingParams) -> bool;                        // short-circuit
fn select_winner(pool: &[CandidateScore], below: Option<SeamGateFailure>, p) -> Decision;  // E5,E7
```

Driver after extraction (short-circuit preserved):

```
score baseline → finalize residual → baseline
if let Some(d) = baseline_terminal(&baseline, p) { return d }           // E1,E2
if should_run_anchor_seam(...) {
    anchors = brackets.map(score + finalize)
    if let Some(d) = anchor_terminal(&anchors, p) { return d }          // E3,E4
}
if BaselineOnly { return select_winner(&pool, below_floor, p) }         // E5
for cell in grid {
    let c = score(cell);
    if is_terminal_high(&c, p) { return Patched(c) }                    // E6
    pool.push(c);
}
select_winner(&pool, below_floor, p)                                    // E7
```

---

## 4. Where it lives

`CandidateScore` + `route_fit_joint` fns → `domain/fit_routing.rs` (pure). `evaluate_seam_gate_fit_joint`
stays in `application` (I/O + measurement). Respects existing layering.

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

## 5b. Touch points (verified for step 3)

Each driver branch and the router primitive + driver-owned residual step that replaces it. `FitJoint
Candidate { outcome, ranking_score, boundary_move }` maps to `CandidateScore` as:
`refined=outcome.refined, confidence=outcome.confidence (Pearson), boundary_move, ranking_score,
anchor_seam_used=outcome.anchor_seam_used`.

| Site (`patch_region.rs`) | Router primitive | Driver still owns |
|--------------------------|------------------|-------------------|
| baseline High `:862-886` | `terminates_high(baseline)` | residual confirm (`try_finalize_high…`) |
| baseline accept `:888-913` | `baseline_only_accepts` | — |
| `accepts_baseline_without_boundary_grid:538-547` | delegate → `baseline_only_accepts` | — |
| anchor High `:759-787` | `best_high(pool)` + is-anchor | residual confirm |
| anchor accept `:789-810` | `best_by_ranking(pool)` + is-anchor + `baseline_only_accepts` | — |
| grid early High `:976-989` | `terminates_high(cell)`, scan order | residual confirm |
| winner `select_joint_fit_winner…:463-505` | `pool_winner_order` | lazy residual walk + `best_below_floor` |

**Two driver changes the wiring depends on (do these in step 3, not the router):**

1. `evaluate_seam_gate_fit_candidate` must emit **Pearson** confidence with residual *unmeasured*
   (today `:1446-1454` already does this in the `defer_residual` branch; make it unconditional). The
   selected winner's final `confidence` comes from `finalize_fit_outcome_residual` at selection.
2. Set `anchor_seam_used = anchor_seam_bracket` on the candidate **at construction** (the scorer knows
   it), retiring the post-hoc `mark_anchor_outcome` stamping (`:723-740`, plan §6.3).

`best_below_floor` stays driver state: gate failures recorded at scoring, residual vetoes recorded
during the `pool_winner_order` walk.

---

## 6. Parity risks (pin before cutting)

1. **Grid early-exit (E6)** — a pure fn over a fully materialized list would force scoring every cell
   (perf regression). Mitigation: driver keeps short-circuit via `is_terminal_high`.
2. **Residual can downgrade High** (`try_finalize_high_joint_candidate:438-460`) — "High" provisional
   until residual confirms. Mitigation: finalize residual before building `CandidateScore`.
3. **Anchor marking** (`:723-740`) — today `anchor_seam_used` stamped post-hoc; comment warns against
   mis-stamping baseline. After extraction the scorer sets it intrinsically → footgun removed.
4. **`best_below_floor` precedence** — which failure surfaces when nothing passes; preserve exactly in
   `select_winner`.

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
- [ ] **3b. Collapse `defer_residual`** (deferred, scoped follow-up) — make
  `evaluate_seam_gate_fit_candidate` emit Pearson confidence unconditionally + apply residual at
  selection; drop the pool-vs-best fork and the `_to_pool` twins; set `anchor_seam_used` at
  construction. Higher-risk structural rewrite, guarded by the same suites. Split out from 3a so the
  decision-logic delegation (low-risk, high-value) lands and is verified on its own.
- [ ] **4. Payoff suite** (number-driven routing tests):
  - `baseline pre=post=0.99 → E1, anchors never built` (the A5 blind spot, asserted)
  - `baseline=0.10, anchor{pre:0.40,move:300} → E3, anchor_seam_used, AnchorTrusted`
  - `all<0.12 → Skip(HardSkip)`; `0.12–0.27 symmetric → Skip(DeadZone)`
  - `BaselineOnly + marginal baseline → E2 (no grid, no anchor)`
- [ ] **5. (Optional) `CandidateScorer` trait** — driver-level fake returning scripted numbers, to
  test loop ordering (E2-before-E3-before-E6). Only if step 4 leaves a gap.

## 8. What stays fixture-bound

Only the **audio→score mapping**: "a throat-offset speech gap physically scores baseline-dead-zone +
anchor-bracket-high." Keep *one* such fixture as the seam anchor; everything about *what the pipeline
decides given scores* moves to step 4.

---

## Decision log

- (open) Build the baseline-fails/anchor-rescues fixture, or rely on the number-driven suite + one
  measurement fixture? Leaning: number suite owns routing, one fixture owns the audio→score seam.
- (open) Start at step 1 now, or defer the whole refactor? Step 1 is worthwhile regardless.
