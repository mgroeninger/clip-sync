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
- `--fingerprint-gap N` (repeatable, or comma-separated: `--fingerprint-gap 1,3,12`) — characterize
  **only** these gaps. Omit it to characterize **all** gaps. Each characterized gap gets full
  decision/repair detail (per-bracket gate `failure_stage`, `baseline_lag`, `splice_dualfit`, …).
  Unlike `--only-gaps` it takes **bare numbers only** — no `START-END` / `START..END` ranges, no
  timestamps.
  **`N` is 1-based**, matching the `#` column of the repair gap table (and every other user-facing gap
  number in the tool). `0` and out-of-range values are rejected.
  The **emitted corpus stays 0-based**: `GapFingerprint::index` and the `g{:03}` filename segment are
  array positions, so `--fingerprint-gap 3` writes `…_g002_….json`. This is deliberate — existing
  corpus dirs and the `equivalence-calibration` / `gap-fingerprint-stats` joins are unaffected. Locate
  a gap's file by the A-timeline timestamp already in the name (`…_t01-42-08_g002_…`), not by counting.
- `--fingerprint-diagnostics` — also write the **Tier-3 X-set** (`seam_probe`, `wide_envelope`,
  `b_levels`, diagnostic `lag`). Off by default (decision/repair fields only); slower. Needed for the
  analyzer's seam-probe reports.

**`--only-gaps` / `--skip-gaps` do not narrow a dump.** They filter the *repair* plan; the dump
selects with `--fingerprint-gap`. On a **scan-only** run (no `--wav` / `--mux` / `--repair-preview`)
the repair selection is validated and then discarded, so passing them there is rejected pre-scan
rather than silently producing a full-corpus dump the caller believes was narrowed. Alongside a real
repair they are still accepted and still bound the repair — the dump just stays full-corpus.

### `--gap-listen` — hear what the numbers describe

```
clip-sync-repair A.mkv B.m4v --gap-fingerprints gap-files/x --gap-listen --fingerprint-gap 10,14 \
  --no-skip-equivalent-gaps
```

A **WAV side channel on the dump**, not a second corpus owner: one decode produces the fingerprint
JSON *and* listenable clips, so ears and numbers join on the same stem. Requires
`--gap-fingerprints`; gaps are selected with `--fingerprint-gap`.

| Invocation | WAV root |
|---|---|
| `--gap-fingerprints JSON_DIR --gap-listen WAV_DIR` | `WAV_DIR` |
| `--gap-fingerprints JSON_DIR --gap-listen` | `JSON_DIR` (noted on stderr when it defaults) |
| `--gap-listen` without `--gap-fingerprints` | **error** |

Per selected gap, named by the same `entry_stem` as the gap's JSON:

| File | When |
|---|---|
| `<stem>_a_surround.wav` | always — the gap ± `gap_signature_context_secs` (3 s) from A |
| `<stem>_b_surround.wav` | when the gap reaches the fill plan — the mapped donor span, same ± context so the two clips are comparable by ear |
| `<stem>_a_patched.wav` | **only when the production gate patches and the splice applies** — the identical A window after the splice |

A missing `_a_patched.wav` always has a stated reason on stderr: a gate refusal (`skipped`), a
plan-time exclusion (`not_planned`), or — the bug case — `NOT APPLIED`, meaning the gate approved
the gap and the splice then failed, leaving A unchanged across it. The file is withheld rather than
written identical, because an "after" clip that is really the "before" clip is the one artifact that
would invert a listening finding.

**The patched clip comes from the production engine** (`characterize_region` → `execute_region_spec`
→ `splice_into_a`), not from the fingerprint oracle. The oracle's `any_ok` can disagree with the
production gate by design, so splicing from it would answer a different question than "is the repair
this tool would actually make any good?". On a production skip **no patched WAV is written** — an
invented fill would be worse than a missing file, and the skip reason is printed instead.

