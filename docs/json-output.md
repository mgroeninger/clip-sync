# JSON output contract — v1

Authoritative field-by-field contract for `--format json` output of both CLIs. **Frozen as v1 (2026-06-10).** Additive revisions: optional `end_clip_anchor` on analyzer/repair alignment reports (2026-06-17); optional `video_b_window_*` on clip entries when B differs from A; optional `audio_timeline_skew` on repair `GapReport` (2026-06-19).

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
| `gap_offset_agreement` | [GapOffsetAgreement](#gapoffsetagreement) \| null | always | Present when `scan_both` ran and both files had silence intervals |
| `decode_chunk_secs` | integer | always | Decode chunk size used during sequential scan |
| `scan_block_ms` | integer | always | Analysis block size for silence-run detection |
| `silence_peak_fraction` | number | always | Peak-fraction threshold used for silence classification |
| `limit_fill_to_mapped_region` | bool | always | When true (default in query-reference mode), gaps outside mapped clip coverage are reported but not fillable |
| `audio_timeline_skew` | [AudioTimelineSkew](#audiotimelineskew) \| null | always | Present when gap scan measured PTS vs decoded-sample clock; `null` when not measurable (e.g. seek-based scan fallback). Human report emits a warning when `delta_secs > 1.0` |

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
| `patched_count` | integer | always | Gaps spliced |
| `patched_marginal_count` | integer | always | Patches in warn tier (`confidence: marginal`) |
| `skipped_count` | integer | always | Planned but skipped during splice |
| `not_planned_count` | integer | always | Excluded at plan time |
| `donor_relation` | string | when residual measured on ≥1 gap | `same_master` \| `mixed` \| `diff_capture` — inferred from informative-floor fraction |
| `gaps` | array of [GapPatchOutcome](#gappatchoutcome) | always | Per-gap outcomes in scan order |

### GapPatchOutcome

| Field | Type | Presence | Meaning |
|-------|------|----------|---------|
| `a_start_secs` / `a_end_secs` | number | always | Gap bounds on A |
| `status` | [GapPatchStatus](#gappatchstatus) | always | Outcome (externally tagged enum) |
| `tags` | [GapTags](#gaptags) | always | Vocabulary tags derived at patch time (see [gap-repair-guide.md](gap-repair-guide.md) § Vocabulary) |
| `residual` | object | when measured | Full [`SeamResidualVerdict`](#seamresidualverdict) (gate active or `measure_residual`) |

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
| `anchor_seam_used` | bool | when true | Editorial anchor bracket won (not scan-throat placement) |
| `anchor_bracket_move_frames` | integer | when `anchor_seam_used` and > 0 | Total frame displacement from scan-refined baseline |
| `dual_fit_used` | bool | when true | A3 dual-fit (G6) rescued this gap after the bracket search exhausted, not ordinary bracket-search fitting |

### GapPatchStatus

Externally tagged (serde default): exactly one of the following keys.

- `{"patched": {"pre_correlation": number, "post_correlation": number, "align_adjustment_secs": number, "waveform_adjustment_secs": number, "structure_trusted": bool, "confidence": "high"|"marginal", "gap_start_adjust_frames": number, "gap_end_adjust_frames": number, "residual_db": number, "floor_db": number, "headroom_db": number, "anchor_seam_used": bool, "anchor_bracket_move_frames": number, "dual_fit_used": bool}}`

Optional `residual_db`, `floor_db`, `headroom_db` (worst-side scalars) are present when residual was measured; omitted otherwise. `anchor_seam_used` and `anchor_bracket_move_frames` are omitted when false / 0 (baseline-throat placement). `dual_fit_used` is omitted when false (ordinary bracket-search fit); when `true`, this gap was rescued by the A3 dual-fit path (G6) after the bracket search exhausted — see [gap-fill-modes.md](gap-fill-modes.md) § Dual-fit rescue. Full per-side detail is in `GapPatchOutcome.residual`.

`structure_trusted` is `true` only when `fill_mode` was `gate` and structure scores skipped the waveform gate. Under default `fill_mode = fit`, it is always `false`. `confidence` is `marginal` when the patch passed the warn tier (`min_fill_correlation - fill_marginal_margin` ≤ `min(pre, post)` < `min_fill_correlation`). `gap_*_adjust_frames` record how far the winning A gap edges moved from the pre-search refined bracket (fit mode).
- `{"skipped": {"reason": <GapPatchSkipReason>}}`
- `{"not_planned": {"reason": <GapFillSkipReason>}}`

**GapPatchSkipReason** — string `"b_extract_failed"` | `"boundary_alignment_failed"` | `"aligned_segment_out_of_range"` | `"zero_length_gap"` | `"program_quiet"` (reserved — not emitted by production patch; D11 program-quiet is an analyzer/plan-time label, see [gap-fill-modes.md](gap-fill-modes.md) § Program-quiet (D11)), or object forms `{"correlation_below_threshold": {"pre_correlation", "post_correlation", "min_correlation", "best_attempt"?}}` (`best_attempt`: `{pre_correlation, post_correlation, source}` when a later placement beat the reported scores) | `{"residual_headroom_exceeded": {"pre_correlation", "post_correlation", "headroom_db", "floor_pre_db", "floor_post_db", "margin_db"}}`.

**GapFillSkipReason** — string `"not_fillable"` | `"track_layout_mismatch"` | `"track_compatibility_unavailable"` | `"outside_reference_coverage"`.

---

## Revision procedure

1. Change the report DTOs (`application/report.rs`) or repair output types — never re-derive `Serialize` on domain types.
2. Run the ignored fixture generators and review the diff:
   `cargo test -p clip-sync-cli write_full_surface_alignment_golden -- --ignored`
   `cargo test -p clip-sync-repair write_full_surface_repair_golden -- --ignored`
3. Update this document (bump the version marker for breaking changes; additive fields may stay v1 if they are optional-absent).
4. Land doc + fixture + code in the same commit.

Crate/binary semver (`Cargo.toml`, git tags) is independent — see [development.md](development.md) § Versioning and release.
