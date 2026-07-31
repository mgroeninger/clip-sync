# Gap fingerprint

A **gap fingerprint** is a licensing-safe numeric characterization of one repair gap: levels, silence
floor, energy contour, editorial anchors, structure/seam scores, and a lag fingerprint — **no audio
samples, no transcripts**. Fingerprints from real (licensed) media can be committed as a
regression/calibration corpus.

Source: `application/gap_fingerprint/` (`mod.rs`, `measure.rs`, `schema.rs`, `project.rs`). Plan
(archived): [TEMP-gap-fingerprint-plan.md](archive/TEMP-gap-fingerprint-plan.md).

> Status: **shipped** — schema + builder + from-decode dump (`characterize_gaps_from_decode`) +
> oracle `failure_stage` + `--gap-fingerprints` corpus library + equivalence / dual-fit /
> fill-placement axes. Validated on real media. Requires the `calibration` feature for the CLI flags.

## Producing fingerprints

```
clip-sync-repair A.mkv B.m4v --gap-fingerprints gap-files/ [--fingerprint-gap 3]
```

- `--gap-fingerprints DIR` — after scan, write a corpus directory: `corpus.json` (all characterized
  gaps), one self-contained single-gap JSON **per gap** (the library), and a non-identifying
  `manifest.json`.
- `--fingerprint-gap N` (repeatable) — characterize **only** these gaps. Omit it to characterize
  **all** gaps. Each characterized gap gets full decision/repair detail (per-bracket gate
  `failure_stage`, `baseline_lag`, `splice_dualfit`, …).
  **`N` is 1-based**, matching the `#` column of the repair gap table (and every other user-facing gap
  number in the tool). `0` and out-of-range values are rejected.
  The **emitted corpus stays 0-based**: `GapFingerprint::index` and the `g{:03}` filename segment are
  array positions, so `--fingerprint-gap 3` writes `…_g002_….json`. This is deliberate — existing
  corpus dirs and the `equivalence-calibration` / `gap-fingerprint-stats` joins are unaffected. Locate
  a gap's file by the A-timeline timestamp already in the name (`…_t01-42-08_g002_…`), not by counting.
- `--fingerprint-diagnostics` — also write the **Tier-3 X-set** (`seam_probe`, `wide_envelope`,
  `b_levels`, diagnostic `lag`). Off by default (decision/repair fields only); slower. Needed for the
  analyzer's seam-probe reports.

**Bulk runs (many pairs):** use [`scripts/measure-gap-fingerprints.ps1`](../../scripts/measure-gap-fingerprints.ps1)
with the same manifest format as [`measure-repair-perf.ps1`](../../scripts/measure-repair-perf.ps1)
(`label, A, B [, extra]`). One `--gap-fingerprints` dump per row under `-CorpusRoot/<label>/`.
Requires `--features …,calibration`. Prefer gitignored `gap-files/` for corpora; keep manifests/logs
(which contain media paths) out of git — same media-hygiene rule as [repair-perf.md](repair-perf.md).

```powershell
./scripts/measure-gap-fingerprints.ps1 -Manifest pairs.csv -CorpusRoot gap-files/my-corpus
./scripts/measure-gap-fingerprints.ps1 -Manifest pairs.csv -ScanArgs "--min-gap-ms 500" -FingerprintDiagnostics
./scripts/measure-gap-fingerprints.ps1 -Manifest pairs.csv -Check   # post-dump integrity via gap-fingerprint-stats --check
```

**Dump health check** (after a bulk run, or with `-Check` on the measure script):

```powershell
cargo run -p clip-sync-repair-harness --features calibration --bin gap-fingerprint-stats -- --check gap-files/fingerprint-corpus
```

Asserts writer invariants: gate Ok ↔ `start_frame`/`fill_frames`, patch/skip ↔ bracket passes, library
file count / manifest consistency, and a loose `|fill − geometry duration|` sanity bound (default 5.1 s;
override `GAP_FP_FILL_SLACK_SECS`). That bound is **not** the Phase B slack-use metric — end-search
excursion is `|fill − bracket span|`; `|fill − gap|` includes anchor widening. See
[archive/TEMP-fill-placement-axis-plan.md](archive/TEMP-fill-placement-axis-plan.md) Phase B. Incomplete pair dirs are
warnings. This is **not** the prevalence analyzer (omit `--check` for that).