**Selector and run modes.** `--only-gaps` / `--skip-gaps` are rejected (one selector must drive both
the corpus and the fill plan, or they cover different sets). `--wav`, `--mux` and `--repair-preview`
are rejected too — each makes the patched clip impossible or ambiguous, and unlike the `--mux` /
`--gap-fingerprints` warn-and-ignore precedent this **errors**, because silently delivering
two-thirds of an explicitly requested multi-hour diagnostic is the failure mode the flag exists to
avoid. `dry_run` is *not* rejected: it defaults on and is cleared only by an output flag, so it is
set on every legal listen run.

**Cost.** Every selected gap costs three WAVs *and* a production characterize — the dominant term in
a repair run. A bare `--gap-listen` selects the whole pair; above 25 gaps it warns before the decode
so an accidental multi-hour run can still be aborted. Narrow with `--fingerprint-gap`.

> **A listen corpus is a partial corpus.** The selector filters the dump, so the `corpus.json` a
> listen run writes covers only the listened gaps. That keeps the run cheap, but it is **not** the
> corpus of record for band analysis — keep roll-up tooling pointed at a full-pair dump, and write
> listen runs somewhere they won't be mistaken for one.

> **The WAV root holds licensed audio; the JSON does not.** Every `_surround` / `_patched` file is
> decoded source material, so a listen WAV root is in the same class as `gap-files/`: **gitignored,
> ephemeral, never committed, never quoted.** Point `--gap-listen` at a path already covered by
> `.gitignore` — the bare-flag form puts licensed audio next to committable JSON, which is convenient
> for listening and a hazard at commit time. The *stems* are `entry_stem`s (id prefixes and
> timestamps, never titles or paths), so names are safe to quote in notes even though the files are
> not. That is by construction; keep it that way if the naming authority ever changes.

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
with `--wav` currently runs both and decodes A/B twice — fine, but not free.)* `--gap-listen` is the
exception to the warn-and-ignore rule: with `--mux` it **errors** rather than being dropped.

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

