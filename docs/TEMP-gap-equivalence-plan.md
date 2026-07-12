# Gap content equivalence (skip redundant fills) — plan (DRAFT)

Status: **REDESIGNED to a silence-character gate (2026-07-11) — Phase 0 built + media-validated.** The original
seam/lag "does B match A at nominal" approach (§5 below) was **refuted on real media**: two independent
recordings **drift** (~150–200 ms residual lag per gap even after alignment), so nothing matches at lag 0 and
the seam read was useless (0/14 matchable gaps matched at lag 0). Operator ground truth then showed the actual
discriminator is the **silence character**, not seam correlation:

- **A-side:** A's gap RMS **relative to the recording's own noise floor** — a true dropout sits **≥35 dB below**
  it (signal died); a genuine quiet passage sits **at** it (room tone). Self-calibrating (no absolute dB).
- **B-side:** `donor_silence_fraction` (bimodal ~0 vs ~1 corpus-wide) — is B occupied.

**Vocabulary + gate (`domain/gap_equivalence.rs`, `classify_gap_equivalence`):** `repairable_dropout`
(dropout ∧ B occupied → keep) · `shared_silence` (B silent → drop) · `ambient_quiet` (room tone ∧ B occupied →
drop) · `not_evaluated`. Tunable (`dropout_margin_db≈35`, `donor_silence_thresh≈0.5`), **off by default**;
emitted as the `equivalence` block on `--gap-fingerprints` ([gap-fingerprint.md](gap-fingerprint.md)).
**Validated:** classifies all 15 gaps of a licensed pair to operator ground truth (8 repairable→keep,
4 mutual-silence→drop, intro/tail→drop). **Remaining:** dump one more pair to confirm the A-side threshold
generalizes; then **v1** = the production plan-time drop (config flags + `build_gap_fill_plan` hook). **§4–§14
below describe the superseded seam approach — kept for history; the silence gate replaces §5's algorithm.**

Companions: [gap-scan.md](gap-scan.md), [pipeline.md](pipeline.md) § Fill plan,
[gap-vocabulary.md](gap-vocabulary.md), [seam-scoring.md](seam-scoring.md),
[TEMP-gap-selection-plan.md](TEMP-gap-selection-plan.md), [json-output.md](json-output.md),
[TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md) §1 gate inventory.

Motivating use case: sensitive gap scan (`min_gap_ms=500`, `scan_block_ms=100`, …) finds many
silent runs on A that ffmpeg also sees, but a large fraction are **not editorial dropouts** — A
already carries the same program audio as B at the aligned position (scan false positives, or a
fill would be ~identity). Today `b_has_energy=true` marks them **repairable** and they enter the
expensive patch path, often skipping later with weak seam scores. A **content-equivalence** gate
would classify “B already matches A here” **before** patch and skip with an explicit reason.

Real-world anchor: a licensed 5.1 A/B pair — 39 scan gaps vs 14 ffmpeg
`silencedetect` hits at `d=0.5`; extras cluster around low-level dips where B has matching chase /
room content at sync time.

---

## 1. Problem (one paragraph)

Phase 2 answers: “Is A silent here, and does B have *any* audio at the mapped time?” (`b_has_energy`).
Phase 4 answers: “Can we *splice* B into A with acceptable seams?” (structure + Pearson + residual).
Neither answers: “Would patching **change** the program audio?” When scan is loosened to match ffmpeg
on borderline sub-second silences, many gaps are **repairable** but **redundant** — nominal B content
already matches A’s borders / same-source residual confirms identity. Patching them wastes decode +
search time and clutters reports. We need a cheap **equivalence** measurement between aligned A and B
spans that skips plan-time fill when a splice would be a no-op (or when scan misclassified a low-level
dip as a dropout).

---

## 2. Relationship to existing signals

| Signal | Question | Equivalence gate |
|--------|----------|------------------|
| `b_has_energy=false` | Is B silent at map time? (**shared pause**) | **Out of scope** — already `unfillable` / `NotFillable` |
| `program_quiet_at_nominal` (G5) | Is B interior mostly silent at nominal map? | **Opposite** — B empty, not “same sound” |
| Patch Pearson + structure | Can we place a splice? | **Later, expensive** — “can splice” ≠ “should splice” |
| Residual same-source (G4) | Does B cancel A at throat? | **Reuse at nominal, lag 0** — `seam_chosen_and_floor`, not oracle throat search |
| `GapFillSkipReason::NotFillable` | No B donor | Different family |

