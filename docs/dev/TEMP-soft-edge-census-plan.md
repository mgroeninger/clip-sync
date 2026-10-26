# TEMP — Soft dropout edges: census, then snap or close

**Status:** draft, 2026-10-26; revised the same day after review (patch-time refinement, onset rule,
dual-fit evidence); revised again after a readiness review (pilot inputs, write-start bias,
earliest-fall false positives, cross-check caveat). **Phase 1 done (2026-10-26): Go.** The
`edge-onset` bin is built, and the pilot refined its onset rule three times (§ 5). Phase 2 (media
mode, full census) is the next action.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement*, *Gap start lands after A's
dropout begins* row.

**Companions:**
- Found in [archive/TEMP-dualfit-interior-edit-plan.md](archive/TEMP-dualfit-interior-edit-plan.md)
  § 4, *The 50/g7 blip is in A, not in any edit*.
- Scanner contract: the `SilenceRunScanner::feed` doc and `is_silent_interleaved` in
  `domain/policies/silence.rs`.
- Patch-time edge walk: `refine_gap_frames` and `GAP_EDGE_REFINE_SECS` in
  `domain/policies/gap_borders.rs`.
- Listen-clip framing: step 5 of `application/gap_listen/mod.rs`.
- The independent-instrument pattern this plan's bin follows: [shoulder-sweep.md](shoulder-sweep.md).

**Media hygiene:** pair index + gap index only. Listen-run directories are named by date and label;
clip stems are never quoted here. `<pair>/g<n>` means dump `index` n, the
[shoulder-sweep.md](shoulder-sweep.md) § 3 convention.

---

## 0. Decision (normative)

This section is the plan. Later sections only implement it. If a later sentence disagrees with this
one, this one wins.

**Measure first, then decide. The census measures every gap edge's soft band, the stretch between
A's dropout onset and the edge the patch actually writes from, with an instrument that shares no
code with the scanner or the patch-time edge walk. If the exposed band is audible on the worst
cases, an edge snap moves the patch's start to the onset, opt-in first. If it is not audible, close
with a settled row. The silence floor (`absolute_silence_rms`) is not touched under either
outcome.**

Why not the floor: the scan-time equivalence gate shipped on a recipe validated at this floor (8
pairs, 121 gaps), and every gap boundary in the corpus depends on it. Lowering it moves all edges to
fix the few that have a soft band, and it reopens a validation that is closed. A snap moves only the
edges that show a dropout onset.

## 1. Evidence

One ear-confirmed case, **50/g7**. It was heard in every arm across three blind sittings, at about
2.8 s in the `--gap-listen` clip, and the patch never touches that audio.

| Event | Clip time | Level |
|-------|-----------|-------|
| Dropout onset: a −20 dB fall in 2 ms, on every channel | 2.974 s | to −51 … −65 dBFS |
| Write starts (fill start less the 10 ms crossfade) | 2.990 s | — |
| Scan boundary (= clip context, 3 s) | 3.000 s | — |
| Digital silence reached | 3.004 s | −104 dB |

So ~16 ms of partial dropout plays at full level before the pre seam's crossfade begins. The onset is
the largest spectral-novelty event in the clip (100th percentile).

**Nothing that moves A's start caught it.** `refine_gap_frames` walks the start back only through
frames below the floor, so it stops at this band (§ 2). The sequential
`gap_start_extend_on_pre_seam_fail` retry runs only under `fill_mode = gate`. Under `fit`, the
default, A's start moves only through the joint boundary grid, which scores seams, not onsets.

**Which path the evidence covers.** 50/g7 was measured on **dual-fit** renders (the `dual-xf` arms of
`2026-10-02-interior-edit`). The listen tree does not record which route sent it to dual-fit:
`listen.json` carries disposition and confidence only. Whether single-lag `fit` patches have the same
exposure has not been checked, but the Phase 1 pilot can answer it at no media cost (§ 4). The mirror case, a soft trailing edge at gap end, has never been
measured.

## 2. What the code already says

**"Silent" means every sample on every channel is below −60 dBFS.** The scanner's frame-level edge
walk tests one frame at a time. For a single sample, peak equals RMS, so the `rms < peak ×
silence_peak_fraction` arm can never pass, and only the absolute floor (0.001, −60 dBFS) can. At
block level the same arm needs a 40 dB crest factor, which real programme does not have. A band at
−51 … −65 dBFS is therefore programme **by definition**, and the edge walk stops where the scanner's
contract says it should. The backlog row's "likely mechanism, not verified" reads as confirmed by
code. What is unknown is how often this happens, how long the band lasts, how loud it is, and whether
it is audible.