**This is a dump cost, and `--gap-listen` inherits all of it.** The per-bracket loop
(`measure.rs:1680`) is gated on `DetailTier::Full` and runs over every feasible bracket
**unconditionally** — nothing about it is triggered by the gate's verdict. Measured on a synthetic
non-matching donor (2026-08-02): ~350 s total, of which the production gate's refusal was **0.65 s**
and the rest was the dump's `anchor_matchability` / `local_anchor_xcorr` sweep. A plain production
repair does *not* pay this (it evaluates the gate once per gap, not once per bracket). Practical
consequence: on real media **every gap the gate refuses adds minutes of anchor oracle to a listen
run** — and refused gaps are exactly what a margin-band experiment selects. Budget for it. Two
cheapening hypotheses (bounding the fill search; shrinking the timeline) were measured and **both
refuted**.

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
| `levels` | key always; **`bin_ms`/`profile_db`/`speech_peak_db` not measured on the production path** | intended: `bin_ms`, `profile_db[]` (RMS dBFS across pre→post context), speech-peak/noise-floor/gap-floor dB. In a production dump only `noise_floor_db` and the gap floor are real — see *Not measured* below |
| `silence` | key always; **not measured on the production path** | intended: collar RMS/peak ratio + whether it clears the **relative** silence test (border walk-off discriminator) |
| `contour` | key always; **not measured on the production path** | intended: `has_anchor_seam_contour`, pre/post envelope flatness |
| `anchors` | key always; **not measured on the production path** | intended: pre/post candidates `{ time, source, prominence, rms_db }`. Production dumps carry `pre: []`, `post: []` |
| `brackets` | full | feasible brackets `{ span, move, seam_*, structure_*, failure_stage, residual_margin_db? }`; `start_frame`/`fill_frames` only on brackets that pass the gate. On `structure_floor`, scores live in `structure_*` (not overloaded onto `seam_*`); on `residual`, `residual_margin_db` carries the applied headroom margin. Gap-level `structure`/`seams` blocks remain omitted under `skip_baseline_placement` (F1) |
| `structure` / `seams` | **omitted on the production path** | intended: baseline scores; seams carry per-channel + selected channels. Suppressed by `skip_baseline_placement`; deferred as Finding F1 (`archive/TEMP-pipeline-perf-redesign-plan.md` §8g.4a) |
| `baseline_lag` | full, B present | **decision** per-shoulder lag fingerprint registered at **`b_mapped`** (see *Registration & dual-fit*) |
| `splice` | full, B present | first-class registration step derived from `baseline_lag` mono: `step_ms`, per-side `peak_r`/`peak_z`, `edge_pinned` |
| `donor_interior` | full, B present | B occupancy over the **aligned** bridge span (`b_mapped_start+L_pre … b_mapped_end+L_post`): `rms_db`, `silence_fraction`, `longest_silence_ms`, `continuous` |
| `donor_interior_nominal` | full, B present | B occupancy over the **nominal** geometry span (no lag adjustment) — registration-independent; the D11 program-quiet signal |
| `splice_dualfit` | full, B present | dual-fit viability: seams scored at per-shoulder placement + `gate_pass` / `trim_frames` / validators (see below) |
| `residual` | full, B present | least-squares same-source cancellation (dB) vs noise floor at the decision seam |
| `outcome` | B present | plan_kind, tier, fit_path, signature_mode, skip_reason. `seam_shape` is **not measured on the production path** (always `""`) |
| `equivalence` | B present | **gap-equivalence class (diagnostic)** — does this gap need patching? (silence-character; see below) |
| `scan_equivalence` | scan classified | the **authoritative production** verdict for the same gap (`GapReport::gap_equivalence`; block size = the `scan_block_ms` knob), copied in so one dump holds both readings for calibration. **This is the authoritative one** — see below |
| `lag` | diagnostics | **Tier-3** per pre/post anchor lag fingerprint at the best-energy bracket / structure throat — requires `--fingerprint-diagnostics` |
| `wide_envelope` | diagnostics | **Tier-3** 100 ms-bin RMS-envelope lag peak at `b_mapped` — cross-scale confirmer of `baseline_lag` |
| `seam_probe` | diagnostics | **Tier-3** encoding-robust seam metrics (R2/R4/spectrum/env/recovered); not used by any gate |
| `b_levels` | diagnostics | **Tier-3** symmetric B-side `LevelProfile` (validation instrument for the program-quiet hypothesis) |

### Not measured — the fields a production dump does *not* fill

Several fields above are structurally present but never measured on the path a real
`--gap-fingerprints` run takes. They are not missing data to be chased; they are **questions the
production path does not ask**. The type system cannot say so — `bin_ms` is a `u32`, not an
`Option<u32>` — so a `0` is indistinguishable from a measured zero, and every consumer that read one
as a measurement read a fabrication.

Two emitters produce them, both by design:

- `spec_to_fingerprint_summary` (`project.rs`) rebuilds each **measured** gap from its plan spec, and
  the spec carries no envelope, collar, contour, anchor set, or seam shape. So it writes structural
  defaults.
- `projected_level_profile` (same file) hardcodes `bin_ms: 0`, an empty `profile_db`, and
  `floor_db`/`speech_peak_db` at `SILENCE_FLOOR_DB` (−120).