To decide *which* gaps are worth characterizing, use the **normal repair run's gap table** — it lists
every gap's authoritative patch/skip + reason. A summary tier (cheap, no gate detail) still exists in
the API (`characterize_gaps`) but the bin path always characterizes its selected gaps at full detail,
because only the full tier carries the A-vs-B verdict (`failure_stage` / registration) needed to build
a fixture.

**Repair takes priority.** This is a repair tool first, so a real repair wins over the diagnostic:
if `--mux` is set, fingerprinting is **skipped** (with a warning if `--gap-fingerprints` was also
passed). `--gap-fingerprints` therefore runs on a scan-only / `--wav` run. *(Note: `--gap-fingerprints`
with `--wav` currently runs both and decodes A/B twice — fine, but not free.)*

## Performance

The dump characterizes **every selected gap at full detail** — it runs the **full per-bracket oracle**
(`oracle_score_fit_candidate` over all feasible brackets via `compute_region_measurements` in
`gap_fingerprint/measure.rs`), with **no routing or short-circuit** like the production patch path.
That per-bracket structure+seam search over the `--fill-border-search-secs` haystack is the dominant
cost.

Measured on a real 5.1 dump (a licensed HE-AAC 5.1 pair, 2026-07-11): **per-bracket oracle ≈ 82 % of wall-clock**
(decode ≈ 12 %), **~8.4 s per bracket** score, 11–22 brackets per short skip gap. Cost scales with the
**bracket count**, not gap duration (a 228 s gap with 0 feasible brackets is ~free). Treat that as a
dated upper-bound snapshot, not a current SLA.

The current figure is **4.3–5.0 s per bracket** (~4.33 s avg over 5064 brackets), from the 17-pair
characterize baseline in [repair-perf.md §2](repair-perf.md) (2026-07-23) — flat across 15 of 17 pairs.
Fingerprint mode enumerates brackets exhaustively, so even that is an upper bound on the production
path, which §3 measures at 2.7–4.1 s/bracket. Both numbers are snapshots; cite §2 rather than copying
a bare figure forward.

Why it resisted a cheap speedup: on the licensed corpus, the expensive gaps are **timing-offset skips**
(same audio shifted ~150–200 ms, lag-corr ≥ 0.98) that score every bracket only to fail the lag-0
`waveform_floor` seam. A correlation-based pre-filter can't reject them (correlation is *high*), and the
gate seam sits at a structure-search-chosen placement a fixed probe can't predict — so the fingerprint
perf work (`TEMP-pipeline-perf-redesign-plan.md` §2.5.4 sub-step **8g.5**) was **deferred** after two
approaches were refuted by measurement. Full analysis + cost hierarchy: that plan's §1.3 + 8g.5 row.

## Shape

`GapCorpus { source: SourceMeta, gaps: [GapFingerprint] }`, serialized as JSON. Per gap:

| Field | When | Meaning |
|-------|------|---------|
| `geometry` | always | A reported/refined edges, duration; B mapped edges + fill offset (B present) |
| `levels` | always | `bin_ms`, `profile_db[]` (RMS dBFS across pre→post context), speech-peak/noise-floor/gap-floor dB |
| `silence` | always | collar RMS/peak ratio + whether it clears the **relative** silence test (border walk-off discriminator) |
| `contour` | always | `has_anchor_seam_contour`, pre/post envelope flatness |
| `anchors` | always | pre/post candidates `{ time, source, prominence, rms_db }` |
| `brackets` | full | feasible brackets `{ span, move, structure_*, seam_*, start_frame, fill_frames, failure_stage }` (see *Bracket placement* below) |
| `structure` / `seams` | B present | baseline scores; seams carry per-channel + selected channels |
| `baseline_lag` | full, B present | **decision** per-shoulder lag fingerprint registered at **`b_mapped`** (see *Registration & dual-fit*) |
| `splice` | full, B present | first-class registration step derived from `baseline_lag` mono: `step_ms`, per-side `peak_r`/`peak_z`, `edge_pinned` |
| `donor_interior` | full, B present | B occupancy over the **aligned** bridge span (`b_mapped_start+L_pre … b_mapped_end+L_post`): `rms_db`, `silence_fraction`, `longest_silence_ms`, `continuous` |
| `donor_interior_nominal` | full, B present | B occupancy over the **nominal** geometry span (no lag adjustment) — registration-independent; the D11 program-quiet signal |
| `splice_dualfit` | full, B present | dual-fit viability: seams scored at per-shoulder placement + `gate_pass` / `trim_frames` / validators (see below) |
| `residual` | full, B present | least-squares same-source cancellation (dB) vs noise floor at the decision seam |
| `outcome` | B present | plan_kind, tier, seam_shape, fit_path, signature_mode, skip_reason |
| `equivalence` | B present | **gap-equivalence class (fine)** — does this gap need patching? (silence-character; see below) |
| `scan_equivalence` | scan classified | the **coarse production** verdict for the same gap (`GapReport::gap_equivalence`; block size = the `scan_block_ms` knob), copied in so one dump holds both readings for calibration. **This is the authoritative one** — see below |
| `lag` | diagnostics | **Tier-3** per pre/post anchor lag fingerprint at the best-energy bracket / structure throat — requires `--fingerprint-diagnostics` |
| `wide_envelope` | diagnostics | **Tier-3** 100 ms-bin RMS-envelope lag peak at `b_mapped` — cross-scale confirmer of `baseline_lag` |
| `seam_probe` | diagnostics | **Tier-3** encoding-robust seam metrics (R2/R4/spectrum/env/recovered); not used by any gate |
| `b_levels` | diagnostics | **Tier-3** symmetric B-side `LevelProfile` (validation instrument for the program-quiet hypothesis) |

### Bracket placement — `start_frame`, `fill_frames`, and why the seam does not choose them

There are two placement paths in the fingerprint code; only one feeds the dump's
`brackets[].start_frame` / `fill_frames`.

**`place_on_b` (structure-only).** Runs the unified search with **`waveform_weight: 0.0`**. The reason
is narrow and specific: `seam_pre` / `seam_post` recorded at that placement feed
`classify_bracket_stage`, and **those two fields ship with no prominence or z companion.** Read at a
structure-chosen placement they mean *"structure found a placement; does the waveform corroborate?"* —
which is what makes `waveform_floor` a meaningful failure stage. Let the seam influence *those* fields'
placement and they become an unguarded argmax over the search radius: max-of-noise, and the stage stops
distinguishing anything. Do **not** flip this weight in place.

**From-decode dump (`compute_region_measurements`).** The live `--gap-fingerprints` path scores each
bracket with `oracle_score_fit_candidate` (the **production-weights** gate). Its
`SeamGateOutcome.alignment` already carries `start_frame` / `fill_frames`; the dump projects them onto
each `BracketInfo` on gate **pass** (`None` on gate failure — no chosen placement). That is the
placement that can observe an end-search scoring change. Cost is zero: same search, stop discarding the
field. See [archive/TEMP-fill-placement-axis-plan.md](archive/TEMP-fill-placement-axis-plan.md) Phase A.

**This is not an argument against seam-chosen placement in general.** `splice_dualfit` places each
shoulder at its own seam peak, unconditionally, for every gap — and answers the same estimator-bias
concern by *publishing validators* rather than by abstaining: `*_seam_z` (**primary** — whole-curve z
over the placement search, periodicity-robust), `pre_seam_prom` / `post_seam_prom` (secondary ±30 ms
single-rival margin, which reads low on correct-but-periodic content), and `post_seam_global_r`. A
second production-weights placement with its own validators is fine to add as **additional** fields —
never by overwriting the structure-only seam pair. See that plan's Phase B.

