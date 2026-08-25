# JSON output contract — v1

Authoritative field-by-field contract for `--format json` output of both CLIs. **Frozen as v1 (2026-06-10).** Additive revisions: optional `end_clip_anchor` on analyzer/repair alignment reports (2026-06-17); optional `video_b_window_*` on clip entries when B differs from A; optional `audio_timeline_skew` on repair `GapReport` (2026-06-19); additive scan-recipe echo keys `min_gap_ms`, `silence_hold_ms` (effective), and `absolute_silence_rms` (normalized) on repair `GapReport` (2026-07-31); optional `fill_geometry` on repair `PatchSummary` (2026-08-17).

Any change to field names, types, optionality, or nesting is a contract revision: update this document, regenerate the golden fixtures, and call the revision out explicitly in the changelog/commit.

| Producer | Payload | Rust type | Golden test |
|----------|---------|-----------|-------------|
| `clip-sync --format json` | Alignment report (stdout) | `clip_sync::AlignmentReport` (`crates/clip-sync/src/application/report.rs`) | `clip-sync-cli/tests/cli_output.rs` → `full_surface_alignment_json_golden`, fixture `tests/fixtures/full_surface_alignment.json` |
| `clip-sync-repair --format json` | Repair report (stdout) | `RepairJsonOutput` (`crates/clip-sync-repair/src/infrastructure/cli/output.rs`) | output-module test `full_surface_repair_json_golden`, fixture `tests/fixtures/full_surface_repair.json` |

The JSON shape is owned by application-layer report DTOs, not by domain types — refactoring the domain model must not change this contract. JSON is emitted pretty-printed (`serde_json::to_string_pretty`). Field order follows the struct declaration order documented below; consumers should not depend on key order.

General conventions:

- All time values are seconds as JSON numbers (`f64`); suffix `_secs`.
- **Optional ⇒ key absent** means the key is omitted entirely when there is no value (`skip_serializing_if`).
- **Nullable ⇒ key present** means the key is always present and is `null` when there is no value.
- On process failure nothing is printed to stdout (see [error-mapping.md](error-mapping.md)); the JSON document is only produced on exit 0.

---

## Analyzer report (`clip-sync`)