- `projected_lag_entry` (same file) builds each `baseline_lag` shoulder from four stored scalars —
  `peak_r`, `frac_lag_ms`, `peak_z`, `prominence`, all real — and fabricates the rest of the row:
  `window_ms`/`max_lag_ms`/`peak_lag_samples`/`frac_lag_samples` at `0`, `lag0_r` as a **second copy
  of `peak_r`**, and `verdict` hardcoded `timing_offset`.

  The last two matter beyond bookkeeping. `lag0_r == peak_r` says the shoulder peaks exactly at zero
  lag — textbook perfect registration — on every gap it touches, while the real lag-0 correlation is
  not carried anywhere. And `verdict` reads as a classification while being a constant: **a projected
  row can never be `decorrelated` or `ambiguous`**, so "every gap in the corpus is `timing_offset`"
  describes this function, not the media.

  **This is now the fallback, not the norm.** `lag_at_placement` already sweeps ±`lag_max_lag_ms` at
  `b_mapped` on the from-decode path, and `characterize_gaps_from_decode` hands that real
  `LagFingerprint` to `spec_to_fingerprint_summary` via `MeasuredDetail` — the same pass-through
  brackets use. Only the oracle path (`GapRepairSpec` alone, no PCM) still projects, because the spec
  stores the four scalars and nothing else. So the declaration is **conditional**: the six
  `baseline_lag.*` paths live in `PROJECTED_BASELINE_LAG_FIELDS` and are appended to `not_measured`
  only when some gap actually got a fabricated row.

  Corpora dumped **before 2026‑08‑03** predate the pass-through: every `baseline_lag` row in them is
  projected, and none of them declare it. Any conclusion drawn from their verdict distribution or
  their apparent zero-lag registration needs re-deriving from a fresh dump.

Note the inversion this creates: a gap that the pipeline **measured successfully** is stripped, while
a gap that failed early enough to skip projection keeps its real values. On the 2026‑07‑31 corpus that
was 802 stripped and 27 intact — so "some gaps have anchors" is not evidence the rest lack them.

The dump therefore **declares** the list, in `source.not_measured` (`SourceMeta`), populated from
`NOT_MEASURED_BY_PROJECTION` (`schema.rs`). Consumers should treat a listed field as absent rather
than as a value. Two guards keep the declaration honest:

- a unit test (`production_dump_declares_and_honours_its_unmeasured_fields`) asserts the production
  builder emits the list **and** that every listed field really is at its default;
- `--check` (harness) fails a corpus whose gaps show the tell-tale constants without the declaration,
  and fails one that declares a field it actually measured.

Corpora dumped before 2026‑08‑01 have no `not_measured` key. That absence is not a promise the fields
are real — it predates the declaration.

### Gate recipe — seam-gate thresholds on `source`

From-decode dumps stamp `source.gate_recipe` with the seam-gate floors used to assign every
`brackets[].failure_stage`:

| field | role |
|-------|------|
| `min_structure_match_score` | `structure_floor` |
| `min_fill_correlation` / `fill_absolute_floor` / `fill_marginal_margin` | `waveform_floor` (plus short-gap mean / one-strong-seam flags) |
| `short_gap_mean_correlation_secs` / `short_gap_one_strong_seam_fallback` | short-gap structure/waveform relaxations |
| `residual_headroom_margin_db` / `residual_gate` | `residual` stage |

Absent on pre-2026-08-03 corpora and on summary-only / refused corpora. With bracket scores, this is
enough to audit stage assignment without re-scoring PCM. Equivalence has the same pattern in
`scan_equivalence.thresholds`.

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

The fingerprint **always emits** `equivalence` (diagnostic) and `scan_equivalence` (production) for calibration —
the dump itself never drops gaps. Production plan-time drop is separate and **on by default**
(`skip_equivalent_gaps = true`; `--no-skip-equivalent-gaps` to patch all). See [gap-scan.md](../gap-scan.md).

### `equivalence` vs `scan_equivalence` — a second opinion, not an oracle

They feed the **same classifier**. They once did so from **differently defined** inputs; through
2026-07-30/31 those definitions were converged one at a time (F15, then I1, then I3), each validated on
media. Noise-floor context followed (both ±`EQUIVALENCE_CONTEXT_SECS`). What remains is not a
tunable parameter split — see the open rows at the bottom of the table. Read them accordingly.

- **`scan_equivalence` is authoritative.** It is the verdict production acts on (`skip_equivalent_gaps`),
  measured on scan blocks (size = the `scan_block_ms` recipe knob — not a constant, and not 250 ms).
  The curated gap **cells** in [gap-vocabulary.md](gap-vocabulary.md) are scan-time cells, so a fixture's
  declared cell is checked against *this* field.