`fill_frames` is the B-derived fill length. The end search's nominal is the **bracket span**
(`span_secs` / refined post−pre), not the original silent-run gap; `fill_frames` differs from that
span by up to `fill_length_slack_secs` (default 1.0 s; end-search only). The B haystack tail is
sized separately by `fill_extract_tail_slack_secs` (default 5.0 s; `max` with
`fill_align_margin_secs`). Measuring `|fill − original gap|` mostly
reads anchor widening, not slack use — see [archive/TEMP-fill-placement-axis-plan.md](archive/TEMP-fill-placement-axis-plan.md)
Phase B. **On the dump path they are the only projection of the end search's decision.** Before they
existed, a change that moved every fill length on the corpus left the golden diff green. They are
`None` on dumps written before that date, on projected (non-measured) brackets, and on brackets that
failed the gate.

**Where the bracket-span nominal comes from (cite this, do not re-derive it).**
`gate_structure_align` computes `gap_frames = refined.end_frame − refined.start_frame` where `refined`
is the **candidate bracket**, not the baseline gap (`patch_region.rs:1398`); the end sweep's
`end_min` / `end_max` are then centered on that value (`gap_fill_fit.rs:966-970`). Two checks
reproduce it straight from any dump, without reading code:

- `span_secs × sample_rate == splice_dualfit.gap_frames + move_frames`, exactly, per bracket.
- Within a multi-bracket gap, `corr(move_frames, fill_frames) == 1.00` — the fill tracks the
  per-bracket widening, not an independent length hunt over the original hole.

This trap has been walked into twice. `|fill − original gap|` reads a ~2 s median on the 17-pair
corpus and looks like a saturated ±5 s slack; against the bracket span the signed median excursion is
0 ms (abs median 24 ms, p95 91 ms, max 388 ms).

**Which bracket the golden reads — the predicate is the *best* one, not *any* one.**
`best_seam_bracket` ranks by min-seam over every bracket with a complete seam pair, **gate failures
included** (`seam_pre` / `seam_post` are populated on failures via `stage_of`; `fill_*` is not). So a
gap whose min-seam winner failed the gate carries **null** `fill_*` even when other brackets in that
same gap placed. On the 17-pair corpus that shape occurs in **3 of 121 patch gaps (~2.5%)**, so a
newly harvested fixture hits it roughly **1 time in 40**. Verify a candidate fixture's
best-by-min-seam bracket passed *before* adding it, or `curated_golden_fill_placement_is_armed` will
fail for a reason that has nothing to do with a regression.

### `equivalence` — does this gap need patching?

Classifies each scanned gap from its **silence character** (no seam/lag math — that failed on drifting
recordings). Two signals, both already in the fingerprint: A's gap RMS **relative to the recording's own noise
floor** (a dropout sits far below it; room tone sits at it — self-calibrating), and donor silence at nominal
(is B occupied). Vocabulary + gate: [gap-vocabulary.md](gap-vocabulary.md) § Silence-character pre-gate.

| Field | Meaning |
|-------|---------|
| `class` | `repairable_dropout` (keep) · `shared_silence` / `ambient_quiet` (drop) · `not_evaluated` |
| `drop` | whether `class` resolves to "remove from fill plan" (the two silence classes) |
| `a_gap_rms_db` | A gap interior RMS (dBFS) |
| `noise_floor_db` | the recording's noise floor (A `levels.noise_floor_db`) |
| `a_below_noise_db` | `a_gap_rms_db − noise_floor_db` — the self-calibrated dropout signal (dropout ≲ −`dropout_margin_db`) |
| `donor_silence_fraction` | B occupancy over the nominal span (occupied ⇒ `< donor_silence_thresh`) |

**Classes:** `repairable_dropout` = A's signal died (≥ `dropout_margin_db` below the noise floor) **and** B
occupied → **keep**; `shared_silence` = B silent → nothing to fill → **drop**; `ambient_quiet` = A is only room
tone (not a dropout) though B has content → genuine quiet → **drop**. Thresholds (`dropout_margin_db ≈ 35`,
`donor_silence_thresh ≈ 0.5`) are tunable.

The fingerprint **always emits** `equivalence` (fine) and `scan_equivalence` (coarse) for calibration —
the dump itself never drops gaps. Production plan-time drop is separate and **on by default**
(`skip_equivalent_gaps = true`; `--no-skip-equivalent-gaps` to patch all). See [gap-scan.md](../gap-scan.md).

