# TEMP — Soft dropout edges: census, then snap or close

**Status:** draft, 2026-10-26; revised the same day after review (patch-time refinement, onset rule,
dual-fit evidence); revised again after a readiness review (pilot inputs, write-start bias,
earliest-fall false positives, cross-check caveat). **Phase 1 done (2026-10-26): Go.** The
`edge-onset` bin is built, and the pilot refined its onset rule three times (§ 5). **Phase 2 done
(2026-10-27): the early exit fails.** 91 of 581 patched gaps over 85 pairs carry more than 5 ms of
estimated exposure, nearly all on the lead edge (§ 5). The Phase 4 opt-in snap prototype is
built, and the Phase 3 ear check is pre-registered in § 5 (2026-10-28). Rendering is next.

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

- Extend `edge-onset` with a media mode (`--report`). It decodes A with the scanner's own
  sequential bucket decode (`scan_interleaved_buckets`, so frame *k* is the scan's frame *k*) and keeps
  ±250 ms around each scan edge. No B, no oracle, and no full-PCM hold. The scan edge is
  `trunc(video_a_start_secs · rate)`, the same arithmetic as the listen-mode boundaries.
- One `--patch-only --format json` report per pair supplies everything. The gap list is
  `scan.gaps`. Disposition and fill path come from `patch.gaps`, which is index-parallel to
  `scan.gaps` with the same bounds. The report also carries `dual_fit_used`, `dual_fit_route`,
  `anchor_seam_used` and the two adjust fields. `gap_start_adjust_frames` is the winning start minus
  the *refined* edge, so positive means inward. `gap_end_adjust_frames` is the winning end minus the
  refined edge, so positive means outward. Both only correct the write lead. The pass is cheap in
  time, but it still holds the whole pair in memory.
- **Anchor-seam edges get no estimate.** The anchor seam writes anywhere from the predicted point to
  2.2 s out (Phase 1). The report gives only a total `anchor_bracket_move_frames`, not the side, so
  covered and exposed bands look the same from the record (pilot: 38/g2 and 62/g3 covered, 52/g9
  and 18/g39 exposed). These edges are reported as their own group with exposure *unknown*. They
  reach Phase 3 only through a listen clip, never through the estimate.
- **Decode check.** Media mode on pair 50 must reproduce the pilot's 50/g7 lead reading (onset 25.94
  ms, floor 0.0 ms). Every other pilot edge in the census must also match its listen-clip onset and
  floor. A mismatch means the bucket decode and the listen export disagree on frame positions, and
  the census is void until that is explained.
- Measure **every** gap, because the boundary is upstream of patching. Audibility only matters for
  `patched` gaps, so report that subset separately.
