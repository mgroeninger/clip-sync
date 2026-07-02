# Seam-splice / dual-seam independent fit — findings + plan (DRAFT)

Status: **ARCHIVED — capture + viability proof done; §4 production repair unbuilt (A3).** Do not update this
doc for status or next steps. **Live index:** [TEMP-seam-repair-status-ledger.md](../TEMP-seam-repair-status-ledger.md)
(claim rows + **§4 wire spec** for A3). **Schema:** [gap-fingerprint.md](../gap-fingerprint.md) § Registration &
dual-fit. **Retained in code:** `baseline_lag`/`splice`/`donor_interior`/`splice_dualfit` capture,
`dualfit_target()` analyzer predicate, `diag_splice_timescale` (§3.6 experiments). **`diag_splice_dualfit` sim
deleted** — replaced by scan-native `splice_dualfit` (ledger E-tombstone). _Original status below._

_Original status:_ **DRAFT — `b_mapped` capture (A2) DONE; `diag_splice_dualfit` scaffold DONE; §4 repair unbuilt (A3).**
The *per-seam warp* and *cross-codec validator* directions are **refuted** (B2).

Supersedes: [TEMP-w5-timing-offset-rescue-plan.md](TEMP-w5-timing-offset-rescue-plan.md)
(per-seam detect-and-warp — archived, dead) and
[TEMP-cross-codec-seam-impl-plan.md](TEMP-cross-codec-seam-impl-plan.md) (R2/R4 validator-swap —
**archived**; its `domain/seam_robust.rs` + R2/R4 fingerprint fields are retained as **diagnostics**).
Confirms and concretizes the **registration/step axis** of
[TEMP-gap-vocabulary-redesign-plan.md](../TEMP-gap-vocabulary-redesign-plan.md).

Reading: [seam-scoring.md](../seam-scoring.md), [gap-fingerprint.md](../gap-fingerprint.md) § Lag fingerprint,
[gap-fill-modes.md](../gap-fill-modes.md).

---

## 0. One-paragraph theory

Each repair gap is a **quiet/silent hole in A** to be filled from **B** (a complete alternate recording
of the same master, different encoding, different start time). At the gap — inside A's silence, where two
encoder frames meet — a small amount of silence is **trimmed or duplicated**, because silence carries no
timing landmark for the encoder to keep frame timing continuous. The audio **content on each shoulder is
un-stretched**; only the **spacing between the two shoulders changes**. So the pre-seam and the post-seam
each register cleanly against B *at their own lag*, but those two lags differ by a **step**. A single
rigid donor placement cannot satisfy both seams; the repair is to fit the two seams **independently** and
reconcile the `step` with a **length edit (trim/pad) at the gap's lowest-energy interior point** — which
both seams then validate against using the **existing, unchanged waveform gate** (no loosening).
*(A nonzero step is the normal registration signature of **both** patched and skipped gaps; what makes a gap
skip is **bracket-search exhaustion**, not the step — see §1b.)*

## 1. What the corpus shows

> **Scope note:** the table below is the **early 4-pair subset** (dirs 2–5, 14 gated gaps) used to form the
> hypothesis. The **authoritative full-corpus view is all 6 pairs (19 matched, 6 skipped)** — see §1b,
> §2, and the analyzer (`splice_text` / `dualfit_scope_text` / CSV). The skip set is **6**, not the 3 shown
> here.

Per-side best-lag Pearson over ±200 ms (`baseline_lag`), with the inter-side step:

| pair·gap | outcome | step (post−pre) | pre peak_r @ lag | post peak_r @ lag |
|----------|---------|-----------------|------------------|-------------------|
| 2·g1 | **skip** | +13.1 ms | 0.982 @ −28.7 | 0.958 @ −15.6 |
| 2·g2 | **skip** | +13.6 ms | 0.982 @ −36.5 | 0.921 @ −23.0 |
| 2·g3 | patch | +4.4 ms | 0.990 @ −10.5 | 0.956 @ −6.2 |
| 3·g2 | patch | −37.6 ms | 0.988 @ +9.2 | 0.933 @ −46.9 |
| 3·g3 | patch | +40.3 ms | 0.990 @ −67.8 | 0.973 @ −27.5 |
| 4·g2 | patch | +22.9 ms | 0.986 @ −34.4 | 0.916 @ −11.6 |
| 4·g3 | patch | +17.1 ms | 0.999 @ −7.7 | 0.929 @ +9.4 |
| 5·g2 | patch | −34.7 ms | 0.999 @ −1.5 | 0.994 @ −36.2 |
| 5·g3 | patch | +72.4 ms | 0.926 @ −81.0 | 0.967 @ −8.5 |
| 5·g6 | **skip** | −32.0 ms | 0.992 @ −1.9 | 0.986 @ −34.0 |