### `equivalence` vs `scan_equivalence` — a second opinion, not an oracle

They feed the **same classifier** from **differently defined** inputs. This is deliberate and is not
scheduled to converge; read them accordingly.

- **`scan_equivalence` is authoritative.** It is the verdict production acts on (`skip_equivalent_gaps`),
  measured on scan blocks (size = the `scan_block_ms` recipe knob — not a constant, and not 250 ms).
  The curated gap **cells** in [gap-vocabulary.md](gap-vocabulary.md) are scan-time cells, so a fixture's
  declared cell is checked against *this* field.
- **`equivalence` is diagnostic only.** Nothing in the plan or patch path reads it; it exists to be
  compared. Sample-level A gap RMS over the **refined** span, fine-bin noise floor, 50 ms donor bins.

**It was called "the fine reference" here until 2026-07-30. That was wrong**, and the wording bred a
recurring error: reading a divergence as "the coarse gate is inaccurate". Both known differences bias
the *fine* side toward `drop`, so it is the **more aggressive** of the two, not the safer one:

| bias | mechanism | direction |
|---|---|---|
| `gap_floor_db` | fine takes the max over **all** bins in the span (a content peak); scan takes the max over A's **silent** blocks (an actual floor) | fine's floor is higher ⇒ more donor blocks read silent ⇒ toward `shared_silence` |
| noise floor | **dominant:** fine `mono_rms` (amplitude / downmix) vs scan interleaved power; **secondary:** ±3 s / 50 ms vs ±2 s / 100 ms | fine reads **lower** — measured on 10/10 gaps of one pair (~3–19 dB), 5/5 on a 17-pair corpus, and 3/3 on the committed curated fixtures ⇒ smaller `a_below_noise` ⇒ away from `repairable_dropout`. Matching reduction alone collapses the bias on 7/10 gaps (see below) |

**Measured population:** 5 divergent / 297 gaps (1.7 %) across a 17-pair corpus, **0 in the dangerous
direction**. The mechanism behind the divergences is a donor whose level sits *between* the two floors —
silent to fine, occupied to scan. Pinned media-free by
`tests/gap_corpus/fingerprints/equivalence_divergence/`; full analysis in
[TEMP-equivalence-divergence-findings.md](TEMP-equivalence-divergence-findings.md) § F15.

#### `equivalence.silent_core_probes` — measuring the candidate fix before adopting it

F15's decided direction is to give the fine path a **silent-core** `gap_floor_db` (max over A's *silent*
bins, matching the scan path's definition). Rather than change the field and re-measure afterwards, each
fine verdict now carries candidate floors computed that way and **classified on by nothing**:

| field | meaning |
|---|---|
| `bin_ms` | bin width this candidate was measured at |
| `floor_db` | max RMS over the **silent** bins — the candidate `gap_floor_db`. Absent when no bin was silent |
| `a_rms_db` | energy mean over the same bins — the candidate A-side signal (the *other* open axis, free in the same pass) |
| `silent_bins` / `total_bins` | the population behind both, so a max can be read against how many bins it summarizes |