- **Strictly one pair at a time.**
- **Estimated exposure** = onset-to-floor-edge − 10 ms crossfade − the outward move from the adjust
  field. The outward move is `−gap_start_adjust_frames` on the lead edge and `+gap_end_adjust_frames`
  on the trail edge, so a start moved inward adds exposure. Phase 1's `write_vs_predicted_ms` is
  what licenses this estimate. It is written only for patched, non-anchor-seam edges with an onset.
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
which licenses Phase 2's estimate. The worst case is 52/g9: its 0.38 ms floor makes it − 9.6 ms.
21 of 64 distinct edges have an onset (18 lead, 3 trail). Every band is above the floor and
2.3–29.1 ms long. Estimated exposure (the interval's lower end) is positive on **10 distinct
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

### Phase 2 — census (2026-10-27): done

**Pair 50, decode check (2026-10-26): passed.** One current-build `--patch-only --format json`
report (seam-extend-fill on, 50 ms dual-fit bound), then media mode over it: 20 gaps, 40 edges,
AAC. Every edge the pilot measured on pair 50 reads identically to the hundredth of a millisecond:

| edge | media mode (onset / floor) | listen clip |
|---|---|---|
| 50/g7 lead | 25.94 / 0.00 ms | 25.94 / 0.00 ms |
| 50/g7 trail | no onset | no onset |
| 50/g10 lead, trail | no onset | no onset (both arms) |
| 50/g14 lead | 2.63 / 0.00 ms | 2.63 / 0.00 ms (both arms) |
| 50/g14 trail | no onset | no onset (both arms) |

The bucket decode and the listen export agree on frame positions, so the census stands. Two
windows were clipped at the media ends: the first gap's lead and the last gap's trail, neither with
a floor reading.

**Pair 50's distribution.** 10 of 40 edges have an onset: 7 lead, 3 trail. All are above the floor,
none flags the room-tone shape, and none has a fall inside the silent run. 22 edges are patched
(11 gaps: 8 anchor-seam, 6 dual-fit, 8 single-lag edges); 8 of those have an onset.
- **Estimated exposure > 5 ms: 50/g7 lead (15.9 ms) and 50/g8 lead (11.8 ms).** Both are
  single-lag with no outward move. 50/g7 was dual-fit in the pilot's runs and is single-lag on the
  current build; per Phase 1 that does not move the pre-seam write.
- The other non-anchor patched onsets (50/g14, g16, g18 leads, 2.6–5.8 ms) estimate −7.4 to −4.2
  ms: inside the crossfade.
- Anchor-seam onsets, exposure unknown: 50/g9 lead and trail (5.8 / 6.9 ms) and 50/g15 lead
  (5.4 ms). Their writes moved 400–2000 ms outward.

**New exposed edge: 50/g8 lead.** Onset 21.77 ms before the boundary, floor at the boundary, band
peak −41.9 dBFS, single-lag fill, adjust 0. It estimates 11.8 ms, in the pilot table's range
(between 32/g13 and 10/g8). It was not in either pilot tree, so it is the first edge the census
found without a listen clip. On pair 50 alone, **the early exit already fails**: a patched edge
besides 50/g7 has more than 5 ms of estimated exposure. 50/g8 is a Phase 3 candidate; its
estimate is not yet confirmed against a write measurement.

**Verbose cross-check (pair 50): matches, null case only.** A `--patch-only --verbose` run planned
11 gaps and printed **no** `A gap (refined)` line (the log's `gap #N` is 1-based). The census reads
the floor edge at 0.00 ms on all 22 patched edges, so refined edge = scan edge everywhere, as
predicted. Pair 50 has no edge where refinement moved, so the moved case is covered only by the
pilot's dump check (52/g9, 83/g7). The verbose run's `patch.gaps` statuses are identical to the
first report's (20 of 20), so the report is reproducible across runs.

**Corpus census (2026-10-27): the early exit fails decisively.** One current-build
`--patch-only --format json` report per pair over `gap-files/corpus-pairs.csv`, then one media-mode
run over all of them. **85 pairs, 1775 gaps, 3550 edges, all AAC.** Pair 54's first repair aborted
on a 3.4 GB allocation failure during the splice, under memory pressure. Run alone, it completed.

*Decode check, corpus-wide:* all 118 pilot edge rows reproduce their listen-clip onset and floor
within 0.05 ms (0 mismatches).

*Quality flags:* 9 edges flag the room-tone shape (`onset_far_ms ≠ onset_near_ms`; 4 patched:
5/g5 lead, 30/g2 lead, 43/g6 trail, 66/g13 lead). 2 have a fall inside the silent run (68/g5 and
83/g7 leads). 170 windows are clipped, exactly each pair's first lead and last trail (silence at
the media ends), and none of them is patched.

| split | edges | with onset | est > 0 | est > 5 ms |
|---|---|---|---|---|
| not patched (`not_planned` 2364, `skipped` 24) | 2388 | 193 | — | — |
| **patched** | **1162** | **391** | **122** | **91** |
| patched lead | 581 | 292 | 117 | 89 |
| patched trail | 581 | 99 | 5 | 2 |
| patched `dual_fit` | 452 | 137 | 65 | 47 |
| patched `single_lag` | 376 | 137 | 57 | 44 |
| patched `anchor_seam` | 334 | 117 | unknown | unknown |

Every band with an onset is above the floor. Onset → floor on patched edges: median 7.5 ms, p90
22.4 ms, max 107.5 ms (58/g2 trail, anchor seam). Excluding the anchor seam, the largest is 31.3 ms
(64/g14 lead).

**Read:**
- **91 patched gaps over 56 pairs** carry more than 5 ms of estimated exposure: 16% of the 581
  patched gaps. **51 are positive even on the hard bound** (est > 10 ms), 21 exceed 15 ms, and the
  maximum is 64/g14 lead at 21.3 ms. Band peaks on the est > 5 ms edges: −50.7 to −21.3 dBFS,
  median −38.7.
- **It is a lead-edge defect.** Half of all patched leads have a soft band (292/581), and 89 of the
  91 exposed edges are leads. Trails are mostly hard, as on pair 50.
- **The fill path does not matter.** Dual-fit and single-lag expose at the same rate (47/137 vs
  44/137 onsets), which confirms Phase 1's finding at corpus scale.
- **The estimate is just onset-to-floor − 10 ms here.** No non-anchor patched edge with an onset has
  a non-zero adjust field.
- **Anchor seam: 117 patched edges with an onset, exposure unknown.** This is up from 14 rows in
  the pilot. Their onset → floor spans 2.0–107.5 ms. They reach Phase 3 only through listen clips.
- Pilot drift on the current build: 32/g13 lead is now `anchor_seam` (dual-fit in the pilot), and
  50/g7 is now `single_lag`.

**Phase 2 is done; Phase 3 is next.** The ear check is not n = 1. Pre-register it per § 4: the worst
N exposed edges ranked on the hard bound (est − 10 ms), plus 2 hard-edge controls. The Phase 4
opt-in snap prototype is needed to render the snapped arm. The census is at
`gap-files/2026-10-26-edge-onset/edge-onset/census.{csv,json}` (gitignored, regenerable).

### Phase 4 prototype — opt-in edge snap (2026-10-28)

Built ahead of Phase 3, because the snapped arm needs it. Off by default.
- **Setting:** `edge_snap_max_ms` (`RepairConfig`, TOML, `--edge-snap-max-ms`); `0` = off. It flows
  to `PatchAudioRequest` and to `FingerprintConfig::from_request`, so the patch and the fingerprint
  geometry read one value. The contract test pins the two defaults together.
- **Home (b):** `refine_and_snap_gap_frames` in `domain/policies/gap_borders.rs`, which is
  `refine_gap_frames` followed by `snap_gap_edges_to_onset`. Every production edge walk calls it:
  patch characterize, injected-time detection, and the two fingerprint measure paths. Tests and
  fixture oracles keep calling `refine_gap_frames` alone.
- **Rule:** § 3's rule, ported (not shared with the bin). 2 ms peak bins, max across channels. An
  onset is a fall of 15 dB below the loudest of the two bins before it, sustained into the gap and
  refined to the sample. Onsets within two bins merge to the outermost, and the **nearest** onset
  wins.
  - The bins end at the refined edge, as the bin's end at the scan boundary, so the step from the
    band into silence is never itself an onset.
    - **Corrected 2026-10-28, before any listening.** The first port reached two bins into the gap,
      so that room tone stepping to silence would read as "no band". But every exposed census
      candidate has that same shape: `floor_ms` = 0 and a band peaking at −21 to −40 dBFS right up
      to the boundary (the 36/g5 lead: 29.9 ms, peak −28 dBFS). The step at the edge won as the
      nearest onset, so the lead never moved.
      - Render check 2 caught it on 36/g5 and 3/g6: their first difference was at the post seam,
        not near the onset. The lock probe on 36/g5 confirmed it: the dual-fit `a_start` was
        unchanged.
      - The window inside the gap also moved the 36/g5 trail by 58 frames (1.2 ms), where the
        census sees no onset.
    - The rule cannot tell room tone after a word from a soft band. The cap is the only guard,
      as in § 3.
- **Ordering with the advance loop:** the lead does not snap when refinement advanced the start past
  the reported start, because the scanner opened that run inside programme.
- **Cap:** the prototype runs at 40 ms. Where `onset_far_ms` and `onset_near_ms` agree, every patched
  band is at most 31.3 ms (non-anchor leads: n = 204, p50 12.2, p99 29.2 ms; non-anchor trails: max
  20.7 ms; anchor-seam leads: max 30.6 ms). The one exception is an anchor-seam trail at 107.5 ms
  (58/g2), which the cap refuses. The long bands are on unpatched edges (trails: p50 26.8, max
  196 ms), the § 3 programme-decay shape. 40 ms is a prototype value; the default decision sets it.
- **Tests:**
  - 7 domain tests (`edge_snap_*`): soft lead and trail snap to the band's first frame; a band past
    the cap is refused; a band ending in a hard step to silence snaps (36/g5's shape); a hard edge
    and a sub-floor bucket edge are no-ops; an advanced start is not undone; off equals refine
    alone.
  - `edge-onset --snap-cross-check-ms 40` (media mode, diagnostic) runs the production snap on
    the bin's edge windows, with the scan boundary as the edge, and prints both readings per edge.
  - CLI test `edge_snap_writes_over_a_soft_lead_band` (`cli_wav_integration`): without the flag, the
    band's first 5 ms are raw A; with it, the fill covers them. Audio more than 100 ms before the
    onset is identical in both.
  - Corpus churn (`diff-patch-outcomes.ps1`) is not run yet. It belongs to the default decision.

### Phase 3 — ear check protocol (pre-registered 2026-10-28, before any render)

**Question:** is the exposed soft band audible on the worst edges, and does the snap remove it?

Executable half: `scripts/measure/build-edge-snap-listen-set.ps1` (`-PlanOnly` prints the walk and
round-1 runs; it re-derives the ranking from the census and stops if it differs from the list below).
Round 1 runs the first 12 candidates and both controls. Each later round runs only the replacements.

**Selection.** Candidates are patched, non-anchor-seam edges with an onset, ranked on
`exposure_est_ms` descending. Ranking on the hard bound (est − 10 ms) gives the same order, because
no candidate has a non-zero adjust field. Anchor-seam edges are excluded because their exposure is
unknown (Phase 2 above). Every candidate is a lead edge. The fixed walk order is:

64/g14, 36/g5, 38/g2, 76/g9, 66/g7, 64/g2, 39/g5, 58/g7, 3/g6, 30/g43, 3/g10, 74/g26, 64/g15, 3/g4,
79/g1, 34/g6, 50/g7, 11/g6, 83/g5, 9/g4, 3/g12, 83/g3, 64/g47, 68/g3

Take the first **12** that pass the render checks. A candidate that fails a check is skipped and
recorded with its reason, and the walk continues. The walk never goes past rank 24. If fewer than
12 pass, listen to those that did and say so.

**Controls (2).** In each of the two highest-ranked pairs that have one, take the lowest-index
patched, non-anchor gap with no onset on either edge: **64/g3** (single-lag) and **36/g4**
(dual-fit). On a hard edge the snap is a no-op, so the two arms must be byte-identical (a render
check). These trials measure the listener's false-difference rate.

**Repeats (2).** Two of the 12 exposed gaps, drawn with the seed, are presented twice.

**Arms.**
- *current*: the build under test, with defaults.
- *snapped*: the same build with `--edge-snap-max-ms 40`.

Both arms are real repairs (`--only-gaps`, `--mux` to `.mkv` with FLAC audio), one run per pair and
arm, run sequentially. `--gap-listen` is not used: it disagreed with production placement by 10 ms on
47/g14 (border-standoff § 7), which is the scale of this test. Each clip is A's patched audio from
4 s before the gap to 4 s after it.

**Render checks.** The set is not built unless every selected gap passes all three.
1. **Reference match.** The current arm reproduces `gap-files/2026-10-26-edge-onset/<pair>.json` for
   the gap: patched, the same `dual_fit_used` and adjust frames, and `align_adjustment_secs` and
   `registration_residual_ms` within 0.1 ms.
2. **The snap reached the write.** In the snapped arm, the first frame that differs from the current
   arm lies in [onset − 12 ms, onset + 1 ms]. The onset is `onset_near_ms` before the scan lead
   frame, and 12 ms is one 10 ms crossfade plus 2 ms. The arms are identical from the post seam on:
   from a_end (plus the end adjust) when the census has no trail onset, else from the trail onset
   plus 12 ms. On the controls, the arms are byte-identical over the whole clip.
   - *Tightened 2026-10-28, before any listening.* The first version allowed differences up to
     a_end + 100 ms, and 38/g2 passed with its post crossfade differing by −3.7 dB relative to the
     signal. That trial would have carried a second, unrelated difference.
3. **Placement only.** The snapped arm keeps the current arm's `dual_fit_used` and moves
   `align_adjustment_secs` by at most 1 ms. Otherwise the trial would test a re-placed fill rather
   than the band, so the candidate is skipped.

**Blind key.**
- Trials are `tNN_a.wav` / `tNN_b.wav` in `gap-files/2026-10-28-blind-edge-snap/`. The key is in
  `gap-files/2026-10-28-blind-edge-snap-key/`.
- Labelled renders and the muxed runs stay in the session scratchpad, never in `gap-files`.
- 16 trials: 12 exposed, 2 repeats and 2 controls. The order is shuffled with seed 20261028, and no
  two trials of one gap are adjacent.
- *snapped* is on *a* in exactly 7 of the 14 non-control trials. Only those counts are inspected
  before listening.
- One sitting. Score each trial a better / b better / same (can't tell), with an optional location
  (before the gap, at the seam, inside the fill).

**Decision rule.**
- **Scoring.** Each verdict maps to *snapped better*, *current better* or *same*. A repeated gap
  counts once, and only when its two verdicts agree; otherwise it counts as *same*.
- **Noise `k`:** the number of non-*same* verdicts on the 2 controls, plus 1 for each repeated gap
  whose two verdicts are opposite.
  - **k ≥ 2: inconclusive.** No decision; the snap stays opt-in.
  - Otherwise the tolerance is `w = k`.
- Over the 12 gaps:
  - **Adopt the snap** (go to the default decision: churn, then a default-on proposal) when
    *snapped better* ≥ 3 and *current better* ≤ `w`.
  - **Close as inaudible** (settled § A, naming the worst edges heard) when both *snapped better*
    and *current better* are ≤ `w`.
  - **Refute the snap** (settled § A) when *current better* > *snapped better* and *current better*
    ≥ 2.
  - **Otherwise keep it opt-in** (settled § C), latched on a second positive sitting drawn from
    ranks 13–24.

### Phase 3 — result (2026-10-29): ADOPT

Scored blind in one sitting; the key was opened only after every verdict was in.
- **Noise:** both controls were *same*. The two repeats agreed: 64/g2 snapped better twice, and
  38/g2 *same* twice. So `k = 0` and `w = 0`.
- **The 12 gaps:** *snapped better* 4 (3/g6, 30/g43, 36/g5, 64/g2), *current better* 0, *same* 8.
  - The rule's adopt branch (*snapped better* ≥ 3 and *current better* ≤ `w`) holds.
  - All 5 non-*same* verdicts, counting 64/g2's repeat, fell on the snapped arm, with the same note
    each time: a blip in the other clip, which was the current arm every time. The blip is the
    exposed soft band.
  - 4 of 4 one-sided is p = 0.0625 on its own. The decision rests on the pre-registered rule, not
    on that p value.
- **Where it was heard:** 3 dual-fit gaps (3/g6, 30/g43, 36/g5) and 1 single-lag gap (64/g2).
  Rank does not order it. The top-ranked 64/g14 (21.3 ms estimate) was *same*, and 3/g6 (16.7 ms)
  was heard. Exposure size alone does not predict audibility; this sample cannot say what does.
- **Next:** the default decision (§ 4 Phase 4): corpus churn with `diff-patch-outcomes.ps1` at
  40 ms, then a default-on proposal.

### Phase 3 — render (2026-10-28)

- **Passed (12, in walk order):** 64/g14, 36/g5, 38/g2, 64/g2, 39/g5, 58/g7, 3/g6, 30/g43,
  3/g10, 74/g26, 3/g4 and 79/g1. Each one's first difference is 9.98 ms before the onset (one
  crossfade), and the largest align move is 0.19 ms (79/g1). Three rounds were needed.
- **Controls:** 64/g3 and 36/g4 are byte-identical across the arms.
- **Skipped (3):**
  - 76/g9 (check 3): the snapped arm moves `align_adjustment` by 31.1 ms, so it re-places the fill.
  - 66/g7 (check 2): its post crossfade also differs, to 9.98 ms past a_end, with no trail onset.
  - 64/g15 (check 2): its first difference is 16.4 ms after the onset.
- **Set:** 16 trials, snapped on *a* in 7 of the 14 non-control trials. Only those counts were
  inspected; the key is unread.
- **Script fixes before the set was written:**
  - The repeat draw piped `Shuffle`'s single wrapped array into `Select-Object`, so it drew all 12
    gaps as one trial. It now indexes the array, and the script refuses a malformed trial list.
  - The README's location hint said the arms differ "around the start of the gap". Some candidates
    also snap a small trail band, so it now says "at the edges of the gap".

### Phase 4 — corpus churn at 40 ms (2026-10-29): contained, but not write-only

Candidate: one `--patch-only --format json --edge-snap-max-ms 40` report per pair over
`gap-files/corpus-pairs.csv`, default features. Baseline: the 2026-10-26 census reports. The
baseline is not the same binary, but the library code differs only by the snap (the last library
commit predates it), and the attribution below finds no change outside snapped gaps.
`diff-patch-outcomes.ps1` joined 1775 of 1775 gaps.

- **Changed: 315 gaps** (312 patched with field changes, 1 skipped with field changes, 2 status
  flips), across 84 pairs. Unchanged: 1460. Run-level: `donor_relation` flipped on 4 pairs
  (38, 75 diff_capture→mixed; 50, 68 mixed→diff_capture).
- **Attribution: contained.** 313 of the 315 have a census onset within 40 ms of the boundary.
  The other 2 are 68/g5 and 83/g7, whose census onset is 82 / 93 ms out. These are the two edges
  with a fall inside the silent run (Phase 2 quality flags). Most likely refinement moved their
  edge, so the snap measures from a different origin than the census does. That is inferred, not
  checked. None of the 254 patched gaps without an onset changed, and neither did any
  `not_planned` gap. 15 patched gaps with an onset ≤ 40 ms are unchanged (10 dual-fit, 3
  anchor-seam, 2 single-lag).
- **Status flips: 2, both skipped→patched, none lost.** 11/g9 and 31/g7 were both
  `shoulders_abut`. 31/g7 is the step-boundary gap whose one-sided close was refuted (post shoulder
  at −1160 ms), so its new patch needs an ear check before it counts as a rescue. 14/g10 stays
  skipped (`correlation_below_threshold`).
- **Not write-only: downstream decisions move on about 95 gaps.** The ear check covered only edges
  where the snap changed the write and nothing else (render checks 2 and 3). The corpus shows
  that the ≤ 40 ms move changes the placement search on many more gaps:
  - `anchor_seam_used` flipped 38 times (21 on, 17 off) and `dual_fit_used` 30 times (13 on,
    17 off).
  - `patch_tier` changed on 48 (high→marginal 12, marginal→high 12, anchor_trusted→high 8,
    high→anchor_trusted 8, and so on). The direction is balanced, with no net drift.
  - 95 gaps move the write's start or end adjust, the anchor bracket, or `align_adjustment` by more
    than 40 ms. The largest align move is 426 ms (69/g9), and anchor brackets move up to 4.0 s
    (32/g10).
  - The other changed gaps move by scoring only: pre-seam correlation and residual fields shift
    as the pre window moves with the edge.
- **Reading.** The snap does what it was listened for on the gaps the ear check could isolate.
  But the placement search treats a ≤ 40 ms edge move as a new problem, and on about a third of
  the snapped gaps it then picks a different patch. Those re-placements are balanced churn, not an
  improvement anyone has heard. Default-on as built ships them unheard.
- **Options for the default decision:**
  1. *Write-only snap (recommended).* Search and place on the unsnapped edges, then extend only the
     write outward to the onset. Churn should shrink to the edge writes the ear check covered.
     Re-run this diff to confirm: every remaining change should be an edge write.
  2. *Default-on as built*, after a blind listen of a sample of the 95 re-placed gaps plus 31/g7.

### What the soft lead band is (2026-10-30): encoder noise after a hard cut

`edge-onset --frame-grid --grid-zero-amp 0.0000763` (hidden diagnostic, media mode) over all 85
pairs. It finds each edge's true silent run: every channel within ±2.5 LSB at 16 bits, for at
least 256 frames, up to 4 hops into the gap. It records where that run starts on the decoded
timeline, mod 1024, and the envelope of the 3 hops beside it. Output:
`gap-files/2026-10-30-frame-grid/frame-grid.csv`.
- **Patched gaps are true digital dropouts.** 1152 of 1162 patched edges open onto a silent run at
  1 LSB at 16 bits (−90.3 dBFS). The fdk-aac decoder emits 16-bit PCM. Only 235 of 2364
  `not_planned` edges do, so those are mostly quiet passages.
- **The silence starts after the scan boundary.** The scan's −60 dBFS floor crossing is not
  digital silence. On hard leads the run begins a median 141 frames (2.9 ms) later.
- **Soft-lead shape:** programme drops about 20 dB in one step at the onset, to around −55 dBFS.
  Then a smooth tail decays to about −80 dBFS and stops at true silence. The census band is the
  part of that tail above −60. Programme never fades through it.
- **Its length is bounded by one AAC long window.** Onset to silent run: 217 of 218 soft leads
  are ≤ 2048 frames (median 1292, q25 1099, q75 1454).
- **Its end is locked to the 1024-frame block grid.** Within a pair, a soft lead's silent-run start
  lies within ±64 frames of the other soft leads' circular mean in 60% of cases (n = 171;
  uniform 12.5%). Hard leads: 25%. Hard trails: 11%, so there is no lock.
- **Reading.** The original audio was cut hard to digital zero at the onset. The encoder coded the
  cut in a long block, which spreads quantization noise forward to the window's end, a block
  boundary. The decoder reproduces that noise about 20 dB under the programme. So the band is a
  codec artifact, and the onset is the true cut. That fits the ear check: the "blip" is the noise
  burst. Hard leads, and nearly all trails, are cuts the encoder resolved tightly.
  - Why trails rarely have a band is not settled. Encoders switch to short blocks on attacks,
    which would confine the noise. But the trail mod-128 test is weak (19% vs 12.5%), so that is
    a hypothesis.
- **For the snap:** moving the write to the onset is correct by construction; it does not
  overwrite programme. A principled lead cap is one long window, 2048 frames (42.7 ms at 48 kHz),
  in place of the empirical 40 ms. It does not change the churn question: placement should
  still not move with the edge.
- **Superseded by a signal guard (2026-10-30).** A per-codec window bound needs a codec table,
  which is rejected ([settled.md](settled.md) § A). Proposed instead, to be tested offline on
  `frame-grid.csv` before any code changes: snap only when the band opens with a step down and its
  tail ends in digital zero. That is the codec-smear signature. It also rejects § 3's programme-decay
  shape, which fades from programme level and doesn't step. The ms cap stays as a loose,
  codec-agnostic backstop.

### Signal-guard test (2026-10-30): zero-termination adopted; the cap stays load-bearing

Offline, over `frame-grid.csv`: every edge with a census onset, with its envelope at 128-frame
resolution from 3 hops before the silent run. Only planned gaps (patched or skipped) are snapped in
production, so they are the population that matters.

| group (onset distance) | edges | gap reaches digital zero | onset → zero ≤ 2048 frames |
|---|---|---|---|
| planned, onset ≤ 40 ms | 391 | 389 | 389 |
| planned, onset > 40 ms | 5 | 3 (two are refinement-origin cases) | 2 |
| unpatched, 4–40 ms | 90 | 12 | 11 |
| unpatched, > 40 ms | 75 | 3 | 0 |

- **"The gap reaches digital zero" is adopted as a guard.** A run of ≥ 5 ms with every channel
  within 2.5 LSB at 16 bits, starting within 85 ms of the edge. Every planned soft band passes
  except 10/g11 trail and 14/g10 trail. Their gaps sit at −62 to −66 dBFS: quiet programme, not
  dropouts, so refusing them is right. On planned edges reaching zero, the zero run starts at most
  1376 frames (29 ms) past the refined edge.
- **The step half adds nothing new.** The onset rule already is a step test: a ≥ 15 dB fall within
  4 ms, sustained. Measured steps on patched leads: p10 21 dB, median 26 dB. A smooth programme
  fade cannot produce one.
- **A decay-shape test is rejected.** Patched leads decay through the band (tail end: median
  −82 dBFS, p90 −75). Patched trails don't: 26 of 46 end above −70, a flat band of median length
  345 frames (7 ms). That fits short-block pre-echo, but is unverified. A decay test would refuse
  them.
- **The ms cap cannot be replaced; it is load-bearing, not loose.** 58/g2 trail (anchor seam) is a
  flat band at −57 dBFS for 107 ms that ends in a hard cut to zero. It passes the step and zero
  tests, and only the cap refuses it: quiet programme running up to the cut. On this corpus any
  cap between 31.3 ms (the longest real band) and about 89 ms (the shortest such programme case)
  gives the same result. **40 ms stays.** It is already churned and listened at.
- **What "codec-agnostic" now means.** The guard is AAC-LC validated. A codec without a transform
  window has no band, so the snap is inert there. A codec with longer windows (HE-AAC, about
  85 ms) has bands that can exceed the cap. Those stay unsnapped, which is today's behaviour, so
  the failure is safe.
- **Trails are unheard.** Phase 3's candidates were all leads. The trail snaps (99 patched trail
  edges have an onset) ship with default-on, and their band shape differs from the leads'. The
  step 3 listen must include a trail-snapped stratum.

