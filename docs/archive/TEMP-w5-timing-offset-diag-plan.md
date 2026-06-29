# W5 timing-offset gap — fixture + recoverability diagnostic (COMPLETE — archived 2026-06-29)

Status: **COMPLETE — Phases A–D done; archived.** A (fixture, incl. skip-faithful refinement), B
(self-validation), C (recoverability diag), D (g003 committed as the real `timing_offset` exemplar with
a regression test). The fixture reproduces g003's **skip** (not just its lag signature) and the gate
probe asserts it. The production correction is **deferred to a follow-on**:
[../TEMP-w5-timing-offset-rescue-plan.md](../TEMP-w5-timing-offset-rescue-plan.md) (§6b sketches it). See
§5 Phase C results, §6.

Companion to [TEMP-anchor-seam-plan.md](TEMP-anchor-seam-plan.md) and
[TEMP-w5-anchor-rescue-diag-plan.md](TEMP-w5-anchor-rescue-diag-plan.md). Reading:
[../gap-fingerprint.md](../gap-fingerprint.md) § Lag fingerprint, [../seam-scoring.md](../seam-scoring.md) §3–4.

---

## 1. Problem (one paragraph)

The anchor-rescue line of work (A6) characterized one W5 failure mode: a **decorrelated** seam where A
and B genuinely differ at the throat and a *moving editorial-anchor bracket* recovers a matchable cut.
A real fingerprinted gap — **g003** (`gap-files/68686c7f_fd11_t00-13-52_g003_full_timing_offset.json`)
— is a **different** mode that the current fixture family does not model. Its envelope/structure
aligns almost perfectly (`structure.baseline_pre 0.988`, `baseline_post 0.961`), yet **every** feasible
bracket fails identically at `failure_stage: "waveform_floor"` (baseline seam `0.019 / 0.032`; best
*moving* bracket ~0.16, never near the 0.35 floor), so moving the bracket cannot help and the gap is
skipped (`outcome: skip / "gate skipped"`). The lag fingerprint shows why this is wrong: the content is
the **same master, time-shifted** — pre-anchor `peak_r 0.996 @ −16.24 ms` (−780 samples), post-anchor
`peak_r 0.98 @ −7.9 ms` (−380 samples), both verdict **`timing_offset`**. The gate measures the seam
only at lag 0 (dead, `lag0_r ≈ −0.12`) and never applies a per-seam shift, so a genuinely recoverable
seam reads as decorrelated. **The asymmetry (−16 vs −8 ms across ~5 s) is the most informative fact:**
it is a *residual drift/skew at the seam*, not the constant clip-level offset alignment already removes.

We have **no synthetic fixture** that reproduces this signature, so we cannot quantify how recoverable
the class is, nor calibrate a detector. This plan builds that fixture and a diagnostic that maps the
recoverable→unrecoverable boundary. **It does not touch the production seam gate** (deferred — see §6).

---

## 2. What this plan is / is not

| In scope | Out of scope |
|----------|----------------|
| Timing-offset fixture builder (linear-drift model) | Production seam-gate changes (detection, rescue) |
| Fixture self-validation (reproduces g003 signature) | A new patch tier / `timing_offset_trusted` |
| `diag_w5_timing_offset` recoverability sweep | Sub-frame seam alignment in the gate |
| Promote g003 as the real regression exemplar | Replacing scan / anchor candidate algorithms |

**Tier:** Phase A/B are **default** (fast domain). Phase C is **diagnostic** (`diagnostic-tests`, emits
data, no PR gate). Phase D is corpus wiring.

---

## 3. The g003 signature (the target)

| Aspect | g003 value | Meaning for the fixture |
|--------|-----------|-------------------------|
| Structure / envelope | `baseline_pre 0.988`, `baseline_post 0.961` | A small sub-frame/multi-ms shift must **not** disturb the 50 ms-bin envelope |
| Collar | `collar_above_relative_floor=true`, `collar_rms_peak_ratio 0.112` | Real content abuts the gap (not silence walk-off) → seam template stops on content |
| Seam waveform @ lag 0 | all brackets `waveform_floor`; baseline `0.019 / 0.032` | Lag-0 Pearson is **dead** → broadband collar shifted by many samples |
| Lag sweep (pre) | `peak_r 0.996 @ −780 samples (−16.24 ms)`, `timing_offset` | B = A shifted; sweep recovers near-perfect r |
| Lag sweep (post) | `peak_r 0.98 @ −380 samples (−7.9 ms)`, `timing_offset` | **Different** offset than pre → linear drift across the gap |

Pre/post anchors sit ≈5 s apart (≈829.8 s / ≈834.9 s); the lag moves +8.3 ms over that span
(≈ +1.7 ms/s). The fixture models this as a single linear time map (offset + drift).

---

