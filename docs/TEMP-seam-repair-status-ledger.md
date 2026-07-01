# Seam-repair status ledger — proven / open / important (triage index)

**Purpose.** The two working docs
([TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md),
[TEMP-gap-vocabulary-redesign-plan.md](TEMP-gap-vocabulary-redesign-plan.md)) hold ~30 claims at every
stage of proof. This ledger is the **index over them**: one row per claim, scored **Confidence × Importance
× Target**, so we can see the critical path and what to incorporate. The two docs stay the detail; this is
the map. Update this when a claim's status changes.

**Legend.** Confidence: `PROVEN` (data) · `SUPP` (strong, small n) · `DECIDED` (policy chosen, not yet in code) · `OPEN` · `REFUTED`.
Importance: `CRIT` (blocks a working repair) · `HIGH` · `MED` · `LOW`.
Target: `VOCAB` · `PIPE` (detect/repair) · `CAP` (fingerprint capture) · `—` (conclusion/park/tombstone).

**Evaluation cohort (do not merge denominators).**
```text
Full-corpus rescan (dirs 1–7, real media, post sequential-fix, 2026-07-01): 7 pairs, 69 gaps, 62 matched,
  23 patched / 39 skipped. Silence-splice view: alias-suspect 32 · one-sided-dead 8 · splice 15;
  both-sides-recoverable 15/55. **Dual-fit candidates (skip + bracket-exhausted + both-sides-recoverable): 13/39.**
  This is now the primary cohort — supersedes the pre-A2 6-pair counts.
Legacy: the pre-A2 6-pair corpus was 19 matched / 6 skipped (6 bracket-exhausted). Counts in B1/B8 predate the
  rescan and refer to that legacy cohort.
```

---

## A. The critical path (do these, in order)

The claims that actually gate a working repair. Everything else is supporting.