**A second, separate mechanism exists.** A run that opens on the first block of a decode bucket
cannot walk backward, because the prior samples are gone, so its start falls back to the block edge
(up to one 100 ms block late). A bucket-edge band is below −60 dBFS throughout. A floor-mechanism
band has samples **above** it.

**The patch re-walks the start, with the same predicate.** Before any seam search, the patch path
calls `refine_gap_frames` on the full A PCM. It retracts the start through frames that pass
`is_silent_frame`, within a 0.75 s budget (`GAP_EDGE_REFINE_SECS`). The fingerprint measure and
injected-time paths call the same function with the same budget. Three consequences:

- **A bucket-edge band is a scan-report error, not an audible one.** Refinement walks back through
  it, so the patch writes from the true floor crossing. The scan report still reads late.
- **A floor-mechanism band survives both walks.** The scan and the patch stop at the same frame, the
  last one before the boundary with any channel at or above −60 dBFS. This document calls that
  frame the **floor edge**.
- When both occur on one edge, refinement removes the below-floor part and stops at the floor edge.
  So the exposure that can be heard runs from the onset to the floor edge, less the write's lead.
  The write's lead is the crossfade, plus any start move the fit grid makes.

The listen clip is framed on the fill plan's A start, which is the scan boundary
(`video_a_start_secs`), before refinement.

**Hypothesis H1, codec smear (unverified).** 26–30 ms is about one AAC frame (1024 samples = 21.3 ms
at 48 kHz) plus the MDCT overlap. The "fall of 20 dB, then decay to silence" may be the encoder
smearing a hard dropout. H1 predicts:
- band durations cluster near the frame length, and the cluster depends on codec;
- a mirror **pre-echo** at gap end: a low-level rise before the programme returns.

H1 does not change the fix. A snap to the onset is right either way. It changes how the census is
read: a codec-shaped cluster says the band is structural and will recur on every lossy pair.

## 3. The instrument: `edge-onset` bin

A new bin next to `shoulder-sweep`, written against this section, with the same independence rule:
it must not call the scanner, `is_silent_interleaved`, `is_silent_frame`, or `refine_gap_frames`.
Its −60 dBFS comparisons are its own code. Sharing WAV I/O with the sibling bins is fine.

**Wiring.** `[[bin]] edge-onset` at `src/bin/edge_onset.rs` with `required-features =
["calibration"]`. Tests live in the bin and use synthetic PCM only, never licensed media. They run on
PR through the `pr-repair` calibration invocation in `scripts/test-tier.ps1`, next to `--bin
shoulder-sweep`. Outputs go under gitignored `gap-files/`. The Phase 2 media mode decodes real A, and
`he-aac` registers the only AAC decoder (even for AAC-LC), so media runs need `--features
calibration,he-aac`. The WAV-only pilot and the PR tests need `calibration` alone.

**Boundary placement.** The bin recovers the clip context from clip length and dump geometry, and
falls back to `--context-secs` (default 3.0) only when no dump JSON exists. This is the
[shoulder-sweep.md](shoulder-sweep.md) § 2.1 rule. The scan boundary is therefore *usually* at
3.000 s, but `frame_range` clamps at media edges, so the bin does not assume it.