**Findings:**

1. **Both sides align cleanly at their own lag** (peak_r 0.92–1.00) on every gated gap — content is
   **not** stretched within a side. The only thing that varies is the **step** (~0 to 72 ms).
2. **The "cross-codec" diagnosis was a measurement artifact.** 5·g6 — the gap flagged
   `cross-codec(R2/R4)` because `recovered_r` was 0.17 — has its post side at **0.986 @ −34 ms**, just
   *outside* `seam_probe`'s **±25 ms** fine-search window. R4 stayed high only because it is
   phase-invariant. Widen to the ±200 ms `baseline_lag` already captured and it is an ordinary
   silence-splice with a 32 ms step. **No genuine phase-scramble gap exists in the 4-pair data.**
3. **Patch vs skip is not a different mechanism.** Skips share the silence-splice signature; the
   discriminator is **bracket-search success, not step magnitude** (§1b): a skip is a gap where *no* bracket
   in the search space makes both seams pass Pearson@0, even though each shoulder is per-side recoverable
   (which the throat Pearson@0 + ±25 ms window hid). A large step is the *normal* signature of both patches
   and skips — it does **not** cause the skip.
4. **B fits the skipped gaps.** A is genuinely empty across the gap (clean −109 dBFS block: 2·g1 ≈ 1.28 s,
   5·g6 ≈ 3.15 s). B carries the correct content at *both* shoulders (0.95–0.99 per side). The broad
   envelope (`structure`, ±3 s) correlates with B **across** the gap (2·g1 post 0.975, 5·g6 post 0.971),
   so B has bridging content where A has none. The length reconciliation is **~1 % of the gap** (13 ms /
   1.28 s; 32 ms / 3.21 s) — a small correction, not a rebuild.

## 1b. Step does NOT predict patch vs skip — bracket-search success does

A nonzero throat step is the **normal registration signature, not the skip signature** — 18/19 matched
gaps have `|step| > 2 ms`. Patch vs skip is decided by whether **anchor/boundary search finds any bracket**
where both seams pass Pearson @ lag 0, *not* by step magnitude. Full 6-pair corpus
(`dualfit_scope_text`, current data — no rescan needed):

- **patched 13** — every one has ≥1 passing bracket; **skipped 6** — every one is bracket-exhausted (0 pass).
- `|step|` ms: patched 0.2 / 28.9 / 72.4 · skipped 9.8 / 32.0 / 71.9 — **fully overlapping**, so step does
  not separate.
- **best-bracket seam: patched median 0.62 vs skipped max 0.11** — *that* is the separator.

Canonical contrast (same +72 ms step, opposite outcome): **5·g3 patches** (18/25 brackets pass, best 0.63)
vs **1·g19 skips** (0/16 pass, best 0.07). So a skip is "per-side recoverable at the throat, but **no bracket
in the search space** makes both seams pass @0" — not "step too large." **Decision tree:**

```text
structure placement OK?
  ├─ anchor/boundary search finds a lag-0 bracket?      → patch (today; do NOT dual-fit)
  ├─ else: both shoulders recoverable at own lag + donor continuous?  → dual-fit candidate
  └─ else                                               → skip (other mechanism)
```

Dual-fit therefore targets only the **bracket-exhausted-yet-recoverable** skips. On current data that is
**3/6** (1·g19, 1·g22, 6·g6 — the clean splices); the other 3 are `alias-suspect` under the 250 ms metric
and may join once the rescan applies 1 s `peak_z`. (**Review F1 — FIXED (2026-06-30):** `baseline_lag` /
`seam_probe` / `donor_interior` / `wide_envelope` / `splice` now measure at the gate's own zero-move throat
(`oracle_throat_structure_frame` → `structure_start_frame`, via the shared `gate_structure_align` that the
gate itself uses — no duplication, no drift), replacing the divergent `place_on_b`. So "recoverable but no
bracket passes" now compares the *same* placement. **Requires a rescan with the rebuilt binary** — the prior
corpora and any scan started before this fix carry the old `place_on_b` placement.)

