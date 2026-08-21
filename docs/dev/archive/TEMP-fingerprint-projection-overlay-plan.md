# TEMP — Fingerprint post-projection overlay (restore X-records + scan alignment)

**Status:** implemented 2026-08-21. **One deliverable:** after `spec_to_fingerprint_summary`,
copy back the pass-1 `silence` / `contour` / `anchors` the rebuild currently drops, and stamp a
licensing-safe scan-alignment projection on `SourceMeta`. Same overlay pattern as
`equivalence_production`. No extra PCM, no production gate, no dump-shape fork on `--gap-listen`.

Parent: Fingerprint-unification 8g (lossy-by-projection on those three fields was filed as an 8g.5
fidelity item and never restored;
[TEMP-pipeline-perf-redesign-plan.md](TEMP-pipeline-perf-redesign-plan.md)).
Prompted by comparing the production patch pipeline to what a fingerprint dump can actually answer
(map-step is pair-level; editorial anchors are a live production mechanism whose dump record is
stripped). Does **not** copy `GapPatchStatus` / `align_adjustment_secs` / `anchor_seam_used` — those
are not in hand on the default dump path.

> **Verification rule.** A `file:line` reference belongs only in a checklist item that is about
> to be executed; design sections state the decision and its reason. Before coding, re-read
> `characterize_gaps_from_decode` (the `*fp = spec_to_fingerprint_summary` rebuild and the
> `equivalence_production` overlay that already runs *after* it), `spec_to_fingerprint_summary`
> (the `None` writes), `ScanAlignment` / `GapOffsetAgreement` on `GapReport`, and
> `production_dump_omits_unmeasured_option_fields` (the test this Change will flip for the three
> restored groups, not for the levels envelope or `seam_shape`).

---

## 1. Problem

A `--gap-fingerprints` dump is two passes. Pass 1 (`characterize_gaps` → `build_gap_fingerprint`)
**measures** the collar silence test, envelope contour / `has_anchor_seam_contour`, and the
editorial candidate lists. Pass 2 rebuilds every Full-tier gap from a `GapRepairSpec` via
`spec_to_fingerprint_summary`, which has no slot for those fields and writes `None`. The
measurement exists; it is thrown away.

That was allowed at the 8g dump flip because the three groups were labelled **X** (the gate
recomputes `build_gap_signature` and its own anchors; `golden_baseline` does not read them). The
restore was deferred to 8g.5, which then went after per-bracket cost and was itself deferred. A
later honesty pass (2026-08-03) changed zeros into omission — so a current dump simply lacks the
keys. Absence now reads as “unmeasured,” which is true of the *projection* and false of the
*run*.

Separately, map-step questions (pair 31 ≈ −1.15 s, pair 50 ≈ +2.8 s) are pair-level, and the dump
has no scan-alignment provenance. `GapReport.alignment` and `gap_offset_agreement` are already in
hand at `dump_gap_fingerprints`; they never reach `SourceMeta`. Per-gap `geometry.fill_offset_secs`
is the mapped offset, not the clip-level drift / confidence / silence-cross-check that produced it.

Two holes, one mechanism: **fields the spec cannot carry, already computed or already on the
report, overlaid after projection.**

---

## 2. Goal / non-goals

**Goal**

- Full-tier from-decode dumps emit the pass-1 `silence` / `contour` / `anchors` again.
- Every from-decode corpus (including a channel-layout refuse, which still ran scan) carries
  `source.alignment`: recommended offset, drift, clip windows + confidence, query-reference flag,
  overlap, silence-vs-chromaprint cross-check.
- Absence on a dump still means “this dump predates the overlay” (or the gap never reached
  pass 1), never “this gap has no anchors.”

**Non-goals**

- Production `GapPatchStatus`, `align_adjustment_secs`, `registration_residual_ms`,
  `dual_fit_used`, `anchor_seam_used`, `fit_path` of the winner. Default dump does not run
  `PatchAudio`; `listen.json` stays the sibling for disposition. Own TEMP if/when the run-mode
  cost is measured.
- `fill_level` / `normalize_gain` / `patch_anchors_used`. Splice-time or empty under default
  `fill_offset_mode = recommended`.