**New vocabulary cell (proposed):** **Already-equivalent** — A reads as a gap, B has energy, but
aligned content matches; action = plan-time skip (`already_matches_reference`). Distinct from
**Program-quiet** (B silent), **Decorrelated** (B different), and **Unfillable** (no donor).

Orthogonal to [TEMP-gap-selection-plan.md](TEMP-gap-selection-plan.md): selection = user subset;
equivalence = automatic “no-op fill” detection.

---

## 3. Definitions

| Term | Meaning |
|------|---------|
| **Nominal map** | `b = a + gap_offset_secs` using `fill_offset_mode` (same as fill plan) |
| **Gap interior** | `[a_start, a_end]` on A; `[b_start, b_end]` on B under nominal map |
| **Border windows** | `fill_seam_search_secs` (default 0.25 s) or `border_standoff_secs`-aware templates on each side |
| **Equivalence** | Metrics indicate B@map is the same program source as A’s border-implied content; patch ≈ identity |
| **Scan false positive** | A block-scanner silence run where A’s low-level PCM still corresponds to B’s content at sync time |

**Not in v1:** perceptual / chromaprint identity on the interior, ML classifiers, or PTS-clock comparison.

---

## 4. User-facing semantics

| Rule | Detail |
|------|--------|
| **Default off** | `skip_equivalent_gaps = false` — zero behavior change until opted in |
| **Write + scan-only** | Equivalence is computed when enabled; **plan skip** only affects write mode (phase 3). Scan-only runs may show a new **advisory** column / JSON field |
| **Full scan table** | All detected gaps remain listed; equivalence does not remove rows |
| **Conservative skip** | When metrics are ambiguous, **do not skip** — fall through to patch (prefer false negative over skipping a real dropout) |
| **Precedence** | `NotFillable`, `OutsideReferenceCoverage`, track blocks beat equivalence; equivalence beats `GapNotSelected` (when selection ships) |
| **Status string** | `not planned: already matches reference` (machine: `already_matches_reference`) |

**stderr when active:**

```text
Equivalence: 8 of 21 repairable gaps skipped (already match B at nominal map)
```

---

## 5. Algorithm

### 5.0 Existing primitives (built vs missing)

The **mechanical comparison machinery already exists** in production and fingerprint paths. What is
missing is a **single cheap equivalence block** that composes those primitives at **nominal map, lag 0**,
plus plan-time wiring. Do **not** reimplement correlation or residual math.

| Piece | Status | Location | Notes |
|-------|--------|----------|-------|
| **E1** lag-0 seam Pearson | **Built** | `domain/policies.rs` — `fill_seam_correlations`, `border_templates_for_gap` | Production patch + `gap_fill_fit` use at chosen placement; fingerprint uses `seam_local_peak` (±600 ms) for dual-fit viability — **not** the equivalence read |
| **E2** same-source residual | **Built** | `domain/policies.rs` — `seam_chosen_and_floor`, `SeamResidualVerdict` | Fingerprint/oracle path uses `oracle_measure_residual` **after structure align / throat frame** — too expensive for equivalence |
| **E3** donor interior @ nominal | **Built** | `domain/donor.rs` — `donor_interior_at`, `program_quiet_at_nominal` | Already computed in `characterize_gaps_from_decode` as `donor_interior_nominal` |
| **E4** A gap quiet / floor | **Built** | Scan `Gap` fields + level profiling in fingerprint | Bounded A extract still needed at plan time |
| **E5** interior bridge (v1.5) | **Not built** | — | Optional later |
| **`measure_cheap_equivalence`** | **Not built** | planned `domain/gap_equivalence.rs` | Composer + conservative policy (§5.4) |
| **Plan-time skip** | **Not built** | `build_gap_fill_plan` | No `AlreadyMatchesReference` in `GapFillSkipReason` today |
| **Cheap equivalence block type** | **Not built** | planned `CheapEquivalenceArtifacts` (§5.3) | Lag-0 Pearson + nominal residual only |