### 1b-i. F1 impact — verified on real data (6·g6, new-schema scan + wider-lag probe)
The new-schema scan (dirs 2–7, still at the **pre-F1 `place_on_b`** placement) flagged **6·g6 as
`one-sided-dead`**: pre 0.996 @ −92.5 ms but post **0.335 @ +199.7 ms** (pinned at the ±200 ms search edge)
— the apparent *first* genuine cross-encoding shoulder. Re-running `diag_splice_timescale` on 6·g6 with the
window reconstructed from the **geometry `b_mapped`** placement and a **±600 ms** fine lag
(`SPLICE_EXP_FINE_LAG_MS=600`) reverses it completely:

```
pre  2 s | 0.994 @ −132.8 ms | prom 0.62 | peak_z 23.4
post 2 s | 0.995 @ −132.7 ms | prom 0.61 | peak_z 24.6
```

**Both seams peak at −132.8 ms — step ≈ 0, a clean ~133 ms constant offset, peak_z ~24.** 6·g6 is *not*
one-sided-dead; it is highly recoverable. Production read +199.7 ms @ 0.335 only because `place_on_b`
diverged ~332 ms from `b_mapped`, shoving the true peak past the ±200 ms edge. **This is a quantified,
real-data demonstration that the F1 placement bug can manufacture a fake "cross-encoding" gap** — and that
the corpus still contains **zero genuine cross-encoding shoulders**. The F1-fixed rescan must confirm the
gate's own `structure_start_frame` lands at the same clean placement (i.e. `structure_start_frame` ≈
`b_mapped` here); if it instead diverges from `b_mapped` too, that is a separate registration question.
6·g6 also reconfirms the 1–2 s window (post prom 0.075 @ 250 ms → 0.61 @ 2 s) and the weighted level (quiet
center-dominant: mono −52 vs center −37 dB).

## 2. Caveats / what is NOT yet proven

- **Steps are not cleanly frame-quantized** (13, 32, 34, 37, 40, 72 ms; quantization test ≈ 0.84× chance).
  So the splice is likely **sub-frame / partial-sample** (or a resampler boundary), not a clean integer
  frame drop. Does not undermine the fix (we measure the actual step) — just don't market it as "one
  dropped frame."
- **Uniqueness must gate the step.** Several sides are periodic aliases (5·g3 pre, peak 0.926 / 2nd 0.905,
  margin 0.02). The measured step is trustworthy **only when both sides clear a peak_r floor AND a
  second-peak margin floor**. Both skips pass; some patched gaps would not.
- **Donor interior energy is unmeasured.** The `levels` profile samples **A's** gap interior (the −109 dB
  block), not B's. The cross-gap `structure` correlation (0.97) is strong indirect evidence B is not empty
  there, but it is a ±3 s envelope, not a targeted B-interior probe. See §3.
- ~~n = 4/6 pairs.~~ **RESOLVED — all 6 in (19 matched, 6 skipped).** `one-sided-dead = 0` across the
  whole corpus: every shoulder of every gap aligns at *some* lag (worst peak 0.91), so the genuine
  cross-encoding case never appears and the cross-codec validator-swap is fully refuted. Of the 6 skips,
  3 pass the strict uniqueness gate as clean splices (margins 0.31–0.50); 3 sit at 0.14–0.19
  (alias-suspect) — the gaps where the brittle uniqueness metric bites (§3.5).

## 3. Measurement additions

Diagnostic-only, like R2/R4 — no gate wiring. **Validate every candidate in the offline experiment (§3.6)
on the source media *before* committing it to the capture schema.**

### Done (analyzer-only, no re-scan)
- **Splice step + both-sides-recoverable** — `step = post_peak_lag − pre_peak_lag`, per-side `peak_r`,
  and the `min` second-peak margin, all from the ±200 ms `baseline_lag` already captured. Implemented as
  `splice_text` / `SpliceDiag` / `both_sides_recoverable` in the analyzer. **This also subsumes the old
  "widen `seam_probe` ±25 → ±100 ms" item** — `baseline_lag` *is* the ±200 ms search, captured for free, so
  the `seam_probe` window stays ±25 ms (no 4×-cost rescan) and the analyzer simply reads `baseline_lag`.

