# Cross-codec seam validation — implementation plan (DRAFT)

Status: **ARCHIVED — do not implement the validator-swap.** The archival trigger is met: the full **6-pair**
run gives **`one-sided-dead = 0`** (every shoulder of every gap aligns at *some* lag), so the "cross-codec"
bucket this plan targets is confirmed to be a **measurement artifact** — gaps it flagged (R2/R4 high while
Pearson@0 dead) have *both* shoulders aligning at 0.95–0.99 against B at their **own lag**; the post-side lag
simply fell outside `seam_probe`'s ±25 ms window, so `recovered_r` looked dead and R4 (phase-invariant)
looked special. At the ±200 ms `baseline_lag` they are ordinary **silence-splices**. The real direction is
[TEMP-seam-splice-dualfit-plan.md](../TEMP-seam-splice-dualfit-plan.md) (independent per-seam fit + length
reconciliation, validated by the existing gate — no loosening). **Retained:** `domain/seam_robust.rs` and the
R2/R4 fingerprint fields stay as **diagnostics**. NOTE: a genuine one-sided-dead gap in a future pair (e.g.
the 7th fileset) does **not** revive this plan — that signals genuinely-different content, where an R2/R4-high
"loosen the validator" rule is the dangerous false-accept; re-examine via the retained R2/R4 diagnostics, not
this validator-swap. _Original status below._

_Original status:_ **DRAFT — not started.** **Hypothesis test only until Phase B go/no-go is recorded here.**

Implements the redirect in [TEMP-gap-vocabulary-redesign-plan.md](TEMP-gap-vocabulary-redesign-plan.md)
§7d. **Does not** implement [TEMP-w5-timing-offset-rescue-plan.md](TEMP-w5-timing-offset-rescue-plan.md)
(shelved: no per-seam detect-and-warp).

Reading: [seam-scoring.md](seam-scoring.md), [gap-fingerprint.md](gap-fingerprint.md).

---

## 0. Framing — what this is and is not

**This is not a "timing-offset rescue."** The lag-fingerprint arc (g003 → `timing_offset` verdict →
constant/drift split → time-warp) **over-fit a mechanism** from one exemplar and a lag sweep that
answers *"does sliding this short window help correlation?"* — not *"what physical fault is this?"*
Corpus work refuted clock-skew drift on real pairs (§7b vocabulary plan); uniqueness/residual funnels
(§7c) then over-claimed the opposite before §7d reframed with operator ground truth. **Treat lag
verdicts as observability, not repair routing.**

**Stable conclusion across those swings:**

| Layer | Question | Status on cross-encoded same-master pairs |
|-------|----------|-------------------------------------------|
| Placement | Where on B does this content go? | Envelope/structure largely **works** (13/19 patched) |
| Validation | Is the splice acceptable at that placement? | Sample Pearson@lag0 often **too strict** |
| Mechanism | Why do waveforms disagree? | **Open** — do not name it drift, timing-offset, or PTS until tested |

**This plan tests one narrow claim:** at the gate's **throat placement**, does a
**cross-codec-robust** seam score stay high where Pearson@lag0 is low? If yes → consider a looser
**validator** tier. If no → skips may be correct; revisit placement bounds, not warp/shift/lag-best.

**Synthetic assets** ([archive/TEMP-w5-timing-offset-diag-plan.md](archive/TEMP-w5-timing-offset-diag-plan.md),
`build_w5_timing_offset_seam`, g003 fingerprint) remain useful as *"envelope alive, waveform dead"*
**shape** regression — **not** as proof of clock skew in production.

---

## 1. Problem (one paragraph)

On **cross-encoded** pairs of the same soundtrack, unified search often finds a B placement whose
**envelope/structure** agrees with A while **sample-level Pearson at lag 0** fails and the gate skips
(`waveform_floor`). That pattern is a **validator mismatch**, not evidence of wrong placement, wrong
donor, or a recoverable ppm skew. Local lag/uniqueness/residual probes are **mis-calibrated** for this
regime and must **not** drive repair logic. Operator ground truth: these gaps are fillable when other
sections patch cleanly. This plan **measures** whether coarser seam metrics agree with placement before
any production accept/reject change.

---

## 2. Scope

| In scope | Out of scope |
|----------|----------------|
| Cross-codec seam **validation** metrics (diagnostic first) | `timing_offset_rescue`, fill warp/shift from `baseline_lag` |
| Corpus hypothesis table: robust vs Pearson @ throat | Mechanism claims (drift, buffer drop, PTS step) |
| Phase D production tier **only after** Phase B + listening/decoy checks | Routing repair on lag verdict / uniqueness / residual |
| New cross-codec synthetic fixture (`build_w5_cross_codec_seam`) | Renaming `gap_tags.rs` vocabulary (vocabulary plan P4) |

**Prerequisites (done):** `baseline_lag` at decision placement; uniqueness + residual on fingerprints
— **for CSV/analysis only**, not go/no-go inputs for this plan.