Two probes are emitted per gap: one at `gap_signature_bin_ms` (the fingerprint's own binning) and one at
the scan recipe's `scan_block_ms`, so the like-for-like comparison against `scan_equivalence.gap_floor_db`
is available without assuming bin size is irrelevant — whether it is, is one of the open questions. An
absent `floor_db` means the empty-silent-bin fallback is load-bearing on that gap and has to be decided
before the fix lands; if it never occurs across the corpus, it is a defensive case and nothing more.

This is scaffolding with a scheduled death: once the fix is adopted or rejected, the probes come out.

#### `equivalence.noise_floor_probes` — separating the second axis's three variables

The other open F15 axis. Both paths estimate the noise floor the *same way* (median of context bins
outside the gap) but over **±2 s / 100 ms** (scan) vs **±3 s / 50 ms** (fine), and fine reads
systematically lower. Several variables, one observed difference — so the probe emits the grid:

| field | meaning |
|---|---|
| `context_secs` / `bin_ms` / `reduction` | the combination this row was measured at |
| `floor_db` | median dB over the context bins. Absent when the context was empty — *not* the −120 placeholder, so "no context" stays distinguishable from "silent context" |
| `context_bins` | the population behind the median; the two windows differ in exactly this |

Rows are the cross product of `{EQUIVALENCE_CONTEXT_SECS, gap_signature_context_secs}` ×
`{scan_block_ms, gap_signature_bin_ms}` × `{Interleaved, Downmix}`, deduped so a collapsed grid doesn't
emit the same measurement twice and read as corroboration. Mono material is the one case that is *not*
deduped: the two reductions read identically there, but keeping both rows is what lets a reader tell
"the axis was measured and was flat" from "the material had nothing to say about the axis".

##### The third variable: `reduction`

`ChannelReduction` is how a bin's channels are collapsed to one level before it goes to dB:

- **`Interleaved`** — RMS over all interleaved samples, a **power** mean across channels. What scan's
  `block_rms_db` does (via `rms_interleaved`).
- **`Downmix`** — average the channels per frame, *then* square: an **amplitude** mean. What fine's
  `mono_rms` / `interleaved_to_mono` do.

They differ by the zero-lag cross-correlation *between the channels*. With equal per-channel power and
mean pairwise correlation `ρ̄` over `N` channels, `R_downmix² / R_interleaved² = (1 + (N−1)·ρ̄) / N`:

| `ρ̄` | difference | reached by |
|---|---|---|
| 1 | 0 dB | pointwise-**identical** channels only (mono duplicated into 5.1, hard-centred mix) |
| 0 | `10·log10(N)` = 7.78 dB at 6ch | decorrelated channels — **or** one active channel over `N−1` silent ones, which hits the same ratio |
| → `−1/(N−1)` | → −∞ dB | cancelling channels |

Two traps this table is here to close. **7.78 dB is not a bound** — it is the `ρ̄ = 0` point, and real
content reads past it whenever `ρ̄` goes slightly negative; a measurement "over the ceiling" is not an
anomaly. And **the sign is a theorem**: Cauchy–Schwarz gives `Downmix ≤ Interleaved` always, so a
uniformly-signed corpus result is not evidence *for* the reduction hypothesis, only compatible with it.

The `(2 s, scan_block_ms, Interleaved)` row is the **anchor**: it matches scan's recipe on all three
variables and should reproduce `scan_equivalence.noise_floor_db`. **Measured 2026-07-30** on the F15
pair (`fp_silent_core_floor_probe_reduction/`): it does so on **7/10** gaps (err −0.78…+0.51 dB,
median +0.03); its `Downmix` twin was the old anchor and undershot by 3.13–7.96 dB on every gap — the
difference between the two rows *is* the reduction term, read directly. The 3 misses (g5/g6/g10, all
~+2.1 dB) track **window/bin estimator instability** on non-stationary context (those gaps' Interleaved
NF swings several dB across the grid), not a fourth variable and not the excluded span — a fixed
refined-vs-core offset would be stable across window/bin, and these residuals are not. Full write-up:
[TEMP-equivalence-divergence-findings.md](TEMP-equivalence-divergence-findings.md) § *Probe results*
under *Noise-floor probes*.

Measured by calling `level_profile` itself, not by re-deriving the bin walk, so a probe cannot drift
from the measurement it characterizes. That is also why `level_profile` returns its context-bin count
as a second tuple element rather than as a `LevelProfile` field: that type is serialized into every
dumped gap, and this is scaffolding.

`equivalence-calibration DIR` diffs the two per gap and exits 1 only on the **dangerous** direction —
scan *drops* what fine would *keep*. That gate is worth keeping precisely because it is the one
direction fine's biases do **not** produce, so a hit there is a real signal rather than a known offset.
A merely "divergent" gap is expected at ~2 % and is not by itself a defect. See
[gap-vocabulary.md](gap-vocabulary.md) § *Silence-character pre-gate*.

## Lag fingerprint

[`lag_correlation_curve`] sweeps the seam's lag-0 Pearson ([`clip_sync::normalized_correlation`])
over integer shifts; [`summarize_lag_curve`] reports the lag-0 value, the integer peak, a
parabolic-interpolated (fractional) peak, and a verdict:

- **`timing_offset`** — `peak ≥ 0.5` and away from lag 0: a shift recovers correlation (read
  `frac_lag_ms`). Fixable by tightening alignment.
- **`decorrelated`** — `peak < 0.3`: no shift recovers correlation; sources genuinely differ.
- **`ambiguous`** — otherwise.

This is what distinguishes a sub-sample/timing offset (recoverable) from genuine A/B decorrelation
(the seam gate is right to refuse) — see [seam-scoring.md](../seam-scoring.md) §3–4.

The diagnostic `lag` field (Tier-3) is emitted only with `--fingerprint-diagnostics`. Decision
registration is always in `baseline_lag` below.

## Registration & dual-fit measurements

The diagnostic `lag` field sits at the **diagnostic** placement (best-energy bracket / structure throat)
and can wander on quiet gaps. The **decision** registration lives in `baseline_lag`, and the dual-fit
repair predicate is built from seam-local placement (not from `baseline_lag` itself). **Read each field
at the placement it defines — never compare across placements.**

### `baseline_lag` — decision registration at `b_mapped`

Each shoulder is swept **mono** over ±600 ms centered on the geometry **`b_mapped`** nominal, and the post
shoulder is registered **sequentially** (its search is centered on `S + D_A + round(L_pre)`, not the naive
`S + D_A`) so `L_pre` doesn't stack into the post lag. Per shoulder, `summarize_lag_curve` records:

- `peak_r` / `frac_lag_ms` — best correlation and its (fractional) lag.
- **`peak_z`** — the peak's z-score over the whole lag curve. The periodicity-robust uniqueness metric
  (the primary addressability gate; unique ≈ ≥12 at the 1 s window). Deflates on periodic/leveled content
  where a single-rival `prominence` would not.
- `prominence` / `second_peak_r` / `top2_spacing_ms` — single-rival margin and the spacing to it (a
  recurring spacing *is* the content's period). `prominence` is a low-floor tiebreaker, not primary.
- **`edge_pinned`** — the integer peak sits within ~2 ms of the searched boundary (read from the curve
  extremes, so high-side masking counts). The optimum may lie **beyond** ±600 ms, so `frac_lag_ms` /
  `peak_r` are a window-clipped lower bound → `step_ms` is unreliable. Widen the sweep to clear it.

### `splice` — the registration step

Derived from `baseline_lag` mono: `step_ms = post_frac_lag − pre_frac_lag` (the length discontinuity the
repair reconciles), plus per-side `peak_r`/`peak_z` and a combined **`edge_pinned`** (true if *either*
shoulder was search-exhausted ⇒ `step_ms` is GIGO). A nonzero step is the normal signature of **both**
patched and skipped gaps; what makes a gap skip is bracket-search exhaustion, not the step.

### `donor_interior` / `donor_interior_nominal` — is there anything to fill?

B occupancy over the span it would fill. `donor_interior` uses the **aligned** bridge (shoulders at their
own lags); `donor_interior_nominal` uses the **nominal** program-time span with **no lag adjustment**, so it
is registration-independent — the read used to classify a gap as **program-quiet** (B silent at the same
program time ⇒ nothing to fill, not a dropout; ledger D11). Both carry `rms_db`, `silence_fraction`,
`longest_silence_ms`, and `continuous` (no internal sub-floor run longer than 150 ms ⇒ B bridges the hole).

### `splice_dualfit` — would a length-reconciled fill pass the gate?

The **offline predictor** of the dual-fit repair (the repair algorithm itself is
[seam-scoring.md](../seam-scoring.md) § 6 — Dual-fit repair). Computed on the scan's own decode: each
shoulder is placed with `seam_local_peak` re-anchored on **nominal `b_mapped`** (pre butts at
`b_mapped_start`, post at `b_mapped_start + gap_frames`, ±`SEAM_LOCAL_SEARCH_MS`) — **not** on the gross
1 s `baseline_lag` (that older anchor clipped live seams whose lag diverged from the 1 s peak). Seams are
scored at those placements against the **unchanged** gate thresholds:

- `pre_seam_r` / `post_seam_r` and **`gate_pass`** — do both clear `min_fill_correlation` and
  `fill_absolute_floor`?
- `gap_frames` / `bridge_frames` / `trim_frames` (`bridge − gap`, = the step in frames; the interior
  trim/pad amount).
- **`post_seam_global_r`** — the post seam scored at the *pre* offset (step forced to 0). If it also passes,
  a single constant shift suffices and the step is a registration artifact; if only the own-lag post passes,
  the step is real.
- **`pre_seam_z` / `post_seam_z`** — whole-curve z-score of each seam's peak over the ±600 ms search
  (**primary** alias guard; periodicity-robust).
- `pre_seam_prom` / `post_seam_prom` — prominence over the best rival within ±30 ms (secondary; low on
  correct-but-periodic content).

**A shoulders are raw `mono(refined ± w)`**, matching `build_dual_fit_input` / `try_dual_fit` exactly —
**not** the `border_templates_for_gap` construction the seam gate uses (silence walk-off, standoff,
low-energy trim). That is deliberate: this block exists to *predict* production's dual-fit decision, and a
trimmed pre template moves `pre_lag`, which moves `post_seam_global_r`, which flips step-real (F14 —
[TEMP-equivalence-divergence-findings.md](TEMP-equivalence-divergence-findings.md)). Consequence: the block
is **absent** when either shoulder is shorter than `w`, the same range guard production declines on, rather
than being computed against a clipped template.

### Diagnostic-only fields

`wide_envelope`, `seam_probe`, diagnostic `lag`, and `b_levels` are **Tier-3** — emitted only with
`--fingerprint-diagnostics`. They are **not gated on** by any repair decision; they explain decisions and
validate hypotheses. See the analyzer's `legend_text()` for the authoritative placement/window of every
field.

## Source identity & the corpus library

A fingerprint is an **A-vs-B** measurement and is **encoding-sensitive** (the lag/decorrelation verdict
depends on each file's exact decoded waveform — a different B encoding or a partial clip yields a
different result). So identity is **per file**, not per logical source:

- `source.a_source` / `source.b_source` each carry an opaque **`id`** = a stable strided digest of the
  **decoded** PCM (`source_id`). A remux / lossless re-container → *same* id; a different
  codec/bitrate/partial clip → *different* id. The entry's identity is the **pair** `(a_id, b_id)`.
- `source.scan_recipe` echoes the scan params (`min_gap_ms`, `silence_*`, `scan_block_ms`) so two
  entries are known-comparable.
- **No paths or titles** appear anywhere in the committed output.

**Library file names** (from `--gap-fingerprints`):
`<a8>_<b4>_t<hh-mm-ss>_g<idx>_<tier>_<verdict>.json` — opaque-id prefixed, sorted by A start time,
tagged by tier and lag/outcome verdict (e.g. `…_full_timing_offset.json`). Each file is a complete
single-gap `GapCorpus`. A `manifest.json` indexes them (ids, times, tiers, verdicts).

The **committed** instance of this library is the curated per-gap-**type** fixture set at
`crates/clip-sync-repair/tests/gap_corpus/fingerprints/curated/` — one representative single-gap `GapCorpus`
per gap cell (see [gap-vocabulary.md](gap-vocabulary.md)), the media-free input for the gap-classification
tests (`gap_cell_fixtures`, `golden_baseline_invariance`, `gap_repair_spec_diff`).

A second, smaller committed set sits alongside it at
`crates/clip-sync-repair/tests/gap_corpus/fingerprints/equivalence_divergence/` — gaps where
`scan_equivalence` and `equivalence` disagree, pinning the divergence class for `equivalence_divergence`.
Deliberately **not** in `curated/`: a cell is a property of a gap, a divergence is a property of the two
front-ends reading it, so it has no `GapCellType` and is not subject to the per-cell coverage invariants.

**Licensing guardrail:** the only place the real `id → title/path` mapping should live is a
**git-ignored** local file (e.g. `corpus/.sources.local.toml`). Keep it out of the committed corpus.