- **`equivalence` is diagnostic only.** Nothing in the plan or patch path reads it; it exists to be
  compared. Silent-core A gap RMS and floor, interleaved reduction, `scan_block_ms` bins — the same
  definitions the scan gate uses.

**Do not call these two "fine" and "coarse" (rename completed 2026-07-31).** `equivalence` was called
"the fine reference" here until 2026-07-30, and the wording bred a recurring error: reading a divergence
as "the coarse gate is inaccurate". The label was never right. The 50 ms binning it named was itself an
accident — inherited by proximity from `gap_signature_bin_ms`, a value tuned for structure *pattern
matching*, where finer genuinely is better; a **max** statistic and a **threshold-crossing fraction**
want the opposite. So "fine" described an accident, and implied *more accurate* where the truth was
*more biased*. I1 then removed the binning difference outright, making the factual half false too. The
axis is **production/authoritative** vs **diagnostic**, not resolution.

Do not cite the old table either — it described defects that no longer exist:

| input | status | note |
|---|---|---|
| `gap_floor_db` | **converged** (F15) | the diagnostic path took the max over **all** bins in the span (a content peak); it now takes the max over A's **silent** bins, like scan. Measured 0.00 dB apart on 10/10 gaps |
| A gap RMS | **converged** (F15) | same silent-core filter and span rule; 0.00 dB apart on 10/10 |
| channel reduction | **converged** (F15) | both interleaved power mean. This was the *dominant* noise-floor term (Cauchy–Schwarz bounds downmix ≤ interleaved, gap up to `10·log10(N)` = 7.78 dB at 6ch) |
| bin width | **converged** (I1) | the overlay had inherited `gap_signature_bin_ms` = 50 ms; now `scan_block_ms`. This is the row that retired the word "fine" |
| donor predicate | **converged** (I3) | the diagnostic path now applies scan's `b.silent ‖ rms < floor` disjunction |
| noise-floor context | **converged** | both ±2.0 s (`EQUIVALENCE_CONTEXT_SECS`). The diagnostic path had briefly used `gap_signature_context_secs` = 3.0 by inheritance from the signature job, never by choice for this one |
| **bin lattice phase** | **open, not convergeable by parameter** | scan bins on its media-absolute timeline; the overlay bins from `gap_start − context_frames` (A) and the donor window start (B), with a ragged final bin at full weight. Equal spans, counts and floors still disagree by a block |
| **donor window** | **open, small** | scan maps the **core**, the diagnostic path the **nominal** `b_mapped` span. Median `donor_silence_fraction` delta 0.008 (max 0.067), every gap within ±1 block, mixed signs |

So the diagnostic path is still the **more aggressive** side, but only by the last two rows, and only
**one** of them is one-signed — the donor window is mixed-sign, so do not say *both* open differences
bias toward `drop` (that sentence was true of the F15-era pair and silently re-pointed when they
converged). Sizes are medians over **one pair, ten gaps** — they bound the axes on that pair, not on
the corpus.

**Measured population (2026-07-30):** 5 divergent / 297 gaps (1.7 %) across a 17-pair corpus, **0 in the
dangerous direction**. Still exactly correct *as a historical measurement*, but do not quote it as a
current rate: it was taken before I1/I3, so a fresh run should read lower. Read the `0 dangerous` with one
caveat: every pair in that corpus is lossy and bottoms out near −101 dB, so it structurally could not
reach the −120 digital-silence condition that I3 fixed. **That caveat is permanent** — no lossless
media is available, so no corpus will ever produce the condition, and the corpus statistic is
retired as evidence for I3 rather than pending a future pair. The evidence lives in tests instead:
`digitally_silent_donor_reads_silent_against_a_digitally_silent_floor`
(`application/gap_equivalence.rs`) reproduces the −120 condition and pins the mechanism inline
*including* the negative control (it asserts a floor-only predicate would read digital silence as
occupied), and `lossless_silence_pair` carries the same condition through a real container end to end. The original mechanism was a donor whose level
sat *between* the two floors — silent to the diagnostic read, occupied to scan. That band is closed
(F15 + I1); the committed
`tests/gap_corpus/fingerprints/equivalence_divergence/band_donor.json` is now a regression fixture
pinning agreement on that gap. Full analysis in
[archive/TEMP-equivalence-divergence-findings.md](archive/TEMP-equivalence-divergence-findings.md) § F15.