**Implementer rule:** equivalence = **new orchestration on old primitives**, not new seam scoring.

### 5.1 Placement in pipeline

```text
Align → ScanGaps → [CheapEquivalenceBlock?] → build_gap_fill_plan → PatchAudio → [characterize_region if not skipped]
```

- **v1 hook:** `build_gap_fill_plan` (plan-time). Run the **cheap equivalence block** as early as possible —
  immediately after scan/report, **before** any per-gap bracket search, dual-fit, or structure align.
- **Design priority:** one bounded A+B extract per gap → **lag-0 Pearson + nominal residual** (E1–E4) →
  verdict. This is intentionally cheaper than fingerprint `splice_dualfit` / oracle throat residual.
- **Artifact reuse (required):** anything computed in the cheap block that later stages might need must be
  **stored on a per-gap artifact struct** and passed forward so characterize/patch does not re-decode or
  re-score the same windows (§5.3, §7.1).
- **Phase 0:** emit the same block under `--gap-fingerprints` for a licensed 5.1 pair tuning without enabling production skip.

### 5.2 Per-gap inputs

From existing report + alignment:

- `Gap` geometry on A; `video_b_*` map; `b_has_energy`
- `gap_offset_secs` per `fill_offset_mode` (reuse `resolve_gap_offset_secs`)
- `sample_rate`, `channels` (downmix policy: **mono mean** for metrics, same as structure path)
- Config thresholds (§6)

Skip early when:

- `!gap.is_fillable()` → existing `NotFillable`
- `gap_outside_reference_coverage` → existing reason
- Gap duration `< equivalence_min_gap_secs` (default 0 — no floor) or `> equivalence_max_gap_secs` (default none — long spans need patch path)

### 5.3 Dedicated cheap equivalence block

Implement in new `domain/gap_equivalence.rs` (name illustrative). The block is the **only** equivalence
measurement path in v1 — do not piggyback on fingerprint dual-fit or oracle throat residual.

**Core measurements (v1 — run in this order, bail early when cheap):**

| ID | Measurement | Cost | Reuse | Equivalence call pattern |
|----|-------------|------|-------|--------------------------|
| **E4** | **A gap RMS vs floor** | O(gap) | scan `Gap` + block RMS | Confirm A is a dropout; skip E1–E3 work when A is not quiet |
| **E3** | **Donor interior @ nominal** | O(gap) | `donor_interior_at` | `b_mapped_start .. b_mapped_start + gap_frames` on B mono — **occupied** required (not program-quiet) |
| **E1** | **Nominal seam Pearson** `pre₀`, `post₀` | O(seam) | `fill_seam_correlations` | `SeamPlacement { start: b_mapped_start, gap_frames, pre_window, post_window }` at nominal map — **lag 0** (no shoulder search) |
| **E2** | **Same-source residual @ nominal** | O(seam) | `seam_chosen_and_floor` → `SeamResidualVerdict::from_parts_with_placement` | Pre/post at nominal throat with `chosen_delta = 0` — **do not** call `oracle_measure_residual` or `gate_structure_align` |
| **E5** | **Interior bridge correlation** (v1.5) | O(gap) | new | Correlate B interior with linear bridge from A pre→post envelopes |

**Explicitly out of scope for the cheap block:** `seam_local_peak` (±600 ms), `splice_dualfit_at`,
`baseline_lag`, bracket grid, structure search, `oracle_measure_residual` (needs throat frame from search).

#### Artifact struct (save for downstream reuse)

Decode once per gap; compute cheap metrics once; **retain artifacts** so later characterize/patch paths do
not repeat the same work when the gap is not equivalence-skipped.

```rust
/// Bounded PCM + cheap nominal metrics for one gap. Owned by characterize when patch runs later.
pub struct CheapEquivalenceArtifacts {
  // PCM (or reader window handles) — sized to seam margins + gap only
  pub a_border_templates: SeamTemplatesOwned,  // from border_templates_for_gap
  pub b_mono: Vec<f64>,                        // nominal span + seam margins
  pub b_mapped_start: usize,
  pub gap_frames: usize,

  // Cheap metrics (always populated when decode succeeds)
  pub nominal_pre: Option<f64>,
  pub nominal_post: Option<f64>,
  pub residual: Option<SeamResidualVerdict>,
  pub donor_interior_nominal: Option<DonorInterior>,
  pub a_gap_rms_db: Option<f64>,

  // Verdict (policy output)
  pub verdict: EquivalenceVerdict,
}
```

