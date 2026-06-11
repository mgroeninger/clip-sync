# Temporary plan: query-reference alignment (short clip vs long video)

> **Status:** Not started — **blocked** on prerequisite plans (see [Cross-plan sequencing](#cross-plan-sequencing)). Archive to `docs/archive/query-reference-alignment-plan.md` when shipped.

**Problem:** `clip-sync` and `clip-sync-repair` assume two recordings of roughly the same event with symmetric multi-clip fingerprint windows (default 15m start + end on long media). When **B is much shorter than A** (an excerpt, phone clip, or partial export), `clip_windows_with_options` yields **different window counts** → `align_extracted_pair` fails with `clip count mismatch`. Even when counts accidentally match, windows are anchored to each file’s start/end, so content that appears **mid-timeline** on the long file is never searched.

**Goal:** Support **query-reference localization**: treat the shorter input as a query fingerprint, search the longer reference timeline for the best match position, emit a global offset and **mapped coverage region**, then reuse the existing repair gap-scan / patch pipeline within that region.

**Primary use case (repair):**

- **A** = long recording with silent dropouts
- **B** = shorter clip with clean audio for one segment of the same event
- Find where B sits on A’s timeline → scan A for gaps → patch from B where B has coverage

**Workspace split:** Localization algorithm, domain types, and `AlignVideos` routing live in **`crates/clip-sync`**. Repair scan/report wiring in **`crates/clip-sync-repair`**. Optional standalone “locate only” output in **`crates/clip-sync-cli`**. **Do not** add a new crate or binary — extend existing tools (see [Extend vs new crate](#extend-vs-new-crate)).

---

## Cross-plan sequencing

**Prerequisites — land first:**

| Plan | Why it blocks this plan |
|------|-------------------------|
| [archive/media-session-redesign-plan.md](archive/media-session-redesign-plan.md) | Shipped 2026-06-11: `MediaSession` `&mut self`; `reset_io` removed; `MediaExtent`; scan policy in `application/media_scan.rs` |
| [TEMP-verification-hardening-plan.md](TEMP-verification-hardening-plan.md) | Verification retries up to 3 candidates; `candidates_tried` on `OffsetVerification`; `clip_with_label` / `start_clip()` helpers; `alignment_fixtures` adoption; chirp test-role split |

**Already shipped (rebase on these, do not re-litigate):**

- [archive/output-error-contract-plan.md](archive/output-error-contract-plan.md) — JSON contract v1 frozen; new fields require explicit revision (see [JSON contract revision](#json-contract-revision)).
- [archive/layer-purity-plan.md](archive/layer-purity-plan.md) — domain types carry **no serde**; JSON DTOs live in `application/report.rs`.

**Rebase checklist before Q0:** `&mut MediaSession`, `MediaExtent`, report DTO split, shipped verification retry path, `clip_with_label`, `json-output.md` revision procedure.

---

## Current codebase baseline

Audit against the tree **after** media-session redesign and verification hardening land. Pre-requisite snapshot: 2026-06-11 (layer-purity shipped; query mode not started).

| Area | Path | Current state (post-prereqs) | Target phase |
|------|------|------------------------------|--------------|
| **Symmetric align** | `application/align_videos.rs` | Multi-clip extract + fingerprint; requires equal window counts | Q2 (branch, keep default path) |
| **Clip planning** | `domain/policies.rs` | `clip_windows_with_options` — per-file `MediaExtent::effective()` | Q1 (query mode bypasses symmetric plan on reference) |
| **Clip count gate** | `align_videos.rs` `align_extracted_pair` | Hard error on window count mismatch | Q2 (skip when query mode) |
| **Chromaprint match** | `infrastructure/chromaprint/aligner.rs`, `matching.rs` | `match_fingerprints` handles unequal fingerprint lengths (substring match) | Q1 (reuse per search window) |
| **Sequential scan** | `ports.rs` `scan_mono_buckets` (`&mut self`); `symphonia/extract.rs` `scan_mono_buckets_with_state` | EOF-driven production scan on Symphonia; trait defaults in `application/media_scan.rs` | Q1 (**first production mono scan caller**) |
| **PCM discover** | `application/offset_refinement.rs` | `refine_offset_around_prior`, `pcm_discover_offset` on pre-extracted clips | Q1 (`refine_query_anchor` extracts haystack, then reuses PCM search) |
| **Hold-out verify** | `application/offset_verification.rs` | Retry up to 3 candidates; `candidates_tried`; `MediaExtent` on input | Q2 (delegate to shipped path; mapped-region placement) |
| **High-rate refine** | `application/high_rate_refinement.rs` | `MediaExtent` on input; no `reset_io` | Q2 (segment inside mapped region) |
| **AlignmentResult** | `domain/alignment.rs` | `start_overlap`, clips, `clip_with_label` / `start_clip()` | Q1–Q2 (add `query_localization`, `alignment_mode_used`) |
| **JSON report** | `application/report.rs` | `AlignmentReport` mirrors domain; no serde on domain | Q2–Q3 (add `QueryLocalizationReport`, contract revision) |
| **Repair align** | `clip-sync-repair/.../aligner.rs` | `align_with_defaults` only | Q3 |
| **Gap scan** | `clip-sync-repair/.../scan_gaps.rs` | Full A timeline; `overlap = alignment.start_overlap`; `&mut` sessions | Q3 (mapped region overlap + optional region filter) |
| **Gap fill** | `clip-sync-repair/.../gap_fill.rs` | Fill any gap where B has energy; no region gate | Q3 (`GapFillSkipReason::OutsideReferenceCoverage`) |
| **Patch** | `clip-sync-repair/.../patch_audio.rs` | Per-gap structure match — unchanged | — |
| **CLI repair** | `clip-sync-repair/.../cli/args.rs` | No query-mode flags | Q3 |
| **CLI analyzer** | `clip-sync-cli/.../cli/args.rs` | No locate-only mode; headline via `start_clip()` | Q4 (optional) |
| **Corpus** | `tests/corpus/`, repair `tests/` | Symmetric pairs only | Q4 (generated tier for query case) |

**Naming:** **Query** = shorter file (typically B in repair). **Reference** = longer file (typically A in repair). Offset convention unchanged: seconds to **add to A’s timeline** to align with B (`b = a + offset`).

---

## Decisions

Locked before implementation. Change only with an explicit plan revision.

| Topic | Decision |
|-------|----------|
| **Product shape** | Extend **`clip-sync` + `clip-sync-repair` + optional `clip-sync-cli` flags** — no new crate/binary. |
| **Repair I/O** | Keep `VIDEO_A` = gaps (long), `VIDEO_B` = reference (short clip). Auto-detect query mode from **effective** durations; allow override via config/CLI. |
| **Mode selection** | `AlignmentMode::Auto \| Symmetric \| QueryReference`. **Auto** (default): use query mode when `dur_short / dur_long < query_min_duration_ratio` (effective durations) **or** symmetric clip window counts differ. Ratio default **0.5**. |
| **Which file is query** | Always the **shorter** effective duration (tie → symmetric). Repair convention: short B, long A. |
| **Search strategy (v1)** | **Coarse-to-fine:** (1) fingerprint full query clip; (2) sliding windows on reference via `scan_mono_buckets`; (3) Chromaprint match per window; (4) cluster candidates by anchor on A; (5) PCM refine top candidate(s); (6) optional high-rate + hold-out verify on mapped region. |
| **Coarse window length** | `L = min(query_prepared_duration, clip_length, reference_remaining)` — same prep pipeline as discovery (`prepare_clip_for_fingerprint`). |
| **Coarse stride** | Configurable `query_search_stride_secs` (default **60**). Stride ≤ window length; minimum stride **15** s. |
| **Anchor definition** | `anchor_a_secs` = A timeline position where query **t = 0** aligns. `recommended_offset_secs = -anchor_a_secs` (equivalent to existing sign convention). |
| **Mapped region** | A: `[anchor_a_secs, anchor_a_secs + query_duration_secs]` clamped to `extent_a.effective()`. B: `[0, query_duration_secs]` clamped to `extent_b.effective()`. Exposed as `TimelineOverlap` on `QueryLocalization.mapped_region`. |
| **Overlap field** | In query mode, set `AlignmentResult.start_overlap` from **mapped region** (not start-clip window). Repair `GapReport.overlap` follows. |
| **ClipMatch report** | Query mode emits **one synthetic `ClipMatch`** (label **`Start`**) describing the winning search window on A + match confidence — aligns with `start_clip()` headline selection after verification hardening. |
| **Ambiguity** | Reuse `select_best_segment` cluster ambiguity (×0.5 confidence). Surface `query_localization.ambiguous: bool`. Do **not** hard-fail; warn in human output. |
| **Verification** | Delegate to shipped `apply_offset_verification` (retry up to 3 candidates, `candidates_tried`). Hold-out **inside mapped region** on A (`holdout_window_candidates` with discovery window = winning coarse window; `pick_duration` from `MediaExtent::effective()`). Skip if mapped region shorter than `clip_length`. |
| **High-rate refine** | Run when enabled, **after** coarse+PCM localize, **before** verification — same order as symmetric path. Segment from mapped region, not file start. No `reset_io` (adapter recovers internally). |
| **Gap scan scope** | Default **full A timeline** (gaps outside B coverage still reported). Add repair config `limit_fill_to_mapped_region` default **true** — gaps outside region stay in report but are not fillable (`GapFillSkipReason::OutsideReferenceCoverage`). |
| **Alignment failure** | No match above threshold → `recommended_offset_secs: None`, scan A anyway (same as today). Exit **0** in repair report mode. |
| **Memory / perf** | Coarse search fingerprints windows incrementally; do **not** retain all window PCM. Cap scored windows per file (`query_max_windows_scored`, default **500**) with coarser stride fallback + warn. |
| **Scan re-entrancy** | `scan_mono_buckets` callback must **not** call back into the session (documented on `MediaSession` after media-session redesign). Fingerprint inside callback only. |
| **try_all_tracks** | Query mode: try all decodable pairs; pick highest localization confidence (same pattern as `align_best_track_pair`). |
| **min clip length** | Query clip must satisfy existing `MIN_CLIP_LENGTH` (60s) after prep, else skip query mode with clear error/skip reason. If duration-less open is relaxed (media-session Phase 4), fail here when effective duration is still unknown. |
| **Symmetric path** | Unchanged when mode is `Symmetric` or Auto chooses symmetric. |
| **Phasing** | Q0 spike → Q1 lib core → Q2 lib integrate + refine/verify → Q3 repair → Q4 CLI + corpus → archive. |
| **Layer purity** | New domain types in `domain/` — **no serde**. Report DTOs + human formatters in `application/report.rs`. |
| **User-facing report (query mode)** | Lead with **where the clip sits on the long file** — start/finish on A (and B) — not offset/overlap jargon. Offset and `TimelineOverlap` remain in JSON and for symmetric-mode compatibility; de-emphasize or omit offset in default human output on this path. |
| **Report labels (human)** | Prefer: `Clip on A: 45:00 – 53:00` (or `Match on video A: …`). Optional verbose: `Clip on B: 0:00 – 8:00`. Avoid leading with `offset -2700s` or `Overlap:` in default human repair/analyzer output. |
| **Report labels (JSON)** | Friendly aliases on `QueryLocalizationReport`: `clip_on_a_start_secs` / `clip_on_a_end_secs` (mirror `mapped_region.video_a_*`); keep `recommended_offset_secs` and `start_overlap` for scripts. |

---

## Config

### Library (`AlignConfig.alignment`)

Add fields to **`AlignmentConfig`** in `crates/clip-sync/src/application/config.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlignmentMode {
    #[default]
    Auto,
    Symmetric,
    QueryReference,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignmentConfig {
    // ... existing fields ...

    /// How to align when inputs differ greatly in length.
    #[serde(default)]
    pub mode: AlignmentMode,

    /// In Auto mode, treat shorter file as query when dur_short / dur_long < this ratio.
    #[serde(default = "default_query_min_duration_ratio")]
    pub query_min_duration_ratio: f64,

    /// Coarse search stride on the reference timeline (seconds).
    #[serde(default = "default_query_search_stride_secs")]
    pub query_search_stride_secs: f64,

    /// Maximum coarse windows to fingerprint per run (stride widens if exceeded).
    #[serde(default = "default_query_max_windows_scored")]
    pub query_max_windows_scored: u32,

    /// Minimum Chromaprint confidence to accept a localization candidate.
    #[serde(default = "default_query_min_match_score")]
    pub query_min_match_score: f32,

    /// Number of top coarse candidates to PCM-refine (1 = winner only).
    #[serde(default = "default_query_refine_top_k")]
    pub query_refine_top_k: u32,
}

fn default_query_min_duration_ratio() -> f64 { 0.5 }
fn default_query_search_stride_secs() -> f64 { 60.0 }
fn default_query_max_windows_scored() -> u32 { 500 }
fn default_query_min_match_score() -> f32 { 0.3 }
fn default_query_refine_top_k() -> u32 { 1 }
```

Add **`AlignmentConfig::validate()`** (or extend `AlignConfig::validate()` to call it): require `query_search_stride_secs >= 15.0`, `0.0 < query_min_duration_ratio <= 1.0`, `query_refine_top_k >= 1`. Today `AlignConfig::validate()` only calls `clip.validate()` — wire alignment rules there.

### Repair (`RepairConfig`)

Add to `crates/clip-sync-repair/src/infrastructure/config.rs`:

```rust
pub struct RepairConfig {
    // ... existing ...

    /// When query-reference alignment is used, only treat gaps inside the mapped B coverage
    /// region as fillable (gaps outside are still reported).
    #[serde(default = "default_true")]
    pub limit_fill_to_mapped_region: bool,
}
```

Repair default align config: set `alignment.mode = Auto` explicitly in `default_repair_align_config()` (document in repair TOML example).

### CLI flags

**`clip-sync-repair`** (`args.rs`):

| Flag | Maps to |
|------|---------|
| `--query-reference` | `alignment.mode = QueryReference` |
| `--symmetric-align` | `alignment.mode = Symmetric` |
| `--query-stride <SECS>` | `query_search_stride_secs` |
| `--no-limit-fill-region` | `limit_fill_to_mapped_region = false` |

**`clip-sync-cli`** (Phase Q4, optional):

| Flag | Maps to |
|------|---------|
| `--query-reference` / `--symmetric-align` | same as repair |
| `--query-stride <SECS>` | same |

TOML example (`tests/fixtures/repair.toml`):

```toml
[alignment]
mode = "auto"
query_search_stride_secs = 60
query_min_duration_ratio = 0.5

[repair]
limit_fill_to_mapped_region = true
```

---

## Types

### Domain (`crates/clip-sync/src/domain/`)

New types in **`domain/alignment.rs`** or **`domain/query_localization.rs`** (re-exported from `domain/mod.rs`). **No serde derives** (layer purity).

```rust
/// How this alignment run chose its algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignmentModeUsed {
    Symmetric,
    QueryReference,
}

/// Result of searching a short query clip against a long reference timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryLocalization {
    /// A timeline position where query t=0 aligns.
    pub anchor_a_secs: f64,
    /// Same as `mapped_region.video_a_start_secs` — explicit alias for human-oriented output.
    pub clip_on_a_start_secs: f64,
    /// Same as `mapped_region.video_a_end_secs`.
    pub clip_on_a_end_secs: f64,
    /// Same as `mapped_region.video_b_start_secs` (usually 0).
    pub clip_on_b_start_secs: f64,
    /// Same as `mapped_region.video_b_end_secs`.
    pub clip_on_b_end_secs: f64,
    /// Shared region implied by anchor + query duration (same shape as TimelineOverlap).
    pub mapped_region: TimelineOverlap,
    /// Coarse search stride actually used (may widen if window cap hit).
    pub search_stride_secs: f64,
    /// A timeline bounds of the winning coarse window.
    pub winning_window_start_secs: f64,
    pub winning_window_end_secs: f64,
    pub confidence: f32,
    pub ambiguous: bool,
    /// Coarse windows fingerprinted before cap/stride adjustment.
    pub windows_scored: u32,
    pub skip_reason: Option<String>,
}

// AlignmentResult — add:
pub alignment_mode_used: Option<AlignmentModeUsed>,
pub query_localization: Option<QueryLocalization>,
```

`compute_mapped_region(anchor_a, query_duration, extent_a, extent_b) -> TimelineOverlap` clamps to **`MediaExtent::effective()`**, not raw container duration.

### Report DTOs (`crates/clip-sync/src/application/report.rs`)

Mirror domain fields on serializable report types; extend `AlignmentReport`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AlignmentModeUsedReport { Symmetric, QueryReference }

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueryLocalizationReport { /* same fields as QueryLocalization */ }

// AlignmentReport — add:
#[serde(skip_serializing_if = "Option::is_none")]
pub alignment_mode_used: Option<AlignmentModeUsedReport>,
#[serde(skip_serializing_if = "Option::is_none")]
pub query_localization: Option<QueryLocalizationReport>,
```

Add **`format_query_localization_lines(...)`** alongside `format_high_rate_refinement_lines` / `format_offset_verification_lines`. Export on `clip_sync` facade for CLI and repair.

### Repair fill skip reason (`clip-sync-repair/src/domain/patch_result.rs`)

Gaps outside mapped region are **not planned for fill** — extend **`GapFillSkipReason`**, not `GapPatchSkipReason`:

```rust
pub enum GapFillSkipReason {
    // ... existing ...
    OutsideReferenceCoverage,
}
```

Update `docs/json-output.md` and repair golden fixture (`full_surface_repair.json`) per [JSON contract revision](#json-contract-revision).

### JSON contract revision

Per [json-output.md](json-output.md) revision procedure:

- [ ] Add `alignment_mode_used`, `query_localization` to `AlignmentReport` table (optional-absent keys).
- [ ] Add `outside_reference_coverage` to **GapFillSkipReason** enum in repair section.
- [ ] Regenerate `tests/fixtures/full_surface_alignment.json` and `full_surface_repair.json`.
- [ ] Note revision in changelog/commit (additive v1 extension).

---

## Phases

### Phase Q0 — Spike (lib)

**Goal:** Prove coarse sliding fingerprint search finds a known anchor on synthetic media.

**Lib (`crates/clip-sync`)**

- [ ] Unit test helper: build long A (e.g. 30 min chirp/noise) + short B (5 min segment copied from A at known anchor, e.g. 45:00)
- [ ] Prototype function (test module or scratch): fingerprint B, `scan_mono_buckets` on A every 60s with window = B duration, score with `ChromaprintAligner::find_offset` — use `&mut` fake session per post-redesign port
- [ ] Assert best anchor within **±2 s** of truth on spike fixture
- [ ] Assert symmetric path still fails or misaligns on same pair (clip count mismatch or wrong offset)
- [ ] Record: windows scored, runtime order-of-magnitude, whether stride=60 is sufficient for 2h reference

**CLI / repair:** none

### Phase Q1 — Core localization (lib only)

**Lib (`crates/clip-sync`)**

- [ ] `domain/query_localization.rs` — `compute_mapped_region(anchor_a, query_duration, extent_a, extent_b) -> TimelineOverlap`
- [ ] `application/locate_query.rs` — **new** `LocateQueryInReference` use case:
  - Input: **`&mut` sessions**, tracks, `AlignConfig`, **`MediaExtent`** per file
  - Extract + prep full query clip (shorter file) via `session.extract_mono`
  - `session.scan_mono_buckets` on reference @ `target_sample_rate` — **first production mono scan caller**; callback fingerprints only (no session re-entry)
  - For each bucket: build window length L, fingerprint, match, convert segment → candidate `anchor_a_secs`
  - Cluster candidates (reuse lag clustering idea from `matching.rs` on anchor positions)
  - Respect `query_max_windows_scored` (widen stride + log warn)
  - PCM refine top `query_refine_top_k` via **`refine_query_anchor`** in `offset_refinement.rs` (extract reference haystack with `extract_mono`, then reuse `refine_offset_around_prior` / `pcm_discover_offset`)
  - Return `QueryLocalization` + `ClipMatchEstimate`
- [ ] `build_query_alignment_result(...)` — synthetic single `ClipMatch` (label `Start`), `recommended_offset_secs`, `start_overlap = mapped_region`
- [ ] Unit tests: known anchor pass, ambiguous repeat (lower confidence), no match (confidence below threshold), query shorter than MIN_CLIP_LENGTH skip, effective-duration clamp
- [ ] **`AlignVideos` not wired yet** — callable from tests and `locate_query` integration tests

**CLI / repair:** none

### Phase Q2 — Integrate into `AlignVideos` (lib)

**Lib (`crates/clip-sync`)**

- [ ] `resolve_alignment_mode(mode, extent_a, extent_b, plan_a, plan_b) -> AlignmentModeUsed` in `domain/policies.rs`
- [ ] `AlignVideos::execute()` — after open, resolve **`MediaExtent`** per video, then branch:
  ```text
  let mut session_a = open(...)?;
  let mut session_b = open(...)?;
  let extent_a = resolve_extent(&mut session_a, ...)?;
  let extent_b = resolve_extent(&mut session_b, ...)?;

  if mode == QueryReference || (mode == Auto && should_use_query(..., extent_a, extent_b, ...)):
      outcome = locate_query_track_pair(&mut session_a, &mut session_b, extent_a, extent_b, ...)
      apply_high_rate_refinement(...)   // MediaExtent inputs; segment inside mapped region
      apply_offset_verification(...)    // shipped retry path; mapped-region placement
  else:
      existing symmetric path (&mut sessions, MediaExtent in clip planning)
  ```
- [ ] Set `result.alignment_mode_used`, `result.query_localization`
- [ ] `refresh_start_overlap` → in query mode use `mapped_region` helper instead of start clip window
- [ ] `application/report.rs` — `QueryLocalizationReport`, `AlignmentModeUsedReport`, `From` impls, `format_query_localization_lines`
- [ ] `default_pipeline.rs` / public facade: export new config enums + formatters; no breaking pipeline API change
- [ ] Integration tests in `align_videos.rs`: one real-WAV E2E query-mode oracle; unit tests for Auto detection and symmetric override — follow [verification hardening test roles](TEMP-verification-hardening-plan.md) (no redundant chirp duplication)
- [ ] Use `application/testing/alignment_fixtures.rs` for hand-built `AlignmentResult` in new tests
- [ ] JSON contract revision + golden fixture update

**CLI (`clip-sync-cli`):** none required for repair path (Q3 handles repair CLI)

### Phase Q3 — Repair integration

**Repair (`crates/clip-sync-repair`)**

- [ ] `default_repair_align_config()`: document Auto mode; consider `num_clips = 1` when query mode expected (optional — Auto handles mismatch)
- [ ] CLI flags: `--query-reference`, `--symmetric-align`, `--query-stride`, `--no-limit-fill-region`
- [ ] `scan_gaps.rs`:
  - `overlap` from `alignment.start_overlap` (mapped region in query mode)
  - `check_gap_offset_agreement_in_overlap`: use mapped region when `query_localization` present
  - When `limit_fill_to_mapped_region`: mark gaps outside `mapped_region` as not fillable → `GapFillSkipReason::OutsideReferenceCoverage`
- [ ] `gap_fill.rs` / `patch_result.rs`: `OutsideReferenceCoverage` variant + human label in `format_fill_skip_reason`
- [ ] `infrastructure/cli/output.rs` + lib `application/report.rs` (`format_query_localization_lines`):
  - **Human (default):** lead with clip placement, not offset — e.g. `Match on A: 45:00 – 53:00  (8m clip, confidence 0.91)` via `start_clip()` confidence
  - **Human (`--verbose`):** add B span, coarse-search stats, offset for debugging
  - **JSON:** `AlignmentReport` passes through `query_localization` including `clip_on_a_*` / `clip_on_b_*`
  - Replace or subordinate repair `Overlap:` line in query mode — use `Match on A:` / `Clip coverage:` instead
  - Warn when `ambiguous == true`
- [ ] Integration tests:
  - Long A + short B with gap inside mapped region → fillable
  - Gap outside mapped region → reported, `OutsideReferenceCoverage` when `limit_fill_to_mapped_region`
  - Clip count mismatch pair succeeds under Auto

**Lib:** none (unless Q2 gaps found)

### Phase Q4 — Analyzer CLI + corpus + documentation

**CLI (`clip-sync-cli`)**

- [ ] Mirror query-mode flags on analyzer for debugging
- [ ] Human/JSON lines for `query_localization` (reuse `format_query_localization_lines` from `clip_sync`)
- [ ] Default human output: **start/finish on A** as primary line; offset only with `--verbose`; headline confidence via `start_clip()`

**Corpus (alignment — `tests/corpus/`)**

- [ ] `manifest.toml` case `wav_query_reference_45min_anchor` — 60 min A, 8 min B embedded at 45:00 — **generated tier only** (too large for committed corpus; unrelated to verification hardening’s ~75 s committed case)
- [ ] `CorpusCase` extensions: `alignment_mode`, `expect_clip_on_a_start_secs`, tolerance
- [ ] `application/testing/corpus_fixtures.rs` — generator for long+short pair
- [ ] Wired into existing corpus test harness (`corpus_generated_cases` tier)

**Corpus (repair — `clip-sync-repair/tests/`)**

- [ ] Integration: `repair_query_mid_file_gap` — long A with gap inside clip coverage → patched
- [ ] Integration: `repair_query_gap_outside_coverage` — gap before clip anchor → skipped
- [ ] Synthetic WAV only (same pattern as existing chirp fixtures); no gap-corpus manifest unless size budget allows

**Documentation**

- [ ] **README** — new subsection “Short clip against long recording”: when Auto/query mode triggers, example command, sample human output (`Match on A: …`), note that gaps outside clip coverage are reported but not filled
- [ ] **README** — symmetric vs query-mode flag table (`--query-reference`, `--symmetric-align`)
- [ ] **docs/corpus-validation.md** — describe `wav_query_reference_*` (generated tier) and test roles
- [ ] **docs/development.md** — brief pointer if corpus env vars apply
- [ ] **BACKLOG.md** — link this plan until archived
- [ ] Archive this doc → `docs/archive/query-reference-alignment-plan.md`

---

## Design

### End-to-end flow (repair, query mode)

```mermaid
flowchart TD
    OPEN["Open A (long) + B (short)"]
    EXT["Resolve MediaExtent per file"]
    MODE{"Auto → query mode?"}
    SYM["Symmetric align_videos"]
    LOC["LocateQueryInReference"]
    HR["apply_high_rate_refinement\n(mapped region)"]
    VER["apply_offset_verification\n(shipped retry path)"]
    SCAN["ScanGaps: full A timeline"]
    FILL["Gap fill gated by mapped region"]
    PATCH["PatchAudio (unchanged)"]

    OPEN --> EXT --> MODE
    MODE -->|no| SYM --> SCAN
    MODE -->|yes| LOC --> HR --> VER --> SCAN
    SCAN --> FILL --> PATCH
```

### Coarse search (reference timeline)

```text
query_clip = extract_mono(B, [0, extent_b.effective())) → prepare → fingerprint → FP_Q

stride = config.query_search_stride_secs
L_secs = min(query_prepared_duration, clip_length)
dur_a = extent_a.effective()

scan_mono_buckets(A, bucket_secs = stride):   // &mut session; callback must not re-enter session
  for each bucket starting at pos:
    window = [pos, pos + L_secs) clamped to dur_a
    if window shorter than MIN_CLIP_LENGTH: continue
    FP_W = fingerprint(bucket PCM → prepare)   // PCM from bucket callback, not a second extract
    estimate = aligner.find_offset(FP_W, FP_Q)   // left=window, right=query
    anchor_a = pos + estimate.offset_secs
    record candidate(anchor_a, estimate.confidence, ambiguous)

cluster candidates by anchor_a (±2 s)
pick best by confidence (×0.5 if ambiguous)
if best.confidence < query_min_match_score: no recommendation
```

**Offset sign check:** After picking winner, set `recommended_offset_secs = -anchor_a_secs` and verify with `b_pos = a_pos + offset` at anchor.

**Window cap:** If `ceil(dur_a / stride) > query_max_windows_scored`, multiply stride by 2 until under cap (log `tracing::warn`).

### PCM refine (winner)

Extend `offset_refinement.rs`:

```text
refine_query_anchor(
  query_clip: MonoPcmClip,           // full prepared query
  reference_session: &mut impl MediaSession,
  track_a: &AudioTrack,
  coarse_anchor_a: f64,
  query_duration_secs: f64,
  search_radius_secs: f64,           // default max(15, 0.1 * query_duration)
  resampler, correlator,
) -> (anchor_a_refined, confidence)
```

- `extract_mono` reference haystack `[anchor - radius, anchor + query_duration + radius)`
- Reuse `refine_offset_around_prior` / `pcm_discover_offset` with query as template
- Update `QueryLocalization.anchor_a_secs` and `recommended_offset_secs`

### High-rate + verification in query mode

Delegate to shipped `apply_high_rate_refinement` / `apply_offset_verification` — change **only** segment placement and input extents:

```text
discovery_windows = [ClipWindow::new(win_start, win_end)]  // winning coarse window on A
mapped_region = query_localization.mapped_region
pick_duration = extent_a.effective().min(extent_b.effective())
holdout_window_candidates(pick_duration, discovery_windows, segment_length, Δ)
// verification: retry up to 3 candidates; report candidates_tried (verification hardening)
Δ = recommended_offset_secs (post-PCM refine)
```

### Gap scan and fill (repair)

| Gap location | Report | Fillable when `limit_fill_to_mapped_region` |
|--------------|--------|---------------------------------------------|
| Inside mapped region, B has energy | Yes | Yes (existing gates) |
| Inside mapped region, B silent | Yes | No |
| Outside mapped region | Yes | No (`GapFillSkipReason::OutsideReferenceCoverage`) |
| No alignment offset | Yes, no B coords | No |

`build_gap_fill_plan` unchanged structurally; add region check in `ScanGaps` when building `Gap` rows or before `Gap::is_fillable()`.

### Auto mode selection

```text
fn should_use_query(mode, extent_a, extent_b, windows_a, windows_b) -> bool:
  match mode:
    QueryReference => true
    Symmetric => false
    Auto =>
      let dur_a = extent_a.effective().as_secs_f64()
      let dur_b = extent_b.effective().as_secs_f64()
      let (short, long) = if dur_a <= dur_b { (dur_a, dur_b) } else { (dur_b, dur_a) }
      if short / long < query_min_duration_ratio: return true
      if windows_a.len() != windows_b.len(): return true
      return false
```

When Auto picks query mode, **query is always the shorter file** (typically B in repair).

### Interaction with existing features

| Feature | Query mode behavior |
|---------|---------------------|
| `num_clips`, `clip_length` | Ignored for planning on query path; `clip_length` caps coarse window L |
| `require_consistent_offsets` | N/A (single localization) |
| `prefer_start_clip` | N/A |
| `refine_offset_with_pcm` | Used inside `refine_query_anchor` |
| `refine_offset_high_rate` | Segment from mapped region; `MediaExtent` inputs |
| `verify_offset` | Shipped retry path; hold-out inside mapped region; `candidates_tried` in JSON |
| `check_clip_repetition` | Run on query clip + winning window; downgrade localization confidence |
| `try_all_tracks` | Pick best track pair by localization confidence |
| `scan_both` (repair) | Unchanged; cross-check uses mapped overlap |
| Patch structure match | Unchanged |
| `clip_with_label` / `start_clip()` | Synthetic `Start` clip drives headline confidence |

---

## Extend vs new crate

| Option | Verdict |
|--------|---------|
| **Extend `clip-sync` + `clip-sync-repair`** | ✅ **Recommended.** ~90% of repair pipeline (scan, fill plan, patch, WAV/mux) is reusable. |
| **New `clip-sync-locate` crate** | ❌ Thin wrapper; duplicates Symphonia/Chromaprint wiring in `default_pipeline.rs`. |
| **New repair binary** | ❌ Same UX (`A` gaps, `B` reference); splits config/tests for one mode flag. |
| **Optional `clip-sync --query-reference`** | ✅ Debug/locate-only convenience in Q4; not a separate product. |

---

## Tests

| Test | Phase | Crate | Asserts |
|------|-------|-------|---------|
| `coarse_search_finds_known_anchor` | Q0 | lib | Spike fixture anchor ±2 s |
| `compute_mapped_region_clamps_to_effective_duration` | Q1 | lib | Region bounds vs `MediaExtent` |
| `locate_query_passes_mid_file_embed` | Q1 | lib | 45 min anchor, confidence ≥ threshold |
| `locate_query_fails_below_threshold` | Q1 | lib | Unrelated A/B → no recommendation |
| `locate_query_respects_window_cap` | Q1 | lib | Stride widens, `windows_scored` ≤ cap |
| `locate_query_ambiguous_lowers_confidence` | Q1 | lib | Repeated content → `ambiguous` |
| `locate_query_scan_callback_no_reentry` | Q1 | lib | Bucket callback does not call session |
| `resolve_alignment_mode_auto_ratio` | Q2 | lib | 8 min / 60 min effective → query |
| `resolve_alignment_mode_auto_clip_mismatch` | Q2 | lib | Different window counts → query |
| `align_videos_query_mode_integration` | Q2 | lib | End-to-end `execute()` → `AlignmentReport` JSON shape |
| `symmetric_path_unchanged_regression` | Q2 | lib | Equal-length corpus cases still pass |
| `repair_query_gap_inside_region_fillable` | Q3 | repair | Patch succeeds |
| `repair_query_gap_outside_region_skipped` | Q3 | repair | `outside_reference_coverage` fill skip |
| `repair_auto_no_clip_count_mismatch_error` | Q3 | repair | Long+short pair completes |
| `cli_query_reference_flags` | Q3 | repair | Config roundtrip |
| `corpus_query_reference_45min_anchor` | Q4 | lib | Generated manifest case |
| `cli_human_query_mode_start_finish_line` | Q4 | CLI | Default human shows A start–finish, not offset |
| `cli_human_query_mode_verbose_offset` | Q4 | CLI | Offset appears only with `--verbose` |
| `repair_json_clip_on_a_fields` | Q3 | repair | JSON includes `clip_on_a_start_secs` / `clip_on_a_end_secs` |
| `full_surface_alignment_json_golden` | Q2 | CLI | Golden fixture regenerated with query fields absent by default |

### Corpus case `wav_query_reference_45min_anchor`

| Field | Value |
|-------|-------|
| Tier | **Generated only** (60 min + 8 min exceeds committed size budget) |
| Generator | Long chirp A (3600 s); B = 480 s slice from A @ 2700 s |
| `alignment.mode` | `query_reference` |
| Assert | `\|anchor_a_secs - 2700\| ≤ 2` |
| Assert | `recommended_offset_secs ≈ -2700` |
| Assert | `mapped_region.shared_length_secs ≈ 480` |

---

## Output examples

### Human (repair, query mode)

**Default** — start/finish on the long file first; offset omitted:

```text
Match on video A: 45:00 – 53:00  (8m clip, confidence 0.91)

Gaps in video A (2 found, 1 repaired, 1 skipped, 0 unfillable):
  1   45:30 – 46:00   30.0s   patched
  2   10:00 – 10:30   30.0s   skipped (outside clip coverage)
```

**`--verbose`** — debugging fields including offset and B span:

```text
Mode:       query-reference
Match on A: 45:00 – 53:00
Clip on B:  0:00 – 8:00
Offset:     -2700.000s  (add to A to align with B)
Search:     60 windows @ 60s stride
```

Symmetric-mode repair output is unchanged (`Overlap:`, offset-first).

### JSON (`AlignmentReport` excerpt)

Machine-oriented fields preserved for scripts; friendly aliases on `QueryLocalizationReport`:

```json
{
  "alignment_mode_used": "queryreference",
  "recommended_offset_secs": -2700.0,
  "query_localization": {
    "anchor_a_secs": 2700.0,
    "clip_on_a_start_secs": 2700.0,
    "clip_on_a_end_secs": 3180.0,
    "clip_on_b_start_secs": 0.0,
    "clip_on_b_end_secs": 480.0,
    "mapped_region": {
      "video_a_start_secs": 2700.0,
      "video_a_end_secs": 3180.0,
      "video_b_start_secs": 0.0,
      "video_b_end_secs": 480.0,
      "shared_length_secs": 480.0
    },
    "search_stride_secs": 60.0,
    "winning_window_start_secs": 2640.0,
    "winning_window_end_secs": 3120.0,
    "confidence": 0.91,
    "ambiguous": false,
    "windows_scored": 60
  }
}
```

---

## Risks and follow-ups (out of v1 scope)

| Risk | Mitigation in v1 | Follow-up |
|------|------------------|-----------|
| Repeated content → wrong anchor | Ambiguity flag + verify; warn user | Multi-anchor report; user picks |
| 2h reference × 60s stride = 120 windows | `query_max_windows_scored` + stride widen | Hierarchical search (coarse 5m → fine 30s) |
| Query < 60s | Hard skip with message | Lower MIN_CLIP_LENGTH for query-only preset |
| Clock drift over long mapped region | Single offset + per-gap structure match | Segment-wise offset (BACKLOG) |
| Full-file RAM on patch | Unchanged | Streaming WAV encode |
| Multiple disjoint clips | Not supported | Multi-query batch mode |
| MKV tail / under-reported duration | `MediaExtent::effective()` clamp on mapped region | Revisit if under-report warning fires on real media |

---

## References

- Prior discussion: arbitrary clip vs long video repair workflow (2026-06-10)
- Prerequisites: [archive/media-session-redesign-plan.md](archive/media-session-redesign-plan.md) (shipped), [TEMP-verification-hardening-plan.md](TEMP-verification-hardening-plan.md)
- Layer purity (shipped): [archive/layer-purity-plan.md](archive/layer-purity-plan.md)
- JSON contract: [json-output.md](json-output.md)
- Symmetric alignment: `crates/clip-sync/src/application/align_videos.rs`
- PCM discover: `crates/clip-sync/src/application/offset_refinement.rs`
- Hold-out verify (shipped): [archive/offset-verification-plan.md](archive/offset-verification-plan.md)
- Report DTOs + human formatters: `crates/clip-sync/src/application/report.rs`
- Repair output: `crates/clip-sync-repair/src/infrastructure/cli/output.rs`
- Sequential decode: `crates/clip-sync/src/application/ports.rs` (`scan_mono_buckets`); production path `infrastructure/symphonia/extract.rs`
- Scan policy (post-redesign): `crates/clip-sync/src/application/media_scan.rs`
- Chromaprint matching: `crates/clip-sync/src/infrastructure/chromaprint/matching.rs`
- Gap fill (unchanged core): `crates/clip-sync-repair/src/application/patch_audio.rs`
