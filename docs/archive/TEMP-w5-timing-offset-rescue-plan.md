# W5 timing-offset — production detection + drift-resample rescue (DRAFT)

Status: **ARCHIVED / SHELVED — this approach is the wrong one (2026-06-29); the gaps are still fillable.**
Superseded by [TEMP-seam-splice-dualfit-plan.md](../TEMP-seam-splice-dualfit-plan.md): the real mechanism
is a **silence-splice** (un-stretched shoulders separated by a step between the two per-side lags),
repaired by fitting each seam independently + reconciling the step with a length edit validated by the
**existing** gate — not a per-seam warp. The
per-seam *detect-and-warp* model is dead: (i) the steps aren't clock skew (not drift, not block-quantized),
and (ii) the local lag/uniqueness/residual probes are **mis-calibrated for cross-encoding, periodic,
same-master** content — they can't confirm what is, by **operator ground truth, the same soundtrack in
every pair**. So "no clean offset survives the trustworthiness filter" reflects *local-measurement*
failure, not unfillability. The earlier redirect — trust the global clip alignment + a cross-codec-robust
seam validator — lives in
[TEMP-gap-vocabulary-redesign-plan.md](../TEMP-gap-vocabulary-redesign-plan.md) §7d and
[TEMP-cross-codec-seam-impl-plan.md](../TEMP-cross-codec-seam-impl-plan.md) (itself now largely superseded;
see the dual-fit plan). **Do not implement this plan's
warp/per-seam-detect;** keep the synthetic g003 fixture + detection primitives as regression assets.
Original framing below.

---

_Original status:_ Framed as completing the *place → align →
validate* resolution ladder (§2), gated on P0 (prevalence scan). **But the clock-skew → time-warp model
(§7, P2 step 3) is refuted by the real corpus:** the pre↔post seam **steps are NOT clip drift** (the
offset doesn't accumulate with gap time — `diag_fingerprint_corpus` mechanism check, well-sampled files
reject it) and are **NOT block/frame-quantized** (no clean dropped-buffer signature). A step of tens-to-
100+ ms across a 1–2 s gap is physically impossible as smooth clock skew (drift over 2 s ≈ µs). So the
warp path applies only to the **synthetic** g003 fixture (which was *built* on the skew model). The real
mechanism is **undetermined** — real timeline discontinuity vs periodicity-corrupted lag measurement —
pending the uniqueness re-scan (`second_peak_r`). **Do not implement P2's warp** until the mechanism is
resolved. The detect stage (P1) and the **constant-shift** path (P2 step 2, §8) are unaffected.

Follow-on to [archive/TEMP-w5-timing-offset-diag-plan.md](archive/TEMP-w5-timing-offset-diag-plan.md)
(the diagnostic that characterized the class and shipped the skip-faithful fixture + recoverability data
this plan builds on). Reading: [gap-fingerprint.md](gap-fingerprint.md) § Lag fingerprint,
[seam-scoring.md](seam-scoring.md) §3–4.

---

## 1. Problem (one paragraph)

A **timing-offset** W5 gap (g003) is filled from B content that is the *same master* as A but
**time-shifted by a drifting sub-frame-to-multi-ms lag** (g003: −16 ms at the pre seam, −8 ms at the
post seam). Envelope/structure aligns, so placement is correct, but the waveform seam is scored at lag 0
where it is dead — the gate skips a genuinely recoverable seam (`waveform_floor`). The diagnostic plan
proved the class is detectable (the lag sweep cleanly returns `timing_offset` vs `decorrelated`) and
recoverable in a band (low–moderate drift, offset within the lag search). What's missing in production is
the step that *measures the seam lag and re-aligns the fill* so both seams clear the gate.