Top-level object: **AlignmentReport**.

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `clips` | array of [ClipMatch](#clipmatch) | always | One entry per extracted clip pair, in window order |
| `start_aligned` | bool | always | Start clip matched above the confidence threshold |
| `end_aligned` | bool \| null | always | `null` when only one clip was extracted (no separate end window) |
| `recommended_offset_secs` | number \| null | always | Best single offset (seconds to add to video A's timeline to align with B); `null` when no clip aligned or offsets disagree under `require_consistent_offsets`. With two clips, may be confidence-weighted fusion when both offsets are near their median even if `offsets_consistent` is false. After dual-anchor high-rate refinement, recomputed from updated per-clip offsets when `high_rate_recommended_refusion` is on (default). |
| `offsets_consistent` | bool | always | All aligned clip pairs report the same offset within tolerance (0.5 s) |
| `offset_drift_secs` | number | **absent when unavailable** | End-clip offset minus start-clip offset; only when both clips aligned |
| `start_overlap` | [TimelineOverlap](#timelineoverlap) \| null | always | Shared timeline region implied by the start clip match; `null` when not aligned |
| `high_rate_refinement` | [HighRateRefinement](#highraterefinement) | **absent when feature off/not run** | Native-rate hold-out FFT correction details |
| `offset_verification` | [OffsetVerification](#offsetverification) | **absent when feature off** | Hold-out lag-0 verification details |
| `offset_ambiguous_mod_secs` | number | **absent when not periodic** | Repeat period **T** (seconds) when start-clip repetition makes offset ambiguous mod **T** |
| `alignment_mode_used` | `"symmetric"` \| `"queryreference"` | **absent on the legacy symmetric path** | How this run chose its algorithm (query-reference feature) |
| `query_localization` | [QueryLocalization](#querylocalization) | **present only in query-reference mode** | Where the short clip sits on the long file |
| `end_clip_anchor` | `"file_tail"` \| `"shared_timeline"` | **absent in query-reference and single-clip runs** | End-clip placement policy used for this symmetric multi-clip run |

### ClipMatch

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `label` | `"start"` \| `"interior"` \| `"end"` | always | Window position class |
| `window_start_secs` | number | always | Clip window start on video A's timeline |
| `window_end_secs` | number | always | Clip window end |
| `aligned` | bool | always | Matched above the configured confidence threshold |
| `offset_secs` | number \| null | always | Per-clip offset estimate; `null` when not aligned |
| `confidence` | number | always | Fingerprint match confidence in [0, 1]; `0.0` is the contract for "clips did not match" (not an error) |
| `video_a_decode_skips` | integer | always | Corrupt decode packets skipped extracting this clip from A |
| `video_b_decode_skips` | integer | always | Same for B |
| `video_b_window_start_secs` | number | **absent when same as A** | B-side clip window start when paired planning differs from A |
| `video_b_window_end_secs` | number | **absent when same as A** | B-side clip window end when paired planning differs from A |
| `repetition` | [Repetition](#repetition) | **absent when `check_clip_repetition` off** | Internal-repeat diagnostics |

### Repetition

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `a` | [RepetitionFinding](#repetitionfinding) \| null | always | Finding for video A's clip; `null` when no repeat detected |
| `b` | [RepetitionFinding](#repetitionfinding) \| null | always | Same for B |

### RepetitionFinding

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `lag_secs` | number | always | Positive seconds between repeated content |
| `confidence` | number | always | Repeat match confidence |
| `items_count` | integer | always | Fingerprint items supporting the repeat |

### TimelineOverlap

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `video_a_start_secs` | number | always | Overlap start on A's timeline |
| `video_a_end_secs` | number | always | Overlap end on A's timeline |
| `video_b_start_secs` | number | always | Overlap start on B's timeline |
| `video_b_end_secs` | number | always | Overlap end on B's timeline |
| `shared_length_secs` | number | always | Overlap length (0 when windows do not intersect) |

### QueryLocalization

Present only when `alignment_mode_used` is `"queryreference"`. Describes where the short *query* clip sits relative to the long *reference* file. The result is always framed in repair roles: `mapped_region`, `clip_on_a_*`, and `clip_on_b_*` use A/B timelines regardless of which file was longer. Scripts should prefer the parent `recommended_offset_secs` / `start_overlap` for placement math.

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `anchor_ref_secs` | number | always | Position on the **longer (reference)** file where the short clip's `t = 0` aligns (= `mapped_region.video_a_start` when A is reference, else `video_b_start`). **Deprecated alias:** `anchor_a_secs` (accepted on deserialize for backward compatibility). |
| `clip_on_a_start_secs` | number | always | Clip start on **A's** timeline (= `mapped_region.video_a_start_secs`) |
| `clip_on_a_end_secs` | number | always | Clip end on **A's** timeline |
| `clip_on_b_start_secs` | number | always | Clip start on **B's** timeline (usually 0 when B is the query) |
| `clip_on_b_end_secs` | number | always | Clip end on **B's** timeline |
| `mapped_region` | [TimelineOverlap](#timelineoverlap) | always | Shared region implied by the anchor + query duration (A/B-oriented) |
| `search_stride_secs` | number | always | Coarse search stride actually used (may widen if the window cap was hit) |
| `winning_window_start_secs` | number | always | **Reference**-timeline bounds of the winning coarse search window (A when A is longer, B when B is longer) |
| `winning_window_end_secs` | number | always | |
| `confidence` | number | always | Localization confidence in [0, 1] (×0.5 when ambiguous) |
| `ambiguous` | bool | always | A competing anchor scored comparably (repeated content) |
| `windows_scored` | integer | always | Coarse windows fingerprinted (after any stride widening) |
| `skip_reason` | string | **absent on success** | Why query mode produced no localization (then no recommended offset) |

### HighRateRefinement

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `segment_start_secs` | number | always | Start-anchor hold-out segment start (A timeline) |
| `segment_length_secs` | number | always | Segment length |
| `adjustment_secs` | number | always | Start-anchor native-rate correction (seconds) |
| `correlation_peak` | number | always | FFT cross-correlation peak at start anchor |
| `applied` | bool | always | At least one anchor correction was applied |
| `skipped` | bool | always | Refinement did not run |
| `skip_reason` | string | **absent when not skipped** | Why refinement was skipped |
| `end_anchor` | [HighRateAnchorRefinement](#highrateanchorrefinement) | **absent on single-hold-out runs** | End-window native-rate refinement when `num_clips ≥ 2` |
| `refined_drift_secs` | number | **absent when unavailable** | End − start clip offset after high-rate updates |

When dual-anchor high-rate runs and `high_rate_recommended_refusion` is enabled (default), `recommended_offset_secs` is recomputed from the updated clip offsets (fusion / preference), not `prior_recommended + adjustment_secs`.

### HighRateAnchorRefinement

Present on `high_rate_refinement.end_anchor` for symmetric multi-clip runs.

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `segment_start_secs` | number | always | Hold-out segment start on A timeline |
| `segment_length_secs` | number | always | Segment length |
| `offset_before_secs` | number | always | End-clip offset before this anchor correction |
| `adjustment_secs` | number | always | Native-rate correction applied to end clip |
| `correlation_peak` | number | always | FFT cross-correlation peak |
| `applied` | bool | always | Correction was applied |
| `skipped` | bool | always | This anchor did not run |
| `skip_reason` | string | **absent when not skipped** | Why this anchor was skipped |

### OffsetVerification

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `window_a_start_secs` | number | always | Hold-out window on A |
| `window_a_end_secs` | number | always | |
| `window_b_start_secs` | number | always | Hold-out window on B (A window + offset) |
| `window_b_end_secs` | number | always | |
| `confidence` | number | always | Lag-0 fingerprint match confidence |
| `verified` | bool | always | Offset independently confirmed |
| `skipped` | bool | always | Verification did not run |
| `skip_reason` | string | **absent when not skipped** | Why verification was skipped |
| `candidates_tried` | integer | **absent when skipped or zero** | Hold-out windows scored before reporting |
| `independent_offset_secs` | number | **absent when parallel recheck did not run** | Calendar-parallel PCM recheck offset estimate |
| `parallel_recheck_delta_secs` | number | **absent when parallel recheck did not run** | `recommended_offset_secs - independent_offset_secs` |
| `verify_inconclusive` | bool | **absent when false** | Option A scored a pass but periodic gating rejected it |

---

## Repair report (`clip-sync-repair`)

Top-level object: **RepairJsonOutput**.

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `scan` | [GapReport](#gapreport) | always | Gap scan results |
| `patch` | [PatchSummary](#patchsummary) | **absent in report-only runs** | Splice outcomes when patching ran |

### GapReport

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `video_a` | string (path) | always | Input A |
| `video_b` | string (path) | always | Input B |
| `track_compatibility` | [TrackCompatibility](#trackcompatibility) \| null | always | `null` when B could not be opened or has no decodable track |
| `overlap` | [TimelineOverlap](#timelineoverlap) \| null | always | From the alignment start clip; `null` when alignment failed |
| `alignment` | [AlignmentReport](#analyzer-report-clip-sync) | always | Embedded analyzer report (same contract as above) |
| `gaps` | array of [Gap](#gap) | always | Silent regions detected in A |
| `gap_equivalence` | array of [GapEquivalenceVerdict](#gapequivalenceverdict) | when non-empty | Per-gap silence-character classification (advisory), index-parallel to `gaps`; input to the `skip_equivalent_gaps` drop |
| `gap_offset_agreement` | [GapOffsetAgreement](#gapoffsetagreement) \| null | always | Present when `scan_both` ran and both files had silence intervals |
| `decode_chunk_secs` | integer | always | Decode chunk size used during sequential scan |
| `min_gap_ms` | integer | always | Minimum silent-run length (ms) that produced this gap list |
| `silence_hold_ms` | integer | always | **Effective** hold (`silence_hold_blocks × scan_block_ms`), not the configured TOML `silence_hold_ms` |
| `scan_block_ms` | integer | always | Analysis block size for silence-run detection |
| `silence_peak_fraction` | number | always | Peak-fraction threshold used for silence classification |
| `absolute_silence_rms` | number | always | Absolute RMS floor as **normalized** amplitude in `[0, 1)` (default ≈ `0.001007`, not the CLI 0–32767 display scale) |
| `limit_fill_to_mapped_region` | bool | always | When true (default in query-reference mode), gaps outside mapped clip coverage are reported but not fillable |
| `audio_timeline_skew` | [AudioTimelineSkew](#audiotimelineskew) \| null | always | Present when gap scan measured PTS vs decoded-sample clock; `null` when not measurable (e.g. seek-based scan fallback). Human report emits a warning when `delta_secs > 1.0` |
| `program_levels` | [ProgramLevels](#programlevels) | when the scan populated it | Whole-program A/B level readings. **Report-only** — no gain is derived from it |

### ProgramLevels

Whole-program level readings derived from the scan's per-block RMS timelines. Capture only: nothing
in the patch path reads them and no fill gain is derived from them.

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `a` | [ProgramLevelContext](#programlevelcontext) | always | A-side context |
| `b` | [ProgramLevelContext](#programlevelcontext) | when B produced a level timeline | Absent when B was not scanned — which includes a readable B with **no alignment offset**, since the B scan only runs when both exist |
| `offset` | [AbLevelOffset](#ableveloffset) | when enough blocks paired | The A↔B level relationship |

#### AbLevelOffset

The paired estimator: for each A block that is **active** (RMS above the recipe's absolute silence
floor) and **steady** (both neighbours within 3 dB), the mapped B block at `offset_secs` is looked up
and `rms_db_B − rms_db_A` recorded. Deliberately *not* two per-side summaries subtracted: A is the
damaged file, so its dropouts depress any independent rank statistic and the subtraction would report
A's damage as a level offset. Pairing measures the same audio twice, so dropouts, pads and edit
differences drop out of the pairing instead of biasing it.

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `offset_db` | number | always | Median paired difference. **B minus A** — positive means B is louder |
| `spread_db` | number | always | MAD of the differences about `offset_db`. **Does one scalar exist?** Tight ⇒ a real master offset; wide ⇒ dynamics/compression differ or the alignment is wrong. This, not `n_pairs`, is the trust gate |
| `p10_db` / `p90_db` | number | always | Nearest-rank deciles — asymmetry and bimodality a single MAD hides |
| `n_pairs` | integer | always | Accepted pairs (fewer than 50 ⇒ the whole object is absent) |
| `n_active` | integer | always | A blocks above the active threshold, **before** the steadiness filter. Same predicate over the same timeline as `a.n_active`, so the two always agree — a cross-check, not a second quantity |
| `n_steady` | integer | always | Active *and* steady A blocks — the pairing denominator. `n_pairs / n_steady` is B-coverage yield (low ⇒ B covered little of A); `n_steady / n_active` is what the steadiness filter cost, the only way to see whether the 3 dB tolerance suits this material |
| `first_pair_secs` / `last_pair_secs` | number | always | A-timeline span the offset was measured over |
| `channels_comparable` | bool | always | **False when A/B channel counts differ** — block RMS is taken over interleaved samples, so the offset then carries a correlation-dependent constant and is not a pure gain |
| `chunks` | array of [AbLevelOffsetChunk](#ableveloffsetchunk) | when ≥ 2 chunks qualified | Per-bucket medians: *is the offset constant?* |

Decoder gain (AC-3 dialnorm, DRC, decode-path scaling) is included **deliberately** — a fill inherits
it.

#### AbLevelOffsetChunk

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `start_secs` / `end_secs` | number | always | Bucket bounds on A's timeline (600 s buckets) |
| `offset_db` | number | always | Median paired difference inside the bucket |
| `n_pairs` | integer | always | Pairs in the bucket (≥ 20; smaller buckets are not emitted) |

#### ProgramLevelContext

Per-side context. **Never subtract two of these** — that is the design the paired offset replaced.

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `mid_db` | number | when any block was active | Median dB of active blocks |
| `floor_db` | number | when any block was quiet-but-not-digital-zero | Median dB between digital zero and the active threshold |
| `n_active` | integer | always | Active block count (blocks shorter than half the nominal scan block are excluded from every population here) |

### AudioTimelineSkew

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `pts_secs` | number | always | Packet PTS mapped to seconds at the observation point |
| `sample_clock_secs` | number | always | Sequential decoded-sample clock at the same point |
| `delta_secs` | number | always | Absolute difference \|PTS − sample_clock\| (maximum observed during scan) |

Gap positions in `gaps[]` use the **decoded-sample clock**. When `delta_secs` is large, times may not match `ffmpeg silencedetect` or container timestamps. See [cli-output.md](cli-output.md) § Timeline / duration warnings.

**Human-only diagnostics (not in JSON):** overlap-start warning (derived from `overlap.video_a_start_secs`) and patched-PCM vs container warning (after write mode) appear as `Warning:` lines on stdout only. Parse `scan.overlap` and compare patched WAV duration to container metadata if scripting those checks.

### TrackCompatibility

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `a_channels` / `b_channels` | integer | always | Channel counts |
| `a_sample_rate` / `b_sample_rate` | integer | always | Sample rates (Hz) |
| `channels_match` / `rate_match` | bool | always | Per-property comparison |
| `verdict` | `"identical"` \| `"compatible"` \| `"mismatch"` | always | Whether splice fill is allowed (`compatible` = resample B) |

### Gap

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `video_a_start_secs` | number | always | Gap start on A's timeline |
| `video_a_end_secs` | number | always | Gap end |
| `video_b_start_secs` | number \| null | always | Mapped position in B; `null` when alignment produced no offset |
| `video_b_end_secs` | number \| null | always | |
| `b_has_energy` | bool | always | B has audio energy at the mapped position (potential fill source) |

### GapOffsetAgreement

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `silence_based_offset_secs` | number | always | Offset derived from silence structure |
| `alignment_offset_secs` | number | always | Chromaprint alignment offset |
| `delta_secs` | number | always | Absolute difference |
| `agrees` | bool | always | Delta within configured tolerance |

### PatchSummary

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `patched_count` | integer | always | Gaps actually spliced into A's PCM |
| `patched_marginal_count` | integer | always | Patches in warn tier (`confidence: marginal`) |
| `skipped_count` | integer | always | Planned but skipped during splice |
| `not_planned_count` | integer | always | Excluded at plan time |
| `not_applied_count` | integer | when > 0 | **Bug indicator.** Gaps the seam gate approved whose splice then failed — A is unchanged across them. Never emitted on a healthy run; any non-zero value is a defect report, not a routine outcome |
| `donor_relation` | string | when residual measured on ≥1 gap | `same_master` \| `mixed` \| `diff_capture` — inferred from informative-floor fraction. **A cancellation rate, not a provenance finding — see note below** |
| `patch_anchors_used` | array of PatchAnchorReport | when non-empty | Offset anchors built from pass-1 successes and reused by the anchored retry |
| `fill_geometry` | object | since 2026-08-17, when patched from a request | Border-geometry **settings** this run patched with: `border_standoff_secs`, `fill_border_search_secs`, `min_border_discovery_secs`, `fill_seam_search_secs`. Settings, not per-gap effective widths — each is clamped against the material available at a given gap. Exists so a two-arm differential is attributable to the flag that changed rather than to the output directory |
| `gaps` | array of [GapPatchOutcome](#gappatchoutcome) | always | Per-gap outcomes in scan order |

> **`donor_relation` claims more than it measures.** It is derived purely from the fraction of gaps
> whose residual floor showed cancellation at the *nominal* alignment, so it reports how often A and B
> cancelled — not where they came from. In particular `diff_capture` does **not** establish that the
> sources differ: a same-master pair yields it whenever it drifts beyond the ±`residual_lag_secs`
> (default 10 ms) probe radius, or when its gaps are silent enough that no floor reference window is
> found. Read the three values as *mostly cancelled* / *partly* / *never*, and do not branch on them as
> a provenance decision — the field gates nothing in the repair pipeline and is emitted as a diagnostic.

### GapPatchOutcome

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `a_start_secs` / `a_end_secs` | number | always | Gap bounds on A |
| `status` | [GapPatchStatus](#gappatchstatus) | always | Outcome (externally tagged enum) |
| `tags` | [GapTags](#gaptags) | always | Vocabulary tags derived at patch time (see [gap-repair-guide.md](gap-repair-guide.md) § Vocabulary) |
| `residual` | object | when measured | Full [`SeamResidualVerdict`](#seamresidualverdict) (gate active or `measure_residual`) |
| `fill_level` | object | spliced gaps, when `measure_fill_level` | [FillLevelCheck](#filllevelcheck) — how loud the written fill is against the A around it |

### SeamResidualVerdict

Same-master confirmation at the decision seam: how deeply B cancels A (least-squares residual, dB)
at the chosen placement, versus a **floor** measured the same way on a clean reference window at the
*nominal* alignment. Headroom (`chosen − floor`) is the reading the residual gate uses; the floor is
what makes it a measurement rather than a tautology.

Per-side (`_pre` / `_post`) values describe the two gap shoulders. On multichannel media the
scalars summarize the **worst-headroom** channel while `informative` follows the **best-cancelling
(min-floor)** channel — deliberately different channels, so do not read one as explaining the other.

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `chosen_pre_db` / `chosen_post_db` | number | always | Residual at the chosen placement, dB. Non-finite when that side could not be measured (serialized as `null`) |
| `floor_pre_db` / `floor_post_db` | number | always | Nominal-alignment floor for that side, dB |
| `floor_source_pre` / `floor_source_post` | string | always | Where the floor's reference window came from — `border` (immediate window past the standoff was usable), `walked` (border was quiet, reference walked outward), `none` (no energetic in-coverage window found). **This is what tells an absent `floor_*_db` apart from a measured one** |
| `informative` | bool | always | Every *governing* side established cancellation (`floor_db ≤ residual_floor_ok_db`). Unmeasured and sourced-NaN (`probe_non_finite`) sides are ignored; `false` when no side governs |
| `placement_slide_frames` | integer | when non-zero | `\|chosen_delta − nominal_delta\|` |
| `max_lag_frames` | integer | when non-zero | Unified lag radius this verdict was measured within (`0` = reach check disabled) |
| `uninformative_pre` / `uninformative_post` | string | when that side has no usable floor | Per-side reason — `no_reference_window` \| `probe_non_finite` \| `floor_above_ok_db`. Diagnostic detail; the combined value on [GapTags](#gaptags) is authoritative |

The gate abstains when `!informative` **or** when the placement slid past `max_lag_frames`
(`placement_slide_frames > max_lag_frames`) — headroom is not meaningful outside the lag radius.
Because it is a disjunction, a verdict can be `informative: true` and still carry
`residual_uninformative: beyond_lag_reach`, which is why that field is **not** the negation of
`informative`.

Combined `residual_uninformative` (on [GapTags](#gaptags)) prefers a governing
`floor_above_ok_db` over a coexisting per-side `probe_non_finite` — matching what failed
`informative`. `probe_non_finite` is the combined reason only when nothing governed.

The four reasons are not interchangeable. `no_reference_window` and `probe_non_finite` are
abstentions ("we could not measure here"), `beyond_lag_reach` is the gate's deliberate refusal, and
`floor_above_ok_db` is a **measurement** — the floor was taken and sits above the threshold.

> **`floor_above_ok_db` is not a provenance finding**, and carries the same caveat as
> `donor_relation`'s `diff_capture`: it means the same-master regime was not established *at this
> seam*, not that B is a different master. A same-master pair yields it whenever it drifts beyond the
> probe radius, or when the reference window is too quiet for cancellation to go deep. It is also
> **threshold-relative** — `residual_floor_ok_db` is configurable, so the variant only means
> something against the run's own settings.

### FillLevelCheck

Record-only: the written fill's loudest bin measured against the A shoulders either side of the
gap. Never changes a verdict, and there is deliberately no threshold — a veto's false positive is
an unrepaired hole, so the number is collected for calibration first. Present only on gaps whose
fill was actually spliced (a failed splice reports nothing) and only when `measure_fill_level` is
on (the default; `--no-measure-fill-level` skips the pass).

Because it is measured *during* the splice, it appears on write-mode runs (`--wav` / `--mux`) and on
`--patch-only` — which runs the same patch and writes no file — but never under `--repair-preview`,
which returns before execute. For a corpus sweep prefer `--patch-only`: a run whose output is thrown
away should not be able to fail on the output.

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `pre_shoulder_db` | number | when there is A before the gap | Interleaved RMS of the pre-gap A shoulder, dBFS. Absent when the available room is under half the shoulder width (head of media) — a sliver is declined, not measured |
| `post_shoulder_db` | number | when there is A after the gap | Interleaved RMS of the post-gap A shoulder, dBFS. Same half-width rule at the tail |
| `reference_db` | number | always | The **louder** shoulder — what the fill is judged against |
| `peak_bin_db` | number | always | Level of the fill's loudest bin, dBFS |
| `peak_delta_db` | number | always | `peak_bin_db - reference_db`. **Diagnostic only — do not rank or threshold on it** (see below); read as the interior half of the pair with `edge_delta_db` |
| `reference_at_floor` | bool | always | The reference bottomed out at the −120 dB floor: every measurable shoulder is digital silence, usually a neighbouring dropout inside the shoulder window. Both deltas are then artifacts — **exclude these rows from calibration** |
| `peak_bin_index` | integer | always | Which bin that was, from the start of the fill |
| `head_bin_db` / `tail_bin_db` | number | always | Level of the fill's first / last measured bin, dBFS — what meets A at each seam |
| `edge_delta_db` | number | always | `min(head_bin_db, tail_bin_db) - reference_db`: how far the fill sits above its neighbourhood **at both seams**. The field to read; see below for what it does and does not say |
| `bins` | integer | always | Bins measured (a trailing partial bin under half width is dropped) |
| `bin_ms` | number | always | Bin width the peak was taken over (100 ms) |

The statistic is a per-bin **peak**, not a whole-fill aggregate: on the gaps this was calibrated
against, a single 100 ms bin well above the surrounding program was what made a patch sound worse
than the unrepaired A, and an average over the fill hides exactly that.

**`peak_delta_db` does not order audibility.** Ear labels over the 39-pair corpus (2026-08-06) put
the corpus maximum at +24.34 dB in the *clean* group, along with both +15 dB rows, while the fill
heard as too loud read +15.93 dB — 0.6 dB from a clean one matching it on every other field. Ten
labels in it has never ordered anything, and a corpus sorted by it yields a listen list of correct
repairs. What separated them was where the excess lived, which is what `edge_delta_db` reports:

| shape | reading | what it was |
|-------|---------|-------------|
| uniformly hot fill | `edge_delta_db` ≈ `peak_delta_db`, both large | the +15.93 dB clip heard as too loud — above A's ongoing material at the seams too, so the ear has a direct comparison |
| loud event inside a quiet gap | `peak_delta_db` large, `edge_delta_db` ≈ 0 | the +24.34 dB corpus maximum, heard as clean — meets A at both seams, loud only in its middle, which is what the missing content was |

**Amended 2026-08-12 — what made that clip uniformly hot was our own gain.** The +15.93 dB row
carries `normalize_gain = 3.88` (+11.78 dB) and renders clean with `--no-normalize`, so it was
normalize boosting a fill against a non-local reference, not a donor-level mismatch (open work:
[BACKLOG.md](../BACKLOG.md) § *Fill normalize boosts against a non-local reference*). Two
consequences for reading this section. The table's conclusion **strengthens**: a uniform gain error
is exactly the shape `edge_delta_db` catches and `peak_delta_db` cannot. But the **numbers are
gain-contaminated** — every delta here is measured on `gained_fill` against ungained shoulders, so
each row is inflated by its own gap's boost (0–12 dB, and not recorded at all before 2026-08-12).
Backing this one out gives ≈4.15 dB. Do not compare these deltas across gaps without dividing out
`normalize_gain` first.

**Extended 2026-08-25 — this is a cohort, not a one-off, and the division separates the two kinds of
loud fill.** Over the 51-pair corpus (308 spliced gaps) 199 (65%) are left at unity by the boost-only
policy. Of the 8 boosted gaps reading hotter than +3 dB at the seams, **6 fall to at-or-below their
own shoulders once the gain is divided out** — 45/g4 (−0.89), 5/g6 (−1.39), 19/g13 (−2.66), 45/g5
(−0.72), 15/g3 (−6.68), 31/g7 (−3.00) — while every donor-driven row above +3 dB is at or near unity
gain. The control is decisive: the correctly-restored gunshot (24/g46, +9.83 dB, the case that
refuted cutting on `edge_delta_db`) carries `normalize_gain` **0.00 dB**. Three of the six are
ear-confirmed audible and a fourth is audible but confounded; the lowest confirmed is 45/g5 at only
+5.61 dB of gain, so the audible threshold sits well below `max_fill_gain_db`. Full sizing, ear
labels and the mechanism reconstruction:
[dev/TEMP-fill-normalize-local-reference-2026-08-25.md](dev/TEMP-fill-normalize-local-reference-2026-08-25.md).
Gap indices there and here are **0-based** (dump-filename convention); the roll-up CSV's
`Gap1Based` column collides with real, differently-numbered gaps if quoted directly.

The 2026-08-06 / 2026-08-08 ear labels also include **mixed A/B orientation**.
`2026-08-08-edge-listen` covers pairs 24, 8, 12, 7, 31 — pairs **7, 8 and 12 are swapped**, so 3 of
its 5 labels are of the reverse pairing and do not transfer ([archive/TEMP-fill-placement-registration-plan.md](dev/archive/TEMP-fill-placement-registration-plan.md)
§0.0). Do not quote "the lowest clip ever labelled clean" as a bound until those are re-labelled.
The oriented fill-level roll-up is `gap-files/2026-08-13-repeat-terms/fill-level-rollup.csv` (317
rows). Roll-ups written before 2026-08-24 have no `normalize_gain` column — join it from the run's
per-pair JSON (`status.patched.normalize_gain` on 317/317); the script emits it as `NormalizeGain`
since then, `null` on reports predating the field.

`edge_delta_db` takes the **quieter** seam, so it takes both seams sitting high to read as hot — the
same conservatism as `reference_db` taking the louder shoulder. On a one-bin fill the head, tail and
peak are the same bin and it degenerates to `peak_delta_db`.

Over the same labels `edge_delta_db` separated every fill heard as too loud from every fill heard as
clean, and it is rare in the way a useful field should be: on 227 patched gaps only 11% are above
their reference at the seams at all, and 1.8% clear +4 dB.

**It is still not a gate, and the reason is a specific counter-example.** One fill measured +9.83 dB
— the largest in the corpus — and was heard as *correct*: a gunshot in a conversation, where being
far louder than the neighbourhood is exactly what the missing content was. The field says how much
louder, never whether that is wrong. What conditioned it on that set was registration quality, from
[GapPatchStatus](#gappatchstatus): `pre_correlation` was 0.998 on the correct one and 0.29–0.47 on
the three heard as too loud. `edge_delta_db ≥ 5` with `pre_correlation < 0.6` selects exactly those
three out of 227 rows — recorded here as the open hypothesis it is, resting on a single
counter-example.

Three limits before reading a number. The seam **crossfade is ignored** (the fill's edge bins are
measured as pure fill, while what lands in A is a blend over `--crossfade-ms` — negligible at the
10 ms default, growing with it), and `edge_delta_db` is built on exactly those bins, so it is the
more crossfade-sensitive of the two deltas. `reference_at_floor` rows must be dropped before any
histogram. And **this pass measures level, not placement**: two of the ten labelled clips carried
damage it cannot see — a fill that duplicated the shoulder before the missing content, and one
running seconds longer than the gap — both at unremarkable deltas. A clean `fill_level` is not a
clean repair. See [BACKLOG.md](../BACKLOG.md) § *Fill placement*.

A third field, `reference_spread_db` (the reference shoulder's loudest bin minus its median), was
recorded here briefly on 2026-08-06 and **removed the same day**: two rounds of labels showed no
separation, and the one direction it showed inverted its own hypothesis — the clip heard as too loud
had the most uniform neighbourhood in the set. Reports written in that window carry it; nothing
reads it.

### GapTags

Orthogonal gap classification tags. `plan_skip_reason`, `fit_path`, and `signature_mode` are omitted from JSON when not applicable.

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `plan_kind` | string | always | `below_scan_floor` \| `unfillable` \| `not_planned` \| `fillable` |
| `plan_skip_reason` | string | when plan not fillable | Same values as [GapFillSkipReason](#gappatchstatus) |
| `patch_tier` | string | always | `high` \| `marginal` \| `anchor_trusted` \| `dead_zone` \| `hard_skip` \| `structure_fail` \| `not_applicable` |
| `seam_shape` | string | always | `balanced` \| `asymmetric_post` \| `asymmetric_pre` \| `symmetric_weak` \| `not_applicable` |
| `fit_path` | string | fit gaps only | `baseline_only` \| `boundary_grid` |
| `signature_mode` | string | fit gaps only | `bool` \| `energy` |
| `residual_band` | string | when residual measured | `cancels` \| `correlates_only` \| `no_floor` |
| `residual_uninformative` | string | when the residual has no usable headroom reading | Why — `beyond_lag_reach` \| `no_reference_window` \| `probe_non_finite` \| `floor_above_ok_db`. Same values and combine rule as [`SeamResidualVerdict`](#seamresidualverdict)'s per-side fields; **read it with `residual_band`**, never alone |
| `anchor_seam_used` | bool | when true | Editorial anchor bracket won (not scan-throat placement) |
| `anchor_bracket_move_frames` | integer | when `anchor_seam_used` and > 0 | Total frame displacement from scan-refined baseline |
| `dual_fit_used` | bool | when true | A3 dual-fit (G6) rescued this gap after the bracket search exhausted, not ordinary bracket-search fitting |
| `normalize_gain` | number | always (since 2026-08-12) | Loudness gain applied to this fill before the splice. `1.0` = untouched donor level |

### GapPatchStatus

Externally tagged (serde default): exactly one of the following keys.

- `{"patched": {"pre_correlation": number, "post_correlation": number, "align_adjustment_secs": number, "structure_trusted": bool, "confidence": "high"|"marginal", "gap_start_adjust_frames": number, "gap_end_adjust_frames": number, "residual_db": number, "floor_db": number, "headroom_db": number, "anchor_seam_used": bool, "anchor_bracket_move_frames": number, "dual_fit_used": bool, "normalize_gain": number, "fit_terms": {"total": number, "structure_combined": number, "waveform_min": number, "late_start_penalty": number, "anchor_penalty": number, "repeat_penalty": number, "repeat_pre": number, "repeat_post": number}, "registration_residual_ms": number}}`

Optional `residual_db`, `floor_db`, `headroom_db` (worst-side scalars) are present when residual was measured; omitted otherwise. `anchor_seam_used` and `dual_fit_used` are **always serialized, including when `false`** (changed 2026-08-12; both were omitted-when-false before that). `false` on `dual_fit_used` means the A3 dual-fit path (G6) did not carry this gap — it ran and declined, or was never reached — and has never meant the path was disabled; the old absence was read that way at least once. `false` on `anchor_seam_used` means baseline-throat placement. When `dual_fit_used` is `true`, this gap was rescued by dual-fit after the bracket search exhausted — see [gap-fill-modes.md](gap-fill-modes.md) § Dual-fit rescue. `anchor_bracket_move_frames` is still omitted when 0. Full per-side detail is in `GapPatchOutcome.residual`.

`normalize_gain` (added 2026-08-12) is the loudness gain the splice applied to this fill: `1.0` when `normalize_fill` is off, when dual-fit produced the fill (never normalized), or when the **boost-only** policy declined to touch a fill already at or above A's border level; `> 1.0` when a quiet fill was raised (bounded by `max_fill_gain_db`). It is **never below 1.0** — runs before 2026-08-12 could attenuate down to `-max_fill_gain_db`, and this field is how you tell the two vintages apart. It exists to make `fill_level` readable: those fields are measured on the *gained* fill, so without the gain a loud fill and a boosted quiet one are indistinguishable, and `peak_delta_db` / `edge_delta_db` cannot be compared across a gain-policy change. **Absence means the run predates the field, not unity** — dumps in `gap-files/` written before 2026-08-12 carry no `normalize_gain` at all, and a roll-up must hold those rows out rather than default them. See [pipeline.md](pipeline.md) § Fill normalization.

`fit_terms` (added 2026-08-12) breaks the unified fit objective into the terms that produced the applied
placement, so a placement can be explained rather than only located. `total` is the objective evaluated at that
placement; `structure_combined` and `waveform_min` are the two weighted terms (`total` = `structure_weight ×
structure_combined` + `waveform_weight × waveform_min`, minus the three penalties); `late_start_penalty`,
`anchor_penalty` and `repeat_penalty` are each recorded as the **positive amount subtracted**, so a zero means
that prior did not fire *at this placement*. `repeat_pre` / `repeat_post` (added 2026-08-13) are the
repeat-window Pearson pair that `repeat_penalty` reduced — diagnostic inputs, not terms of `total`. They are
always present when `fit_terms` is, including when `repeat_penalty` is `0.0`, so a winner-side zero is readable
as a calibration statement (`0.31` against a `0.55` threshold) rather than only as "the branch did not fire".
They are omitted with `fit_terms` when the objective did not run. A zero window (`repeat_window_frames == 0`)
does not measure, and reports `0.0` / `0.0`.

**A zero does not mean the prior is doing no work, and the two zeros do not mean the same thing.** On the
oriented 2026-08-13 run (`gap-files/2026-08-13-repeat-terms/`, 210 `fit_terms` rows) both are `0.0` on every
winner, for unrelated reasons. `anchor_penalty` is a **configuration** zero: the prior is only constructed
under `--fill-offset anchored-retry` (second pass), with `fill_anchor_search_prior_weight > 0` (default `0.0`)
and a non-empty patch-anchor table, so on any default run it is structurally absent rather than declined.
`repeat_penalty` is a **behavioural** zero: the term is enabled at stock config and its correlations are
measured on every candidate, but its thresholds are not reached — pairs sit far under 0.55, and the weak-seam
guard is anti-correlated with a high repeat, so lowering the threshold would not have fired the live arm
either ([gap-fill-modes.md](gap-fill-modes.md) § Fit unified search). `repeat_pre` / `repeat_post` are how to
read that zero; dumps from 2026-08-12 lack the pair. Note these terms are reported for the **winning**
placement only — a penalty that demoted a rival below the winner leaves no trace in the report at all, so
"zero on every gap" bounds what the prior did to the *chosen* placement and says nothing about the candidates
it removed. And `repeat_penalty: 0.0` does **not** mean the repeat correlation had no effect on this gap: the
same hard-coded `0.55` threshold is separately a stop condition for short-bracket extension, which is a veto
this weight neither scales nor disables, and which changes the delivered fill length rather than any reported
term. Open leftovers: [BACKLOG.md](../BACKLOG.md) § *Repeat-at-seam prior is inert on every winner*.

Two asymmetries matter when reading them. `late_start_penalty` is **one field for two
directions**: a *forward* slide is scaled by `fill_fit_late_start_penalty_scale` (default `1.0`), a *backward*
one by `fill_fit_early_start_penalty_scale` (default `0.0`, i.e. unpenalized), so at stock config a non-zero
value here always means a forward slide — but under a non-default early scale it does not, and the direction
has to be read off the sign of `align_adjustment_secs` instead. And `waveform_min` is a **min over the two
shoulders**, so a placement can improve it by degrading the better shoulder. The terms are measured at the
placement that was actually applied — after the end search, the fine polish and the gap snap — not at any
intermediate winner.

**`total` is not the score the search ranked on**, despite being the same formula. The gap snap can change the
fill length after ranking, which moves the bracket end and so both `structure_post` and the placement the
structure term is scored against; on a snapped gap the applied placement is a different candidate from the
chosen one. So `total` is comparable **across gaps and across runs** — which is what a before/after diff of a
scoring change needs — but not to any score the search itself reported.

`fit_terms` is **omitted** when the placement did not come from the unified objective: dual-fit / silence-splice
rescues (`dual_fit_used: true`) and the legacy structure-only gate path. Absence on a bracket patch means the
run predates the field.

`align_adjustment_secs` is the signed slide of the chosen fill start off mapped nominal — the placement the unified fit search won, **not** a measured A→B lag. A companion `waveform_adjustment_secs` ("additional slide from waveform search after structure match") was removed 2026-08-12: the unified search assigns the placement once, so the field was identically `0.0` on every patched gap and read as evidence that placement had been decided in the structure domain.

`registration_residual_ms` (S1, added 2026-08-20) is `applied_ms − shoulder_lag_ms`: the applied placement
(`align_adjustment_secs * 1000`, or `0.0` for dual-fit — see below) minus an independent seam-local
correlation-peak read of the same shoulder, taking whichever of pre/post has the smaller `|residual|` among
shoulders whose peak `r` clears the same `min_fill_correlation`/`fill_absolute_floor` bar the gate itself uses.
It is **record-only** — no gate reads it, and it does not affect the verdict, same as `fill_level` never
affecting the verdict. Omitted when neither shoulder's independent read is confident, or when the run predates
the field. On the dual-fit path (`dual_fit_used: true`) it is computed from the same `pre_lag`/`post_lag`/
`pre_seam_r`/`post_seam_r` `try_dual_fit` already produced — no new sweep — against `applied_ms = 0.0`, matching
`align_adjustment_secs`'s own hardcoded `0.0` for that path (dual-fit places each shoulder independently, so
there is no single rigid "applied lag" to compare against).

`structure_trusted` is `true` only when `fill_mode` was `gate` and structure scores skipped the waveform gate. Under default `fill_mode = fit`, it is always `false`. `confidence` is `marginal` when the patch passed the warn tier (`min_fill_correlation - fill_marginal_margin` ≤ `min(pre, post)` < `min_fill_correlation`). `gap_*_adjust_frames` record how far the winning A gap edges moved from the pre-search refined bracket (fit mode).
- `{"skipped": {"reason": <GapPatchSkipReason>}}`
- `{"not_planned": {"reason": <GapFillSkipReason>}}`
- `{"not_applied": {"reason": <GapPatchNotAppliedReason>}}`

**GapPatchNotAppliedReason** — object forms `{"destination_out_of_range": {"gap_start_frame", "gap_end_frame", "a_frames"}}` | `{"destination_empty_or_inverted": {"gap_start_frame", "gap_end_frame"}}` | `{"fill_shorter_than_gap": {"fill_frames", "gap_frames"}}`.

`not_applied` is categorically different from `skipped`: `skipped` is the seam gate *declining* a gap, which is a routine, expected outcome. `not_applied` is the gate having **approved** the gap and the splice then failing to write it — A is byte-identical across the gap while a repair was reported as planned. Every arm is a geometry defect upstream of the splice, so any occurrence is a bug to file, with the frame counts in the reason identifying which invariant broke. `--gap-listen` writes no `_a_patched.wav` for these gaps, and `patched_count` excludes them.

**GapPatchSkipReason** — string `"b_extract_failed"` | `"boundary_alignment_failed"` | `"aligned_segment_out_of_range"` | `"zero_length_gap"` | `"program_quiet"` (reserved — not emitted by production patch; D11 program-quiet is an analyzer/plan-time label, see [gap-fill-modes.md](gap-fill-modes.md) § Program-quiet (D11)), or object forms `{"correlation_below_threshold": {"pre_correlation", "post_correlation", "min_correlation", "best_attempt"?}}` (`best_attempt`: `{pre_correlation, post_correlation, source}` when a later placement beat the reported scores) | `{"residual_headroom_exceeded": {"pre_correlation", "post_correlation", "headroom_db", "floor_pre_db", "floor_post_db", "margin_db"}}`.

**GapFillSkipReason** — string `"not_fillable"` | `"track_layout_mismatch"` | `"track_compatibility_unavailable"` | `"outside_reference_coverage"` | `"already_matches_reference"` (the `skip_equivalent_gaps` equivalence drop — mutual/ambient silence, nothing to repair) | `"gap_not_selected"` (`--only-gaps` / `--skip-gaps` subset filter).

### GapEquivalenceVerdict

Per-gap silence-character classification (advisory; scan report `gap_equivalence[]` and the `equivalence_diagnostic` /
`equivalence_production` blocks on `--gap-fingerprints`). See [gap-vocabulary.md](dev/gap-vocabulary.md)
§ Silence-character pre-gate.

On `--gap-fingerprints` dumps only, both blocks also carry a `_contract` object stating what the
verdict measures, where, and what it is not. It is reader metadata — nothing reads it, it is absent on
older corpora, and it never appears on the scan report's `gap_equivalence[]`. See
[dev/gap-fingerprint.md](dev/gap-fingerprint.md) § *`_contract`*.

On a `--gap-fingerprints` dump the same gap carries **two** of these. `equivalence_production` is the one
production acts on; `equivalence_diagnostic` is a second opinion kept for calibration, and it
disagrees on ~2 % of gaps (always biased toward `drop`). It is **not** a "fine" reading of the same
measurement — the axis is production vs diagnostic, not resolution (corrected 2026-07-31).
Read `equivalence_production` unless you are
specifically calibrating the two — [dev/gap-fingerprint.md](dev/gap-fingerprint.md)
§ *`equivalence_diagnostic` vs `equivalence_production`*.

| Field | Type | Presence | Notes |
|-------|------|----------|-------|
| `class` | string | always | `repairable_dropout` (keep) · `shared_silence` / `ambient_quiet` (drop) · `not_evaluated` |
| `drop` | bool | always | Whether `class` resolves to "remove from fill plan" (the two silence classes) |
| `a_gap_rms_db` | number | when measured | A gap-interior RMS (dBFS) |
| `noise_floor_db` | number | when measured | Recording's own noise floor (self-calibrating reference) |
| `a_below_noise_db` | number | when both present | `a_gap_rms_db − noise_floor_db` — the dropout signal (dropout ≲ −`dropout_margin_db`) |
| `donor_silence_fraction` | number | when B mapped | B occupancy over the nominal span (occupied ⇒ `< donor_silence_thresh`) |

---

## Revision procedure

1. Change the report DTOs (`application/report.rs`) or repair output types — never re-derive `Serialize` on domain types.
2. Run the ignored fixture generators and review the diff:
   `cargo test -p clip-sync-cli write_full_surface_alignment_golden -- --ignored`
   `cargo test -p clip-sync-repair write_full_surface_repair_golden -- --ignored`
3. Update this document (bump the version marker for breaking changes; additive fields may stay v1 if they are optional-absent).
4. Land doc + fixture + code in the same commit.

Crate/binary semver (`Cargo.toml`, git tags) is independent — see [development.md](dev/development.md) § Versioning and release.
