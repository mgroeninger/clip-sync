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