Code: `gap_reaches_zero` in `domain/policies/gap_borders.rs`, applied after the cap, with two new
tests (a quiet non-zero gap; zero only beyond the reach). Both fail with the guard disabled.

### Next (decided 2026-10-30): option 2, default-on as built, gated by the signal guard

Write-only (option 1) is dropped. With a guard that snaps only on the codec-smear shape, the snapped
edge is the true cut. The unsnapped geometry is then the wrong one: its pre-seam template carries
codec noise. Write-only would keep placement there, and the work across six consumers would be
thrown away. The re-placements are still unheard, so the listen stays. The snap stays at home (b),
in repair. Moving the scan boundary (option (a)) would reopen the equivalence-gate validation and
is not proposed.

1. **Test the guard offline** on `frame-grid.csv` and the census. It must keep nearly all of the
   218 soft leads, and it must reject what the 40 ms cap refuses, or explain the difference. If it
   fails, the 40 ms cap stays and write-only is reconsidered.
2. **Build the guard into `snap_gap_edges_to_onset`.** Add tests for both shapes: step plus a tail
   to zero snaps; programme fading through the band does not. Re-run the corpus churn at
   default-on.
3. **Pre-registered blind listen of a stratified sample of the re-placed gaps.**
   - Strata: anchor flips, dual-fit flips, tier changes, the largest moves (69/g9, 32/g10), and
     trail snaps (never heard; see the signal-guard test).
   - Plus the two status flips, 31/g7 and 11/g9.
   - The decision rule is fixed before rendering.