### Capture additions (need a re-scan; binary is free now that scans are done)
**Timescales/representations are now FROZEN by the §3.6a 5.1↔5.1 experiment:** correlation/uniqueness on
**mono**; uniqueness at a **1 s** window via **peak_z / prominence** (thresholds peak_z ≥ 12 / prom ≥ 0.45,
calibrate); level/SNR on the **energy-weighted downmix**; wide-envelope confirmer at **100 ms** bin.

**Implementation status (2026-06-30):**
- [x] **Robust uniqueness on `LagSummary`** — `peak_z`, `prominence`, `top2_spacing_ms` computed in
      `summarize_lag_curve` (mono); threaded into the analyzer as `GapRow.uniqueness_z` + surfaced in
      `splice_text`. Unit-tested.
- [x] **1 s lag window** — `FingerprintConfig.lag_window_secs = 1.0`; `lag_at_placement` now discovers a
      1 s border (+ lag slack) instead of the ~150 ms structure border.
- [x] **Seam-probe level/SNR on energy-weighted downmix** — `weighted_downmix_rms` over the raw seam frame
      span; correlation stays mono. Unit-tested (center-dominant 5.1 recovers ~6× over straight mono).
- [x] **Donor-interior energy** — `DonorInterior` (rms_db / silence_fraction / longest_silence_ms /
      continuous) over the gap-mapped B span; computed in the gate path next to the seam probe. Unit-tested
      (bridge vs B-side hole).
- [x] **Wide-envelope 100 ms confirmer** — `WideEnvelopeFingerprint`/`EnvPeak` (`wide_envelope_side` +
      `wide_envelope_at_placement`, mirroring `lag_pair`'s windowing so its peak lag is comparable to
      `baseline_lag`). Unit-tested.
- [x] **First-class splice-step fields** — `SpliceSummary` (`step_ms`, per-side `peak_r`/`peak_z`) from the
      mono `baseline_lag`. Unit-tested.
- [x] **F1 placement fix** — registration metrics measure at the gate's `structure_start_frame` via the
      shared `gate_structure_align` (no `place_on_b` divergence). Verified behavior-preserving (patch_region
      24/24, seam_residual integration, gap_fingerprint 13/13).
- [x] **Non-finite serialization guards** — `finite_db` (residual dB) + `finite_corr` (all
      `normalized_correlation` outputs: seam-probe R2/R4/wav/env, lag `peak_r`/`lag0_r`, wide-env). A silent
      gap cancelled to `-inf`/`NaN` → JSON `null` → strict consumers dropped the **whole** pair (the
      residual-null bug); now always finite. Analyzer also made tolerant (`Residual` fields → `Option`).
- [x] Harness projection for `donor_interior` / `splice` / `wide_envelope` + `peak_z` gating + the
      `dualfit_scope_text` C1 view — done (gates on `peak_z` when present; calibrate thresholds post-rescan).

**Capture schema is code-complete and unit-tested; full crate builds clean.** But **hold the rescan** — it
registers at `structure_start_frame`, which mis-registers quiet gaps (§3.7 / ledger A1). Implement
**`b_mapped` registration** in capture (ledger A2) *before* another multi-hour scan.
**Lag width:** pair-6 proves ±200 ms is sufficient at `b_mapped` (~−131 ms cluster inside range; ledger C4).
Widen `lag_max_lag_ms` only if a post-A2 corpus still shows edge-pinned peaks at the correct placement.
1. **Donor-interior energy** over the `b_mapped_start..b_mapped_end` span: RMS / silence-fraction of B
   through the hole + a **donor-continuity flag** (B runs unbroken pre-anchor→post-anchor). Turns "B
   almost certainly fits" into measured. *Cheap (one RMS pass).*
2. **Dual-scale uniqueness** (replaces the brittle single-rival `second_peak_r` — §3.5). A single peak is
   the wrong primitive: produced, post-leveled audio (speech cadence, beat, uniform loudness) manufactures
   small-scale periodicity, so the lag curve grows rival peaks at the period. Capture compact curve shape,
   not one number:
   - **fine waveform curve →** top-K peaks `(lag_ms, r)` (K≈5), curve `mean`/`std` (→ a `peak_z`), and
     peak half-width. Gives *prominence* (#1 vs #2) **and** *top-peak spacing* (a regular spacing **is** the
     periodicity — judge the peak *given* the period, vs scattered rivals = unique).
   - **wide bucketed-envelope curve →** 50 ms bins over a 1–2 s span, wide lag range; capture its peak
     prominence + top-peak spacing. Establishes *which segment* matches — periodicity collapses over 1–2 s
     because phrase/SFX/dynamics aren't periodic at that scale. (Bucketing is wrong for fine alignment, right
     for segment identity — the fine waveform still carries the precise lag.)
   - Robust uniqueness = combine offline: waveform prominence + `peak_z` + wide-envelope agreement
     (does the wide peak lag match the waveform peak lag?) + rival-spacing structure.