**Per edge**, in 2 ms bins, recording per-channel peak dBFS and the maximum across channels (`L`):
- **Leading edge.** In the 200 ms before the scan boundary, the onset is the **earliest** bin `i`
  where, with `R = max(L[i−2], L[i−1])`,
  - `L[i] ≤ R − 15 dB`, and
  - every bin from `i` up to the boundary also stays at or below `R − 15 dB`.

  The reference spans two bins because a fall can straddle a bin edge. 50/g7 does (−22 → −30 →
  −45 dBFS): no adjacent pair reaches 15 dB, and the band's own peaks sit 0.2 dB above the
  straddling bin's −15 level, so a one-bin reference reports no onset at all. The onset is then
  refined to the sample: the frame after the last one in the reference bins above `R − 15 dB`.
  Onsets within the 4 ms reference span of each other are one fall (two bins of one decay can both
  clear 15 dB by a fraction of a dB) and merge to the outermost. A fall nearer the boundary than the
  floor edge is inside the silent run (a sub-floor tail stepping to digital zero, 83/g7): it is
  counted (`n_inside_floor`) and never used as an onset.

  The sustain condition rejects programme transients. A word ending, or a bass waveform crossing
  zero inside a 2 ms bin, falls and then comes back up. Taking the earliest *sustained* fall rather
  than the nearest one keeps a second step inside the decay from being picked instead of the real
  onset. The 200 ms window is wider than the backlog row's 100 ms because the one known band is
  26 ms and longer ones must not be clipped.

  **Known false-positive shape: programme decay before quiet programme.** A loud word that ends
  150 ms before the boundary, then room tone at about −50 dBFS, then the dropout: the word's end is
  a sustained ≥ 15 dB fall, and it is the *earliest* one, so the rule reports a 150 ms band. This is
  systematic, not an occasional transient. So the bin records, per edge, `onset_far_ms` (the rule
  above) and `onset_near_ms` (the sustained fall closest to the boundary). In the room-tone case the
  real dropout is the final fall from room tone to silence, which `onset_near_ms` catches, while
  `onset_far_ms` catches the word's end. On a clean dropout they agree. Where they disagree, the
  edge is flagged, not resolved. A "band reaches the floor" test does not separate the two: every
  gap is sub-floor at its boundary, so it is true everywhere.
- **Trailing edge (mirror).** In the 200 ms after the scan end, the onset is the **latest** bin `j`
  where, with `R = max(L[j], L[j+1])`, every bin from the boundary up to `j − 1` stays at or below
  `R − 15 dB`; the merge and inside-floor rules mirror too. The false-positive shape mirrors as
  well: quiet programme after the boundary, then a loud word inside the window. The same
  `onset_far_ms` / `onset_near_ms` pair applies (here the latest and the nearest rise).
- **Floor edge.** This is the latest sample before the scan start (or the earliest after the scan
  end) where any channel has `|x| ≥ 0.001`. It is searched within the same 200 ms window.
- No onset found ⇒ `onset = none`. That is a result (a hard edge), not an error.

**Columns:**
- pair, gap index, edge (`lead` / `trail`);
- onset-to-boundary ms, floor-edge-to-boundary ms, and onset-to-floor-edge ms (the band refinement
  cannot remove);
- `onset_far_ms` and `onset_near_ms` (ms to boundary), plus a flag when they disagree; `n_falls`
  and `n_inside_floor`;
- band peak dBFS, and whether the band reaches −60 dBFS on any channel (bool);
- band duration ms, codec;
- `dump_refined_ms` (production's refined edge from the dump's `a_refined_*`, joined beside the
  measurement, never an input) and `floor_vs_refined_ms`, which reads ~0 when this bin's floor edge
  is production's refined edge;
- `disposition` from `listen.json` or the patch report;
- fill path (`dual_fit` / `anchor_seam` / `single_lag` / `not_patched`, empty when no log is
  found), from the run log's status row. `anchor_seam` is its own path: the rescue writes a wide A
  region by design.
- Phase 1 only: `write_first_diff`, `b_level_dbfs`, `write_bias_ms`, the write-start interval, and
  onset-to-write ms as an interval (see below), plus the arm (`single` / `dual`) on band50 clips.

**Write start is measured, not assumed.** On a `--gap-listen` clip, `_a_patched.wav` equals
`_a_surround.wav` until the write starts, so the exposed band (onset to write start) needs no
knowledge of the refinement or the seam path.

The first differing sample is **not** the write start, though. `apply_seam_crossfade` opens the pre
blend at `gap_start − cf` with A's weight `cos 0 = 1` and B's weight `sin 0 = 0`, so the first frames
differ from A only by about `sin(t)·b`. Over a −51 … −65 dBFS band, after WAV quantization, the first
non-zero difference can land a millisecond or more into the ramp.

The ramp start cannot be recovered exactly from the clip. Fitting the difference against B would
need the fill's own B samples, and those are not at the same frames as `_b_surround.wav`: the fit
search slides the fill off the nominal map, dual-fit uses its own lag, and `--seam-extend-fill`
takes the pre blend from a separate head margin. The dump JSON records shoulder lags, not the chosen
placement. Recovering it would be a new correlation stage, which this bin does not add.

