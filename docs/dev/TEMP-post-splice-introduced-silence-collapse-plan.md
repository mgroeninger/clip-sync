# TEMP — Post-splice check: collapse introduced silence on skip gaps

**Status:** draft, 2026-08-27. One Change. **Startable the day**
[TEMP-early-shoulder-locks-collapse-skip-plan.md](TEMP-early-shoulder-locks-collapse-skip-plan.md)
ships `shoulders_abut` (this Change reads that reason; it does not re-search).

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *Post-splice
introduced-silence collapse*.

**Companions:** skip identity
[TEMP-early-shoulder-locks-collapse-skip-plan.md](TEMP-early-shoulder-locks-collapse-skip-plan.md)
§ 4; Hole 2 [gap-vocabulary.md](gap-vocabulary.md) § *Known holes*; pipeline splice
[pipeline.md](../pipeline.md) § 4 step 5; mux duration
[cli-output.md](../cli-output.md) § Timeline / duration warnings.

**Media hygiene:** pair index + gap index only.

---

## 0. Architecture (normative)

This section is the plan. Later sections only implement it. If a later sentence
disagrees with this one, this one wins.

**Fills complete first. Collapse is a later, duration-changing pass on the
already-spliced A PCM. Mux receives that shorter timeline and applies the same
cuts to video.**

```text
characterize → keep / skip (shoulders_abut already decided)
execute fills
splice_into_a          // length-preserving; original A frame clock
introduced_silence?    // visit every remaining skip, after splice
  pass → drain gap from A PCM, concatenate the remainder in memory
mux                    // audio is the collapsed PCM; video drops the same spans
```

Fills replace silence with equal-length donor audio. Their destinations are
frame indices on the **scan / decode clock**. Shifting that clock before splice
would invalidate every later patch. So splice every keep, **then** walk the
skips, **then** shorten A.

Collapse is not a fill. It deletes the silent interior and joins the two A
shoulders that the skip-gate already proved are adjacent in B. The tail of A
shifts left by the cut. Multiple cuts: apply **last-to-first** on the original
clock so earlier indices stay valid.

`--mux` copies A’s video today (`-c:v copy`, `-shortest`). That is wrong after
a mid-timeline cut: `-shortest` trims the **tail**, so everything after the
cut is A/V-desynced. Mux must drop the **same A-clock spans** from video.
Stream-copy cannot cut at arbitrary audio-frame times, so a collapse run
**re-encodes video** through a drop-range filter. `--wav` is just the shorter
PCM; no video work.

`--repair-preview` never splices, so it never collapses. It may *forecast*
`would_collapse` from `shoulders_abut`. `--patch-only` splices and collapses
in memory (no file); the report carries the new duration.

One sentence: **keep/skip on the original clock; splice every fill; then
check each leftover skip for introduced silence; drain those spans from A
PCM; mux the collapsed audio with video cut at the same stamps.**

---

## 1. What “introduced silence” is

The motivating case is 31/g7: A holds ~1.12 s of silence between two program
edges that are **adjacent in B**. A has the content plus extra dead time; B
has the content. Any donor fill duplicates program (Hole 2). The honest
repair is not a fill — it is **delete the dead time**.

The skip-gate’s `shoulders_abut` row *is* that geometry: live locks, 
`bridge_frames <= 0`, SilenceSplice cell. Vocab still refuses a causal story
(A-inserted padding / B-side content cut / break padding all produce the
same numbers). All three are “A is silent for a span B does not have.”
Cutting A’s silence makes A’s remaining PCM match B’s continuity. That is
the intended action.

**Not introduced silence** (leave the skip, do not cut):

| Skip | Why cutting is wrong |
|------|----------------------|
| `ProgramQuiet` | Shared pause. Cutting would make A shorter than B. |
| `CorrelationBelowThreshold` / Decorrelated | B occupied, no lock. Joining A’s edges is an unmeasured jump. |
| `ResidualHeadroomExceeded` | Seams passed and B ≠ A. |
| `BoundaryAlignmentFailed` / Unfillable | No placement, or geometry failed. Do not invent a join. |
| `NotApplied` | Gate approved a fill; splice failed. Still a bug, not a cut. |

