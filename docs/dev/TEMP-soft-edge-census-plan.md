# TEMP — Soft dropout edges: census, then snap or close

**Status:** draft, 2026-10-26; revised the same day after review (patch-time refinement, onset rule,
dual-fit evidence). Not started. Phase 1 (pilot) is the next action.

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
exposure has not been checked. The mirror case, a soft trailing edge at gap end, has never been
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
shoulder-sweep`. Outputs go under gitignored `gap-files/`.

**Boundary placement.** The bin recovers the clip context from clip length and dump geometry, and
falls back to `--context-secs` (default 3.0) only when no dump JSON exists. This is the
[shoulder-sweep.md](shoulder-sweep.md) § 2.1 rule. The scan boundary is therefore *usually* at
3.000 s, but `frame_range` clamps at media edges, so the bin does not assume it.

**Per edge**, in 2 ms bins, recording per-channel peak dBFS and the maximum across channels (`L`):
- **Leading edge.** In the 200 ms before the scan boundary, the onset is the **earliest** bin `i`
  where
  - `L[i] ≤ L[i−1] − 15 dB`, and
  - every bin from `i` up to the boundary also stays at or below `L[i−1] − 15 dB`.

  The sustain condition rejects programme transients. A word ending, or a bass waveform crossing
  zero inside a 2 ms bin, falls and then comes back up. Taking the earliest *sustained* fall rather
  than the nearest one keeps a second step inside the decay from being picked instead of the real
  onset. The 200 ms window is wider than the backlog row's 100 ms because the one known band is
  26 ms and longer ones must not be clipped.
- **Trailing edge (mirror).** In the 200 ms after the scan end, the onset is the **latest** bin `j`
  where `L[j] ≥ L[j−1] + 15 dB`, and every bin from the boundary up to `j − 1` stays at or below
  `L[j] − 15 dB`.
- **Floor edge.** This is the latest sample before the scan start (or the earliest after the scan
  end) where any channel has `|x| ≥ 0.001`. It is searched within the same 200 ms window.
- No onset found ⇒ `onset = none`. That is a result (a hard edge), not an error.

**Columns:**
- pair, gap index, edge (`lead` / `trail`);
- onset-to-boundary ms, floor-edge-to-boundary ms, and onset-to-floor-edge ms (the band refinement
  cannot remove);
- band peak dBFS, and whether the band reaches −60 dBFS on any channel (bool);
- band duration ms, codec;
- `disposition` from `listen.json` or the patch report;
- fill path (`dual_fit` / `single_lag` / `unknown`), from the patch report when one is joined.
- Phase 1 only: the measured write start, and onset-to-write ms (see below).

**Write start is measured, not assumed.** On a `--gap-listen` clip, `_a_patched.wav` equals
`_a_surround.wav` until the write starts. The first differing sample **is** the write start, so the
exposed band (onset to write start) needs no knowledge of the crossfade, the refinement, or the seam
path. The bin also records `write_vs_predicted_ms`: the measured write start against the floor edge
less the 10 ms crossfade. That is the Phase 2 estimate, checked on clips where both are known.

## 4. Phases

### Phase 1 — Pilot on existing listen clips (no media cost)

Inputs, deduplicated by gap (arms of the same run share raw A):
- `2026-10-02-interior-edit`: 9 gaps, **including 50/g7**, the blip gaps from that plan. All are
  dual-fit renders.
- `2026-10-23-listen-band50`: 50 patched gaps over 44 pairs. The fill path is unknown unless that
  run's patch JSON or verbose log is on disk to join.

**Go / no-go:**
- **Go** if all of the following hold:
  - 50/g7 reproduces: onset at 2.974 s ± 2 ms, measured write start at 2.990 s ± 1 ms, and the
    floor edge within 1 ms of the 3.000 s scan boundary. Refinement did not move this start,
    since the write sits one crossfade before the boundary.
  - Every patched pilot gap with `|write_vs_predicted_ms| > 1` is explained by a named cause, such
    as a fit-grid start move or a dual-fit placement.
- **No-go** (fix the detector, do not run Phase 2) if 50/g7 misses, or if more than a handful of edges
  report onsets that, on inspection, are programme transients rather than dropouts. If that happens,
  record the false-positive shape here before changing the rule.

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
  verbose patch log, which prints whenever refinement moved an edge.

**Result:** per-edge distribution, split by `lead` / `trail`, by whether the band reaches the floor,
by codec, by fill path, and by `patched` vs not. Record it in § 5.

**Exit early** if no patched edge besides 50/g7 has more than ~5 ms of estimated exposure. Then 50/g7
is n = 1, and Phase 3 is a single-gap ear check.

### Phase 3 — Ear check (pre-registered before listening)

Write this section's protocol into § 5 **before** the first sitting:
- which gaps: the worst N exposed edges by onset-to-write ms, plus 2 hard-edge controls;
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

*(empty until Phase 1 lands)*

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