## 4. Fixture model — linear drift (skew)

Decision (2026-06-29): model B as a **resample of A under a linear time map**, one skew knob, not two
independent endpoint offsets. Physically honest (A/B clock skew) and gives a single recoverability axis
to sweep.

```text
t_A = (t_B − offset0_secs) / (1 + drift)        drift = drift_ppm · 1e-6
B[j] = linear_interp(A, t_A · sample_rate)       (0 outside A's range)
```

Builder (Phase A), beside `build_w5_noise_collar_anchor_rescue` in `energy_signature_fixtures.rs`:

```rust
pub fn build_w5_timing_offset_seam(
    sample_rate: u32,
    channels: usize,
    peak_offset_secs: f64,   // anchor bursts at gap ± this (matches g003 geometry; envelope only)
    collar_secs: f64,        // real-content collar flanking the gap
    seam_offset_ms: f64,     // offset0 at the gap (≈ 16 → −780 samples)
    drift_ppm: f64,          // linear skew (≈ +1700 ppm reproduces −16 → −8 ms over ~5 s)
) -> EnergySignatureFixture
```

Construction (final, skip-faithful — see §6 for the refinement that got here):

1. **A:** a **continuous non-stationary broadband bed** flanking the gap (`fill_noise_band_limited`,
   `hold = 48`, single seed, above silence floor; per-50 ms-bin amplitude modulation), then a zeroed
   throat. No isolated tone/burst features — the bed's louder modulation bins are the anchor candidates.
2. **B = resample_linear(A, offset0, drift, ref_secs)** over the whole timeline — the bed shifts by the
   drift map (lag-0 decorrelates, sweep recovers), envelope unchanged.
3. **Overwrite B's throat** `[gap_start, gap_end]` with `fill_speech_like` (B carries audio in the
   A-only dropout, as in the other W5 fixtures).

**Why resample (not independent seeds like the noise collar):** the noise-collar fixture used
*different* seeds for A vs B → genuinely decorrelated, **no** lag recovers (`decorrelated`). Resampling
the *same* noise is the essential difference: lag-0 dies but the sweep peaks at the shift
(`timing_offset`). That single change is what makes this fixture model g003 rather than A6. The collar
shape (continuous, non-stationary, `hold = 48`) is what makes the *gate* skip it — see §6.

To reproduce g003: `seam_offset_ms ≈ 16`, `drift_ppm` chosen so the post seam lands ≈ −8 ms (≈ +1700
ppm given ~5 s pre↔post separation). The diagnostic (Phase C) sweeps both.

---

## 5. Phases

### Phase A — fixture builder (default tier) — **DONE (2026-06-29; refined skip-faithful, §6)**

`build_w5_timing_offset_seam` + private helpers `resample_linear(src, ch, offset0_secs, drift,
ref_secs, sample_rate)` (lag anchored at the gap via `ref_secs`, not accumulated from t=0),
`fill_noise_band_limited(.., hold)`, and `modulate_per_bin(.., bin_frames, seed, gmin, gmax)` in
`energy_signature_fixtures.rs`. No new struct fields (`b_dropout_shift_frames = 0`). Collar `hold = 48`
(~1 ms autocorrelation): wide enough that within-window drift still recovers under the lag sweep, narrow
enough that the pre↔post drift split across the gap defeats any single placement (§6). The first cut
used `hold = 128` + isolated triangular bursts and reproduced the lag signature but **not** the skip —
see §6 for the refinement.

### Phase B — self-validation test (default tier, fast)

New default-tier test asserting the fixture reproduces the g003 signature, using the **same** lag API
the fingerprint uses (`gap_fingerprint::{lag_correlation_curve, summarize_lag_curve, LagChannel,
LagVerdict}`) and `clip_sync::normalized_correlation`:

| Assertion | Reproduces |
|-----------|-----------|
| Pre-seam window: `summarize_lag_curve` verdict == `TimingOffset`, `frac_lag_ms` ≈ `−seam_offset_ms` | g003 pre `timing_offset @ −16 ms` |
| Post-seam window: verdict == `TimingOffset`, `frac_lag_ms` magnitude **< pre** (drift) | g003 post `@ −8 ms`, asymmetry |
| Lag-0 `normalized_correlation` at the seam is low (e.g. `< 0.2`) | all brackets `waveform_floor` |
| Peak r ≥ 0.9 at the recovered lag | `peak_r 0.996 / 0.98` |

Home: new `tests/w5_timing_offset.rs` (default tier — no `required-features`). Fast (single fixture,
direct windows, no `PatchAudio`).

### Phase C — recoverability diagnostic (`diagnostic-tests`) — **DONE (2026-06-29)**

`test_support/w5_timing_offset_diag.rs` (shared seam-lag helper `w5_timing_offset_seam_lag`, reused by
the Phase B test; sweep types + CSV) + `tests/diag_w5_timing_offset.rs` (binary). Two diagnostics:

**1. `diag_w5_timing_offset_recoverability_grid`** — sweeps `seam_offset_ms × drift_ppm`, measuring the
pre/post seam [`LagSummary`] directly (fingerprint lag API, no gate → fast). CSV columns
`seam_offset_ms, drift_ppm, {pre,post}_{lag0_r,peak_r,frac_lag_ms,verdict}, recoverable`. **Result:** a
clean recoverability boundary —

- **Constant offset (drift 0):** `lag0_r ≈ 0`, `peak_r = 1.0`, `timing_offset` for every offset up to
  the ±50 ms search edge. Perfectly recoverable.
- **Drift smears the peak:** `peak_r` falls monotonically with `|drift|` (e.g. 16 ms: 1.00 @ 0 ppm →
  0.95 @ −9000 → 0.78 @ −36000) as the within-window lag drift exceeds the collar autocorrelation
  width. Still `timing_offset` until the peak crosses 0.5.
- **Offset past the search range breaks it:** at 48 ms offset + high drift the *pre* local lag exceeds
  ±50 ms → `peak_r ≈ 0.32`, verdict flips to `ambiguous` (`recoverable=false`). This calibrates
  `classify_lag` (`peak ≥ 0.5`, away-from-0) against synthetic ground truth.

**2. `diag_w5_timing_offset_gate_probe`** — runs representative cells through the production unified gate
(`score_w5_fixture`) to see how the gate *treats* a recoverable seam. **This is where the synthetic
diverges from g003 — the key Phase C finding (§6).**

#### Phase C results — gate-probe divergence (important)

| Cell | Baseline throat (lag-0) | Joint winner | Reading |
|------|------------------------|--------------|---------|
| 16 ms, **drift 0** | `min = 0.53` | **Baseline** | A *constant* offset is a clip-level shift the **haystack slide already recovers** — not a skip, not g003 |
| 16 ms, −4500 ppm | `min = 0.09` (dead) | **Anchor** (move 48000) | Drift defeats the single slide, but a **moving anchor bracket + local re-slide** recovers it on stationary content |
| 8 ms / 32 ms, drift | `min ≈ 0.09` | **Anchor** | Same — the stationary collar is *escapable* by relocating the seam |

**The stationary fixture reproduces g003's lag *signature* but not its *skip outcome*.** g003 is skipped
(every bracket `waveform_floor`) because its **non-stationary real content** defeats both escape routes:
the coarse 50 ms-bin envelope placement cannot resolve a multi-ms lag, and a bracket move lands on
*different* audio that does not match at lag 0 either. The recoverability the lag fingerprint detects (a
**fractional shift at the same seam**) is a different operation than what the gate offers (an **integer
bracket move to a different seam**); only the former recovers g003, and the gate does not do it.

**Consequence for Phase A (refinement, next):** to reproduce the *skip*, the collar must be
**non-stationary** (or fine-structured enough that envelope placement ≠ waveform alignment) so a bracket
move cannot incidentally re-align. The current uniform band-limited collar is locally stationary →
slide/move escapes. Options: a non-repeating broadband bed (e.g. distinct noise per bin), or modulated
content where the 50 ms envelope is flat but the waveform carries unrecoverable-by-move fine structure.

### Phase D — promote g003 as the regression anchor — **DONE (2026-06-29)**

A single **curated** copy of the g003 fingerprint is committed at
`tests/gap_corpus/fingerprints/g003_timing_offset.json` — deliberately **separate** from the gitignored
`gap-files/` scratch/output dir (which `--gap-fingerprints` overwrites). It is the canonical **real**
`timing_offset` exemplar, guarded by the default-tier test
`g003_real_fingerprint_is_timing_offset_exemplar` in `tests/w5_timing_offset.rs`: every measured seam
reads `timing_offset` (lag-0 dead, strong recovered peak), the offset drifts `|pre| > |post|` (−16 vs
−8 ms), and the gate **skipped** the gap.

Why curated-copy, not committing `gap-files/`: a fingerprint is licensing-safe (numbers only, no
samples — `docs/gap-fingerprint.md`), so the worry isn't licensing; it's *fixture hygiene*. A regression
test should own one frozen, deliberately-chosen file, not depend on a scratch/output directory. So
`/gap-files/` stays gitignored and the curated copy lives under `tests/` beside the WAV corpus. The
identifying `id → title` map stays in a gitignored `*.sources.local.toml`, never committed.

One note: the exemplar is parsed via a **minimal struct** (`index`, `lag`, `outcome` only), not the
whole `GapCorpus` — it predates a `seams.per_channel` schema change (`Option`→`(f64,f64)`) and no longer
round-trips whole. The minimal parse makes the regression robust to unrelated schema drift; regenerating
the file with current code (needs the real media) would let it round-trip again.