**Reuse rules:**

| Artifact | Later consumer | If equivalence-skipped |
|----------|----------------|-------------------------|
| `a_border_templates`, `b_mono`, `b_mapped_start` | `characterize_region` / structure if ever needed | Not reached |
| `nominal_pre`, `nominal_post` | Fingerprint JSON, tuning spreadsheets | Stored on skip row |
| `residual` (nominal) | Optional fast-path hint; full patch may still re-run residual at **chosen** placement | Do not assume identity with patch residual |
| `donor_interior_nominal` | `GapFingerprint`, dual-fit decline hints | Stored |
| `verdict` | `build_gap_fill_plan`, human/JSON output | Plan-time skip |

When a gap **passes** equivalence (→ `AttemptPatch`), pass `CheapEquivalenceArtifacts` into the existing
characterize path (or a shared per-gap cache keyed by gap id) so border templates and B mono are not
re-extracted. Align with [TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md) §2.5
“characterize owns B access” — equivalence characterize is the **first** B touch at plan time.

Decode strategy:

- **v1:** one bounded A+B extract per gap (reuse `fill_border_search_secs` margin + gap length) via the
  existing media reader window API used by patch.
- **Do not** run unified structure search or bracket grid inside the equivalence block.

### 5.4 Decision policy (conservative — v1)

Record all metrics on `EquivalenceVerdict` regardless of skip.

```rust
pub enum EquivalenceDisposition {
    /// Metrics say patch would be redundant / identity.
    Skip,
    /// Metrics inconclusive or contradictory — attempt patch.
    AttemptPatch,
    /// Equivalence not evaluated (disabled, missing B map, etc.).
    NotEvaluated,
}
```

**Skip** (`AlreadyMatchesReference`) only when **all** hold:

1. **E3 occupied:** `donor_interior_nominal.continuous == true` AND `silence_fraction < PROGRAM_QUIET_SILENCE_FRAC` (B has program audio across hole).
2. **E4 quiet A:** A interior RMS below scan silence floor (or `gap_peak < absolute_silence_rms`) — confirms scan target is a dropout, not a loud false negative.
3. **E1 strong seams:** `min(pre₀, post₀) >= equivalence_min_seam` (default **0.35**, same as `min_fill_correlation`).
4. **E2 same-source:** nominal residual (`seam_chosen_and_floor` at lag 0, not oracle throat) —
   `informative == true` AND `worst_headroom_db() <= equivalence_residual_headroom_db` (default reuse
   `residual_headroom_margin_db`).

**Never skip** when:

- `min(pre₀, post₀) < equivalence_min_seam` but within marginal band — **chaotic** scenes (car chase) must reach patch search.
- E2 not informative or beyond lag reach.
- E3 program-quiet (B mostly silent) — that is `unfillable`, not equivalence.
- Any metric missing (decode fail) → `AttemptPatch`.

**v1.5 optional tier — `RedundantScanDip`:** weaker rule for scan false positives only:

- A interior very quiet, `min(pre₀, post₀) >= equivalence_marginal_seam` (default 0.27), E2 pass — skip with `redundant_scan_dip` reason. **Off by default.**

### 5.5 Outputs

```rust
#[derive(Debug, Clone, Serialize)]
pub struct EquivalenceVerdict {
    pub disposition: EquivalenceDisposition,
    pub nominal_pre: Option<f64>,
    pub nominal_post: Option<f64>,
    pub residual_headroom_db: Option<f64>,
    pub residual_informative: bool,
    pub donor_silence_fraction: Option<f64>,
    pub a_gap_rms_db: Option<f64>,
    pub skip_reason: Option<EquivalenceSkipReason>,
}

pub enum EquivalenceSkipReason {
    AlreadyMatchesReference,
    #[serde(rename = "redundant_scan_dip")]
    RedundantScanDip, // v1.5
}
```

