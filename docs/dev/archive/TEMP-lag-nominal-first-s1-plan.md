# S1: plumb nominal-first envelope rescue into production dual-fit

**Status:** draft, not started, 2026-08-20. Follows [`TEMP-lag-nominal-first-plan.md`](TEMP-lag-nominal-first-plan.md)
(implemented + validated 2026-08-20, record-only diagnostic today).

## Problem

`gap_fingerprint::measure::lag`'s nominal-first ordering (measure at the nominal window, triage
`is_recoverable`, only then lazily widen to a coarse envelope search) is diagnostic-only: it runs in
`--gap-fingerprints` and never touches a repair decision. Production's own per-shoulder lag search —
`domain::dual_fit::try_dual_fit`, called from `skip_or_dual_fit` (`region.rs:647`) as the last-resort
rescue after the ordinary E1-E7 seam gate (`patch_region.rs`) exhausts all brackets — has no coarse-search
fallback at all. `try_dual_fit` does one fixed-window `seam_local_peak` sweep per shoulder
(`seam_window_frames` = `fill_seam_search_secs · sample_rate`, currently 250ms, `max_lag_frames` =
0.6s) and declines outright (returns `None`) if that peak doesn't clear the gate. A shoulder that's
decorrelated at the nominal placement but recoverable at a large offset — exactly the class
`31/g7`'s post shoulder demonstrated (recoverable only at −1160ms, memory
[[31g7-post-shoulder-found]]) — is invisible to production today: it falls through to a plain skip.

S1 gives `try_dual_fit` the same nominal-first escape hatch: try the narrow window first (unchanged
behavior when it succeeds), and only when it's dead, retry with a wider, coarser search before
declining.

**This must be a strict port of the validated mechanism, not just the shape.** Nominal-first's rescue
(`rescue_shoulder`/`envelope_shoulder`/`envelope_peak_at`, `lag.rs:250-332,447-513`) is a *two-stage*
search: (1) a coarse **RMS-envelope** correlation (`fine_rms_envelope`, 100 ms bins,
`WIDE_ENV_BIN_MS`/`WIDE_ENV_WINDOW_SECS` = 2 s window, over `cfg.lag_envelope_max_lag_ms` = 2000 ms
default, `DEFAULT_LAG_ENVELOPE_MAX_LAG_MS`) proposes a recenter offset only if the envelope peak clears
`min_envelope_r` and isn't edge-pinned (`envelope_recenter_frames`); (2) the **same narrow fine sweep**
(`seam_local_peak`'s algorithm) is then re-run at that recentered anchor, and the rescue is accepted
only if *that* re-sweep itself clears the recoverable bar. It is not a single wider raw-sample
correlation sweep. That distinction matters for two reasons this plan got wrong in its first draft:
- A raw Pearson sweep over ±1.16 s of full-rate audio is both the expensive regime
  `lag_correlation_curve_auto`'s FFT crossover exists for (`seam_local.rs:122-128`) and more prone to
  spurious peaks on broadband/periodic content than envelope correlation, which is coarser and more
  tolerant of local decorrelation by construction. There is no evidence a plain wide raw sweep
  reproduces nominal-first's 31/g7 result; the envelope-first shape is what was actually validated.
- Because the final accepted lag always comes from a re-run of the *narrow* fine sweep (just recentered),
  the assembled result is always narrow-window, sidestepping any "mixed window width" ambiguity in the
  post-seam step-real check (see Change §3 below).

## Non-goals

- **No re-decode.** `try_dual_fit`'s inputs (`a_pre_mono`/`a_post_mono`/`b_mono`) are sliced directly
  from `a_samples`/`b_samples` already held in `RegionPatchMedia`/`DualFitRepairInput`
  (`region.rs:517-534`, `build_dual_fit_input`). The wide-search variant must slice a bigger window
  from those same in-memory buffers — never touch the decoder.
- **No change to the narrow-window fast path.** When the nominal 250ms sweep already clears
  `min_fill_correlation`/`fill_absolute_floor` and `step_real`, S1 must be a no-op: byte-identical
  `DualFitResult` to today. This is the dual-fit analogue of nominal-first's own D6 byte-identical bar.
- **Do not restructure `skip_or_dual_fit`'s re-validation.** The hard invariant from
  `dual_fit_result_passes_the_production_revalidation_gate` (`dual_fit.rs:337-429`) stands: confidence
  is classified from `try_dual_fit`'s own returned `pre_seam_r`/`post_seam_r`
  (`region.rs:684`), never re-derived via `fill_splice_seam_correlations_interleaved`. Whatever
  `try_dual_fit` returns after a wide retry must carry real seam scores measured at the placement it
  actually used — no placeholder/optimistic scores.
- **Do not import `gap_fingerprint::measure::lag` types as-is.** They're
  `pub(in crate::application::gap_fingerprint::measure)` and live in `application`; `dual_fit.rs` is
  `domain`. This is not a visibility tweak — `domain` code cannot depend on `application` code (wrong
  direction). What's reusable is the *algorithm* — coarse envelope correlation → recenter → narrow
  re-sweep — not the structs (`LagSummary`, `EnvelopeCfg`, `LagSideSweep`). `domain::seam_local`'s
  `lag_correlation_curve`/`lag_correlation_curve_auto`/`seam_local_peak` are already shared between
  both call sites and get reused as-is for the narrow fine sweep (both nominal and post-recenter). The
  envelope stage's two primitives (`fine_rms_envelope`, `pcm.rs:252`; the peak/edge-pinned extraction
  in `envelope_peak_at`, `lag.rs:447-483`) currently exist only in
  `application::gap_fingerprint::measure` and must be ported into `domain::seam_local` (or a new
  `domain` module) as free functions with no dependency on `LagSummary`/`FingerprintConfig` — this is
  new code, not a reuse, since nothing at this fidelity exists in `domain` today. `min_envelope_r`
  already lives in `domain::gap_equivalence::DonorRegistrationParams` (`lag.rs:518`) and can be reused
  directly.