#### `measurement` — the live recipe on each verdict (Track B)

Permanent replacement for the deleted `silent_core_probes` grid. Nested on both `scan_equivalence`
and `equivalence` so a calibration diff can attribute a residual to an instrument difference without
reading source. Provenance only — nothing classifies on it. Spec:
[TEMP-fingerprint-provenance-plan.md](archive/TEMP-fingerprint-provenance-plan.md) §3a.

| field | meaning |
|---|---|
| `context_secs` | noise-floor context half-width (`EQUIVALENCE_CONTEXT_SECS` = 2.0 on both sides) |
| `bin_ms` | bin width **actually measured** (from the level stream on scan; `scan_block_ms` on diagnostic) |
| `reduction` | `interleaved` on both paths today |
| `a_span` | `core` on both today (block-confirmed / raw gap) |
| `donor_span` | `core` (scan, offset-mapped) vs `nominal` (diagnostic `b_mapped`) — the remaining donor-window residual |

Also flat beside the signals: `a_gap_silent_blocks` / `a_gap_total_blocks` and the donor pair. The
bin-divergence check is `a_gap_total_blocks × measurement.bin_ms ≈ span_secs` (I1 class). Absent
`measurement` (and absent A population counts) means a pre-Track-B corpus, or a scan with an empty
level stream — `None`, not `Some(0)`. Counts are only meaningful alongside a present `measurement`.

`silent_core_probes` was **hard-deleted** from the emit/type once this landed. Committed fixtures may
still carry the old JSON key — serde ignores it; do not rewrite fixtures just to strip it.
`noise_floor_probes` still ships on the diagnostic `equivalence` verdict (see below); `scan_equivalence`
never fills it, so the key is omitted there.

#### `thresholds` — what the class was decided *against* (2026-08-01)

`measurement` records how a verdict was measured; `thresholds` records what those measurements were
compared to. Both live on `scan_equivalence` and `equivalence`.

| field | meaning |
|---|---|
| `dropout_margin_db` | `GapEquivalenceParams::dropout_margin_db` as applied (35.0). A is a dropout below `noise_floor_db − dropout_margin_db` |
| `donor_silence_thresh` | `GapEquivalenceParams::donor_silence_thresh` as applied (0.5). B is occupied below this fraction |

Every *measured* input to the class was already emitted — `a_gap_rms_db`, `noise_floor_db`,
`a_below_noise_db`, `donor_silence_fraction`, and the block populations behind the last. The values
they are compared against were not, so a reader could only recompute a class by assuming the defaults
in force the day the dump was written, and `GapEquivalenceParams` is explicitly overridable. Both
front-ends hardcode `..Default::default()` today, so the assumption did hold for every dump written
before 2026-08-01 — recording it is what stops that from being a fact about one month.

**Presence is load-bearing.** `thresholds` is `Some` *iff the classifier actually compared something*:
absent on both `NotEvaluated` returns (gate off, or a missing signal), present on every decided class.
`measurement` cannot answer that question — the front-ends attach it after the fact and it appears on
all four classes, including the 20 `not_evaluated` gaps of the 39-pair corpus.

#### The margin band — `equivalence-calibration --band`

The band asks which gaps production **drops** sit close enough to a class boundary that a small
instrument error would have changed the verdict. It exists because the two failure directions are not
symmetric: a false drop ships an unrepaired hole, a false keep costs one declined patch attempt.