Attach to plan skip:

```rust
pub enum GapFillSkipReason {
    // ... existing ...
    AlreadyMatchesReference,
    // RedundantScanDip, // v1.5
}
```

JSON gap row (additive):

```json
"equivalence": {
  "evaluated": true,
  "skip": true,
  "nominal_pre": 0.91,
  "nominal_post": 0.88,
  "residual_headroom_db": 1.2,
  "plan_skip_reason": "already_matches_reference"
}
```

---

## 6. CLI and config (v1)

### Flags

```text
--skip-equivalent-gaps     Enable equivalence gate at fill-plan time [default: off]
--equivalence-min-seam <N> Nominal min(pre,post) to skip [default: 0.35]
```

TOML (`[repair]`):

```toml
skip_equivalent_gaps = true
equivalence_min_seam = 0.35
# equivalence_residual_headroom_db = 6.0  # default: same as residual_headroom_margin_db
```

### Interaction with scan knobs

Equivalence does **not** change scan thresholds. Document that sensitive scan + `skip_equivalent_gaps`
is the intended pair for “find everything ffmpeg sees, patch only real dropouts.”

---

## 7. Implementation sketch

### 7.1 Types and module

| Item | Location |
|------|----------|
| `CheapEquivalenceArtifacts`, `EquivalenceVerdict`, `measure_cheap_equivalence` | `domain/gap_equivalence.rs` |
| `GapEquivalenceParams` (thresholds + seam window secs) | `domain/gap_equivalence.rs` or `RepairConfig` |
| Bounded PCM extract + artifact cache | `application/gap_equivalence.rs` or extend `patch_audio` / `run_repair` extract path |
| Per-gap artifact cache (optional) | `application/patch_audio.rs` — keyed by gap; consumed by `characterize_region` when patch proceeds |
| Plan hook | `domain/gap_fill.rs::build_gap_fill_plan` |
| Orchestration | `run_repair.rs` — **cheap block before plan**; skipped gaps never enter characterize |

### 7.2 Fill plan signature

```rust
pub fn build_gap_fill_plan(
    report: &GapReport,
    crossfade_ms: u64,
    selection: &GapSelection,        // from gap-selection plan; default All
    equivalence: &GapEquivalenceSet, // per-gap verdict; default empty / not evaluated
) -> GapFillPlan
```

When `equivalence[i].disposition == Skip`:

```rust
GapFillSkipped {
    reason: GapFillSkipReason::AlreadyMatchesReference,
    ...
}
```

### 7.3 Tags and human output

`domain/gap_tags.rs`:

- `PlanKind::NotPlanned` + `plan_skip_reason: AlreadyMatchesReference`
- Human suffix: `[already equivalent]` (optional, `-v`)

`infrastructure/cli/output.rs`:

- Count line: `N skipped as already equivalent`
- Table prefix: `~` or status `not planned: already matches reference`

### 7.4 Fingerprint path (phase 0 / v1)

Add a dedicated **`equivalence`** block to `GapFingerprint` — **not** a projection of `splice_dualfit` or
oracle-throat residual. Call the **same** `measure_cheap_equivalence` / `CheapEquivalenceArtifacts` path
the production gate will use (single source of truth).

```json
"equivalence": {
  "evaluated": true,
  "disposition": "skip",
  "nominal_pre": 0.91,
  "nominal_post": 0.88,
  "residual_headroom_db": 1.2,
  "residual_informative": true,
  "donor_silence_fraction": 0.04,
  "a_gap_rms_db": -52.1,
  "plan_skip_reason": "already_matches_reference"
}
```

Emit under `--gap-fingerprints` even when `skip_equivalent_gaps=false` so a licensed 5.1 pair tuning does not require
write mode. Fingerprint may still run heavier diagnostic fields (`seam_probe`, `splice_dualfit`, throat
residual) **in parallel** for comparison during phase 0 — but the `equivalence` block must remain the cheap
lag-0 + nominal-residual read.

---

## 8. Interactions

