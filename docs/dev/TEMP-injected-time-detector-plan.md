# TEMP — Injected-time detector (pre-patch warning)

**Status:** draft, 2026-09-08. Rewritten the same day to put the operator
contract first; construction pins (caller, types, fixtures) added after
a source-code readiness review the same day. Corpus read done
(constellation dump, no re-scan); ship branch picked — **T = 150 ms,
N = 2**. Production wiring not started.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Post-join defects (pairs 31
and 50)* — those pairs are the existence proofs. This Change is a
file-level operator warning (sibling of A1–A3), not a join-quality
fix and not a fill.

**Predecessor (context, not superseded):**
[archive/TEMP-constellation-fingerprint-plan.md](archive/TEMP-constellation-fingerprint-plan.md)
§ 11: the remaining-skip population that probe aimed at is dominated
by step-shaped **injected-time** regions, which no lag matcher locates.
This plan detects that file flaw and **warns**, before the patch
search spends the time those regions cost.

**Companions:**
[TEMP-disjoint-offseam-lock-plan.md](TEMP-disjoint-offseam-lock-plan.md)
(one shoulder, one gap, off-seam lock — complementary, not this
warning); [cli-output.md](../cli-output.md) § Timeline / duration
warnings (A1–A3); [json-output.md](../json-output.md) additive
revision in the same Change.

**Media hygiene:** pair index + dump `gNNN` (0-based) in this note.
User-facing copy and CLI arguments use **gap numbers** (1-based, the
table `#` column) — [gap-vocabulary.md](gap-vocabulary.md) § *Gap
numbering*. Corpus pairs are existence proofs and negative controls
for (T, N). They are not the identity of the warning.

---

## 0. Architecture (normative)

This section is the plan. Later sections only implement it. If a later
sentence disagrees with this one, this one wins.

**After A and B PCM are in hand and before characterize, warn once if
the planned gaps look like injected time — a bounded span of extra or
missing program on one file relative to the other. The run continues.
No new flag. No skip. No fill change. Ctrl-C is the abort.**

```text
decode A+B
pre-pass: shoulder_lags_at_b_mapped, every planned gap
sequence pass at T = 150 ms, N = 2
  if flagged: stderr now; record for stdout report + JSON
characterize → …                // unchanged; operator may Ctrl-C
```

The detector is the sequence pass. One isolated spike is
`--dual-fit`'s job when its gate can fire. Whole-file
`offsets_consistent` / `offset_drift` already cover a step that lasts
to the end clip. This warning's extra job is a **bounded island**.

The pre-pass measurement is the **mono Full-tier `lag_at_placement`
construction** (§ 5), not a `lag_pair` call with fill-border templates.
`lag_pair` is the last step of that construction, not the statistic
§ 6 measured.

It does **not** name which file carries the extra or missing time, does
not stop the patch loop, does not change `shoulders_abut` or
`--dual-fit`, does not stamp per-gap lag fields onto `GapTags` / v1
JSON gap rows, and does not fix any fill. Scan-only (`dry_run`, no
preview/write) never holds A+B PCM — out of scope; a later
scan-time pass could warn before decode, and is not this Change.

One sentence: **decode both files; cheap ungated pre/post lag at every
planned gap, using the same mono construction Full-tier `lag_decision`
uses; if two or more consecutive planned gaps disagree by more than
150 ms, tell the operator this pair looks like injected time, point at
those gap numbers, and continue.**

---

## 1. What “injected time” is

One file has extra or missing program in a **bounded** span relative
to the other (an ad insert, a dropped scene, a padded silence that
shifts everything after it). Alignment's start-clip offset is then
wrong through that island: both shoulders of each gap in the span
correlate against the wrong B content, so filling those gaps is
significantly harder. Gaps outside the island can still patch.

The statistic that lights it, without claiming A vs B, is a *run* of
ungated pre/post shoulder-lag disagreements at the alignment-mapped
(`b_mapped`) position. `--dual-fit`'s `placement_slide` is not that
measurement: its confidence gate (`peak_r >= ~0.35`) is `None` on
exactly the gaps where the divergence is loudest.
`anchored-retry`'s `PatchOffsetAnchor` is a single scalar on
`Patched` rows only.