---

## 3. Hypothesis (Phase B go/no-go)

At the gate's **throat placement**, for gaps that **skip** on `waveform_floor` with structure already
strong (`min(structure_pre, structure_post) ≥ 0.55`):

> **H:** at least one candidate metric **R2 or R4** (see §4) has `min(pre, post) ≥ τ_r` while
> `min(pearson_pre, pearson_post) < 0.35`.

**Not H:** lag-best Pearson, warp, or "timing_offset class" prevalence.

| Outcome | Next step |
|---------|-----------|
| H holds on ≥ 4/5 corpus skips **and** F-decoys stay low | Phase C shadow → Phase D candidate |
| H weak or R1 ≈ structure only | No production tier; metric or placement work |
| H holds but listening bad | Validator still wrong — do not ship |

**Corpus slice:** 6-pair fingerprints — ~5 skipped + strong-structure rows. **Too small to ship alone;**
Phase D also requires synthetic decoys + manual listen on patched outputs.

---

## 4. Candidate metrics

Same border templates and B windows as `fill_seam_correlations` ([seam-scoring.md](seam-scoring.md) §1–4).

### Production candidates (prioritize)

| ID | Metric | Notes |
|----|--------|-------|
| **R2** | Band-limited Pearson @ lag 0 | Low-pass (~300 Hz) both windows, then `seam_pearson` |
| **R4** | Magnitude-spectrum correlation | rFFT magnitudes, peak-normalized Pearson; phase-invariant |

### Reference / redundancy checks (not sufficient alone)

| ID | Metric | Notes |
|----|--------|-------|
| **R5** | `structure_pre/post` @ throat | **Already on fingerprint.** If R1 ≈ R5, envelope-at-seam adds nothing — do not ship R1 as the validator. |
| **R1** | Envelope seam (50 ms bins on collar) | Implement for Phase B **correlation with R5**; promote only if it **beats** R5 on separation |

### Diagnostic only — do not wire to Phase D

| ID | Metric | Why excluded |
|----|--------|--------------|
| **R3** | Lag-best Pearson (`seam_pearson` at `baseline_lag` frac) | Re-enters the shelved lag-repair story; uniqueness-gated or not, same short-window failure mode as §7d |
| Lag verdict / `peak_r` | Fingerprint fields | Symptom logging only |
| Residual headroom | Fingerprint | Codec-blind on cross-encoded pairs |

**Phase B deliverable:** scatter table per metric — *(skipped, strong-structure)* vs *(patched)* vs
*(F-decoy / decorrelated fixture)*. Pick **R2 or R4** with best margin; record choice in Status.

---

## 5. Phases

### Phase A — Metric primitives (default tier) — **code DONE (2026-06-30; data only)**

Implemented as `domain/seam_robust.rs`: `bandlimited_pearson` (R2, ~300 Hz boxcar low-pass + Pearson) and
`spectrum_correlation` (R4, Hann-windowed rFFT magnitude Pearson). Unit tests: identical→high,
independent-noise→low, spectrum shift-invariant. Pure functions on `(a_win, b_win)` (no `SeamTemplates`,
no `baseline_lag`). **Not wired to the gate.** (R1 envelope-at-seam already lives in the seam probe for the
R1-vs-R5 redundancy check.)

**Original spec:** `crates/clip-sync-repair/src/domain/seam_robust.rs`

```rust
pub struct CrossCodecSeamScores {
    pub bandlimited_pre: f64,
    pub bandlimited_post: f64,
    pub spectrum_pre: f64,
    pub spectrum_post: f64,
    /// Diagnostic: correlation with existing structure scores (R5).
    pub envelope_pre: f64,
    pub envelope_post: f64,
}

pub fn cross_codec_seam_scores(
    templates: &SeamTemplates<'_>,
    b_start: usize,
    gap_frames: usize,
    pre_window: usize,
    post_window: usize,
    sample_rate: u32,
    bin_frames: usize,
) -> CrossCodecSeamScores;
```

**No `baseline_lag` parameter** in the public API.

**Unit tests:**

| Case | Expect |
|------|--------|
| Identical windows | R2/R4 high ≈ Pearson |
| Independent noise | All low |
| `build_w5_cross_codec_seam` (new) | Structure high, Pearson@0 low, R2 or R4 high |
| `build_w5_timing_offset_seam` | Shape regression only: envelope high, Pearson@0 low (mechanism not asserted) |

---

### Phase B — Fingerprint + corpus diagnostic (diagnostic tier) — **code DONE (2026-06-30); awaiting re-scan**