4. **Build the snap reporting** (added 2026-10-31; spec in *Step 4 — snap reporting* below). It
   comes after the listen and before the flip. The listen does not read it, and its one
   behaviour-adjacent change needs a full-corpus run that the flip pays for anyway.
5. **Flip the default if the listen passes.** Then close per § 7, with one settled row.
   - The claim is AAC-validated only. On codecs without a transform window the guard is inert by
     construction.

### Step 2 — corpus churn with the guard (2026-10-30): exactly as predicted

Candidate: `measure-patch-outcomes.ps1 -RepairArgs "--edge-snap-max-ms 40"` over all 85 pairs, with
`gap_reaches_zero` built in. Output: `gap-files/2026-10-30-edge-snap-guard/`, with diff CSVs
`diff-vs-10-29.csv` and `diff-vs-10-26.csv`. Both diffs joined 1775 of 1775 gaps.

- **The guard's own effect (vs the unguarded 2026-10-29 run): 2 changed, 1773 bit-identical.** The
  changes are 10/g11 (patched: the anchor seam is dropped and the write's end returns 900 ms) and
  14/g10 (still skipped, correlation fields only). Those are the two trails the offline test
  refused. Both are now bit-identical to the 2026-10-26 baseline. No run-level change.
- **Default-on churn (vs the 2026-10-26 baseline): 313 changed** (311 patched with field changes,
  2 status flips) across 83 pairs, against 315 unguarded. Every change has a census onset. The
  only ones further out than 40 ms are 68/g5 and 83/g7, the refinement-origin cases above. Status
  flips are unchanged: 11/g9 and 31/g7, both skipped→patched, both now single-lag `high` with dual
  fit. No patch is lost. *(Both flips were an abut artifact. Reverted by the abut fix, see the chain
  audit below. With it, default-on churn is 311 gaps and has no status flips.)*

**Listen strata (input to step 3; not pre-registered and not rendered).** Counted over the 311
patched→patched gaps. "Moved" means the write's start or end adjust, the anchor bracket, or
`align_adjustment` changed by more than 40 ms.

| stratum | gaps | detail |
|---|---|---|
| `anchor_seam_used` flip | 37 | 20 on, 17 off |
| `dual_fit_used` flip | 30 | 13 on, 17 off |
| `patch_tier` change | 46 | marginal↔high 12/12, anchor_trusted↔high 8/8, anchor_trusted→marginal 5, marginal→anchor_trusted 1 |
| moved > 40 ms | 94 | start adjust 35, end adjust 23, anchor bracket 22, align 14 (largest: 32/g10, anchor bracket 4.0 s) |
| **re-placed (union of the four above)** | **111** | 55 in one stratum, 24 in two, 24 in three, 8 in all four |
| named: 69/g9 | 1 | anchor off, dual on, end adjust moves 1150 ms (align 426 ms) |
| named: 32/g10 | 1 | anchor on, dual off, anchor bracket moves 4.0 s |
| trail snap (trail onset ≤ 40 ms) | 96 | 40 also re-placed; 63 also snap the lead; **27 trail-only and not re-placed** (the clean trail stratum, like Phase 3's leads) |
| status flip | 2 → 0 after the abut fix | 11/g9, 31/g7 (skipped again) |
| lead-only, scoring-only | 144 | no stratum: the population Phase 3 already heard |

- The 10-29 "about 95" counted moves only. The union of all four placement signals is 111. That
  is the population step 3 samples. The guard removed one of its gaps (10/g11).
- Trail onsets are short: median 3.8 ms, at most 20.7 ms. The clean trail stratum isolates a
  ≤ 21 ms post-seam move. A trail snap moves the post seam, so render check 2 needs the
  trail-onset variant, which the Phase 3 script already has.

### Chain audit (2026-10-31): the snap overturns two latched skips; nothing else double-corrects

The user asked whether default-on breaks anything built to cope with the unsnapped edge. That
includes latched and blocked items in settled.md. Each consumer of the refined edge was checked
against the guarded run (`gap-files/2026-10-30-edge-snap-guard/` vs `2026-10-26-edge-onset/`).

- **`shoulders_abut` skip (`SeamLocks::abut`, `bridge_frames <= 0`): BROKEN, two latches
  overturned.** The status flips are not repairs. Both gaps keep exactly the same shoulder lags
  (11/g9 pre −71.96 / post −675.71 ms; 31/g7 pre −38.23 / post −1160.25 ms). Their base bridges sat
  just below zero: −205 and −153 frames (−4.3 / −3.2 ms). The lead snap lengthens each gap by its
  band (5.6 / 7.3 ms), which pushes the bridge to +1.3 / +4.1 ms. `abut()` stops firing, and
  dual-fit splices across the whole 604 / 1122 ms step with almost no donor bridge. That is the
  case the skip exists to refuse: introduced silence, not missing content. It is the same
  arithmetic coincidence as the retired `collapsed_bridge` reading (bridge = gap + step).
  - settled § C: "11/g9 stays skip until" the introduced-silence collapse decision. 11/g9's post
    lock is the corpus's known false peak (step agreement 607.99 ms, § C two-probe row), so its new
    patch registers its post seam on a false lag (`registration_residual_post_ms` 675.7).
  - settled § D: 31/g7's step-boundary skip verdict is blocked on a listen tree. Default-on would
    patch it anyway, without one.
  - The run-level `shoulders_abut` list, which the latched collapse Change reads for its targets,
    goes from 2 entries to 0.
  - **FIXED (2026-10-31, uncommitted): `abut` is read on the unsnapped geometry.** The snap
    corrects the write, not a skip verdict. Repair characterize now splits refine and snap, and
    `SeamLocks::without_edge_snap(lead, trail)` moves each seam back in by its edge's snap (lags
    unchanged). The skip is decided and recorded on that geometry: bridge, `gap_frames`, the A
    frames, and the B start for `program_quiet`. Placement after the check stays on the snapped
    edges. Unit test `edge_snap_does_not_lift_an_abut`.
    - **Re-run of pairs 11 and 31 with `--edge-snap-max-ms 40`** (`gap-files/2026-10-31-abut-fix/`):
      11/g9 and 31/g7 are `shoulders_abut` skips again. They match the 10-26 baseline exactly on
      lags, bridge (−205 / −153), `gap_frames` (28775 / 53704) and the run-level abut rows. The
      one difference is `pre_z` (19.58 vs 19.87; 14.37 vs 14.14), because the pre lock is still
      measured on the snapped template, as intended. Every other gap in both pairs is identical to
      the guarded run (16/17, 12/13), so the fix only reaches the two skips. PR tier: 1134 passed.
    - **Not mirrored in the fingerprint:** `schema.rs` still reads `shoulders_ordered` on the
      snapped bridge. That is harmless while the fingerprint's `edge_snap_max_ms` defaults to 0. It
      joins the default-flip to-do below.
- **Border (structure) templates: unaffected by design.** They are cut `border_standoff_secs`
  (0.35 s) back from the edge, then trimmed below 12 % of peak. That already excluded the band (at
  about −55 dBFS), so a ≤ 31 ms edge move only translates them. The standoff's unmeasured "click or
  fade" rationale (settled § E) is not touched in either direction.
- **Seam-lock templates (`wide_a_slicer`, flush with the edge, D ≡ 0): moved, as intended.** These
  carried the band, and removing it is the stated reason for keeping placement on the snapped edge.
  This is the source of the 111 re-placements, which step 3 listens to.
- **A-boundary extension (`gap_start/end_adjust`, ≤ 500 ms):** 58 of the 94 large moves. Some
  extensions may have been escaping band-contaminated seams. They are re-placements, so they are in
  step 3's strata, not a separate defect.
- **Short fills (`census-zero-pad.ps1`):** 0 of 359 bracket-eligible gaps on the snapped run, and
  0 of 355 on baseline. The longer snapped gaps did not exhaust a donor.
- **A4 injected-time** (it measures shoulder lags at the snapped edges): no warning in either arm.
- **Run-level `donor_relation`** flipped on 4 pairs. It is a report-only summary of the gap tags
  (`derive_donor_relation`), with no decision consumer.
- **Scan, the equivalence gate and `not_planned` gaps:** untouched by construction (the snap lives
  in repair). 0 `not_planned` gaps changed.
- **To do at the default flip:** `gap_fingerprint/contract.rs` pins the fingerprint's
  `edge_snap_max_ms` default against `RepairConfig`'s, so both must flip together. Otherwise the
  dump geometry drifts from the patch. In the same change, have the fingerprint's
  `shoulders_ordered` read the unsnapped bridge (`without_edge_snap`), so the dump keeps
  predicting the abut skip.

### Step 3 — listen protocol (DRAFT 2026-10-31; pre-registered only once approved, before any render)

**Question:** does default-on, with its re-placements, ever make a gap audibly worse? Phase 3
already showed the snap removes an audible blip. Re-placement is balanced churn, with no expected
direction. So this test is for **non-inferiority**: it looks for regressions, not wins.

**Arms.** One build (the guard build; the library differs from `01de590b` only by
`gap_reaches_zero`). The *current* arm uses defaults. The *snapped* arm adds `--edge-snap-max-ms 40`.
Both are real repairs (`--only-gaps`, `--mux` to `.mkv` with FLAC audio), run sequentially, as in
Phase 3.

**Selection.** Every re-placed gap goes in exactly one primary stratum, checked in this order:
anchor flip, dual flip, tier change, moved > 40 ms. The draw uses seed **20261031**: within each
stratum, sort by (pair, gap), then shuffle. Take candidates in walk order until the quota passes the
render checks. Don't go past rank 8 in any stratum. A candidate that fails a check is recorded with
its reason.

| stratum | n | quota | walk order (ranks 1–8) |
|---|---|---|---|
| named | 2 | 2 | 69/g9, 32/g10 (both always in) |
| anchor flip | 35 | 3 | 48/g3, 48/g5, 83/g7, 50/g8, 49/g34, 85/g85, 7/g4, 86/g12 |
| dual flip | 11 | 3 | 68/g3, 17/g34, 43/g7, 10/g8, 26/g6, 33/g14, 80/g11, 79/g9 |
| tier change | 22 | 3 | 45/g1, 69/g5, 39/g7, 84/g9, 32/g13, 68/g2, 5/g3, 63/g5 |
| moved only | 41 | 3 | 71/g12, 76/g3, 13/g31, 13/g17, 64/g46, 23/g4, 3/g17, 9/g5 |
| clean trail (trail-only snap, not re-placed) | 27 | 4 | 12/g1, 51/g1, 37/g5, 11/g4, 85/g88, 80/g8, 72/g6, 65/g9 |

That gives 18 A/B gaps: 14 re-placed and 4 trail.

**Controls (2), A/B.** In the two named pairs, the lowest-index patched gap that is unchanged and has
no onset on either edge: **69/g6** (dual fit) and **32/g9** (single lag). The arms must be
byte-identical. **Repeats (2):** two of the 14 re-placed gaps, drawn with the seed, presented twice.
In total, 22 A/B trials.

**Status flips (separate block): DROPPED 2026-10-31.** The abut fix landed (chain audit above),
both gaps stay skipped, and there is nothing to hear. The original design is kept below for the
record. It applies only if the fix is ever reverted. For 31/g7 and 11/g9, the current arm is an
unrepaired hole, so an A/B comparison tests nothing. Instead, render each snapped-arm clip on its own.
Shuffle them with two reference clips of unchanged patched gaps, **11/g3** and **32/g3** (pair 31 has
no unchanged, onset-free patched gap). Score each clip *clean* or *defect*, with a location. The
listener is not told which clips are the flips.

**Render checks.**
- *Trail stratum and controls:* Phase 3 checks 1–3 unchanged, with check 2's trail-onset variant.
- *Re-placed strata*, which re-place by design, so check 3 is dropped:
  1. **Reference match.** The current arm reproduces `gap-files/2026-10-26-edge-onset/<pair>.json`,
     as in Phase 3.
  2. **Candidate match.** The snapped arm reproduces `gap-files/2026-10-30-edge-snap-guard/<pair>.json`:
     the same `anchor_seam_used`, `dual_fit_used`, `patch_tier` and adjust frames, and
     `align_adjustment_secs` within 0.1 ms. That way we hear the re-placement the corpus made.
  3. **Confined.** The arms are byte-identical outside [earliest write start − 12 ms, latest write
     end + 12 ms], taken over both arms.
- **Clip window:** 4 s before the earlier of the two write starts, to 4 s after the later write end.
  It is the same window for both arms. Anchor brackets move up to 4 s, so some clips are long.

**Blind key.** As in Phase 3, under `gap-files/2026-10-31-blind-default-on/` with a `-key` sibling.
22 A/B trials, shuffled with the seed, with no two trials of one gap adjacent. *snapped* is on *a* in
exactly 10 of the 20 non-control trials. The 4 absolute clips are scored in their own block. All
scoring happens in one sitting.

**Decision rule.** Tolerance confirmed by the user on 2026-10-31: a single gap that sounds worse
blocks default-on.
- `k` and `w` are defined as in Phase 3: control false-differences plus opposite repeats. **k ≥ 2
  is inconclusive.**
- Over the 18 A/B gaps, a repeat counts only when both its verdicts agree. Status flips pass when
  both are *clean*. A *defect* that also appears on a reference clip counts as listener noise and
  adds 1 to `k`.
  - **Default-on:** *current better* ≤ `w`, and both status flips pass. Build step 4 (snap
    reporting), then flip the default and close per § 7.
  - **Refute default-on as built:** *current better* ≥ 2 and greater than *snapped better*. Record
    it in settled § A. The snap stays opt-in, and write-only (option 1) is reopened.
  - **Otherwise stay opt-in** (settled § C). This includes a single regression, or a flip with a
    defect. It is latched on a second sitting from walk ranks 4–8, plus a look at the losing gap's
    stratum. A regression confined to one stratum would argue for a stratum guard (for example,
    no snap when it would flip `anchor_seam_used`), not for dropping the snap.

**Executable half:** a re-placed mode for `build-edge-snap-listen-set.ps1`, built only after this
draft is approved. Estimated media cost is about 2 × 20 pair-runs at about 6 minutes each, around
4 hours sequential, plus rounds for replacements.

### Step 4 — snap reporting (planned 2026-10-31; built after the listen, before the flip)

**Why.** Default-on moves the write on 311 gaps, and no report says so.
- The scan report keeps the floor edge. That is the accepted cost from § 4 Phase 4.
- The patch entry's `gap_start_adjust_frames` / `gap_end_adjust_frames` are measured from the
  refined edge, and that becomes the snapped edge. A reader can no longer recover the write start
  from `start_secs`.
- `characterize.rs` computes `snap_lead_frames` / `snap_trail_frames` for the abut fix, then drops
  them.
- A's codec is decoded (`SourceDescriptor`) and dropped (`sources: _` in `patch_audio/mod.rs`).
  Only the fingerprint dump keeps it.
- The guard is validated on AAC-LC only (signal-guard test). On any other codec, the report is the
  only place a misfit would show.

**Why after the listen.** The listen reads no new field, and the reporting does not touch the
audio. The one change near behaviour, the search window below, needs a full-corpus run to clear.
The flip needs that run anyway, so both share it.

**Prerequisite: the onset search must reach past the cap.** `lead_onset` / `trail_onset` search only
`max_frames` plus two bins. A band longer than that has no 15 dB fall inside the window, so it returns
no onset, and a cap refusal reads as a hard edge. That is the HE-AAC case (about 85 ms windows), and
it is 58/g2's trail today.
- Fix: search a fixed observation window (200 ms, the census window), and let the cap limit only the
  move.
- This is not provably neutral. `merge_onsets` chains onsets ≤ 4 ms apart to the outermost. An onset
  newly visible past the old window could, in principle, pull a nearby snap outward or into a
  refusal. The verification run checks it.

**Per-edge record.** An optional `edge_snap` object on every gap that reaches characterize, patched
or skipped, with a `lead` and a `trail` entry. `not_planned` gaps never run the snap. Absent when the
snap is off. Frames are at A's rate, as for the adjust fields.

| field | meaning | what it shows on another codec |
|---|---|---|
| `outcome` | `snapped`, `hard` (no onset), `refused_cap`, `refused_no_zero`, or `not_attempted_advanced` (lead only: refinement advanced the start). The refusals follow the code's order: cap first, then zero | The two refusals are the codec canaries |
| `onset_frames` | Onset to the refined edge, also on refusals; absent on `hard` | A codec's band length shows as a new cluster |
| `moved_frames` | How far the write's edge moved (`snap_lead_frames` / `snap_trail_frames`); 0 unless `snapped` | With `start_secs` and the adjust field, it gives the write start |
| `reaches_zero` | The `gap_reaches_zero` result, on every edge with an onset | Kept separate from `outcome`, so a long band that also never reaches zero reads as programme |
| `onset_to_zero_frames` | Onset to the start of the digital-zero run | AAC-LC: ≤ 2048 (217 of 218 soft leads). A different window length shows here |
| `band_peak_dbfs`, `step_db` | Band shape | The encoder-noise shape is a step of about 20 dB to about −55 dBFS |
| `gap_floor_dbfs` | Only on `refused_no_zero`: the lowest 2 ms bin inside the zero search | Separates a noise-filled dropout (dither, comfort noise) from quiet programme (10/g11, 14/g10 trails at −62 … −66 dBFS) |

**Run level.**
- `PatchSummary` gets an `edge_snap` settings echo: the cap in ms, the zero amplitude, the zero
  search and run lengths in ms, and the observation window in ms. This is the `fill_geometry`
  pattern, so a two-arm differential can be traced to the setting.
- `PatchSummary` also gets counts per side per `outcome`, and the largest `onset_frames` among
  `refused_cap`.
- `TrackCompatibility` gets `a_codec` / `b_codec`, with the native sample rate and bit depth when
  the probe has them. Check on one HE-AAC file whether these separate HE-AAC from LC. If they
  don't, also record the profile.

**Human output** (`docs/cli-output.md`). The default output does not change.
- Under `--verbose`, split the `A gap (refined)` line into a refine line and a snap line, for example
  `A gap (snapped): lead −21.8 ms (zero at +27.1 ms)`. A refusal prints as
  `edge snap refused (cap): lead onset −62.0 ms`.
- One run summary line with the outcome counts.

**Other records.**
- **Fingerprint dump:** add `a_unsnapped_start_secs` / `a_unsnapped_end_secs` beside `a_refined_*`
  (`schema.rs`). Then dumps from before and after the flip stay comparable. This lands with the
  `shoulders_ordered` to-do in the chain audit.
- **`edge-onset`:** compare its floor edge against the unsnapped edge. On snapped reports, read
  exposure from `moved_frames`, not from the onset − floor − 10 ms estimate. Otherwise it reports
  exposure that has already been fixed.
- **Abut skip record:** `ShouldersAbutLocks` carries the lead and trail snap frames, so the skip
  shows it was read on the unsnapped geometry.

**Contract.** Every field is optional and additive, so the contract stays v1. Follow the
[json-output.md](../json-output.md) revision procedure. Populate every new field in the
`full_surface_repair.json` golden, including one refusal with `gap_floor_dbfs`. Then regenerate it
with `write_full_surface_repair_golden`.

**Tests.**
- Domain: one test per `outcome`, on the existing `edge_snap_*` fixtures.
- Domain: a band longer than the cap reports `refused_cap` with its onset. Before the window change,
  it reads `hard`.
- Domain: a merge-chain case where the wider window must not change the move.
- CLI: extend `edge_snap_writes_over_a_soft_lead_band` to check that `moved_frames` equals the
  shift of the written start.

**Verification: one full-corpus run, at the flip.** Build the reporting, the window and the default
flip together, including the fingerprint to-do. Run `measure-patch-outcomes.ps1` at defaults over all
85 pairs. It passes when:
- Every field that existed before is identical to the listened build's snapped arm. That means the
  2026-10-30 guard reports, with 11/g9 and 31/g7 taken from `2026-10-31-abut-fix/`. Any difference
  comes from the window change and must be explained before the flip.
- `moved_frames` > 0 on every gap the guarded churn changed (311). Anywhere else, it is > 0 only on
  edges with a census onset ≤ 40 ms whose report did not change. The 2026-10-29 churn had 15 such
  gaps.
- 58/g2 trail reads `refused_cap` (onset 107.5 ms). 10/g11 and 14/g10 trails read `refused_no_zero`,
  with `gap_floor_dbfs` in −62 … −66.
- On snapped leads, `onset_to_zero_frames` ≤ 2048, matching `frame-grid.csv`.

That run is the first report that records the snap. It is also the baseline for any later codec.

**Reading a first run on another codec.** Neither signal changes behaviour; each one triggers a
measurement.
- `refused_cap` > 0, with `onset_frames` clustered: the codec's window is longer than the cap. Those
  edges stay unsnapped, which is safe. The open question is the cap.
- `refused_no_zero`, with `gap_floor_dbfs` well below quiet programme: the codec's dropouts do not
  decode to zero. The open question is the zero threshold.

**Not added.**
- The 1024-frame grid lock. It is a census diagnostic, read per pair, and stays in `edge-onset`.
- Codec labels or per-codec verdicts ([settled.md](settled.md) § A). The report carries signal
  values beside the codec, and the reader groups them.

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