**Not these, and not with their words:**

| Already shipped | What it is | Why this is different |
|-----------------|------------|------------------------|
| **A2** “audio timeline mismatch on video A” | PTS vs decoded-sample clock **on one file** | Same-file remux skew. Prefer-the-MKV is the wrong action here. |
| **Alignment unstable** | Start/end clip offsets disagree **and** silence cross-check `MISMATCH` | Whole-file drift that lasts to the end clip. Do not rephrase that fact. |
| Isolated seam / `--dual-fit` | One gap's pre and post disagree | Neighbors nominal. `N = 2` leaves it silent. |

Existence proofs (dump indices, not copy): pair 31 g7–g8; pair 50
g9–g13 and g15–g18 (g14 is a one-gap dip — see § 6). Collapse's
introduced-silence visit ([TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md))
is a different hole: A has dead time B does not, picture was not
padded, the honest repair is drain. This warning does not drain.

---

## 2. Warning contract (normative)

Same family as A1–A3 ([cli-output.md](../cli-output.md) § Timeline /
duration warnings). Pin the table, the copy, and the JSON key in
those docs when this ships. Today that section says “up to three
`Warning:` lines”; this Change is **A4**.

| ID | When | Condition | Example stdout / stderr line |
|----|------|-----------|------------------------------|
| **A4** | After A+B decode, **before** characterize, on every path that holds both PCM buffers and is about to search: write, `--repair-preview`, `--patch-only`, and calibration `--gap-listen` | Sequence pass at **T = 150 ms, N = 2** (§ 3) | `Warning: this pair looks like it has injected time (extra or missing program in a bounded span) around gap #8–#9 — filling those gaps can fail or sound wrong; the run continues (Ctrl-C to stop before the long search)` |

**Why it fires:** a run of planned gaps at `b_mapped` has pre/post
shoulder lag disagreeing by more than 150 ms. That is the signature
of a bounded map island **outside** the lag window, not a single
bad seam and not start/end clip drift.

**What to do:**

- Interactive default (stderr still live): **Ctrl-C** if this pair
  should not spend characterize time. Inspect the named gaps with
  `--gap-listen` or `--only-gaps` (gap numbers match the table `#`
  column). A follow-up `--only-gaps` that leaves fewer than N = 2
  planned gaps will not re-fire A4 — that is the sequence pass, not
  a bug. Do not expect the default fill path to recover a step
  outside ±600 ms / ±2000 ms envelope. The run does not stop on its
  own.
- `--quiet` / `--format json --quiet`: no mid-run line. Parse
  `patch.injected_time` **after** the run. This Change does not
  abort unattended jobs and does not change the exit code.
- Not A2: do not remux to “fix timestamps.” The clocks can be fine.