What is always true is a bound. The difference is zero before the ramp, and by the ramp's end the
patch carries full fill. So the bin records:
- `write_first_diff`: the first sample where `|patched − surround|` is at least one LSB of the
  clip's bit depth (any integer change);
- the write-start interval `[write_first_diff − cf, write_first_diff]`, with `cf` the configured
  crossfade (10 ms default). This uses a config value, not the seam mechanism.

**The interval is usually much tighter than one crossfade.** Across the ramp the difference is
`(cos θ − 1)·a + sin θ·b`, with `θ` running 0 → π/2. A's quiet band hardly contributes, so how fast it
clears one LSB depends on **B's level**. B is the donor and normally carries programme there. So the
bin also records `b_level_dbfs`, B's RMS (max across channels) over the crossfade before
`write_first_diff`, taken from `_b_surround.wav`. A level envelope is insensitive to a few
milliseconds of misregistration, so this needs no fill placement. From it, the bin estimates the
bias as `write_bias_ms = cf · (2/π) · asin(min(1, LSB / b_rms))`.

This is an **estimate, not a bound**. B oscillates, so at the crossing `|b(t)|` can sit below its
RMS, and the true bias can exceed the estimate by up to about a period of B's dominant content. RMS
rather than peak keeps the estimate from being systematically optimistic. The hard bound stays one
crossfade. So the bin reports both:
- the estimated interval `[write_first_diff − write_bias_ms, write_first_diff]`;
- the hard interval `[write_first_diff − cf, write_first_diff]`.

When B is silent at that point, the estimate is the full `cf` and the two agree. A small
`write_bias_ms` means a trustworthy reading; a large one marks a quiet-B edge whose reading is soft.

A synthetic bin test checks the estimate at two B levels (a sinusoid) over a known 10 ms crossfade
on a quiet A band. At a loud B, `write_first_diff` lands within `write_bias_ms` plus one B period of
the ramp start, and that is a few samples. At a quiet B it lands later, under the same tolerance.
In both cases it stays inside the hard interval.

The bin also records `write_vs_predicted_ms`: `write_first_diff` against the floor edge less the
10 ms crossfade. Because `write_first_diff` is late by up to `write_bias_ms`, this reads in
`[0, write_bias_ms]` when the prediction is right. That is the Phase 2 estimate, checked on clips where both
are known.

## 4. Phases

### Phase 1 — Pilot on existing listen clips (no media cost)

Inputs, deduplicated by gap within and across runs (arms share raw A, and at least 5/g8 and 28/g18
appear in both runs):
- `2026-10-02-interior-edit`: 9 gaps, **including 50/g7**, the blip gaps from that plan. All are
  dual-fit renders (`dual-xf`, the only arm on disk).
- `2026-10-23-listen-band50`: **25 gaps over 22 pairs, each rendered twice**, in a `single/` arm and a
  `dual/` arm (50 patched clips, not 50 gaps). The fill path is known per arm. Per the run's
  `run.ps1`, `single` used default routing, which at the then-default 75 ms bound routes nothing in
  this band, and `dual` forced dual-fit at a 30 ms bound. The per-gap logs under `*-out/` print
  `dual-fit` where it was used, so the join is per gap, not only per arm. Both arms ran with
  `--seam-extend-fill`; that changes the post seam only, so the pre-seam write geometry matches the
  interior-edit renders.

**Single-lag vs dual-fit, on the same raw A.** Because band50 renders every gap both ways, the pilot
answers § 1's open question directly: per gap, compare `write_first_diff` and the onset-to-write
interval between arms. If both arms write from the same frame, that is the answer, not a null. Report it as a pilot output alongside the go / no-go.

**Go / no-go:**
- **Go** if all of the following hold:
  - 50/g7 reproduces: onset at 2.974 s ± 2 ms, the estimated write-start interval contains
    2.990 s ± 1 ms, and the
    floor edge within 1 ms of the 3.000 s scan boundary. Refinement did not move this start,
    since the write sits one crossfade before the boundary.
  - Every patched pilot gap with `write_vs_predicted_ms` outside `[−1, write_bias_ms + 1]` ms is
    explained by a named cause, such as a fit-grid start move or a dual-fit placement.
  - Phase 2's ~5 ms early exit is licensed only if most pilot edges have `write_bias_ms` under
    ~1 ms. Report the `write_bias_ms` distribution; if quiet-B edges dominate, say so here before
    Phase 2 relies on the estimate.