- **No change to `step_real_margin`'s meaning.** `try_dual_fit`'s post-seam-must-beat-post-forced-to-
  pre-lag check has no analog in `rescue_shoulder`'s `peak_r >= 0.5` triage; S1 keeps it as `try_dual_fit`'s
  own acceptance gate, applied identically whether the accepted lag came from the narrow or the wide
  sweep.

## Change

1. **Port the envelope primitives into `domain::seam_local`**: `fine_rms_envelope(x: &[f64], bin: usize)
   -> Vec<f64>` (verbatim from `pcm.rs:252`) and an `envelope_peak(a_win, b_ctx, sample_rate, env_bin,
   max_lag_samples) -> Option<(peak_r: f64, peak_lag_frames: i64, edge_pinned: bool)>` (verbatim
   algorithm from `envelope_peak_at`, `lag.rs:447-483`, minus the `EnvelopeCfg`/ms-based fields — take
   frame counts directly). Both are pure functions over `&[f64]`, no `application` types involved, so
   this is a straight port, not a reimplementation-from-shape.
2. **Add envelope-reach fields to `DualFitParams`** (`dual_fit.rs:27`): `envelope_window_frames`,
   `envelope_bin_frames`, `envelope_max_lag_frames`, populated in `build_dual_fit_input`
   (`region.rs:541`) from constants local to `dual_fit.rs` mirroring nominal-first's own defaults —
   `WIDE_ENV_WINDOW_SECS` (2 s), `WIDE_ENV_BIN_MS` (100 ms), `DEFAULT_LAG_ENVELOPE_MAX_LAG_MS` (2000 ms,
   `config.rs:10`) — not threaded through `RepairConfig`/CLI in S1 (no new user-facing knob; the
   diagnostic tool keeps its own `--lag-envelope-max-lag-ms`). Confirmed the reach fits inside what's
   already decoded on both sides: B's `b_extract_start/end_secs` (`region.rs:1576-1588`) key off
   `fill_border_search_secs`, which defaults to **8.0 s** (`config.rs:1583`) — comfortably covering
   `envelope_window_frames + envelope_max_lag_frames` (≤ 4 s reach); A's `a_pcm` is the full decoded
   track (`decode.rs:94-108`), so its reach is a non-issue. Neither side needs a re-decode — but the A
   side does need new plumbing to reach the already-decoded data, since only a 250 ms slice of it is
   currently threaded into `try_dual_fit` (see Change §3c).
