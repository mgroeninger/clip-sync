# N-way donor alignment — plan (DRAFT)

Status: **draft / not started**. Working plan for repairing one damaged copy (A) from **multiple
donor copies** (B₁…Bₙ) of the *same master soundtrack*, where damage is sparse and generally
**non-overlapping** across copies.

Companion to the seam/residual work: see [seam-scoring.md](../seam-scoring.md),
[gap-fill-modes.md](../gap-fill-modes.md), and the residual-cancellation diagnostic prototype
(`policies::seam_residual_diagnostics`, logged as `fill seam residual diagnostics`).

---

## 1. Why N-way (and why now)

The current pipeline is **single-donor**: align A↔B, then per gap find a B bracket and score the
seam ([pipeline.md](../pipeline.md)). That is the right shape for *two different recordings of one
event*. But a common real case is different: **several copies of the same released soundtrack**
(rips/encodes), each with its own sparse damage (dropouts, glitches). There:

- Any given gap in A is almost always **clean in most other copies** → high fillability.
- All copies share the **same master signal** → a correct fill *cancels* A's border to the
  requantization floor (the premise behind `seam_residual_diagnostics`), not merely correlates.
- Multiple donors let us **cross-check** a fill and **detect damage by disagreement** instead of
  only by silence — directly addressing the P7 "audible hole the scanner never flagged" gap class
  ([gap-repair-guide.md](../gap-repair-guide.md) Layer 1).

Single-donor cannot exploit any of this. N-way turns redundancy into both better fills and a
verification signal.

## 2. Scope

**In scope**
- Ingest N donor inputs alongside A.
- Bring all donors onto A's timeline (per-donor offset + drift, reusing the existing aligner).
- Per gap: gather candidate fills from every donor that covers it; pick/combine by residual fit +
  cross-donor agreement.
- Damage detection by cross-donor disagreement (new gap source feeding the existing patch path).

**Out of scope (this plan)**
- Spectral-coherence scoring and GCC-PHAT alignment (separate, complementary lever).
- Sample-rate conversion beyond what alignment already handles (flag as risk, §8).
- Changing the mux/write phase beyond donor bookkeeping.

## 3. Current pipeline recap (what we build on)

```text
1 Align  →  2 Scan gaps  →  3 Fill plan  →  4 Per-gap patch  →  5 Write/mux
```

- **Align** produces an `AlignmentResult` (offset, clip anchors, overlap) — *can be supplied
  externally* (key: lets us drive it N times).
- **Scan gaps** → `GapReport` on A's decoded clock.
- **Fill plan** maps each A gap → nominal B location (`fill_offset_mode`: recommended /
  interpolated / anchored_retry).
- **Per-gap patch** (`application/patch_region.rs`): refine edges → slice B haystack → structure
  match → seam scoring (`SeamTemplates`, `fill_seam_correlations`) → splice.

The reusable seams of the design: `AlignmentResult` is injectable; `SeamGateParams` already carries
a single B haystack + offsets; the residual diagnostic already scores a `SeamTemplates` placement.

## 4. Proposed design

### 4a. Donor set & timeline normalization
- CLI accepts N donors: `clip-sync-repair A.mkv B1.mkv B2.mkv B3.mkv …` (or repeated
  `--donor` flags). A stays the single repair target.
- For each donor Bᵢ, run the **existing aligner** A↔Bᵢ → `AlignmentResult_i` (offset, clips,
  overlap, confidence). Reuse `--fill-offset interpolated` / drift handling per donor.
- Result: a `DonorTimeline` per donor = function mapping **A-frame → Bᵢ-frame** (+ coverage mask
  for where Bᵢ overlaps A). No global resample; we keep per-donor mapping like today.

### 4b. Candidate generation per gap
For each A gap, for each donor Bᵢ whose coverage includes the gap:
1. Map gap → nominal Bᵢ bracket (existing fill-plan math, per donor).
2. Build `SeamTemplates` from A's borders + Bᵢ haystack (existing
   `border_templates_for_gap` + donor haystack slice).
3. Score the candidate with **both**:
   - existing structure + seam Pearson (`fill_seam_correlations`), and
   - **residual cancellation** (`seam_residual_diagnostics`): `residual_db`, `headroom_db`,
     recovered `gain` + sub-sample `lag`.

Each donor yields a `DonorCandidate { donor_id, alignment, pre/post seam, residual, gain, lag }`.

### 4c. Selection & combination
Order of preference (config-tunable):
1. **Reject** candidates whose residual `headroom_db` exceeds a floor margin (the donor region is
   itself damaged, or alignment is wrong) — much stricter than the Pearson tiers, justified
   because same-master copies *should* cancel.
2. Among survivors, **agreement check**: align two or more donor fills to each other (they should
   also cancel, donor↔donor). Candidates that agree form a consensus set.