- **No-go** (fix the detector, do not run Phase 2) if 50/g7 misses, or if more than a handful of edges
  report onsets that, on inspection, are programme transients rather than dropouts. If that happens,
  record the false-positive shape here before changing the rule. Count the § 3 room-tone shape
  (`onset_far_ms ≠ onset_near_ms`) separately: it is expected, and Phase 2 reads its
  distribution split on that flag rather than treating it as noise.

**Caveat:** both runs were selected for dual-fit or blip questions, so the pilot sample is biased. It
validates the instrument and gives a first look. It is not the distribution.

### Phase 2 — Full census over A media (no oracle)

`--gap-listen` requires `--gap-fingerprints`, and that pays the per-bracket oracle cost on every gap.
That would make a corpus-wide listen pass take days. The census needs only raw A around each
boundary.

- Extend `edge-onset` with a media mode: decode A, take the gap list from the pair's scan report
  (`--format json`), and keep ±250 ms around each edge. No B, no oracle, and no full-PCM hold.
- Dispositions and fill path come from a `--patch-only --format json` pass. Use `dual_fit_used`,
  `dual_fit_route`, and `gap_start_adjust_frames`. The last is measured from the *refined* edge, not
  the scan edge, so it only corrects the write lead. It is cheap in time, but it still holds the
  whole pair in memory.
- Measure **every** gap, because the boundary is upstream of patching. Audibility only matters for
  `patched` gaps, so report that subset separately.
- **Strictly one pair at a time.**
- **Estimated exposure** = onset-to-floor-edge − 10 ms crossfade + any start move from
  `gap_start_adjust_frames`. Phase 1's `write_vs_predicted_ms` is what licenses this estimate.
  Onset-to-scan-boundary is also recorded, but it overstates exposure on every bucket-edge band.
- **Cross-check on one pair.** The bin's floor edge should match the `A gap (refined)` line in the
  verbose patch log. That line prints only when refinement moved an edge by **more than 1 ms**
  (`format_gap_fill_plan_lines` in `application/patch_audio/log.rs`). On a floor-mechanism edge,
  the case of interest, refinement does not move the edge and nothing prints, so "no line" means
  "refined edge = scan edge" and is the expected match. Note that the band50 logs are not verbose,
  so the cross-check needs a fresh `--verbose` patch-only run.

**Result:** per-edge distribution, split by `lead` / `trail`, by whether the band reaches the floor,
by codec, by fill path, and by `patched` vs not. Record it in § 5.

**Exit early** if no patched edge besides 50/g7 has more than ~5 ms of estimated exposure. Then 50/g7
is n = 1, and Phase 3 is a single-gap ear check.

### Phase 3 — Ear check (pre-registered before listening)

Write this section's protocol into § 5 **before** the first sitting:
- which gaps: the worst N exposed edges by estimated exposure (Phase 2), ranked on the interval's
  lower end so that a ramp-bias reading cannot promote an edge, plus 2 hard-edge controls;
- the arms (current vs snapped);
- the blind key;
- the decision rule.

The snapped arm needs the Phase 4 prototype, so Phases 3 and 4 interleave: build the opt-in snap,
render both arms, then listen.

### Phase 4 — Edge snap (opt-in, then default)