| Feature | Behavior |
|---------|----------|
| **`--gap-fingerprints`** | Always record cheap `equivalence` block when characterized; heavier diagnostic fields optional; production skip independent |
| **Artifact cache** | Non-skipped gaps pass `CheapEquivalenceArtifacts` into characterize — no second bounded extract for saved border/B windows |
| **`anchored_retry`** | Equivalence evaluated on pass 1 plan; skipped gaps never anchor pass 2 |
| **`dual_fit`** | Equivalence skip is plan-time — dual-fit never reached |
| **Gap selection** | Equivalence skip wins over “selected for patch”; selected + equivalent → `already_matches_reference`, not `gap_not_selected` |
| **Query-reference / coverage** | `OutsideReferenceCoverage` before equivalence |
| **6ch** | Downmix to mono for metrics; document that per-channel mismatch may false-negative equivalence |

---

## 9. Validation corpus

### 9.1 a licensed 5.1 pair external row (primary)

Add `gap_corpus` **external** case (or harness golden) when media available:

| File | Role |
|------|------|
| `the reference (A)` | A (reference) |
| `the second recording (B)` | B |

ffmpeg ground truth: 14 silences at `noise=-60dB:d=0.5` on the licensed A master.

**Acceptance (tune thresholds):**

| Set | Expected equivalence disposition |
|-----|----------------------------------|
| 14 ffmpeg anchors on the licensed pair | `AttemptPatch` or `NotEvaluated` — **must not** `AlreadyMatchesReference` |
| Scan extras with sensitive recipe (~25 gaps) | Majority `AlreadyMatchesReference` when `skip_equivalent_gaps=true` |
| Mutual silence (#1 leading, #39 tail on that scan) | `NotFillable` before equivalence runs |

### 9.2 Synthetic fixtures (CI)

| Fixture | Expect |
|---------|--------|
| `a_gap_b_same_master` | B = A with zeroed gap; nominal seams ≈ 1.0, residual clean → **Skip** |
| `a_gap_b_shared_pause` | B also silent → **NotFillable**, equivalence not run |
| `a_gap_b_different_content` | B has wrong clip → low seams → **AttemptPatch** |
| `a_quiet_dip_b_matches` | Low-level A dip, B correct → **Skip** (v1.5 `RedundantScanDip` if enabled) |
| `a_dropout_chaotic_seams` | Real silence, B correct but Pearson < 0.35 → **AttemptPatch** (car-chase guard) |

---

## 10. Phased delivery

| Phase | Scope |
|-------|-------|
| **0 — Cheap block + fingerprint** | `measure_cheap_equivalence` + `CheapEquivalenceArtifacts`; `equivalence` block in `--gap-fingerprints`; tune on a licensed 5.1 pair; no production skip |
| **v1** | `skip_equivalent_gaps`, plan-time skip, artifact cache into characterize, `AlreadyMatchesReference`, human + JSON output, synthetic tests |
| **v1.5** | `RedundantScanDip` tier; interior bridge (E5); `--equivalence-min-seam` exposure |
| **v2** | Batch equivalence in scan phase (reuse B silence map decode); optional `equivalence_mode: aggressive\|conservative` profile |

---

## 11. Implementation checklist

### Phase 0 (cheap block + fingerprint)

- [x] `domain/gap_equivalence.rs` — types (`GapEquivalenceParams`/`EquivalenceVerdict`/`…Disposition`/
  `…SkipReason`/`CheapEquivalenceMetrics`) + §5.4 policy `equivalence_verdict` + E1 `nominal_seams` + unit tests
  *(the `CheapEquivalenceArtifacts` PCM-cache is a v1 reuse concern, not built)*
- [x] `application/gap_equivalence.rs` — `measure_gap_equivalence` composing E1–E4 at nominal/lag-0 + the
  A↔B coordinate contract + end-to-end synthetic tests *(bounded reader extract is v1; Phase 0 runs on the
  already-decoded dump buffers)*
- [x] Wire **same** function into `gap_fingerprint.rs` from-decode path (`equivalence` block on every gap)
- [x] JSON schema — documented in [gap-fingerprint.md](gap-fingerprint.md) § `equivalence` *(the production
  gap-row `equivalence` in `json-output.md` is a v1 concern — no production output in Phase 0)*
- [ ] Run on a licensed A/B pair; spreadsheet: gap #, ffmpeg?, cheap equivalence metrics vs dual-fit/oracle (sanity)

### v1 (production gate)

- [ ] `RepairConfig`: `skip_equivalent_gaps`, `equivalence_min_seam`, `equivalence_residual_headroom_db`
- [ ] `Args` + `cli/mod.rs` overrides
- [ ] `GapFillSkipReason::AlreadyMatchesReference` + formatters / `gap_tags` / `PlanKind`
- [ ] `run_repair.rs` — cheap block **before** `build_gap_fill_plan`; artifact cache for non-skipped gaps
- [ ] `characterize_region` / `patch_audio` — consume cached `CheapEquivalenceArtifacts` (no re-extract of saved windows)
- [ ] `build_gap_fill_plan` hook + domain tests
- [ ] Human report + JSON gap fields
- [ ] Integration: synthetic same-master skip; chaotic seam still patches; verify no double-decode on patch path
- [ ] Docs: `gap-scan.md` § equivalence, `gap-repair-guide.md`, `pipeline.md` §3 paragraph

### v1.5

- [ ] `RedundantScanDip` reason + config flag
- [ ] Interior bridge correlation (E5)

---

## 12. Test plan

| Layer | Cases |
|-------|-------|
| **Unit** | Same-master → Skip; decorrelated → AttemptPatch; program-quiet → not evaluated; missing decode → AttemptPatch; E2 uses nominal residual only (no structure align) |
| **Policy** | `min_seam` 0.35 blocks 0.21 car-chase nominal; 0.91 passes |
| **Artifact** | Patch path reuses cached `b_mono` + border templates — no duplicate decode for same gap |
| **Plan** | Equivalence skip excluded from `regions`; `repairable_count` unchanged; `planned_count` reduced |
| **Patch** | Skipped gap samples identical to input A in output |
| **Fingerprint** | Equivalence block present; matches plan verdict when both run |
| **External** | a licensed 5.1 pair: zero ffmpeg anchors equivalence-skipped |

---

## 13. Non-goals

- Replacing `b_has_energy` / mutual-silence detection
- Skipping gaps based on ffmpeg timestamps alone (no ffmpeg runtime dependency)
- Proving perceptual identity to listeners — statistical same-source only
- Equivalence on gaps **without** B map / track mismatch pairs
- Auto-tightening scan thresholds from equivalence results (separate workstream)
- v1 **content-based** “A and B both have audio and sound the same” (non-silent A) — future extension

---

## 14. Open decisions

1. **Default `equivalence_min_seam`:** 0.35 (match `min_fill_correlation`) vs 0.40 (stricter skip) — recommend **0.35** with conservative AND on residual.
2. **Decode cost:** per-gap extract vs batch B decode — v1 per-gap acceptable for ≤50 gaps; profile for 100+.
3. **Scan-only advisory:** show `~` in table when equivalence would skip but write mode off — recommend **yes** when `--skip-equivalent-gaps` set.
4. **Cell name in [gap-vocabulary.md](gap-vocabulary.md):** **Already-equivalent** vs **Redundant-fill** — recommend **Already-equivalent** for residual-confirmed; **Redundant-scan-dip** for v1.5 weak tier.
5. **Order vs [TEMP-gap-selection-plan.md](TEMP-gap-selection-plan.md):** implement equivalence first (automatic noise reduction) or selection first (manual control) — recommend **equivalence fingerprint (phase 0)** immediately, **v1 gate** after phase 0 tuning; selection can ship in parallel.

---

## 15. Promotion / done criteria

When v1 ships:

- Mark status **v1 done**; move operator contract into [gap-repair-guide.md](gap-repair-guide.md) and [gap-scan.md](gap-scan.md).
- Add **Already-equivalent** cell to [gap-vocabulary.md](gap-vocabulary.md).
- Link from [pipeline.md](pipeline.md) fill-plan section.
- Keep phase 0 / v1.5 notes here until implemented or archived.

**Done means:** on a licensed 5.1 pair sensitive scan, `skip_equivalent_gaps=true` removes ≥80% of non-ffmpeg extras from the fill plan without equivalence-skipping any ffmpeg anchor gap (manual verification + external corpus row).