### Wiring (Phase C) — **DONE**

`Cargo.toml` `[[test]] name = "diag_w5_timing_offset"` (`required-features = ["diagnostic-tests"]`),
`test_support/mod.rs` `pub mod w5_timing_offset_diag;`, `scripts/test-tier.ps1` `Invoke-RepairDiagnostic`
(grid in the main lane, slow gate probe via `--ignored`), `development.md` rows. Default Phase A/B/D test
(`[[test]] name = "w5_timing_offset"`, no features) registered (the crate has `autotests = false`).

---

## 6. Skip-faithful refinement — **DONE (2026-06-29)**

The Phase C gate probe showed the original stationary / isolated-burst fixture reproduced the lag
*signature* but not g003's *skip*: the gate escaped the offset (baseline slide for a constant offset; a
moving anchor bracket onto an isolated burst for drift). Making it skip-faithful took three coupled
fixes to the collar/anchor content, each falsifying a way the gate escapes:

1. **Narrow band-limited bed (`hold = 48`, ~1 ms autocorrelation).** Wide enough that within-window
   drift still recovers under the lag sweep, but **narrower than the pre↔post drift split across the
   gap** — so no single B placement aligns both seams. Defeats the baseline slide for any real drift.
   (A *constant* offset has no split → still recoverable → correctly *not* a skip.)
2. **Per-50 ms-bin amplitude modulation (non-stationary).** Gives the bed a distinctive non-repeating
   energy contour: louder bins are energy-peak anchor candidates, but they pin the structure placement
   to bin resolution (blind to the sub-bin lag).
3. **Continuous bed, not isolated bursts.** *This was the decisive one.* Isolated tone bursts let the
   anchor path escape two ways — a pure tone is shift-tolerant (correlates at lag 0 by periodicity), and
   even a *broadband* isolated burst has envelope-shift = waveform-shift, so one envelope-guided slide
   recovers it. A **continuous** drifting bed embeds the anchor peaks in content whose sub-bin lag the
   coarse envelope placement cannot resolve → **every** bracket fails `waveform_floor`.

Result (asserted by `diag_w5_timing_offset_gate_probe`): drift cells `{16 ms/−4500, 8 ms/−4500,
32 ms/−9000}` → **Skip, 0 passing brackets, all 19 `waveform_floor`**; constant `16 ms/0 ppm` →
**Baseline** (recoverable). The fixture now reproduces g003 end-to-end.

## 6b. Deferred — detection + rescue in the production gate (follow-on)

**Detection is in hand:** `summarize_lag_curve` (pub) returns the `timing_offset` vs `decorrelated`
verdict (`peak_r 0.99` vs the `< 0.3` threshold) plus the recovered pre/post `frac_lag`. It is not yet
called near the gate — wiring it there is small.

**The correction is the real work, and it has two regimes** — the original framing ("apply the recovered
`frac_lag` as a per-seam fractional shift") only covers the first:

- **Constant offset** (`frac_lag_pre ≈ frac_lag_post`): one sub-sample shift of the B fill aligns both
  seams. Easy, and largely already handled — the haystack slide recovers a constant offset, which is why
  constant offsets do **not** skip (Phase C `drift=0 → Baseline`).
- **Drift / skew** (g003: −16 ms pre vs −8 ms post): **no single shift fixes both seams.** The fill must
  be **time-warped (resampled)** by the implied rate ratio so its start aligns to the pre offset and its
  end to the post offset — inverting the A/B clock skew across the fill. `resample_interleaved` (already
  used in `patch_audio.rs` to rate-match B) is the reusable primitive; the new logic measures the skew
  and applies it per-fill, then re-gates under a new tier (`timing_offset_trusted`), residual veto
  unchanged.

The skip-faithful fixture + recoverability data now exist to justify and test it. Tracked in
[../TEMP-w5-timing-offset-rescue-plan.md](../TEMP-w5-timing-offset-rescue-plan.md) (gated on a prevalence
scan — g003 is the only real exemplar so far).

---

## 7. Related reading

| Doc | Contents |
|-----|----------|
| [../gap-fingerprint.md](../gap-fingerprint.md) | Lag fingerprint, `timing_offset` vs `decorrelated` verdict |
| [TEMP-w5-anchor-rescue-diag-plan.md](TEMP-w5-anchor-rescue-diag-plan.md) | The *decorrelated* W5 class (A6); diag harness this reuses |
| [../seam-scoring.md](../seam-scoring.md) | Seam definition, 250 ms throat, lag-0 Pearson |
| [../TEMP-w5-timing-offset-rescue-plan.md](../TEMP-w5-timing-offset-rescue-plan.md) | Follow-on: production detection + drift-resample rescue |