3. **Channel handling — reuse `selected_seam_channels`, fix the level, don't move correlation.** The current
   seam-probe level/metrics run on a **straight unweighted mono downmix** (`interleaved_to_mono` = sum/N;
   `policies.rs:243`), which dilutes a center-only mix by N and cancels anti-phase content — **33/84 mono
   seam sides sit below floor** (snr < 6 dB or rms < −60 dBFS). BUT existing-data check (a): selected-channel
   **correlation does *not* beat mono** (mono averaging suppresses per-channel noise; only 7/42 gaps improve
   >0.02; the captured "selected" is `selected.first()` = lowest index, *not loudest*, so it often picks
   left over center and underperforms). Therefore:
   - **Correlation/uniqueness:** keep **mono** (it's as good or better). Do **not** switch to a single channel.
   - **Level/SNR:** capture **per-channel RMS** and gate on the **loudest sufficient channel** (energy rank
     via `seam_score_channel_indices`, but take the **loudest**, not `.first()`), so a center-dialog gap
     isn't falsely flagged quiet by a /N downmix.
   - **Candidate to test in §3.6:** an **energy-weighted downmix** (weight channels by energy) as the mono
     reference — louder than straight mean (no /N dilution of a dominant channel) yet less noisy than one
     channel. May be the best single reference for *both* level and correlation.

### §3.5 Why the single-rival uniqueness is brittle (confirmed)
`second_peak_r` = `peak − tallest competing local maximum` (`secondary_peak_r`, `gap_fingerprint.rs:504`),
computed on the **raw waveform** lag curve (not the bucketed envelope — that drives only `structure` and the
probe's `env_r`). On quiet quasi-periodic content one rival at 0.81 drags the margin to 0.16 even when the
0.98 peak is overwhelmingly dominant. The flaw is the *metric*, not the representation — hence §3.2.

### §3.6a Experiment results — pair 1, gaps 3/19/22 (2026-06-30)
Run of `diag_splice_timescale` on the real media. **A is 5.1 center-dominant. B is a multi-track container
(stereo + 5.1); the pipeline channel-matches A's 5.1 to B's 5.1** (`select_track_for_reference`,
`policies.rs:45`), so the corpus analysis is genuine 5.1↔5.1. *(NOTE: the first experiment run mistakenly
decoded B's `a:0` = the years-old stereo downmix; the harness now probes streams and channel-matches like
the pipeline. Re-run pending — the A-side trends below hold, but the cross-track numbers used the wrong B
track.)* The damage mechanism is the **4K upscale re-encode of A**, which punches the holes; B is the
intact original donor.
- **Level → straight mono is wrong here.** ch2 (center) ≈ −35 dBFS carries the content; L/R ≈ −60; LFE
  ≈ −98 (silent); surrounds ≈ −66. Straight mono (÷6) = **−50 dBFS — ~13–15 dB low**. Energy-weighted
  = −37, loudest-channel = −35.5. **Decision: level/SNR on the energy-weighted downmix** (≈ loudest, keeps
  all channels).
- **Uniqueness → a timescale problem, confirmed.** gap-3 *pre* (the worst alias-suspect, margin 0.14 at
  the 250 ms baseline): prominence 0.10→0.53→**0.61**→0.69 and peak_z 6.1→13→**15**→18 as the window goes
  250 ms→500 ms→**1 s**→2 s. Ambiguous at 250 ms, decisively unique at **1 s**. gaps 19/22 already strong
  and stable. **Decision: capture uniqueness at a 1 s window via peak_z / prominence** (split ≈ z ≥ 10 /
  prom ≥ 0.4), retiring the 250 ms single-rival `second_peak_r`.
- **Wide-envelope** confirms segment identity (peak 0.97–0.99 @ 50–100 ms bins) — keep as the secondary
  dual-scale check.
**Channel-matched 5.1↔5.1 re-run (FINAL — decisions frozen):**
- **Correlation downmix = mono.** The `[repr @1 s]` sweep is *identical* across mono / weighted /
  loudest-channel on every gap (e.g. gap-3 pre 0.920/0.608 vs 0.918/0.607) — Pearson is scale-invariant, so
  the center dominating the mono sum suffices. **No per-channel correlation needed.** (Confirms the earlier
  (a) call on the correct track.)
- **Level/SNR downmix = energy-weighted** (≈ loudest). The *only* axis where representation matters; straight
  mono's −13–15 dB dilution is what over-flagged these gaps "quiet" though they correlate 0.92–0.99.
- **Uniqueness window = 1 s; stat = peak_z / prominence.** At 1 s all pre/post sides are decisively unique
  (prom ≥ 0.58, peak_z ≥ 15); at 250 ms gap-3-pre is ambiguous (0.10). Start thresholds **peak_z ≥ 12 /
  prom ≥ 0.45**, calibrate on the rescan distribution. Retire the 250 ms single-rival `second_peak_r`.
- **Wide-envelope = 100 ms bin, secondary.** Segment match 0.97–0.99 **and** its peak lag (~100–120 ms)
  agrees with the fine-waveform lag (~110–127 ms) — the cross-scale concordance check works.
- B-stereo-mono ≈ B-5.1-mono (stereo is a downmix of the 5.1), so the first wrong-track run was only wrong
  in principle, not materially — conclusions stable.

### §3.6 Offline validation experiment — DONE (`diag_splice_timescale`, pair 1; §3.6a records the frozen results)
`corpus.json` stores only scalars (no curve, no samples), so the metrics can't be prototyped from it. But
every gap carries `a_refined_start/end` + `b_mapped_start/end`, so given the **source media** the exact
windows reconstruct. The diagnostic (a few gaps, not the hour-long pipeline) loads pair 1 (+ optionally
6), and for each gap:
- reports **mono vs per-channel vs energy-weighted** level (resolves the cancellation-vs-quiet question §3.3);
- sweeps **window size** (250 ms / 500 ms / 1 s / 2 s) and **bin size** for the wide envelope;
- computes **top-K peaks → prominence, `peak_z`, top-2 spacing** on the fine waveform;
- shows where uniqueness becomes meaningful for the alias-suspect gaps (1·g3, 2·g1, 5·g6).
**This test assembled the full candidate-metric list (1–3 above + the splice/donor fields) so the *winning*
timescales/representations were chosen from data, not guessed, then frozen into the capture schema (§3.6a).**

Knobs: `SPLICE_EXP_GAPS` (which gaps), `SPLICE_EXP_SR`, and `SPLICE_EXP_FINE_LAG_MS` (default 200; widen to
probe a lag pinned at the ±200 ms edge — the B-context pad scales to match). Used in §1b-i to settle 6·g6's
"one-sided-dead" as a clipped large offset at the wrong placement, not decorrelation.

## 3.7. Quiet-gap registration — `b_mapped` policy (+ outward-anchor diagnostic)

**Problem (A1 — proven).** A gap in a larger quiet/low-volume section has **no distinctive signal at the seam**
and the gate's structure search (`structure_start_frame`, which F1 registers on) **wanders** on flat envelopes
→ shoulders read dead. This is **not** decorrelation — the gross A→B map is already right. **Proven:** 6·g6 and
7·g3 read one-sided-dead at `structure_start_frame` but clean at `b_mapped`; pair-6 sweep (2026-06-30, 4 s
reach) found **5/5** one-sided-dead gaps recoverable at `b_mapped` (~−131 ms constant offset, both
shoulders 0.98+): 6·g2, 6·g6, 6·g7, 6·g9, 6·g10.

**Primary fix (B13 — policy decided, CAP pending).** Register lag / detect metrics at **`b_mapped` nominal**
(geometry `b_mapped_start_secs` → B frame) and run the **existing ±200 ms centered lag sweep**. On pair-6 the
ordinary search at `b_mapped` already finds the peak — no outward search required. **Capture change:** move
`baseline_lag`, `seam_probe`, `donor_interior`, `wide_envelope`, and `splice` off
`oracle_throat_structure_frame` onto `b_mapped` nominal in `gap_fingerprint.rs`. The gate may still use
structure alignment for bracket scoring; fingerprint **registration** coordinates must not follow the wander.

**Outward-anchor (operator idea — diagnostic only, not primary).** Search outward from each shoulder to a
loudest feature, lag-align there, carry lag back. Built in `diag_splice_timescale` as `[outward-anchor]`
(RMS-loudest `500 ms` window within ±4000 ms). **Pair-6 sweep result:** it is **`b_mapped` placement** that
resolves one-sided-dead, not outward-anchor per se. RMS loudest is an **imperfect** selector — loudest ≠ most
unique (6·g9 pre z 22→9, 6·g10 pre z 27→9 when anchor lands on sustained tone). Sometimes helps (6·g2 pre
z 3→7, 6·g6 post z 10→14). **Do not wire RMS outward-anchor into production capture** (ledger D10). If
revisited, select by **`peak_z` distinctiveness**, not RMS.

**What outward-anchor still explains.** It articulates *why* a quiet shoulder fails at the edge (uniqueness
ceiling + structure wander) and why the diagnostic `lag` field (best-energy bracket) can read higher `peak_z`
than the quiet throat. It remains useful as a harness experiment, not as the capture registration method.

**Per-side registration and step.** At `b_mapped`, pair-6 gaps show **constant offset** (~−131 ms, step ≈ 0)
with both shoulders recoverable — the stepped-splice vs constant-offset taxonomy still applies once
registration is trustworthy.

**Boundary condition (outward-anchor only).** A carried lag is valid only if the anchor feature is in the
**same coherent span** as the seam — no other splice between feature and gap.

**Building blocks that already exist:**
- `geometry.b_mapped_*` — stable gross map (the registration anchor).
- `diag_splice_timescale` — timescale/uniqueness sweeps + `[outward-anchor]` block.
- `anchors` / `gap_anchor_seam` — feature location (for a future `peak_z`-ranked anchor if needed).
- **Gap:** capture now uses `b_mapped` for registration metrics; gate bracket scoring still uses structure alignment.

**Status.** Pair-6 one-sided-dead sweep **complete** (C1/B2). Pair-7 spot-check **7·g3/7·g4** (C2 **proven**).
**`b_mapped` in capture** (A2) **done**. **`diag_splice_dualfit` scaffold** (§4.1) **done**. **Next:** rescan →
run dualfit diag on bracket-exhausted skips (C3/C7) → wire §4 repair.

## 4. Repair approach (unbuilt — design)

1. **Detect:** gap is a dual-fit candidate when it is **skip + bracket-exhausted** (§1b), **both shoulders
   recoverable** at their own lag (the frozen 1 s `peak_z` / prominence thresholds, §3.6a), and **donor
   continuity** holds (§3 capture item: `donor_interior.continuous`). Note (§1b): detect must NOT run on
   gaps that already patch.
2. **Fit each seam independently** at its own per-side lag (Lpre, Lpost). Lags come from **`b_mapped`
   registration** (§3.7) — center the lag search on geometry `b_mapped` nominal + ±200 ms sweep — not
   `structure_start_frame`. Outward-anchor is diagnostic-only (D10).
3. **Reconcile the step:** the donor's bridging segment is `step` longer/shorter than A's hole. Trim or
   pad `|step|` at the **lowest-energy interior sample** of the fill (smallest audible splice).
4. **Validate with the EXISTING gate:** after reconciliation both seams butt against their B-matched
   content and should reach their per-side peak_r simultaneously. A bad length edit fails the unchanged
   waveform Pearson exactly as a bad shift does today — **strict gate, no loosening**. This is the key
   property: the fix earns the existing validator rather than relaxing it.
5. **Reject** to skip (as today) if either seam fails post-reconciliation, uniqueness margin is too thin,
   or donor-continuity is false.

Open: the "no single shift fixes both seams" geometry is exactly what the length edit addresses; confirm
the reconciliation is a pure trim/pad (not a within-side warp) — §1 finding (1) says content is
un-stretched, so it should be.

### 4.1. Offline gate simulation — `diag_splice_dualfit` (scaffold)

**Purpose.** De-risk §4 steps 2–4 **before** wiring repair (ledger C3/A3, C7). Answers: *if we apply
independent per-side lags at `b_mapped` and reconcile the step-length mismatch, do both seams pass the
**unchanged** production Pearson gate?*

**Location.** `crates/clip-sync-repair/tests/diag_splice_dualfit.rs` — tier **diagnostic**
(`diagnostic-tests` feature). Sibling: `diag_splice_timescale` (§3.6 registration/uniqueness experiments).

**Inputs (from `corpus.json` + media):**

| Source | Fields used |
|--------|-------------|
| `geometry` | `a_refined_*`, `b_mapped_*` — decode windows + B anchors |
| `baseline_lag` | mono pre/post `frac_lag_ms`, `peak_r` (longest `window_ms` entry per side) |
| `splice` | `step_ms` — cross-check vs computed `post − pre` lag |
| `outcome` / `brackets` | tier, bracket pass count, best seam — compare to simulation |

**Algorithm (per gap):**

1. Decode A/B mono around the gap (ffmpeg span decode, channel-matched B stream — same as timescale diag).
2. Read per-side lags `Lpre`, `Lpost` from `baseline_lag` (registered at **`b_mapped`** in current capture).
3. Align B shoulders independently: `b_pre_aligned = b_mapped_start + Lpre`, `b_post_aligned = b_mapped_end + Lpost`.
4. Extract the B **bridge** `[b_pre_aligned .. b_post_aligned]`; compute `Δtrim = len(bridge) − gap_frames`
   (should equal step in samples).
5. **Reconcile** (reported): trim or pad `|Δtrim|` at the lowest-RMS interior sample (§4 step 3 model).
6. **Score post-dual-fit seams @ lag 0:** Pearson on A pre/post border vs B at the lag-aligned positions,
   using `fill_seam_search_secs` window (default **250 ms**).
7. **PASS** if `min(pre, post) ≥ min_fill_correlation` (0.35) **and** `≥ fill_absolute_floor` (0.12) —
   same thresholds as production gate.

**Default gap selection:** all gaps with `outcome.tier == skip` and `baseline_lag` present. Override with
`SPLICE_DUALFIT_GAPS`; set `SPLICE_DUALFIT_ALL=1` to include patched gaps.

**Run:**

```powershell
$env:SPLICE_DUALFIT_CORPUS = "gap-files/1/corpus.json"   # or SPLICE_EXP_CORPUS
$env:SPLICE_DUALFIT_A = "F:\Video\A.mkv"                 # or SPLICE_EXP_A
$env:SPLICE_DUALFIT_B = "F:\Video\B.m4v"                 # or SPLICE_EXP_B
$env:SPLICE_DUALFIT_GAPS = "19,22"                       # optional — bracket-exhausted skips
# optional: SPLICE_DUALFIT_MIN_CORR=0.35  SPLICE_DUALFIT_ABS_FLOOR=0.12  SPLICE_DUALFIT_SEAM_SECS=0.25
cargo test -p clip-sync-repair --features diagnostic-tests --test diag_splice_dualfit -- --nocapture
```

**Scaffold limits (not yet §4 repair):**

- Scores **independent lag alignment** at the shoulders — the core P1 question — but does **not** splice the
  reconciled fill into a haystack and re-score through the full gate path.
- **Mono** downmix only (matches frozen §3.6a correlation choice).
- On-disk `gap-files/` predates **`b_mapped` capture** until re-scanned — stale `baseline_lag` will mislead;
  prefer a fresh fingerprint pass before trusting C3 results.
- Length reconcile is **computed and reported** (`Δtrim` vs `splice.step_ms` for C7) but seam PASS/FAIL does
  not yet depend on the trimmed bridge content.

**Status:** scaffold **built** (2026-06-30); **C3/C7 proof OPEN** until run on a `b_mapped` rescan of the
primary cohort's bracket-exhausted skips.

## 5. Status / next

**Next-steps are maintained in one place — the ledger's critical path:**
[TEMP-seam-repair-status-ledger.md](../TEMP-seam-repair-status-ledger.md) §A. This doc no longer keeps a
separate checklist (that fragmentation is how the outward-anchor idea kept getting dropped).

Done here (detail): analyzer `splice_text`/`SpliceDiag`/`both_sides_recoverable`/`dualfit_scope_text`; full
6-pair + F1-rescan analysis; §3.6/§3.6a frozen decisions; §3.7 `b_mapped` policy + pair-6/7 validation;
`diag_splice_timescale`; `diag_splice_dualfit` scaffold (§4.1); F1 + `b_mapped` capture. **Live blocker:**
rescan → run §4.1 on bracket-exhausted skips (C3) → wire §4 repair (A3).