It is computed, never stored. `equivalence-calibration --band DIR` re-runs the classification rule
with each boundary loosened — `--band-dropout-db` (default 1.0) and `--band-donor-blocks` (default 1,
the measured donor-lattice disagreement) — and at width zero it reduces to the production rule
exactly, which is what stops it from becoming a second classifier. Gaps whose verdict predates
`thresholds` are **counted and refused**, not banded against assumed defaults.

There is deliberately **no emitted `near_boundary` flag**: the width is a policy under calibration, and
a stored boolean would freeze one width into every dump and drift the moment it is retuned.

Output is a per-pair `--only-gaps` token list — **one-based**, converted from the zero-based
`GapFingerprint::index`, which is why it lives in a tested binary rather than a shell one-liner (every
token of an off-by-one list still resolves, so the mistake yields a clean run against the neighbouring
gaps). Feed it back with the gate disabled to get the counterfactual the dumps cannot supply:

```
clip-sync-repair A B --gap-fingerprints DIR --no-skip-equivalent-gaps --only-gaps 3,7,12
```

Via `scripts/measure-gap-fingerprints.ps1` those go in the manifest's per-pair `extra` column, quoted
or not — the 4th field runs to end of line, so unquoted delimiters inside it are rejoined rather than
read as further columns. This is not `ConvertFrom-Csv`'s behaviour: with a 4-name `-Header` it
discards surplus fields silently (`--only-gaps 3,7,12` → `3`, exit 0, wrong gaps, `$Error` empty).
Quoting dodges that by keeping the field count at 4, but the script parses the row itself so the
correct manifest is not the one that remembered to quote.

#### `equivalence.noise_floor_probes` — context × bin × reduction counterfactuals

Both live paths estimate the noise floor the *same way* (median of context bins outside the gap), at
`scan_block_ms` bins, interleaved reduction, over **±2.0 s**. The grid is not a residual between
front-ends — it is a labelled counterfactual: it still carries the `gap_signature_context_secs` row, so
"would a wider window have decided differently?" is answerable from provenance instead of from an
unlabelled difference inside the verdict being compared. The same grid is what let bin size and
reduction be charged separately before those axes converged:

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
- **`Downmix`** — average the channels per frame, *then* square: an **amplitude** mean. What the
  diagnostic path's `mono_rms` / `interleaved_to_mono` do.

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
[archive/TEMP-equivalence-divergence-findings.md](archive/TEMP-equivalence-divergence-findings.md) § *Probe results*
under *Noise-floor probes*.

Measured by calling `level_profile` itself, not by re-deriving the bin walk, so a probe cannot drift
from the measurement it characterizes. That is also why `level_profile` returns its context-bin count
as a second tuple element rather than as a `LevelProfile` field: that type is serialized into every
dumped gap, and this is scaffolding.