3. **Restructure `try_dual_fit`'s internal ordering to be per-shoulder triage-then-rescue, matching
   `lag_pair`/`rescue_shoulder`'s ordering — this is a real change to the check sequence, not just an
   addition.** Today's `try_dual_fit` runs both narrow `seam_local_peak` calls, *then* gates on the
   combined `smin = pre_r.min(post_r)` (`dual_fit.rs:108-152`) — so a dead post shoulder declines the
   whole gap before pre is ever separately considered, and there is no point in that sequence where a
   single dead shoulder can be rescued without the other shoulder's score participating in the
   decision. S1 replaces steps 1-2 of `try_dual_fit` with, per shoulder independently:
   a. Run the narrow `seam_local_peak` at the nominal anchor (unchanged inputs/window) → `(r, lag, z)`.
   b. Triage: if `r` already clears `min_fill_correlation`/`fill_absolute_floor`, done — this shoulder's
      value is `(r, lag)`, envelope never runs (mirrors `is_recoverable` short-circuit, `lag.rs:303-307`
      — needed for the byte-identical fast-path non-goal).
   c. If dead: run `envelope_peak` over the wider window/bin/lag from step 2. This needs a ~2 s A-side
      template — `a_pre_mono`/`a_post_mono` are only `seam_window_frames` (250 ms) long
      (`region.rs:529-530`), too short to reuse directly. `a_pcm`/`a_samples` is the **full decoded A
      track** (`decode.rs:94-108`, whole-file `extract_interleaved`), not a windowed slice, so there's
      no reach/margin problem — but `try_dual_fit`'s current signature only receives `a_pre_mono`/
      `a_post_mono`/`b_mono`/`b_samples`/`b_mapped_start`/`params`, not the raw array. This is prior
      work at the call site, not inside the existing narrow-sweep body: extend `try_dual_fit`'s
      signature to also take `a_samples`/`channels`/the gap frame bounds (`DualFitRepairInput` already
      carries all three through to `skip_or_dual_fit` for the residual-gate calc, `region.rs:557-561,
      609` — they're just not threaded into the `try_dual_fit` call at `region.rs:659-666` today), and
      slice the wide template **lazily inside `try_dual_fit`**, only for a shoulder that's actually
      dead at nominal — mirroring nominal-first's own lazy `pre_wide_mono`/`post_wide_mono` closures
      (`lag.rs:564-569`) rather than eagerly widening `build_dual_fit_input`'s `mono()` capture (and
      paying that cost) on every gap. If the envelope peak is `None`, below `min_envelope_r`
      (`domain::gap_equivalence::DonorRegistrationParams::default().min_envelope_r`), or edge-pinned →
      decline the rescue, keep the dead nominal `(r, lag)` (mirrors `envelope_recenter_frames`
      returning 0, `lag.rs:514-524`).
   d. Otherwise recenter the anchor by the envelope's proposed frame offset and re-run the **narrow**
      `seam_local_peak` there. Only if *that* re-sweep itself clears the floor is the rescue accepted —
      the shoulder's value becomes the recentered `(r, lag)`; otherwise keep the dead nominal `(r, lag)`
      (mirrors `rescue_shoulder`'s match arm, `lag.rs:328-331`).

   Only after both shoulders have gone through a-d does the *existing* combined logic run unchanged:
   `gate_pass` on `smin = pre_r.min(post_r)`, the bridge-span check, `step_real` (using whichever lag
   each shoulder ended up with — always a narrow-sweep result per the Problem section's second bullet,
   so `post_global`'s `a_post`-vs-`b_mono` computation needs no width bookkeeping), and the donor
   continuity/program-quiet checks. None of those downstream checks change; only what feeds them does.
4. **Record which shoulder(s) were envelope-rescued** on `DualFitResult` (new fields
   `pre_envelope_rescue: bool`/`post_envelope_rescue: bool`) so `skip_or_dual_fit`'s tracing
   (`region.rs:715-720`) and `SeamScoreAttempt` can distinguish a rescued fill from an ordinary one —
   needed for the real-media validation pass in Exit below, and useful in production logs the same way
   `envelope_rescue`/`EnvelopeRescue::{Rescued,Declined,NotConsulted}` are useful in fingerprint dumps.

## Exit

- New primitive unit tests, mirroring `pcm.rs`/`lag.rs`'s own coverage, for the two ported functions in
  `domain::seam_local` (`fine_rms_envelope`, `envelope_peak`): binning correctness, offset-peak recovery,
  edge-pinned detection at the searched boundary — same shapes as `seam_local_peak_recovers_offset_seam`
  but for the envelope path specifically.
- New `dual_fit` unit tests (mirrors nominal-first's Exit shapes, in `dual_fit.rs`'s test module):
  1. Narrow sweep recoverable on both shoulders → envelope path never invoked on either, both rescue
     flags `false`, result identical to pre-S1 `try_dual_fit` (byte-identical `DualFitResult`).
  2. Narrow sweep dead on one shoulder, envelope proposes a recenter that itself re-sweeps recoverable →
     rescue accepted, `*_envelope_rescue = true`, returned `pre_seam_r`/`post_seam_r`/`pre_lag`/
     `post_lag` reflect the recentered narrow re-sweep (not a stale nominal score, not the envelope's own
     `peak_r`).
  3. Narrow sweep dead on one shoulder, envelope peak exists but is below `min_envelope_r` or
     edge-pinned → declined at step 3c, no re-sweep attempted, behaves exactly like today's immediate
     decline.
  4. Narrow sweep dead on one shoulder, envelope proposes a recenter, but the re-sweep *there* is still
     below the floor → declined at step 3d (mirrors `rescue_shoulder`'s `_ => Declined` arm), falls back
     to the dead nominal score — no new false-accepts.
  5. **Ordering regression**: pre dead at nominal but rescuable, post already recoverable at nominal —
     asserts the pre rescue is attempted and can succeed even though `post_r` alone would have failed
     today's pre-S1 combined `smin` gate before either shoulder got a chance (pins the Change §3
     reordering, since this exact case was unreachable before S1: today's code declines via
     `smin = pre_r.min(post_r)` using the dead nominal `pre_r` with no rescue path at all).
  6. `dual_fit_result_passes_the_production_revalidation_gate`-style test re-run against an
     envelope-rescued result: confidence still derives from `try_dual_fit`'s own returned scores, not
     `fill_splice_seam_correlations_interleaved`.
- Real-media re-run: re-fingerprint the corpus gaps currently landing as a plain skip through
  `dual_fit_eligible` brackets (identify via existing `--gap-fingerprints` dumps' `manifest.json`
  skip outcomes) with `--dual-fit` on before/after S1, confirm: (a) no previously-accepted dual-fit
  fill changes when the narrow sweep already succeeded (byte-identical `DualFitResult` there), and
  (b) at least the `31/g7`-shaped case (post shoulder dead at nominal, recoverable at ~−1160ms) now
  produces a `Patched` outcome with `post_envelope_rescue = true` instead of a skip.
- **Ear-check the rescued fill(s), not just the scores.** A strong `post_seam_r` proves the shoulder is
  correlated, not that the splice sounds clean — the same gate class that produced audible false
  positives before (memory [[normalize-gain-is-the-only-gain-read]]: a corpus's only "too loud" clip
  passed its correlation gate cleanly; [[fill-level-threshold-refuted]]: `peak_delta_db` doesn't order
  audibility either). Render 31/g7's rescued output and listen across the seam, the same bar the prior
  dual-fit media pass used (memory [[a3-dualfit-media-validated]]: "clean by ear" was the actual
  acceptance criterion, not the correlation numbers alone). A high-scoring but audibly bad splice is a
  real outcome to plan for here, not a hypothetical — the wide envelope search is specifically hunting
  matches at large offsets, where a spurious-but-correlated match is more plausible than at nominal.
- Perf sanity: the envelope stage is bins-of-100ms over ≤4s reach (tens of correlation points, not a
  wide raw-sample sweep), so it should be cheap relative to the per-bracket E1-E7 gate that already ran
  before `try_dual_fit` is reached — confirm this holds (no pathological cost) rather than assuming it,
  given this codebase's history of per-bracket/per-gap costs dominating wall-clock
  ([[production-perf-gate-search-dominates]], [[gate-refusal-costs-anchor-oracle]]).
- `cargo clippy -p clip-sync-repair --all-targets -- -D warnings` clean; full `cargo test --lib` passing.