**Default home: (b), inside `refine_gap_frames`.** That function is already the patch's edge snap.
It stops at the floor edge because it uses the scanner's predicate. The snap continues the backward
walk past the floor edge, through a sustained-fall band, to the onset (§ 3's rule). A trailing snap
does the same in the other direction. Points that come with this home:
- Every caller sees it: patch characterize, injected-time detection, and fingerprint measure. The
  fingerprint contract test pins all three to one refine budget. Keep the snap behind one setting
  that all three read, or the fingerprint geometry and the patch drift apart.
- The scan report keeps reading the boundary late. That is the accepted cost.
- It is **not** the gate-mode `gap_start_extend_on_pre_seam_fail` path. That path retries on seam
  failure, only under `fill_mode = gate`.
- `refine_gap_frames` has a second start loop that § 2 does not need: it *advances* the start past
  leading non-silent frames when the scanner opened the run too early. The snap must be ordered
  against that loop explicitly (snap after the retract, never undone by the advance), with a test.
- The snap needs its own cap. Given § 3's room-tone shape, an uncapped walk would overwrite real
  programme with B. Bound it, as `max_anchor_bracket_secs` bounds the anchor seam, and size the cap
  from the Phase 2 distribution of bands where `onset_far_ms` and `onset_near_ms` agree.

**Alternative: (a), scan-level snap.** Move the gap's reported boundary to the onset, so every
consumer sees the corrected edge. This changes the gap report's `start_secs` / `end_secs`, and the
equivalence gate and structure signatures see a longer gap. **That is the § 0 objection to the floor
in a smaller form:** it reopens the equivalence validation. Choose (a) only if Phase 2 shows a
consumer other than the patch that is harmed by the late scan boundary. If it is chosen, re-validate
the equivalence gate.

**Either way:**
- A soft-edged fixture in `clip-sync-repair-fixtures`, with a band above −60 dBFS before digital
  silence, and a test that the write lands on the onset.
- A hard-edged fixture where the snap is a no-op.
- A bucket-edge fixture, below the floor throughout, where today's refinement already lands on the
  floor crossing and the snap changes nothing.
- `scripts/measure/diff-patch-outcomes.ps1` over the corpus for churn. A moved edge changes fill
  length and seam anchors, so other gaps may move too. Explain every diff.
- Opt-in flag first, a blind listen (Phase 3), then decide the default. This is the
  `--seam-extend-fill` pattern.

## 5. Results

### Phase 1 — pilot (2026-10-26): **Go**

**Inputs as run:** both listen trees, 118 edge rows. That is 9 interior-edit gaps plus 25 band50
gaps in two arms: 32 distinct gaps over 26 pairs, 64 distinct edges, all AAC and all `patched`.
Fill paths by row: 68 `dual_fit`, 26 `single_lag`, 24 `anchor_seam`. The last is 12 gaps in the
band50 `single` arm whose log rows read `patched (anchor …)`. The plan expected that arm to be
plain single-lag, but default routing took the anchor seam on those gaps. Census files are
`<run>/edge-onset/census.{csv,json}` under gap-files (gitignored, regenerable with the commands in
the bin's header).

**The pilot changed the onset rule three times before the go/no-go was read.** Each change is in § 3
and has a test that reproduces the real case:
1. **Bin-straddling fall: 50/g7 missed under the rule as drafted.** Its fall spreads over the bin
   that straddles it (−22 → −30 → −45 dBFS). No adjacent pair reaches 15 dB, and the band's own
   peaks sit 0.2 dB above the straddling bin's −15 dB level, so the one-bin reference found no onset
   at all. Fix: a two-bin reference. With one bin, the straddle test fails.
2. **Split decay: the fix then read one fall as two, 2 ms apart** (50/g7 at 25.9 / 23.8 ms, and
   similar on 29/g20 and 28/g18 trail). Two bins of one decay both cleared 15 dB by a fraction of a
   dB, which falsely flagged the room-tone shape. Fix: onsets within the 4 ms reference span merge
   to the outermost.
3. **Fall inside the silent run: 83/g7's near onset was a sub-floor tail stepping to digital
   zero**, 24 ms nearer the boundary than the floor edge. Fix: falls nearer the boundary than the
   floor edge are counted in `n_inside_floor` (1 edge in the pilot) and are never an onset.

None of these is a programme transient. With them in place, **0 edges flag the room-tone shape**
(`onset_far_ms ≠ onset_near_ms`), and no window was clipped. The 21 onsets were not inspected one
by one. They pass the sustain rule, all lie within 30 ms of the floor edge, and the two traces that
were dumped (50/g7, 83/g7) are dropouts.

**Go criteria:**
- **50/g7 reproduces.** Onset 25.94 ms before the boundary (2.9741 s, criterion 2.974 ± 2 ms). The
  write starts at 9.98 ms before the boundary with a 0.0 ms bias estimate, so the interval is
  2.990 s. The floor edge is the boundary (0.0 ms).
- **No unexplained write residual.** `write_vs_predicted_ms` lies in [−0.02, +0.42] ms on every
  edge except 14 edges in 12 gaps. Those 14 are all `anchor_seam`, written 0.2–2.2 s past the
  prediction over loud A. That is the rescue's A region, by design (bounded by
  `max_anchor_bracket_secs`, not this plan's concern), and it covers the band.
- **The early exit is licensed.** `write_bias_ms` is at most 0.5 ms and above 0.1 ms on only 5 of
  118 edges. B's level over the crossfade has a median of −36.8 dBFS and a minimum of −68.2 dBFS:
  the donor carries programme where the ramp opens, so quiet-B edges do not dominate.

**Cross-checks:**
- **Floor edge = production's refined edge** (dump `a_refined_*`) on 118 of 118 edges, max
  difference 0.4 ms. Two edges sit off the scan boundary: 52/g9 lead at 0.38 ms and 83/g7 lead at
  90.5 ms, where refinement moved the edge and the bin agrees. This is the § 2 claim, measured
  rather than read from code. Phase 2's verbose cross-check is still needed for media mode, which
  has no dump.
- **Raw A is identical across arms** on all 50 repeated edges.

**Exposure, measured.** On every non-anchor write, the write is one crossfade before the scan
boundary, within 0.4 ms. So **exposure ≈ onset-to-floor − 10 ms** to within 0.4 ms in the pilot,
which licenses Phase 2's estimate. The worst case is 52/g9: its 0.38 ms floor makes it − 9.6 ms. 21 of 64 distinct edges have an onset (18 lead, 3 trail). Every band is above the floor
and 2.3–29.1 ms long. Estimated exposure (the interval's lower end) is positive on **10 distinct
edges**, and 5 of those are positive even on the hard bound:

| edge | onset → floor | band peak | fill path(s) | exposure [hard, est, hi] |
|---|---|---|---|---|
| 38/g2 lead | 29.2 ms | −31.4 dBFS | dual-fit only; anchor seam covers it | [9.2, 19.2, 19.2] |
| 50/g7 lead | 25.9 | −39.4 | dual-fit | [6.0, 16.0, 16.0] |
| 32/g13 lead | 22.4 | −39.6 | dual-fit | [2.4, 12.4, 12.4] |
| 10/g8 lead | 20.8 | −39.7 | dual-fit, single-lag | [0.8, 10.8, 10.8] |
| 28/g18 lead | 20.1 | −35.5 | dual-fit, single-lag | [0.1, 10.1, 10.1] |
| 52/g9 lead | 16.5 | −38.2 | dual-fit, anchor seam | [−3.1, 6.9, 6.9] |
| 18/g39 lead | 16.8 | −42.9 | dual-fit, anchor seam | [−3.2, 6.9, 6.9] |
| 43/g8 lead | 15.2 | −49.5 | dual-fit, single-lag | [−4.8, 5.2, 5.2] |
| 28/g18 trail | 13.2 | −39.6 | dual-fit, single-lag | [−6.8, 3.2, 3.2] |
| 62/g3 lead | 11.1 | −47.6 | dual-fit only; anchor seam covers it | [−8.9, 1.1, 1.1] |

The other 11 onsets lie 2.3–5.6 ms from the floor edge and sit inside the crossfade.

**Single-lag vs dual-fit (§ 1's open question): the fill path does not move the pre-seam write.**
On every repeated edge not written by the anchor seam, both arms write from the same frame within
0.2 ms. The residual is the bias estimate varying with each arm's B. So a soft band is exposed
identically on either path. Only the anchor seam's wide write covers one (38/g2 and 62/g3 in the
single arm).

**Read for Phase 2.** Even in this biased sample, 7 patched edges besides 50/g7 have more than 5 ms
of estimated exposure (4 on the hard bound), so the Phase 2 early exit is not expected to fire. 50/g7
is not n = 1. Phase 2 still has to supply the distribution: this sample was selected for
dual-fit and blip questions.

## 6. Non-goals

- Lowering `absolute_silence_rms` or changing `is_silent_interleaved` (§ 0).
- Changing the gate-mode seam-fail extension retries.
- Reclassifying gaps by band shape. The instrument measures and records; it does not classify.
- Fixing anything the census finds **inside** the write. The band of interest is the stretch of raw A
  the write does not cover.

## 7. Close

The three-step move: archive this plan, add a [settled.md](settled.md) row (snap adopted, or band
shown inaudible with the worst cases named), and delete the BACKLOG row. If the instrument outlives
the plan, give `edge-onset` a live doc the way `shoulder-sweep` has one.