| # | Claim / task | Conf | Why it's the blocker |
|---|--------------|------|----------------------|
| A1 | **Quiet-gap mis-registration is `structure_start_frame` wander**, not decorrelation. Proven: pair-6 (5/5 one-sided-dead), pair-7 **7·g3** (pre 0.986@+94 ms, z 18) and **7·g4** (pre 0.902@+118 ms, post 0.988@+113 ms) — all dead at F1 throat, clean at `b_mapped`. | PROVEN | Diagnosis done. Capture fixed (A2); on-disk corpora need rescan. |
| A2 | **`b_mapped` registration** — center `baseline_lag` / detect metrics on geometry `b_mapped` nominal, **sequentially per-shoulder registered** (post search centered on `S + D_A + round(L_pre)`, not the naive `S + D_A`; see [TEMP-seam-repair-status-ledger-concerns.md](TEMP-seam-repair-status-ledger-concerns.md) §Registration fix); **not** `structure_start_frame`. Outward-anchor (RMS loudest) is **not** the primary fix (pair-6 sweep). | **PROVEN (CAP)** | Gross map + ±600 ms sequential centering landed in `gap_fingerprint.rs` (`lag_pair`, `seam_probe_at_placement`, `wide_envelope_at_placement`, `donor_interior_at`); lib tests green. **Validated on full-corpus rescan (dirs 1–7, real media, 2026-07-01):** one-sided-dead collapsed 27/55 → **8/55**, confirming the fix. |
| A3 | **Dual-fit repair** (independent per-side fit → length reconcile → unchanged gate passes) actually works. | OPEN (unbuilt) | §4 repair not wired. **Prove first:** read scan-native `splice_dualfit` (`dual-fit viability` section) on bracket-exhausted skips after a re-scan — supersedes the retired `diag_splice_dualfit` sim (E-tombstone: sim decode ≠ scan decode). PASS ⇒ safe to wire §4. |
| A4 | **Donor continuity** — B carries unbroken content across the hole. | OPEN / PARTIAL | Coded; now measured at `b_mapped` in capture. On-disk `donor_interior` still from pre-A2 scans — re-measure on rescan (C5). |
| A5 | **Threshold calibration** (`peak_z ≥ 12`, prominence, continuity) on the real distribution. | OPEN — first calibration landed | Needs a **`b_mapped` rescan** (post A2). Calibrate on both patched and skipped distributions (C6). **2026-07-01 finding (dirs 1–7):** of 32 alias-suspect, only **21 fail `peak_z`** (genuinely ambiguous); **9 fail prominence-only** while `peak_z` says unique (z 12.6–26) — and **4 of those 9 were gate-*patched*** (1·g6, 1·g20, 5·g2, 6·g4), proving the flag false. The 0.45 prominence floor undercut B3's `peak_z`-primary decision. **Action taken:** `splice_diag` now makes `peak_z` primary and demotes prominence to a low-floor tiebreaker (**0.45 → 0.15**), catching only true near-duplicate rivals (6·g9 `prom 0.11`). Re-classify (analyzer-only, no rescan): both-sides-unique **15 → 23**. **`peak_z = 12` — keep for now:** gate patch/skip is an *invalid* label for it (peak_z-by-outcome is **inverted** — patched median z 6.8, skipped 21.7 — because patches don't need unique per-shoulder registration while confident splices get skipped). The earlier "5·g1/g3 patched at z 9.3 ⇒ floor too strict" note was wrong (patches aren't dual-fit targets). For the **skip** cohort, z p25 = 11.1, so 12 sits in the discrimination band. Tune only against `splice_dualfit` seam viability on skips (post-rescan). |

**Sequencing consequence:** the 2026-07-01 rescan validated registration (A2/C1/C2/C4), but predates the
scan-native **`splice_dualfit`** field (added 2026-07-01, after the rescan) — so C3/C7 need one more re-scan.
**`diag_splice_dualfit` sim retired** (E-tombstone: decode ≠ scan). **Next:** re-scan one pair → read the
`dual-fit viability` section (C3/C7) → full re-scan → calibrate thresholds (A5/C6) → wire §4 repair (A3).

---

## B. Proven — incorporate now (no more proof needed)

| # | Claim | Conf | Target | Incorporation |
|---|-------|------|--------|---------------|
| B1 | Patch vs skip = **bracket-search success, not step magnitude** (5·g3 vs 1·g19; full step overlap; best-bracket seam 0.62 vs 0.11) | PROVEN | VOCAB + PIPE | Vocab: `bracket_search` axis; W5 = "lag-0/bracket validation failed." Detect: scope dual-fit to `bracket_exhausted`. |
| B2 | **No genuine cross-encoding *type*** — `one-sided-dead` is (mostly) a placement artifact. Pair-6: **5/5** @ `b_mapped` (~−131 ms). Pair-7 spot-check: **7·g3**, **7·g4** both shoulders 0.90+ @ +94 / +118 ms. | **PROVEN** | — | Full-corpus rescan (dirs 1–7, real media, post sequential-fix, 2026-07-01): one-sided-dead collapsed **27/55 → 8/55 (49% → 14.5%)**. The 8 residual cases have genuinely dead shoulders (`peak_r ≤ 0.17`) at large steps (±300–600 ms) even under ±600 ms sequential search — a real floor, not a placement artifact. Corpus-wide PROVEN restored for the collapse; ~15% is an unrecoverable residual, not a cross-encoding *type*. |
| B13 | **`b_mapped` + sequential ±600 ms lag search** resolves quiet-gap registration — pair-6 and pair-7 (7·g3/7·g4) confirmed. | **PROVEN** | CAP | `b_mapped` pre anchor + sequential post centering implemented (A2); ±200 ms → ±600 ms widened. Post centering bug (naive `S + D_A`, stacking `L_pre` into the post search) fixed and **validated on the full-corpus rescan** (2026-07-01): registration resolves for 47/55 (one-sided-dead 8/55). |
| B3 | Uniqueness needs a **1 s window + `peak_z`** (retire 250 ms `second_peak_r`) | PROVEN | CAP (schema) | Decision frozen §3.6a. **Schema done + honored in the classifier (2026-07-01):** `splice_diag` had been OR-ing a high prominence floor (0.45) back in, over-flagging `peak_z`-unique gaps as alias-suspect (9/32; 4 gate-patched — see A5). Now `peak_z`-primary, prominence a low-floor (0.15) tiebreaker. `peak_z` confirmed periodicity-robust on leveled content (the whole-curve z-score deflates on periodic signals; prominence, a single-rival term, did not). |
| B4 | Level/SNR on **energy-weighted downmix** (straight mono `/N` buries 5.1 center 13–15 dB) | PROVEN | CAP (schema) | Frozen; schema done, corpus partial (as B3). |
| B5 | **Correlation on mono** (representation doesn't matter — Pearson scale-invariant) | PROVEN | CAP (schema) | Simplifies: no per-channel correlation. Schema done. |
| B6 | **F1 placement** — register at the gate's own throat, not a divergent `place_on_b` | PROVEN | PIPE (done) | Done via `gate_structure_align`. Quiet-gap registration is separate — **`b_mapped`** (B13/A2). |
| B11 | **Dual-fit ≠ what bracket search already does** — the winning bracket's boundary move is *not* the throat step (5·g3: +72 ms step vs 2600 ms `move_frames`; 0/18 patched gaps have `\|step\|` within 20 ms of a bracket delta) | PROVEN | PIPE | Confirms dual-fit is a distinct operation (interior length edit), not a re-run of anchor/boundary search. Scopes §4. |
| B12 | **Wide-envelope lag concordance** — 100 ms-bin envelope peak lag agrees with the fine-waveform lag | SUPP (pair 1) | CAP (schema) | Secondary registration confirmer; populate at `b_mapped` post A2. |
| B7 | **Content is un-stretched within a side** (both shoulders align at a single lag each) | SUPP | — | The premise that makes reconciliation a **pure trim/pad**, not a warp (A3). |
| B8 | Registration = **offset + step**, not clip drift (per-file slope ≈ 0; 18/19 have `|step|>2 ms`) | PROVEN | VOCAB | Registration axis; drop drift/skew framing. |
| B9 | Residual is the **wrong same-source test** for cross-encoded pairs (`informative=false` expected) | PROVEN | — | Keep as diagnostic; do not gate on it. |
| B10 | **Non-finite/residual-null serialization bug** (silent gaps → `null` → dropped whole pairs) | PROVEN + FIXED | CAP (done) | `finite_db`/`finite_corr`; analyzer tolerant. |

---

## C. Open + important — prove next (ranked)

| # | Question | Conf | Imp | How to prove |
|---|----------|------|-----|--------------|
| C1 | Does the **`one-sided-dead` bucket collapse** at `b_mapped`? | **PROVEN** | CRIT | **Yes.** Full-corpus rescan (dirs 1–7, real media, post sequential-fix, 2026-07-01): one-sided-dead **27/55 → 8/55** (49% → 14.5%). 19 gaps that were window-placement artifacts now recover both shoulders. 8 residuals are genuinely dead (large steps, `peak_r ≤ 0.17`) — the real floor, not artifacts. |
| C2 | Which **placement** for registration? | **PROVEN** | CRIT | **`b_mapped`**, sequentially per-shoulder registered (post centered on measured `L_pre`, not the naive `S + D_A`). Pair-6 + pair-7 confirmed the placement choice; RMS outward-anchor not primary (D10). |
| C3 | Does the **dual-fit repair pass the unchanged gate** on the known skips? | **OPEN — instrument moved in-scan** | CRIT | **`diag_splice_dualfit` (the sim) is retired — decode proven unreliable** (E-tombstone): its independent ffmpeg decode disagrees with the scan at the *same* 1 s window (gaps 9/11/19: sim finds the shoulders at a common offset; the scan's `baseline_lag` finds a unique, prominent step at +260/+279 ms). The repair runs on the scan/harness decode, so the sim's numbers are irrelevant. **Replaced by scan-native `splice_dualfit`** (`gap_fingerprint.rs`): per-shoulder placement, gate-equiv seam @ lag 0, on the scan's own PCM. Answer = the `dual-fit viability` section after a re-scan (one-pair validation first, then full). |
| C4 | Is **±600 ms sequentially-centered lag search** sufficient at `b_mapped`? | **PROVEN** | HIGH | **Yes, for the recoverable population.** Full-corpus rescan (2026-07-01): 47/55 register within the ±600 ms sequential window (both-sides-recoverable 15/55 + alias-suspect 32/55). Sequential centering decouples `L_pre`; residual post lags now measure `\|D_B − D_A\|` alone. The 8 one-sided-dead are dead at the shoulder itself (`peak_r ≤ 0.17`), not clipped by the window — widening won't recover them. |
| C5 | **Donor continuity** true for the skip targets? (= A4, ranked) | OPEN / PARTIAL | HIGH | Re-measure at **`b_mapped`** post A2 capture — on-disk `donor_interior` mis-reads quiet gaps (6·g6). |
| C6 | **Threshold calibration** — `peak_z`/prominence/continuity floors on the real distribution. | OPEN — prominence floor calibrated | HIGH | Calibrate on BOTH patched and skipped distributions. **First pass (2026-07-01, analyzer-only):** prominence floor 0.45 → 0.15 (was flagging `peak_z`-unique, gate-patched gaps as alias-suspect; see A5/B3). **Remaining:** `peak_z` floor (keep 12 — see A5; **do NOT** calibrate against gate outcome, it's inverted), `SPLICE_MIN_PEAK_R` (0.85), donor continuity. **Correct label = `splice_dualfit` seam viability on the *skip* cohort** (post-rescan), not gate patch/skip. |
| C7 | **Trim magnitude ≈ measured `step_ms`** | **RESOLVED (tautological in-scan)** | HIGH | Scan-native `splice_dualfit` places shoulders at their own lags, so `trim_frames = bridge − gap = step` **by construction** (no separate decode to disagree). C7 is no longer an open reconciliation risk; the open question is now *seam viability* (C3) + whether the step is *real* (new validator: `post_seam_global_r`). |

---

## D. Open + low / parked (do not spend cycles yet)

| # | Item | Why parked |
|---|------|-----------|
| D1 | **Mechanism of the step** (silence-splice vs resampler vs PTS; sub-frame, not quantized) | The repair *measures* the step; the physical cause doesn't change the fix. Interesting, not blocking. |
| D2 | **Decorrelated / different-content regime** | Untestable — this corpus is all same-master. Revisit only with different-content data. |
| D3 | **Channel-scope / donor-displacement axes** (vocab §2b) | Surface in analyzer later; not decision-relevant for dual-fit. |
| D4 | **Keep vs deprecate W-tiers**; reconcile `gap_tags.rs`/`content_hint`/`seam_shape` | Vocab P3/P4 decision; after the type set is named. |
| D5 | **Perf** (FFT lag, dedup search, decode reuse) | Deliberately deferred until the plan is proven. **FFT lag sweep now scoped** (~50–150×, `rustfft` present, gate on `fft≈naive` test) — see **Capture parked → FFT lag sweep** below. |
| D6 | **No regression on existing patches** (dual-fit flag off ⇒ unchanged) | Verify after A3 (repair built) — a run-comparison, not an open question yet. |
| D7 | **Audibility of the trim point** (splice at low-energy interior sounds clean) | After A3; gate-pass is necessary, not sufficient (needs a listen). |
| D8 | **Decoy / wrong-placement safety** (a deliberately wrong B offset still fails the gate) | After A3; corpus has only weak negatives (failed brackets), so this needs a synthetic/shifted-haystack test. |
| D9 | **Fingerprint diagnostic stubs** (F2/F3) | Gate path omits per-bracket `structure_*` and leaves `GateOutcome` vocabulary tags empty. Fine for diagnostics today. See **Capture parked**. |
| D10 | **RMS outward-anchor as primary registration** | Pair-6 sweep: loudest ≠ most unique (6·g9 pre z 22→9, 6·g10 pre z 27→9 on sustained tones). `b_mapped` + centered lag already finds −131 ms. Keep `[outward-anchor]` in `diag_splice_timescale` as diagnostic only; if revived, select by **`peak_z` distinctiveness**, not RMS. |

---

## Capture parked (fingerprint layer hygiene)

Parked **CAP** items — not on the critical path until a **`b_mapped` rescan** is worth running.

**Next CAP change (A2):** done — decision metrics register at **`b_mapped` nominal**; `residual` stays at gate
throat. Re-scan when ready.

**F1 (mostly done).** Registration metrics no longer use `oracle_throat_structure_frame`. **Remnant:** top-level `fp.structure` still comes from the summary pass's `place_on_b` and is not refreshed in the gate
overlay; corpus `structure_min` stats may disagree with the oracle throat. `fp.seams.baseline_*` is updated
from the zero-move oracle bracket.

| # | Item | Status | When to fix |
|---|------|--------|-------------|
| F2 | Gate `brackets[]` write `structure_pre/post = None` (oracle has structure internally) | OPEN | Only if analyzer needs per-bracket structure or schema/docs parity |
| F3 | `GateOutcome.seam_shape` / `fit_path` / `signature_mode` empty in gate path | OPEN | Only if vocabulary tags migrate into fingerprints |
| C-docs | `gap-fingerprint.md` omits `baseline_lag`, `seam_probe`, `splice`, `donor_interior`, `wide_envelope` | OPEN | When capture schema is frozen post A2 |
| C-harness | `uniqueness_z` uses a single-sided `splice.peak_z` when only one side is present (slightly optimistic) | OPEN | Low — tighten when calibrating A5/C6 |

**Perf (before a long rescan).** Dominant cost is still N × oracle bracket scoring (required). Avoidable
overhead today: (1) summary `characterize_gaps` still runs one `place_on_b` before the gate overlay; (2)
diagnostic `fp.lag` at the best-energy bracket adds another `place_on_b` + `lag_at_placement`; (3)
`dump_gap_fingerprints` re-decodes A/B after repair. Likely wins when rescans matter: drop summary
`place_on_b` when gate follows; share one border extract at the throat for lag + probe + wide-envelope;
reuse repair decode; **FFT lag sweep** (below).

**FFT lag sweep (`lag_correlation_curve`) — the biggest single win (~50–150×).** `lag_correlation_curve`
(`gap_fingerprint.rs`) is naive `O(n·L)`: one 1 s Pearson (`n ≈ 48k`) at every integer lag over ±600 ms
(`L ≈ 57.6k`) → ~2.8·10⁹ ops **per shoulder**. FFT drops it to `O(M log M)`, `M = n+L`. This is the
dominant scan cost (registration sweep); `peak_z`/`prominence` piggyback on the same curve for free, so
speeding the sweep speeds them too.
- **Primitive already present:** `rustfft` (`FftPlanner`) is used in `domain/seam_robust.rs` — reuse that.
- **The catch — it's *normalized* (Pearson), not raw cross-correlation.** FFT only accelerates the
  numerator `c(lag) = Σ a[i]·b[i+lag]` (= `ifft(conj(FFT(a_pad))·FFT(b_pad))`, zero-pad to `n+L`). The
  **denominator (sliding b-window mean/var) is prefix sums, NOT FFT** — precompute `cumsum(b)`/`cumsum(b²)`,
  window stats O(1)/lag; `a` stats fixed. Assemble `Pearson(lag) = (c − n·mean_a·mean_b) / (n·std_a·std_b)`.
  Forgetting the prefix-sum normalization is the classic bug.
- **Must match the naive path exactly:** the lag convention (`base = max_lag + lag`, `b_ctx[base..base+n]`)
  **and** the edge-lag mask (naive *skips* lags where `base+n > len` — test asserts `curve.len() < 2L+1`).
  `peak_z` is a whole-curve mean/std, so an off-by-one in the included-lag set shifts it.
- **Calibration-neutral IF gated:** f64 rustfft is ~1e-10 relative — negligible for `r`/`peak_z`/`prom`/
  `frac_lag`, and the peak lag is robust. The *only* way it drifts the z=12 / prom=0.15 floors (A5/C6) is a
  porting bug → gate behind a **regression test: `fft_curve ≈ naive_curve` within tight ε** (assert
  `peak_z`/`prominence`/`frac_lag` specifically).
- **Keep a naive fallback for small curves:** the same fn runs the ±25 ms seam probe and 100 ms-bin envelope
  (tiny `L`) where FFT overhead loses — auto-select by `n·L`.
- **Sequencing:** land it *after* the dual-fit rescan + A5/C6 threshold calibration — behavior-preserving,
  but you want a stable naive baseline to write the equivalence test against, not to change the engine
  under a metric mid-calibration. Est. ~1 day incl. tests.

**Do not optimize first:** `donor_interior` RMS; parallel per-gap loops before deduping search and aligning
placement.

---

## E. Refuted — tombstone (do not revive)

| Hypothesis | Verdict |
|------------|---------|
| Per-seam detect-and-warp rescue | Refuted / archived — step is local, content un-stretched |
| Cross-codec validator-swap (R2/R4 loosen the gate) | Refuted — measurement artifact; plan archived (R2/R4 kept as diagnostics) |
| Clip drift / time-warp | Refuted — offset slope ≈ 0 vs gap time |
| "Skip was right" (uniqueness/residual funnel) | Superseded — wrong timescale (250 ms) + wrong residual test |
| **`diag_splice_dualfit` sim (offline gate simulation)** | **Retired — decode unreliable.** Its independent ffmpeg `-ss` decode disagrees with the scan's decode at the *same* 1 s window (2026-07-01, pair-1 gaps 9/11/19/21): the sim locks both shoulders to a common offset (step ≈ 0) while the scan's `baseline_lag` reports a unique, prominent step (+260…+316 ms, `second_peak_r ≤ 0.17`). A ~75 ms→+302 ms per-gap offset that accurate seeking (`-ss` after `-i`) did **not** remove confirms the sim's decode — not the scan's — is the outlier. The repair runs on the scan/harness decode, so the sim's per-shoulder/global-shift verdicts are moot. **Replaced by scan-native `splice_dualfit`** (computed on the scan's PCM; C3/C7). Delete `crates/clip-sync-repair/tests/diag_splice_dualfit.rs` once the in-scan metric is validated on one pair. |

---

## Re-orientation — how the proven ideas fold into vocabulary and pipeline

**Vocabulary (descriptive; `gap-vocabulary-redesign` P2/P3).** Re-root on the axes (B1, B8, B2, B13): a gap is
`{geometry, A-presence, donor-presence, shared-source, registration(offset+step), bracket-search,
envelope}`. Name types from a **`b_mapped` rescan** (post A2). W5 → "same-master, lag-0/bracket validation failed."

**Pipeline (detect → repair; `seam-splice-dualfit` §4).** Order:
1. **Rescan primary cohort** with `b_mapped` capture (dirs 1–6 or full set).
2. **Re-classify skips** via `diag_fingerprint_corpus` — bracket-exhausted set may shrink.
3. **Detect** = `bracket_exhausted` (B1) ∧ both-sides-recoverable at `b_mapped` (B3) ∧ donor-continuous (A4/C5).
4. **Repair proof** = read scan-native **`splice_dualfit`** (`dual-fit viability` section) on bracket-exhausted skips → wire §4 behind flag (A3). *(Sim `diag_splice_dualfit` retired — E-tombstone.)*
5. **Calibrate** thresholds (A5/C6).

**Dual-fit addressable set (primary cohort):** **13 bracket-exhausted, both-sides-recoverable skips** on the
full-corpus rescan (2026-07-01) — all in pairs 1 (11) and 2 (2). One-sided-dead collapsed to 8/55 (B2/C1) —
not a separate rescue path; the residual 8 are genuinely dead shoulders, out of dual-fit scope.

**One-line status:** registration **closed and validated** (A2/B2/B13/C1/C2/C4 all PROVEN on the 2026-07-01
rescan); dual-fit viability now measured **in-scan** (`splice_dualfit` + validators), **sim retired** as
decode-unreliable (E-tombstone). **Live blocker:** re-scan (one pair, then full) to populate `splice_dualfit`
→ read `dual-fit viability` (C3/C7) → wire §4 repair (A3).