The critical subtlety (§7): a **constant** offset is fixed by a single shift and is largely already
handled (the haystack slide recovers it — constant offsets don't skip). The g003 case is **drift**,
where *no single shift fixes both seams* — the fill must be **time-warped (resampled)** to invert the A/B
clock skew across the hole.

---

## 2. Framing — complete the resolution ladder (don't replace the envelope)

The envelope-around-the-gap is **not** the thing to change. It answers *"where does the fill go?"* and
does it well (g003 envelope match 0.99); it is robust to encoding differences and to the silent hole in
the middle. The failure is one rung finer down. Gap evaluation really asks two questions at two
resolutions, and today there is an **empty rung between them**:

| Stage | Tool | Resolution | Sees a 16 ms lag? |
|-------|------|-----------|-------------------|
| Where does the fill go? | envelope / structure match | ~50 ms bins | No |
| **(missing) estimate the seam lag** | **— fine xcorr —** | **sample-level** | **— this plan —** |
| Is the splice clean? | waveform Pearson at the seam | sample-level, scored **at lag 0** | Fails *because of* it |

Placement is too coarse to see the lag; validation is fine enough but scored at lag 0, so the lag kills
it. The timing-offset class lives exactly in that gap. **This plan fills the missing rung:**

```
place coarse (envelope)  →  estimate seam lag (fine xcorr)  →  align  →  validate
```

That ladder is the spine of the plan: detection (P1) is **not** a special-case rescue trigger, it is the
missing **fine-alignment stage**; correction (P2) is the *align* step; the residual veto + re-gate is the
unchanged *validate* step. Two depths of commitment, chosen by the P0 result:

- **Minimal (bolt-on):** run the lag stage only when the validator fails on otherwise-aligned content.
  Surgical, zero cost on healthy gaps. Right if the class is rare.
- **Structural (first-class):** always estimate the seam lag during placement and **classify** the gap
  up front — *clean / constant-offset / drift / decorrelated* — then route to the matching handler
  instead of "run the generic gate, skip on failure." The lag verdict (already computed for the
  fingerprint) becomes a router, not an afterthought. Justified only if P0 shows the class is common.

**What this is *not*:**

- *Not a finer envelope.* Envelopes discard phase, which is the information a sub-millisecond lag needs;
  past ~50 ms you must use the waveform. "Better envelope" is a dead end for this rung.
- *Not pure cross-correlation placement.* You are correlating across a **silent hole** (nothing to lock
  onto in the middle) and local xcorr happily locks onto the wrong content. Envelope-for-placement +
  xcorr-for-fine-lag is the right division of labor; this plan keeps both.

---

## 3. Scope

| In scope | Out of scope |
|----------|----------------|
| The missing **fine seam-lag stage** (verdict + pre/post `frac_lag`) wired into the gate | Replacing the envelope/structure placement (it stays) |
| Constant-offset shift **and** drift time-warp of the B fill (the *align* step) | Cross-clip global resync (alignment layer already owns clip offset) |
| New patch tier `timing_offset_trusted`; residual veto + re-gate unchanged (*validate*) | Non-linear / variable-rate warps (linear skew only, P1–P4) |
| Pipeline oracle on `build_w5_timing_offset_seam` (rescue ON) | Pitch-preserving / spectral repair |

**Minimal vs structural** (§2) is a knob, not a fork in scope: P1–P3 implement the *align* stage; whether
it runs on-failure (minimal) or always-with-routing (structural) is one branch decided by P0.

**Tier ladder:** P0 diagnostic (prevalence). P1–P3 production (behind a mode flag, default off). P4
integration oracle.

---

## 4. Building blocks we already have

| Piece | Where | Role |
|-------|-------|------|
| `summarize_lag_curve` / `lag_correlation_curve` (pub) | `application/gap_fingerprint.rs` | The fine-lag stage: seam lag verdict + `frac_lag_ms` |
| `lag_at_placement` pattern | `gap_fingerprint.rs` | Builds pre/post seam windows at a placement |
| `resample_interleaved` (rubato) | `application/patch_audio.rs` (already imported) | The resampling primitive for the warp |
| Skip-faithful fixture `build_w5_timing_offset_seam` | `test_support/energy_signature_fixtures.rs` | Drift cells currently **skip** → must **patch** after rescue |
| `diag_w5_timing_offset_*` (grid + gate probe) | `tests/diag_w5_timing_offset.rs` | Recoverability band + current Skip behavior |
| Residual gate / veto | `gap_fill_fit.rs`, `patch_region.rs` | Unchanged guard against false positives (the *validate* step) |

**Better lag primitive (to consider):** the current stage is normalized correlation swept over integer
lags + parabolic interpolation. **GCC-PHAT** (generalized cross-correlation with a phase transform) is
the standard sub-sample time-delay estimator and is robust to the spectral coloration that biases plain
correlation. Adopt it for the fine-lag rung if accuracy proves marginal in P1; otherwise the existing
sweep is enough.

---

## 5. Phases

### P0 — Prevalence scan (go/no-go) — **do first**

Run `--gap-fingerprints` per A/B pair (one output dir each, e.g. `gap-files/1`..`gap-files/N`), then
aggregate with the **cross-corpus analyzer** (shipped 2026-06-29,
`clip_sync_repair_harness::gap_fingerprint_corpus`, driver `tests/diag_fingerprint_corpus.rs`):

```powershell
$env:GAP_FP_DIRS = "gap-files"        # auto-discovers gap-files/1 .. gap-files/N (each pair's corpus.json)
$env:GAP_FP_CSV  = "1"                # optional: per-gap CSV under target/gap_fingerprint_corpus.csv
cargo test -p clip-sync-repair --features diagnostic-tests --test diag_fingerprint_corpus -- --nocapture
```

It tallies, across every pair, the lag-verdict mix and gate outcomes, and prints the headline:
**`timing_offset` gaps the gate *skipped*** (the addressable class), split **constant** (single shift)
vs **drift** (needs time-warp, `|frac_lag_pre − frac_lag_post| > eps`, default 1 ms via
`GAP_FP_DRIFT_EPS_MS`), plus the drift / seam-offset / `min(peak_r)` distributions and a per-pair
breakdown. **Decision (also picks minimal vs structural, §2):** if the class is rare, stop or ship the
minimal bolt-on; if common, the structural first-class lag stage + routing earns its keep. A new tier +
resample path is not worth it for one exemplar. (Constant-offset detection/shift may still be a cheap
standalone win; see §8.) Cost: hours, existing tooling, no production code.

### P1 — The fine seam-lag stage (mode flag, default off)

`repair.timing_offset_mode = off | detect | rescue`. This is the **missing rung** from §2, not a rescue
hook. Run the lag sweep on the winning placement's pre and post seam windows — in **minimal** form only
when the gate would fail at `waveform_floor` with structure/envelope aligned (the g003 signature); in
**structural** form on every gap, feeding a *clean / constant / drift / decorrelated* classification.

- Record `verdict`, `frac_lag_pre`, `frac_lag_post` on the gate outcome (and JSON/fingerprint, which
  already carries `lag`).
- `detect` mode stops here: tag the gap `timing_offset` distinct from a `decorrelated` skip (better
  observability, no behavior change). De-risks P2 by surfacing real verdicts before we act on them.

Touch: `patch_region.rs` (`evaluate_seam_gate_fit_candidate`), `SeamGateOutcome`, `gap_tags.rs`.

### P2 — Alignment + correction (`rescue` mode) — the *align* step

Decide and apply per the two regimes:

1. **Confidence gate first.** Require both seams `timing_offset` with `peak_r ≥ τ` (τ ≈ 0.8, calibrated
   from P0 + the recoverability grid), and frac_lags **consistent with a linear skew** (post − pre
   roughly proportional to the seam separation). Otherwise leave the skip.
2. **Constant** (`|frac_lag_post − frac_lag_pre| ≤ ε`): shift the B fill by the common `frac_lag`
   (sub-sample, via `resample_interleaved` at ratio 1 with a fractional-delay, or fractional-index
   interpolation). Re-score both seams at lag 0.
3. **Drift:** time-warp the fill. The skew rate `r = 1 + (frac_lag_post − frac_lag_pre) / fill_secs`;
   resample the B fill so its **start aligns to `frac_lag_pre`** and its **end to `frac_lag_post`**,
   producing a fill whose interior length still equals the A hole (endpoints fixed → interior
   determined). Re-score both seams.
4. **Re-gate unchanged (the *validate* step):** both seams must now clear the waveform floor (the
   recovery the lag sweep predicted); residual veto runs as today (guards same-master illusions / false
   positives).

Touch: new `timing_offset_align(...)` in `patch_region.rs` or a sibling module; `patch_audio.rs` (fill
extraction → warp → splice); reuse `resample_interleaved`.

### P3 — Tier + observability

`PatchTier::TimingOffsetTrusted` (or `timing_offset_corrected: bool` + measured `skew_ppm` on
`GapPatchStatus::Patched`). CLI human line, JSON, `derive_gap_tags_from_status`, docs
(`gap-fill-modes.md`, `cli-output.md`, `gap-repair-guide.md`).

### P4 — Validation

| Case | Expect |
|------|--------|
| `build_w5_timing_offset_seam` drift cells (`16/−4500`, `8/−4500`, `32/−9000`) under `rescue` | **Patched** `timing_offset_trusted`, both seams clear floor, `skew_ppm` ≈ configured |
| Constant `16 ms / 0 ppm` | Patched (baseline or shift) — no regression |
| Decorrelated / F4 decoy | Still **skips** — residual veto holds, no false positive |
| Offset beyond lag search / `peak_r < τ` | Conservatively **skips** (not recoverable) |
| `diag_w5_timing_offset_gate_probe` counterpart | Same cells, rescue ON → Skip flips to Patched |

Pattern to copy: `tests/anchor_seam_oracle.rs` A6 pipeline rows (release-tier `#[ignore]` if slow).

---

## 6. Risks / open questions

1. **Fill length under warp.** The hole length on A is fixed; the warp must output exactly that many
   frames while honoring both seam offsets. Endpoints-fixed linear warp determines the interior — verify
   no off-by-one drift at the splice; trim/pad to the hole as a backstop.
2. **Interpolation quality.** Skews are tiny (tens of ppm); a sinc resampler (rubato) keeps artifacts
   inaudible. Confirm vs naive linear interp on a listening check.
3. **Confidence calibration.** τ and the linearity tolerance must reject `ambiguous`/`decorrelated` and
   the recoverability-grid's broken cells. Calibrate against P0 real verdicts + the synthetic grid.
4. **Multichannel.** A clock skew is uniform across channels — apply one warp to all; don't per-channel.
5. **Sequencing vs anchor rescue.** Both are W5 paths. Under the structural framing (§2) the up-front lag
   classification *is* the router — decide whether `timing_offset` is tried before, after, or instead of
   the editorial anchor rescue.
6. **Same-master assumption.** `timing_offset` implies same master; the residual veto already guards the
   decorrelated case, but confirm the warp can't manufacture a spurious high seam from periodicity.

---

## 7. Why no single shift fixes both seams (the core mechanism)

A and B are the same master but B runs on a slightly different clock (resample/PLL/capture-rate skew),
so B's playback time maps to A's as `t_A = t_B · (1 + s) + c` — a **constant** offset `c` *plus* a
**rate term** `s`. The lag at a seam located at time `t` is `lag(t) = c + s·t`: it grows linearly along
the gap. The pre and post seams sit at different times (≈1.8 s apart in g003), so they have different
lags (−16 vs −8 ms). A single shift adds the same constant to every sample → it can zero the lag at one
seam but leaves `s·Δt` at the other. Only an operation with a **rate** term — a resample/time-warp —
cancels `s` and aligns both at once. (If `s = 0`, the offset is constant, one shift works, and the gate
already recovers it.) This is why the *align* rung in §2 is a warp, not a shift, for the g003 class.

---

## 8. Cheap subset, if P0 says "rare"

If drift is rare but constant timing offsets show up, ship only the **constant-shift** path (P1 detect +
P2 step 2, no warp): smaller, no resample, and it upgrades the already-recoverable constant case to an
explicit tier with the lag verdict recorded. Skip the drift warp until an exemplar count justifies it.