Visit **every** skip so the classifier is explicit. Only `shoulders_abut` is
eligible. Do not infer collapse from `bridge_ms ≈ 0` on other reasons —
that arithmetic is settled-dead ([settled.md](settled.md) § A).

---

## 2. The check (after splice, no re-search)

Admission is the skip reason. The check is a **post-splice PCM confirm** of
the join the cut would produce, not a second dual-fit.

On each eligible skip, with A already containing every keep:

1. Read the lock payload already on the row (`pre_lag_ms`, `post_lag_ms`,
   `bridge_frames`, `gap_frames`) and the refined A `[start, end)` frames
   characterize used. Do not call `seam_local_peak` again.
2. Build the would-be join from **post-splice A**: a seam-local collar
   ending at `gap_start` concatenated to a same-length collar starting at
   `gap_end` (template length = existing 250 ms dual-fit seam window).
3. Pearson that concat against B at the abutting lock (`b_pre_seam` /
   `b_post_seam`, which already coincide). Same floor as a splice seam
   (`SPLICE_MIN_PEAK_R` / `min_fill_correlation` — whichever the skip-gate
   already treated as live). Neighbouring fills may sit in a collar; that
   is the repaired timeline, and that is what we want to score.
4. **Pass** → queue a cut of `[gap_start, gap_end)`. **Fail** → leave the
   skip, stamp `collapse_declined: join_dead` on the row so a census can
   see it. Do not cut on admission alone; the confirm is the check the
   operator asked for.

The silent interior itself is not re-scanned. Scan already required it;
splice did not write it.

---

## 3. In-memory concatenate

Cuts are on interleaved `a_pcm.samples`. For each queued span, last-to-first:

```text
[ 0 , start )  +  optional join crossfade  +  [ end , len )
```

Drop `[start, end)` entirely. The remainder of the PCM shifts left by
`end - start` frames. Reuse `crossfade_ms` (default 10 ms) at the join —
the same helper family as `apply_seam_crossfade`, but A-to-A (no donor).
Negative `bridge_frames` (31/g7 ≈ −48) is a lock overlap of ~1 ms; it does
not change the A cut, which is still the full silent interior.

After all cuts, `a_pcm` is shorter. `pcm_secs` used by A3 / mux preflight
is this new length. A3 today would look like a clock bug; when collapse
fired, the warning (if any) must say the shortening is **intentional**
and list `collapsed_count` + total `cut_secs`. Mux preflight compares
collapsed PCM to **collapsed video** duration, not to the original
container.

Fills already in the buffer move with the tail. No patch is rewritten.
No later fill is re-placed.

---

## 4. Mux

`--wav`: write the collapsed PCM. Done.

`--mux`: pass the cut list (A-clock seconds, same decode clock splice
used). ffmpeg drops those ranges from video, then maps the collapsed
audio. Default `video_codec = copy` **cannot** serve this path —
intra-GOP stream-copy would snap to keyframes and reintroduce drift.
A collapse run forces a video re-encode (existing `repair.output.video_codec`
override still wins if the operator set one other than `copy`; if they
left `copy`, substitute a real encoder for this run only and say so on
stderr). Audio encode is unchanged.

Cuts convert frames → seconds with the same `sample_rate` as splice.
A2 (PTS vs sample-clock skew) is a known risk on sloppy remuxes; it is
not new here. Do not invent a second clock.

Zero collapses → today’s mux argv, including `-c:v copy` and `-shortest`.

PLAN.md’s “video frame alignment / visual sync” out-of-scope row is
**not** this. This is duration surgery so A/V stay on one clock after an
audio cut. No visual matching, no frame interpolation.

---

## 5. Report identity

New `GapPatchStatus` object (not `Skipped`, not `Patched`):

```json
{
  "collapsed": {
    "cut_frames": 53702,
    "cut_secs": 1.1188,
    "pre_lag_ms": -38.0,
    "post_lag_ms": -1160.3,
    "bridge_frames": -48,
    "join_r": 0.91
  }
}
```

`skipped_count` does not include these. New `collapsed_count` on
`PatchSummary`. Human line:

`collapsed introduced silence (cut 1.119 s / 53702 frames, join r=0.91)`

`--gap-listen`: no fill WAV for a collapsed gap. v1 writes nothing extra
(the join is the whole remaining program). Do not pretend a `_a_patched.wav`
exists.

Preview: `would_collapse: true` on a `shoulders_abut` skip; PCM unchanged.

---

## 6. 31/g7 under this plan

After the skip-gate: `shoulders_abut`, no −977 keep, A still contains the
1.12 s hole. After this Change: that skip is visited, concat-confirm
passes, the hole is drained, A PCM is ~1.12 s shorter, mux (if requested)
drops the same span from video. The stutter cannot exist — there is no
fill and no residual silence.

In-reach dual-fit fillable silence-splices never reach this pass (they
were patched). Program-quiet skips never collapse.

---

## 7. Non-goals

- Re-searching shoulders. Locks live on `shoulders_abut`.
- Collapsing any skip other than `shoulders_abut` that passed confirm.
- Cutting during characterize / before splice.
- Stretching or padding audio to keep the original container duration.
- Keeping `-c:v copy` on a collapse mux.
- New CLI. Collapse is the repair for this cell; `--no-collapse` is a
  later knob if an operator needs to keep A’s editorial pause.
- Pair-sequence / step-boundary classifier ([settled.md](settled.md) § D).
- `--gap-listen` join excerpts.
- Chunked PCM (full-file buffer is already resident).

---

## 8. Change shape

1. Domain helper `introduced_silence_join`: collars from post-splice A,
   Pearson vs B at the stored abut lock. `None` only when a collar is
   out of range.
2. After the splice loop in `PatchAudio::execute` (and `--patch-only`),
   walk skip outcomes. Eligible + confirm pass → queue `(gap_index,
   start, end)`. Last-to-first drain + A-to-A crossfade.
3. Restamp those outcomes `Collapsed { … }`; `PatchSummary.collapsed_count`.
4. Mux: if the cut list is non-empty, build a video drop-range filter
   and re-encode video; feed collapsed PCM on stdin as today. Duration
   preflight uses post-collapse lengths. `--wav` needs no mux change.
5. JSON + human format + A3 wording when collapse fired; preview
   `would_collapse`. Hole 2 detection sentence: skip-gate finds it,
   this Change closes A’s timeline.

---

## 9. Checklist

- [ ] `introduced_silence_join` unit tests: abut fixture passes; a
      ProgramQuiet-shaped skip is never admitted; a dead A-A join
      declines; out-of-range collar is `None` (no panic).
- [ ] Splice-then-collapse: two keeps + one eligible skip on a synthetic
      chirp — fills land at original indices, then the skip span is
      gone, PCM length decreases by exactly `gap_frames`, bytes on
      either side of the join match the pre-collapse shoulders.
- [ ] Last-to-first two-cut fixture: both interiors gone, no overlap
      corruption.
- [ ] `--repair-preview` does not shorten PCM; stamps `would_collapse`.
- [ ] `--patch-only` shortens in memory; report `collapsed`.
- [ ] `--wav` writes the shorter file; A3 names collapse when it fired.
- [ ] Mux with one collapse: video duration matches collapsed PCM
      within the existing preflight slack; a no-collapse run still
      stream-copies video.
- [ ] `Collapsed` reason: serde, formatters, JSON + repair-guide tables.
- [ ] 31/g7 production: `collapsed`, not `shoulders_abut` skip left in
      the file (licensed listen WAVs already on disk, 2026-08-18
      post-sweep stem). Depends on skip-gate already making it a skip.

---

## 10. Exit

- Every skip is visited after splice. Only `shoulders_abut` that pass
  the concat-confirm are cut.
- Fills are never reindexed. Collapse runs only on the post-splice
  buffer.
- 31/g7’s silent interior is absent from the output PCM; duration
  drops by the cut.
- `--mux` on a collapse run is A/V-aligned at the cut (not tail-trimmed
  by `-shortest` on a still-full video stream).
- `--wav` / `--patch-only` / preview behave as § 0.
- In-reach dual-fit fills and program-quiet skips are unchanged.