Implemented (data only): `gap_fingerprint.rs` `SeamProbe` (at the throat placement) now carries
`waveform_r` (Pearson@0), `bandlimited_r` (R2), `spectrum_r` (R4), `envelope_r` (R1), `recovered_r` (R3
diagnostic), `rms_db`/`snr_db`. The analyzer (`gap_fingerprint_corpus.rs`) prints the **seam diagnosis**
(quiet / misaligned(R3) / cross-codec(R2/R4) / unresolved), the **hypothesis cell** (`robust ≥ 0.5 while
waveform < 0.35`), per-gap R2/R4/env/recov, and a **provenance legend**. (Note: implemented on the
existing `SeamProbe` struct, not a separate `cross_codec_seams` field.)

**Next (operator):** re-fingerprint the 6 pairs with the rebuilt release binary, run
`diag_fingerprint_corpus`, then **record go/no-go + chosen metric in this doc's Status.** R1-vs-R5
(`envelope_r` vs `structure`) redundancy still to be reported.

**Original spec below.**

| File | Change |
|------|--------|
| `gap_fingerprint.rs` | `cross_codec_seams: Option<CrossCodecSeamFingerprint>` at throat placement |
| `gap_fingerprint_corpus.rs` | CSV: robust + pearson + structure; `hypothesis_cell`; **R1−R5 correlation column** |
| `diag_fingerprint_corpus.rs` | Print hypothesis table + "R1 redundant with structure?" line |

Re-fingerprint 6-pair corpus. **Update this doc's Status** with go/no-go and chosen metric.

---

### Phase C — Shadow probe (diagnostic)

Log `would_cross_codec_patch` from production gate at throat placement; **no** outcome change.

| File | Change |
|------|--------|
| `patch_region.rs` | `cross_codec_seam_probe` config (default false) |
| `diag_cross_codec_seam.rs` | Synthetic grid + decoy rows |

---

### Phase D — Production validator (flag off; **blocked on B + listen + decoy**)

```toml
cross_codec_seam_mode = "off"          # off | shadow | enforce
cross_codec_seam_metric = "spectrum"   # bandlimited | spectrum — NOT lag_best, NOT envelope-alone
cross_codec_seam_min_score = 0.55      # from Phase B ROC
cross_codec_seam_require_structure = true
```

- Tier name: **`cross_codec_seam`** (not `timing_offset_trusted`).
- Unified search + structure gate **unchanged**.
- Enforce: Pearson dead zone + structure passed + `min(robust) ≥ τ` → patch.
- Residual veto unchanged; do not require residual pass (codec-blind).
- **Manual listen** on enforce output for all corpus skips before default-on discussion.

---

### Phase E — Integration oracle (default tier)

| Case | Assert |
|------|--------|
| `build_w5_cross_codec_seam` | Primary shape fixture |
| F-decoy / decorrelated | Enforce still **skips** |
| Pearson-high clean patches | Unchanged |
| g003 WAV + fingerprint (if licensed) | Optional row — shape only |

---

## 6. Wiring checklist

```
A  seam_robust.rs (R2, R4, R1/R5 redundancy check) + cross_codec fixture
B  fingerprint + corpus hypothesis → GO/NO-GO in Status
C  shadow probe
D  enforce (only if B + listen + decoys)
E  oracle + gap-fill-modes.md § cross_codec_seam
```

**Observability only (no repair wire):** `baseline_lag`, `summarize_lag_curve`, g003 synthetic fixture.

---

## 7. Risks

1. **R1 = structure twice.** Phase B must prove incremental value or drop R1 from the doc.
2. **Quiet collars.** Flat envelope → undefined scores; require `collar_above_relative_floor` + min active bins.
3. **Wrong placement + loose validator.** Per-gap offset scatter (−60…+65 ms) may be real local error;
   robust accept could patch audibly wrong splices — **listening is mandatory**.
4. **Small corpus.** Five skips are enough to test H, not enough to calibrate τ alone.
5. **Mechanism agnostic.** Success does not validate drift, timing-offset, or PTS theories.

---

## 8. Success criteria

| Phase | Done when |
|-------|-----------|
| A | R2/R4 unit tests green; `build_w5_cross_codec_seam` distinguishes Pearson-low / robust-high |
| B | Hypothesis table printed; R1 vs R5 redundancy answered; **Status updated with go/no-go** |
| C | Shadow logs on synthetics + corpus |
| D | Enforce patches corpus skips, decoys skip, listens clean |
| E | Default-tier oracle green |

---

## 9. Related reading

| Doc | Role |
|-----|------|
| [TEMP-gap-vocabulary-redesign-plan.md](TEMP-gap-vocabulary-redesign-plan.md) | Axis decomposition; §7b–7d (read §7c–7d as measurement swings, not mechanism proof) |
| [TEMP-w5-timing-offset-rescue-plan.md](TEMP-w5-timing-offset-rescue-plan.md) | Shelved — historical |
| [archive/TEMP-w5-timing-offset-diag-plan.md](archive/TEMP-w5-timing-offset-diag-plan.md) | Synthetic shape regression only |
| [seam-scoring.md](seam-scoring.md) | Border templates |