**Default copy names the run(s) that fired**, as 1-based gap-number
spans matching the table. Two runs → both named, joined by “and”
(`around gap #10–#14 and gap #16–#19`). Do **not** glue across a dip
below T (pair 50's g14 / gap #15). Do not name a corpus pair, a
map-step size, or which file is wrong. Verbose may append the A-time
range of each run (`video_a_start_secs`–`video_a_end_secs` of the
end-cap gaps) — the recipe-stable handle — on the **stdout** line
only. Mid-run `phase()` stays the default copy (one line to abort
on).

**Streams.** Mid-run: one `phase()` line on stderr, same text as the
stdout `Warning:`, as soon as the pre-pass flags. That is an
intentional exception to [cli-output.md](../cli-output.md)
“no duplication” / “stderr is operational”: the value is aborting
**before** `char_gate_search`. Do **not** also `tracing::warn` the
same line — Auto would print it twice. `--quiet` suppresses
`phase()` (`StderrProgressReporter` no-ops when `ProgressMode::Quiet`);
the stdout report (human) still carries A4 at the end.

A1–A3 today go through `collect_repair_warnings` and that loop
`tracing::warn`s every line. **A4 must not join that loop.** Format
it in `domain/diagnostics.rs` (same family, separate function) and
append it on stdout from `infrastructure/cli/output.rs` the way
`not_applied_count` is appended: report text only, no
`tracing::warn`. JSON has no human `Warning:` array (same rule as
A1–A3).

**JSON** (additive optional key on [`PatchSummary`](../json-output.md),
absent when the pass does not flag; scan-only has no `patch` object).
Same pattern as `program_gain` / `shoulders_abut`: the domain summary
is what `--format json` already serializes. Not a `GapTags` dump.
`gap_index` is 0-based (JSON convention); human copy adds one.
`skip_serializing_if` on `None` so existing goldens stay byte-identical
until a run actually flags.

```json
"injected_time": {
  "runs": [
    {
      "first_gap_index": 7,
      "last_gap_index": 8,
      "a_start_secs": 1234.5,
      "a_end_secs": 1238.0
    }
  ]
}
```

`runs` is one object per consecutive-T stretch. `first_gap_index` /
`last_gap_index` are `FillRegion::gap_index` (report index, not plan
slot). `a_start_secs` / `a_end_secs` are the first gap's
`video_a_start_secs` and the last gap's `video_a_end_secs` on the
report — not refined frames. No `pre` / `post` / `delta` / `peak_r`
on this object.

---

## 3. Sequence pass (pinned)

**T = 150 ms, N = 2.** Conservative: prefer a miss over a false
warning. Pinned from the § 6 read; do not re-open for this Change.

A run of ≥ **2** consecutive measured gaps where each gap's
`|post_frac_lag_ms − pre_frac_lag_ms|` exceeds **150 ms** is one
warning for the pair. Consecutive means sorted `gap_index` among
measured gaps differs by **exactly 1**. A missing shoulder, an
unplanned gap (equivalent-skip, unfillable, `--only-gaps` /
`--skip-gaps`), or a dip below T **breaks** a run — it is a hole,
not glue. Isolated single-gap spikes must not warn.

The pair warns once even if two runs qualify. Default copy lists
each run; it does not claim they are one region.

This is a pure domain function. Name and shape:

```text
find_injected_time_runs(
    measured: &[(gap_index, pre_frac_lag_ms, post_frac_lag_ms)],
    t_ms = 150,
    n = 2,
) -> Vec<InjectedTimeRun>
```

`measured` is only the planned gaps that produced **both** anchors,
already sorted by `gap_index`. Unit-test it on delta arrays, not
PCM: pair 31's two-gap run; pair 50's g14 dip (two runs, not one);
the 19/g2 and 25/g2 isolates (silent); a skipped index in the middle
of a loud stretch (two runs, not one).

Expected interior signature when the island is **outside** the lag
window: both shoulders correlate against the wrong B content, so
ungated delta stays elevated through the mis-mapped span, not only
at its two edge gaps. An in-window constant offset that only lights
the two edge gaps is not this warning (it fails N, and it is not
the corpus shape).

Population: every gap in the fill plan that has a mapped B span,
once `PatchAudio` has decoded A and B and **before** any region is
characterized. Disposition is unknown; the sequence pass needs a
lag reading on every planned gap or a real run cannot be told apart
from noise at its edges. Not the constellation remaining-skip
population, and not “gaps `characterize_region` has already placed.”
The § 6 proof is on dump indices of every scanned gap; production
sees the planned subset. That is conservative (a miss, not a false
warning) and is the specified population.

---

## 4. When it runs (modes, cost)

| Path | A4 |
|------|----|
| Write (`--wav` / `--mux`) | yes |
| `--repair-preview` | yes |
| `--patch-only` | yes |
| `--gap-listen` (calibration feature) | yes — same `run_with_decoded`, **after** the fingerprint dump |
| Scan-only / `dry_run` (no preview/write) | **no** — no A+B PCM |
| Empty fill plan (mismatch, nothing selected, all equivalent) | **no** — `run` short-circuits before decode |

After `decode_ab` in `PatchAudio::run_with_decoded`, before the
characterize loop: `shoulder_lags_at_b_mapped` for every planned
region; then the sequence pass. No extra file read.

Not inside `characterize_region`. That loop *is* the cost this
warning exists to precede (`char_gate_search` is the dominant share
of repair time; this sweep is not). Cost class: Full-tier
`lag_decision` (bounded ±600 ms over ~1 s in memory), paid once per
planned gap, always on for the rows in the table — no new flag.
Downmix full `b_samples_full` to mono once for the pre-pass
(`domain::pcm::interleaved_to_mono`); do not redo it per gap.

`--quiet` and JSON still pay the pre-pass (it is cheap next to
characterize) and still record A4 for the final document. They do
not get a chance to abort mid-run. Say so in `cli-output.md`; do
not pretend `phase()` reached them.

`--gap-listen` is `#[cfg(feature = "calibration")]`. It fingerprints
first (`characterize_gaps_from_decode`, Full-tier `lag_decision`
already paid), then `execute_with_decoded` → `run_with_decoded`.
A4 still precedes production `char_gate_search` on that path; it
does not precede the dump. Do **not** reuse dump `lag_decision`
(selection and plan can differ; dump may use `--border-capture wide`).
Accept paying lag twice.

Do not re-run the pre-pass on anchored-retry's second characterize.
A4 is about the map the first search is about to use
(`AnchoredRetryPass::First`).

Scan-time retained windows (warn on dry-run with no second full-file
decode) are a later Change.

Stamp `PatchSummary` via a `with_injected_time` builder (same idea as
`with_shoulders_abut`) at **both** summary construction sites in
`run_with_decoded` (preview return and write return). `empty_plan_result`
leaves the key absent.

---

## 5. Design rules (fences)

1. User-facing identity is **injected time**, not “timeline
   mismatch” (A2's phrase) and not “alignment unstable.” Copy
   names the class, the gap-number span(s), that fills can fail,
   and that the run continues.
2. Warn and continue. Nothing in this plan adds a CLI flag, stops
   the patch loop, skips a gap, or changes a repair decision.
3. One run-level warning and one JSON object. Per-gap
   `pre` / `post` / `delta` / `peak_r` are internals the sequence
   pass consumes; they do not land on `GapTags` or v1 JSON gap rows.
4. The sequence pass **is** this plan. It does not ship without
   fixture- and corpus-pinned (T, N). Isolated spikes must not warn.
5. **Caller construction (the honesty-critical pin).** Production
   calls a domain function `shoulder_lags_at_b_mapped` that **is**
   the mono half of today's `lag_at_placement`. Fingerprint
   `lag_at_placement` becomes a wrapper that still does the
   selected-channel pass. `PatchAudio` does not import
   `gap_fingerprint::measure` and does not invent fill-border
   templates. Knobs and geometry, verified against source
   2026-09-08:

   - Refine A with `policies::refine_gap_frames` (same call
     `characterize_region` already makes: report edges, recipe
     `silence_peak_fraction`, `request.absolute_silence_rms`,
     0.75 s refine budget — today a local `GAP_EDGE_REFINE_SECS` in
     both `patch_audio` and `FingerprintConfig::from_request`, not
     a shared domain const).
   - Probe borders, **not** production fill borders. Standoff is
     **0 frames** (`PROBE_BORDER_STANDOFF_FRAMES` in fingerprint
     `pcm.rs`). Production's `border_standoff_secs` (0.35) would
     shift every `frac_lag_ms` and invalidate T/N. Move that
     constant (or inline the 0) with the domain function so
     fingerprint `pcm.rs` is not the production source of truth.
   - Fine sweep: `lag_window_secs = 1.0`, `lag_max_lag_ms = 600`.
     `FingerprintConfig::from_request` hardcodes both; they are
     **not** on `PatchAudioRequest`. Do not add config keys.
   - Envelope: `request.lag_envelope_max_lag_ms` and
     `request.lag_envelope_window_secs` (already shared with the
     dump). Default 2000 ms / 2 s is what § 6 used. `0` on the
     max-lag knob disables rescue, same as the dump.
   - `BorderCapture::Narrow` always. Production repair never reads
     this knob; it is dump-only. Do not pick up
     `--border-capture wide` from a `--gap-listen` dump.
   - B map: `resolve_gap_offset_secs(..., AnchoredRetryPass::First)`
     at the region's midpoint, same as `characterize_region`.
     `FillRegion.b_start_secs` is the **scan** mapping; under
     `interpolated` / `anchored-retry` it can disagree with the
     offset the search is about to use. Convert refined A start
     with the same formula as fingerprint
     `b_mapped_frame_in_haystack` (move or copy the one-liner;
     it is `pub(in measure)` today) on full decoded B
     (`b_extract_start_secs = 0`).
   - Mono only. Selected-channel `LagSummary` stays in the
     fingerprint wrapper.

   Move with that function, following `seam_local` / `lag_envelope`:
   `lag_pair`, `lag_side_sweep`, `LagSweepParams`, `LagSideSweep`,
   `LagPairInput`, `summarize_lag_curve`, `rescue_shoulder`,
   `LagSummary`, `LagChannel`, `LagVerdict`, the landmark
   constants / `is_landmark`, and `BorderCapture`. `schema.rs`
   re-exports the dump types. **No mapping layer** (`ShoulderLag`
   → dump copy): that is how “extraction does not reproduce
   Full-tier numbers” happens.

   Do **not** move `FingerprintConfig`, `LagFingerprint`,
   `DumpOptions`, or the selected-channel loop.

   The extracted call is **unconditional** at `b_mapped` for every
   planned gap — not gated behind `DetailTier::Full`, `--dual-fit`,
   a confidence floor, or `characterize_region`. No new correlation
   math.
6. If the sequence pass cannot be made honest at (T, N), the plan
   is refuted. It does not fall back to shipping per-gap
   diagnostics.

---

## 6. Honesty proof (corpus read 2026-09-08)

No re-scan. Source: `gap-files/2026-09-07-measure-constellation/`,
51 pair dirs, each `corpus.json` Full-tier. Per gap: mono
`lag_decision.pre_anchor` / `post_anchor` `frac_lag_ms` (same fields
as `gap-fingerprint-stats` `frac_lag_*`; do **not** use
`seam_step_ms`, which prefers `splice_step_ms`). Delta =
`|post − pre|` when both anchors exist; a missing shoulder breaks a
run. Consecutive = adjacent dump `index` in that pair.

**Distribution** (971 two-anchor gaps of 1073): min ~0, p50 = 0.02
ms, p90 = 9.5 ms, p93.2 = 29.6 ms, p98.7 = 42.4 ms, p99 = 159.5 ms,
max = 1688 ms. Matches the constellation note's headline (median
0.02 ms, 90.1% within 10 ms).

**Positives (must warn):**

- Pair 31 g7–g8: deltas 1122.2 / 1688.4 ms. Longest run at T = 150
  is **N = 2**. Neighbors are small (g6 = 36 ms; g9–g11 sit together
  at ~−1150 ms, delta 0–4 ms). Default copy: **gap #8–#9**.
- Pair 50 is **not** one consecutive g8–g18 run. g8 = 19.9 ms (low
  `peak_r`, small delta); **g14 = 38.5 ms** splits the loud stretch:

  | dump gaps | deltas (ms) | default copy |
  |-----------|-------------|--------------|
  | 9–13 | 512, 943, 1040, 433, 932 | gap #10–#14 |
  | 14 | 38.5 | (breaks the run) |
  | 15–18 | 985, 274, 799, 160 | gap #16–#19 |

  Longest run at T = 150 is **5** (g9–g13); g15–g18 are a second
  run of 4. The file-level warning fires either way. Copy names
  **both** spans; it does not say gap #9–#19.

**Negatives:**

- Pairs 22 and 24 were the planned decoys (constellation remaining-
  skip population, not a known map step). They are quiet on this
  axis (pair 22 max ~8 ms; pair 24 max ~28 ms, one-gap). They do
  not stress N.
- Real isolated spikes above 60 ms, corpus-wide: **19/g2 (63.9 ms)**
  and **25/g2 (62.6 ms)**. Each is a single gap. `N = 2` leaves them
  silent. Nothing else in the 51 pairs has even one two-gap run
  above T = 60 ms except 31 and 50.

**T × N grid (pairs that warn):** at every T from 60 through 400
ms, `N = 2` lights **only** 31 and 50; `N = 3` drops 31; `N ≥ 6`
drops 50 (g14 breaks the run). T = 150 sits above the 64 ms isolates
and well above p98.7, and still catches 31's 1122/1688 and 50's
433–1040 ms interior.

A from-decode run is needed only to validate moved
`domain::shoulder_lags_at_b_mapped` matches today's
`lag_at_placement` mono numbers — not to re-derive T/N.

---

## 7. Decision

**Ship** A4 at **T = 150 ms, N = 2**. The § 6 read already separates
the two known hard pairs from the other 49. Pin those numbers in
the code.

`N = 2` is required: pair 31's positive is exactly two gaps. `N ≥ 3`
misses it. Isolated 60–64 ms spikes (19/g2, 25/g2) stay under N = 2.
T = 150 is the conservative pick above that isolate band; T = 60
would also have been corpus-clean at N = 2.

The file-level warning does **not** try to recover pair 50's g8–g18
as one region. A one-gap dip below T (g14) is allowed; bridging
those dips would be a different statistic and is out of scope.

Remaining gates, not a re-open of T/N:

- § 8's cases must pass at this (T, N) before the warning ships.
  If case 3 warns at 150/2, that is a fixture bug or a refute of
  the synthetic model — it is not a prompt to raise T on the
  corpus, which is already clean.
- If the domain-move check does not reproduce today's Full-tier
  mono `lag_decision` numbers: fix the extraction before anything
  else; that is a bug.

---

## 8. Fixture (synthetic, before any production wiring)

Prove the extracted primitives still pass `lag.rs`'s existing unit
tests unchanged (move, § 5 rule 5). `lag_at_placement` wrapper tests
(`lag_at_placement_envelope_recenter_is_per_shoulder`,
`lag_at_placement_envelope_shared_far_unlocks_both`) stay in
`lag.rs` and still pass.

Split the new tests. Do not make sine PCM carry the N = 2 / dip /
isolate matrix — that is `find_injected_time_runs` on delta arrays
(§ 3) and belongs on the PR lib gate.

**In-memory PCM through `shoulder_lags_at_b_mapped` (lib / PR):**

- **Case 1 — real injected-time region.** ≥ 5 planned gaps; a
  bounded middle span of B has a constant offset **outside both**
  the ±600 ms fine window **and** the ±2000 ms envelope (≈ 3 s of
  extra B, or a matching drop on A). The rest of the run is at
  nominal registration. Expect: delta elevated throughout that
  span; `find_injected_time_runs` returns one run. An in-window
  step that only spikes the two edge gaps is the wrong model.
- **Case 2 — isolated seam defect, no region.** One gap's B window
  shifted, neighbors identity. Expect: no run.

**Sequence-pass vectors (lib / PR), not PCM:**

- **Case 3 — noise floor, no region.** Identity PCM through the
  pre-pass is ~0 delta (sines correlate too cleanly to be “corpus
  noise”). The noise check is the § 6 floor on
  `find_injected_time_runs`: p98.7 ≈ 42 ms everywhere, plus the
  63.9 / 62.6 ms isolates as single-gap spikes. Expect: no warning
  at T = 150, N = 2.

**Copy and JSON (lib / PR):** `format_injected_time_warning` on a
synthetic `InjectedTimeReport` (pair 31 span; pair 50 two-span
“and”; no glue across a dip). `PatchSummary` serde omits the key
when `None`.

**Hook (lib / PR, `patch_audio` crate test):** in-memory
`DecodedAb` into `run_with_decoded` (`pub(crate)` if needed). Case 1
PCM: `phase()` fires **before** any `patch-characterize` progress,
summary carries `injected_time`, preview/write both stamp it.
`FakeProgressReporter` no-ops `phase()` — use a recording reporter.
Do not put this on `patch_audio_integration` (not in `-Tier pr`).

---

## 9. Done when

§ 6 has been read; § 7 has picked **T = 150 ms, N = 2**. The Change
is not done until § 8 is green at that pin, A4 fires from the
pre-pass **before** characterize on a case-1 fixture (stderr
`phase()` in Auto, stdout `Warning:` on the report, JSON
`patch.injected_time` present), case 2 and case 3 stay quiet,
production patch bytes and repair decisions are unchanged, and
`cli-output.md` / `json-output.md` record A4.

**Not** done when: the warning is computed only after characterize;
production used fill-border standoff 0.35 instead of probe
standoff 0; `lag_pair` was called without the `lag_at_placement`
mono construction; `GapTags` grew lag fields; a repair decision or
skip reads the flag; copy says “timeline mismatch” or names a
corpus pair; default copy glues across a dip (pair 50 g8–g18 is
not one consecutive-T run); `--quiet` / JSON is documented as able
to abort mid-run; mid-run stderr also `tracing::warn`s the same
line; A4 was appended onto `collect_repair_warnings` (that loop
warns on tracing).

---

## 10. Placement in code (checklist — execute in order)

Design is § 0–§ 5. This list is the Change. § 6/§ 7 have picked
the ship branch (T = 150 ms, N = 2). `file:line` belongs only here,
and only on an item about to be executed — names below are
functions/modules, not lines.

- [x] § 6 corpus read (`gap-files/2026-09-07-measure-constellation`,
      2026-09-08): pair 31 g7–g8 and pair 50 g9–g13 (plus g15–g18)
      warn at 150/2; 22/24 quiet; isolates 19/g2 and 25/g2 stay
      under N = 2; no other pair has a two-gap run above T = 60 ms.
- [x] Construction pins verified against source 2026-09-08
      (`lag_at_placement`, `probe_border_spec` /
      `PROBE_BORDER_STANDOFF_FRAMES = 0`,
      `FingerprintConfig::from_request` 1 s / 600 ms,
      `resolve_gap_offset_secs` First pass in `characterize_region`,
      `collect_repair_warnings` + `tracing::warn` loop,
      `--gap-listen` calibration + dump-then-`run_with_decoded`,
      `PatchSummary::from_outcomes` preview and write sites).
- [ ] Domain move: `shoulder_lags_at_b_mapped` + the § 5 rule 5
      types/functions from
      `application/gap_fingerprint/measure/lag.rs` (and
      `BorderCapture` / `LagSummary` out of `schema.rs`, re-exported
      there). Take the 0-frame probe standoff and the
      `b_mapped_frame_in_haystack` formula with it.
      `lag_at_placement` becomes the selected-channel wrapper.
      Unit: existing `lag.rs` tests still pass at the new
      location; the two `lag_at_placement_envelope_*` tests still
      pass.
- [ ] `find_injected_time_runs` at pinned (T, N) with the § 3
      delta-array cases (31 N=2, 50 dip, isolates, skipped index).
- [ ] § 8 in-memory PCM cases 1–2 against
      `shoulder_lags_at_b_mapped`; case 3 as sequence-pass vectors
      plus identity PCM.
- [ ] Pre-pass in `PatchAudio::run_with_decoded` after decode,
      before the characterize loop: `shoulder_lags_at_b_mapped` at
      `b_mapped` for every planned region; sequence pass; `phase()`
      if flagged (not `tracing::warn`). Stamp preview and write
      summaries. Hook crate test: phase before `patch-characterize`.
- [ ] A4 human line (`format_injected_time_warning`, **not**
      `collect_repair_warnings`) + `patch.injected_time`; pin copy
      and the Quiet / JSON abort limit in `cli-output.md`; pin the
      key in `json-output.md`.