- Putting the three groups *into* `GapRepairSpec` or `MeasuredDetail`. The projector stays
  lossy; the overlay is the caller’s job, like `equivalence_production`.
- Restoring the levels envelope (`bin_ms` / `profile_db` / …) or `outcome.seam_shape` (was a
  hardcoded `""`). Different fields, different lies.
- Rewriting curated fixtures, goldens, or on-disk corpora. New dumps emit; old dumps omit.
- Contracts on the three restored groups. They keep their original keys. Alignment is new and
  confusable with `lag_decision` / `fill_offset_secs`, so *it* gets a `_contract`.
- Any repair-behaviour change. Sequence: palate cleanser, not ahead of the fill-placement /
  step-boundary chain. This overlay does not help the dump-only `--steps` census (JSON already
  on disk).

---

## 3. Placement (locked)

Overlay in `characterize_gaps_from_decode` (and the two other `SourceMeta`-from-report builders:
`characterize_gaps`, `refused_channel_mismatch_corpus`). Do **not** change
`spec_to_fingerprint_summary` to start emitting the three groups — that function is the pure
spec projection; tests that call it without an overlay must still see `None`.

Per rebuilt gap, the same shape as the existing equivalence overlay that already runs *after*
the assignment:

```text
keep silence / contour / anchors off the pass-1 fp
*fp = spec_to_fingerprint_summary(...)
put them back
stamp source.alignment from the report (once per corpus)
```

Gaps that skip the rebuild (no mapped B, zero-length, empty window) already keep pass-1 values;
the keep/restore is a no-op there. Do not special-case them.

Alignment lives on `SourceMeta`, not per gap. It is a pair fact. Helper:
`GapReport` → `CorpusScanAlignment` (new schema type). Call it wherever a corpus is built from
a report. Never copy `GapReport.video_a` / `video_b` (`PathBuf`) onto the fingerprint.

Rejected:

- **`MeasuredDetail` sidecar.** That pocket was added for measurements the spec cannot recover
  *and* that the rebuild would otherwise fabricate (`brackets`, `lag_decision`). These three
  are already on the fingerprint; they only need not to be clobbered. Alignment is corpus-level.
- **Recompute in pass 2.** Pass 1 already ran `list_anchor_candidates_a` / `build_gap_signature`.
  A second detection would be a second instrument.
- **Diagnostics-only.** They were default dump fields before the flip. Restoring them behind
  `--fingerprint-diagnostics` would keep the lie on the lean path.

---

## 4. Wire contract (locked)

### 4.1 Restored groups

Unchanged types (`SilenceProfile`, `ContourInfo`, `AnchorSet`). `Some` including
`AnchorSet { pre: [], post: [] }` means **measured, none found**. `None` / omitted key means
**unmeasured** (pre-overlay dump, or never reached pass 1). That is why they became `Option` in
2026-08-03; this Change finally uses the `Some` side.

Empty lists are not converted back to `None`.

### 4.2 `source.alignment`

New optional object, omitted on pre-overlay corpora and on media-free callers that pass no
report (synthetic fixtures that never had a scan). `Contracted<CorpusScanAlignment>`.

| Field | Source | Why |
|---|---|---|
| `recommended_offset_secs` | `ScanAlignment` | the map every gap’s `fill_offset_secs` starts from |
| `offset_drift_secs` | same | start-vs-end clip disagreement; the pair-level drift number |
| `offsets_consistent` | same | |
| `start_aligned` / `end_aligned` | same | |
| `query_reference_mode` | same | mapped-region fill gate applies |
| `start_overlap` | same | shared timeline; times only |
| `clips[]` | `AlignedClip` | `role`, A window start/end, `aligned`, `offset_secs`, `confidence` |
| `silence_cross_check` | `GapOffsetAgreement` | silence-structure vs chromaprint; `None` when scan did not compare |

Omit per-clip `video_*_decode_skips` and B-window timestamps — they do not answer map-step /
drift questions and B windows duplicate overlap. `ClipRole` needs serde on the projection type
(snake_case), not a derive on the domain struct if that would leak into other reports.

**`_contract` (new, this group only).** `not` must open with a shipped sibling key:

- measures: pair-level chromaprint scan alignment that mapped A onto B
- placement: scan-time, whole pair — not a gap shoulder
- window: the clip windows in `clips[]`
- not: `lag_decision` (per-gap shoulder lag at `b_mapped`); this is the map that produced `fill_offset_secs`

Cap 120 chars, enrolled in `SHIPPED_CONTRACTS`.

### 4.3 Absence / `--check` / goldens

- Pre-overlay dumps: keys absent, valid, no rewrite.
- New Full dumps: three groups present; `source.alignment` present; `not_measured` still empty
  when lag was measured (do **not** declare the restored groups — they are measured).
- `KNOWN_UNMEASURED` in the harness health check **keeps** the silence/contour/anchors paths.
  They still describe *legacy zeros*. A new dump with real values and an empty declaration is
  the healthy case that check already allows.
- `golden_baseline` axes do not include these fields. Leave curated fixtures and
  `curated.golden.json` untouched.
- Flip `production_dump_omits_unmeasured_option_fields`: Full gaps must have `Some` for the
  three groups; keep asserting the levels envelope and `seam_shape` stay omitted.

---

## 5. Tests (CI)

Colocated on the from-decode path (the existing `production_dump_omits_unmeasured_option_fields`
fixture is enough PCM):

| Test | Pins |
|---|---|
| Full-tier from-decode gap has `Some(silence/contour/anchors)` equal to the pass-1 values | restore, not a second detector |
| `anchors: Some({pre:[], post:[]})` stays `Some` | empty ≠ unmeasured |
| `spec_to_fingerprint_summary` alone still emits `None` for all three | projector stays lossy |
| `source.alignment` present on from-decode; `recommended_offset_secs` matches the fixture report | stamp |
| Refused channel-mismatch corpus still carries `source.alignment` | scan ran; refuse is later |
| Media-free / no-report builder leaves `source.alignment` omitted | unanswerable ≠ 0 |
| Alignment contract enrolled and within the length cap | existing contract tests pick this up once registered |

No new `[[test]]`. No licensed media. No golden refresh.

---

## 6. Docs (at implement)

- [gap-fingerprint.md](../gap-fingerprint.md) § Shape: the three groups are present on the
  from-decode dump again; absence is pre-overlay / never pass-1. New `source.alignment` row.
  § *Not measured* loses them from the “omitted on the production path” list (they are overlaid,
  not projected). Levels envelope and `seam_shape` stay on that list.
- Schema comments on `GapFingerprint::{silence,contour,anchors}` that currently say “None on
  the production projection path” — rewrite to “omitted by the spec projector; restored by the
  from-decode overlay.”
- This plan’s line in [README.md](../README.md) § Plans while active; drop when archived.

Do not rewrite the collapsed-bridge / pair-50 notes to pretend dumps always had these fields.

---

## 7. Steps

1. **Schema.** `CorpusScanAlignment` (+ clip / overlap / cross-check types as needed) on
   `SourceMeta`. Alignment contract in `contract.rs`. `ClipRole` serde only on the projection
   type.
2. **Helper + overlay.** `alignment_from_report(&GapReport) -> CorpusScanAlignment`. Keep /
   restore the three groups around the rebuild assignment. Stamp `source.alignment` on every
   report-backed corpus builder (from-decode, summary `characterize_gaps`, refused corpus).
3. **Tests.** §5, including the flipped omit-test. Confirm `cargo test -p clip-sync-repair
   --features calibration` (the characterize module) and harness `--check` unit tests still
   pass on synthetic corpora.
4. **Docs.** §6.

---

## 8. Out of scope / parked

| Item | Why parked |
|---|---|
| Production pass-1 copy-in (`GapPatchStatus`, winner path, `registration_residual_ms`) | Not in hand on the default dump; needs a run-mode + cost TEMP. BACKLOG 33/g17 already names the winner-path hole. |
| `fill_level` / `normalize_gain` | Splice-only; corpus-rewrite-after-patch was refused for listen. |
| `patch_anchors_used` | Empty under default `recommended` offset mode; write-only pass 2. |
| Levels envelope / `seam_shape` | Different omission; not this deliverable. |
| Contracts on silence / contour / anchors | Original keys; C3-adjacent if a reader still misreads them. |
| Feeding `--steps` from `source.alignment` | That census is dump-only over JSON already on disk. |