3. **Pick** the consensus candidate with the lowest residual; or **median/average** the agreeing
   donor fills sample-wise (after gain+lag normalization) to suppress per-copy codec noise.
4. Fall back to single best donor (current behavior) when only one donor covers the gap.

### 4d. Damage detection by disagreement (new gap source)
Independent of A's silence scan: slide a coarse window across the **overlap of ≥2 donors + A**;
where donor-vs-donor (or donor-vs-A) residual spikes for exactly one source, flag that source's
region as damaged. For A, emit these as extra gaps into the existing fill plan (so they flow
through the same patch path). Catches P7 (audible, non-silent damage). Gate behind a flag initially.

## 5. Code changes by layer

| Layer | Change | Reuse |
|-------|--------|-------|
| CLI / config | N donor inputs; `[repair].donors`, residual thresholds, agreement knobs | existing arg parsing |
| application/align | loop aligner over donors → `Vec<AlignmentResult>` | aligner is already injectable |
| application (new) | `DonorSet` / `DonorTimeline` (A-frame→Bᵢ-frame + coverage) | per-donor fill-plan math |
| application/scan_gaps | optional cross-donor disagreement scan → extra gaps | `GapReport` shape |
| application/patch_region | candidate loop over donors; selection/median; residual gate | `SeamGateParams`, `SeamTemplates`, `seam_residual_diagnostics` |
| domain | `DonorCandidate`, consensus/median helpers (pure, unit-testable) | `FillAlignment`, residual structs |
| application/repair_videos | track which donor filled each gap; provenance in report/JSON | existing outcome structs |
| docs / json-output | per-gap `donor_id`, `residual_db`, consensus size | — |

No change required to the splice/crossfade/mux primitives beyond passing the chosen fill.

## 6. Data structures (sketch)

```rust
struct DonorTimeline {
    donor_id: usize,
    alignment: AlignmentResult,           // existing
    // A-frame -> Bᵢ-frame mapping + coverage mask derived from alignment + drift mode
}

struct DonorCandidate {
    donor_id: usize,
    alignment: FillAlignment,             // existing (start/len + pre/post corr)
    residual: SeamResidualDiagnostics,    // existing prototype
}

struct GapFillDecision {
    chosen: FillKind,                     // SingleDonor(id) | Consensus(median of {ids})
    rejected: Vec<(usize, RejectReason)>, // for report/debug
}
```

## 7. Phasing / milestones

- **M0 — diagnostic (done):** residual-vs-floor logged per gap (single donor). Establish the
  bimodal threshold on the corpus before any gating. ← current prototype.
- **M1 — multi-donor ingest + per-donor candidates, report only:** align N donors, log every
  donor's residual per gap, **no change to which fill is chosen**. Validates timelines + coverage.
- **M2 — residual-gated selection:** choose best donor by residual among candidates; behind a flag.
- **M3 — consensus / median fills:** combine agreeing donors; measure codec-noise reduction.
- **M4 — disagreement damage detection:** new gap source feeding the patch path (flagged).

Each milestone independently shippable and corpus-validated.

## 8. Risks / open questions

- **Clock/sample-rate drift across copies.** A global offset won't hold cancellation over a long
  file; need per-gap lag (the residual search already recovers integer+frac lag — promote it to
  drive placement). Confirm donors share sample rate or resample first.
- **Donor↔donor alignment cost.** N² pairwise alignment is wasteful; align all donors **to A only**
  and compare on A's timeline (N alignments, not N²).
- **Codec noise floor varies per copy.** Make residual thresholds relative to a measured per-gap
  floor, not the optimistic LSB `floor_db` (which is a lower bound only).
- **Memory/decoit.** N donors = N decodes; stream per-gap haystacks rather than holding all donors
  fully in memory.
- **Provenance/legal:** report which donor supplied each sample (already implied by JSON change).
- **Tie-break vs existing tiers:** how residual gating interacts with `min_fill_correlation` and
  marginal tiers — likely residual supersedes Pearson for same-master, but keep Pearson for the
  mixed/two-mic case (mode switch?).

## 9. Test strategy

- Extend the synthetic gap corpus ([corpus-validation.md](corpus-validation.md)) with **multi-copy
  fixtures**: one master, N derived copies each with disjoint injected dropouts + per-copy
  requantization/codec noise; oracle = the master.
- Unit: consensus/median helpers; residual gate thresholds; coverage-mask mapping.
- Acceptance: every A gap filled from a clean donor; disagreement scan flags injected P7 holes;
  median fill measurably lower noise than single-donor.

---

## Related

- [pipeline.md](../pipeline.md) — single-donor execution pipeline this extends
- [seam-scoring.md](../seam-scoring.md) — seam mechanics; residual diagnostic lives alongside
- [gap-fill-modes.md](../gap-fill-modes.md) — fit/gate, offset modes (`interpolated`, `anchored_retry`)
- [corpus-validation.md](corpus-validation.md) — corpus to extend for multi-copy fixtures