`equivalence-calibration DIR` diffs the two per gap and exits 1 only on the **dangerous** direction —
scan *drops* what the diagnostic path would *keep*. That gate is worth keeping precisely because it is
the one direction the diagnostic side's biases do **not** produce, so a hit there is a real signal
rather than a known offset. A merely "divergent" gap was seen at ~2 % pre-I1/I3 and is not by itself a
defect. See
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
[archive/TEMP-equivalence-divergence-findings.md](archive/TEMP-equivalence-divergence-findings.md)). Consequence: the block
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
- **Source provenance** (Track A of
  [TEMP-fingerprint-provenance-plan.md](archive/TEMP-fingerprint-provenance-plan.md) §2): each `FileSource` also
  records what the probe read off that side's container, so a corpus can state what media it measured:
  - `codec` — the codec **family** the probe read (`aac`, `ac3`, `eac3`, `mp3`, `flac`, `vorbis`,
    `alac`, `pcm`, `alaw`, `mulaw`, else the raw Symphonia name, which renders as a bare hex id like
    `0x1f2`). Family, not format: Symphonia has 36 linear-PCM ids because the id encodes depth,
    signedness, endianness and planarity together, and all 36 collapse to one `pcm` — depth is
    `bit_depth`'s axis, and per-id tokens would split one population across dozens of census buckets.
    **G.711 `alaw` / `mulaw` are deliberately *not* `pcm`** despite Symphonia naming them
    `CODEC_ID_PCM_*`: companding is lossy, and folding them in would let a census assert losslessness
    about lossy material. Separately, an **absent** `codec` means a pre-Track-A corpus, not PCM and
    not lossless — absence is unanswerable, never a reading.
  - `bit_depth` — one of the pinned tokens `s16` / `s24` / `s32` / `f32` / `other:<bits>`.
  - `native_sample_rate` / `native_channels` — that side's **own** rate/layout, which differ from the
    sibling `sample_rate` / `channels` (the rate everything was *measured* at: A's, with B resampled to
    it). `FileSource::was_resampled()` is `native_sample_rate != sample_rate`, or `None` when the corpus
    predates these fields — **not** `false`; "unanswerable" and "no" are different readings.
  - When `native_channels` disagree, characterize **refuses** pairwise measurement:
    `source.incomparable = "channel_layout_mismatch"`, `gaps` is empty, and `b_source.channels` /
    `duration_secs` / `id` use **B's** layout (not A's) — the opposite of the normal path, where both
    sides are described at the layout everything was measured at. Consequence: a refused corpus and a
    normal one over the same media carry different `b_source.id`s, so do not join the two on `id`.
    Same condition production fill already skips as
    `TrackLayoutMismatch`. The dump prints a progress line naming the refuse; `gap-fingerprint-stats
    --check` **Warn**s on `incomparable`, and also Warns on legacy dumps where `native_channels`
    disagree but `gaps` is still non-empty (pre-refuse silent wrong indexing — re-dump).
  - `source_audio_bitrate_bps` — the measured source bitrate for that side. Do not fold the two sides'
    bitrates into one figure.

  All are raw observations, never verdicts: predicates (lossy? resampled? mixed-codec pair?) are derived
  in code, because a frozen bool in JSON can only be corrected by a full re-decode. All are optional and
  omitted when absent, so pre-Track-A corpora still parse; `check --gap-fingerprints` emits a per-pair
  **Warn** for a corpus carrying none of them. Media-free callers (synthetic fixtures) pass no descriptor
  and so record nothing rather than guessing. `container` remains declared but **never populated**.

  **A corpus without these fields cannot qualify a null result.** "Zero divergent gaps" is only a claim
  about the population it was measured over, and a corpus that cannot name its codecs cannot state that
  population — the null may hold for `flac→flac` and say nothing about `flac→aac`. Read the health
  Warn, or the roll-up's codec census, before generalizing from a zero.

  **The census reads reachability off the codec, so a lossy→PCM intermediate breaks it.** Grouping on
  `codec` answers "could this pair have reached the −120 clamp?" only because codec implies floor.
  Decoding or remuxing lossy sources to WAV keeps the ~−101 dB floor while making the census read
  `pcm`, and a reader would infer reachability that isn't there. Since no genuinely lossless material
  is available, a future `pcm` in a census is *more likely* to be such an intermediate than a real
  lossless source — fingerprint the original containers, and treat a surprise `pcm` as a question
  about provenance rather than as evidence.
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
`crates/clip-sync-repair/tests/gap_corpus/fingerprints/equivalence_divergence/` — originally gaps where
`scan_equivalence` and `equivalence` disagreed; `band_donor.json` is now a **regression** fixture for
the closed F15 band mechanism (paths agree post-F15 + I1). Deliberately **not** in `curated/`: a cell
is a property of a gap, a divergence is a property of the two front-ends reading it, so it has no
`GapCellType` and is not subject to the per-cell coverage invariants.

**Licensing guardrail:** the only place the real `id → title/path` mapping should live is a
**git-ignored** local file (e.g. `corpus/.sources.local.toml`). Keep it out of the committed corpus.
