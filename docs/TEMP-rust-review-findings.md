# Rust review findings — prioritized recommendations

> **Status:** Active ledger (updated 2026-07-23). Workspace review of `clip-sync`,
> `clip-sync-repair`, `clip-sync-cli`, `clip-sync-repair-harness`, and
> `clip-sync-repair-fixtures`. Findings were verified in source where marked
> **confirmed**.
>
> **P0 status:** All five P0 items **fixed** (2026-07-23).
> **P1 status:** Mechanical P1s **fixed** (2026-07-23): M-CLI, M-NaN, M-MUX,
> M-HARNESS-CAST, M-RESAMPLE (count clear), M-SILENT (warn sites). Still open:
> M-HE, M-FDK-RESET, M-AC3-DRAIN, and non-mechanical M-SILENT pieces (report
> flags / unknown TOML keys / `align_videos` `Ok(None)`).
>
> **Context:** Default `cargo clippy --workspace --all-targets` nearly clean
> (2 `dead_code` warnings in `gap_anchor_seam.rs`). Pedantic clippy produces
> ~900 mostly cast/docs noise.

Legend: **sev** = P0 (correctness / panic / decode corruption) · P1 (silent wrong
behavior / user-intent override) · P2 (perf / maintainability) · P3 (hygiene).

---

## Executive summary

Architecture quality is high: hexagonal layering is real, ports/adapters are
testable, production paths avoid `unwrap`, and process I/O (ffmpeg) is carefully
threaded. Remaining open defects cluster in:

1. **Silent degradation** (errors swallowed → plausible but wrong outcomes)
2. **Structural debt** (3–5 kloc modules, four-layer config field copying)
3. ~~Vendored codec shims~~ *(P0 codec / Sync items fixed — see below)*

---

## Fixed (P0) — 2026-07-23

| ID | What | Evidence |
|----|------|----------|
| **H3** | ASC escape sample rate: read 24 bits (was broken 20-bit `&` mask → always 0); `sample_rate_index` returns `Option` (no silent 96 kHz) | `fdk_aac/meta.rs` tests: `read_asc_explicit_*`, `sample_rate_index_*` |
| **H2** | ADTS: reject `None`/unsupported AOT; map SBR/PS→LC profile; enforce 13-bit frame_length; reject escape sf index | `fdk_aac/adts.rs` unit tests; `construct_adts_header` → `Result` |
| **H1** | Clamp subclip start per side when right is shorter than left peak | `pcm_preparation` test `aligned_subclip_pair_clamps_when_right_shorter_than_peak_start` |
| **H6** | Clamp floor-oracle interior end to `samples_a.len()` via `gap_interior_range` | harness `floor_oracle` tests |
| **H5** | Replace `unsafe impl Sync` with `Mutex<Box<dyn OxideDecoder>>` | compiles under `AudioDecoder: Send + Sync`; `cargo test -p clip-sync --features ac3 --lib` |

## Fixed (P1 mechanical) — 2026-07-23

| ID | What | Evidence |
|----|------|----------|
| **M-CLI** | Persist `profile_field_mask` from TOML; CLI `--quick`/`--full` reuses it | `quick_cli_preserves_toml_explicit_border_search` |
| **M-NaN** | Reject non-finite floats before range checks | `rejects_nan_float_thresholds`, `rejects_infinite_residual_lag` |
| **M-MUX** | Single ffprobe; mux to tempfile then `persist`; cleanup on failure | `validate_mux_duration_rejects_large_skew`; `-Tier pr` mux path |
| **M-HARNESS-CAST** | `gap_interior_peak_max: u16` (no `i16 as u16` wrap) | harness compile + floor_oracle tests |
| **M-RESAMPLE** | Clear `decoded_sample_count` after rubato/linear resample | rubato unit tests |
| **M-SILENT** *(partial)* | `warn!` on B-scan failure; `debug!` on coarse-query prepare/fp/align errors; corpus JSON parse/read warnings | code review |

---

## Priority order (remaining)

| # | ID | Sev | One-line | Where |
|---|----|-----|----------|-------|
| 1 | M-HE | P1 | HE-AAC container rate trusted; SBR mismatch | `extract_loop.rs` |
| 2 | M-FDK-RESET | P1 | FDK `reset()` is empty no-op after seeks | `fdk_aac/decoder.rs` |
| 3 | M-AC3-DRAIN | P1 | Single `receive_frame` per packet | `oxideav_ac3/decoder.rs` |
| 4 | M-SILENT | P1 | Remaining: report flags, unknown TOML keys, `align_videos` `Ok(None)` | several |
| 5 | M-FFT | P2 | Injected FFT correlator ignored; O(n·m) search | `offset_refinement.rs:238` |
| 6 | M-CLONE | P2 | Full-clip clones + planner rebuild + per-packet alloc | hot paths |
| 7 | M-CFG | P2 | ~50 knobs copied across 4 struct layers | repair config → patch |
| 8 | M-MOD | P2 | Split 3–5 kloc modules | fingerprint / policies / patch |
| 9 | M-HARNESS | P2 | Harness drifts from production defaults / formulas | harness crate |
| 10 | L-* | P3 | Dead pregate, unused dep, broken-pipe, quiet/verbose | misc |

---

## P0 — Correctness / panic / decode corruption *(all fixed)*

### H3. Explicit-sample-rate path in AAC config always yields 0 *(confirmed → fixed)*

**File:** `crates/clip-sync/src/infrastructure/symphonia/fdk_aac/meta.rs`

Was: `(0xf << 20) & bs.read_bits_leq32(20)?` always 0. Now: `bs.read_bits_leq32(24)?`.
`sample_rate_index` returns `Option<u8>` (no default to 96 kHz).

### H2. ADTS header construction corruptions *(confirmed → fixed)*

**File:** `crates/clip-sync/src/infrastructure/symphonia/fdk_aac/adts.rs`

`construct_adts_header` returns `Result`: rejects `None`/unsupported AOTs, maps
SBR/PS/ER_AAC_LC→LC profile for HE-AAC, enforces `frame_length ≤ 0x1FFF`, rejects
sf index > 11. Call site in `decoder.rs` propagates with `?`.

### H1. Reachable slice panic in `select_aligned_subclip_pair` *(confirmed → fixed)*

**File:** `crates/clip-sync/src/domain/pcm_preparation.rs`

Per-side clamp: `best_start.min(side.samples.len())` before slicing.

### H6. Unclamped slice in harness floor oracle *(confirmed → fixed)*

**File:** `crates/clip-sync-repair-harness/src/floor_oracle.rs`

`gap_interior_range(..., sample_len)` applies `.min(sample_len)` (same as dual-fit).

### H5. Unsound `unsafe impl Sync` on AC-3 decoder *(confirmed → fixed)*

**File:** `crates/clip-sync/src/infrastructure/symphonia/oxideav_ac3/decoder.rs`

`inner: Mutex<Box<dyn OxideDecoder>>`; `unsafe impl Sync` removed.


---

## P1 — Silent wrong behavior / user-intent override

### M-CLI. CLI profile stomps explicit TOML overrides

**File:** `crates/clip-sync-repair/src/infrastructure/cli/mod.rs:13–16`

TOML load computes a `RepairProfileFieldMask` so explicit fields survive a profile
bundle. CLI `--quick`/`--full` applies the bundle with
`RepairProfileFieldMask::default()` (empty) and overwrites every bundle-controlled
field the user set in TOML.

**Fix:** thread the mask from `load_repair_app_config` into `apply_cli_overrides`.

### M-NaN. Config validation accepts NaN

**File:** `crates/clip-sync-repair/src/infrastructure/config.rs` (~642–873)

Most checks are `if field < 0.0` / `<= 0.0` — both false for NaN. Only three
`anchor_seam_*` fields use `is_finite()`. A NaN threshold silently disables gates.

**Fix:** `!field.is_finite() || …` on every float check (or a field table loop).

### M-HE. Container sample-rate hint trusted; HE-AAC / SBR mismatch

**Files:** `extract_loop.rs` (hint short-circuits first-decode validation);
`locate_query.rs` (mixes hint-rate and decoded-rate in stride math).

With `he-aac`, container rate is often half the decoder output. Wrong rate
corrupts seconds↔samples for the whole extract / query search.

**Fix:** always cross-check first decoded packet rate; derive window/stride from
bucket rate on first callback.

### M-MUX. Partial mux output left on disk

**File:** `crates/clip-sync-repair/src/infrastructure/ffmpeg_mux.rs`

On ffmpeg failure (or stdin write fail mid-stream), `-y` leaves a truncated
output that looks like a good file. Also probes duration via `ffprobe` twice.

**Fix:** mux to a temp path, rename on success; probe once and pass the value.

### M-SILENT. Swallowed errors without signal

| Site | Behavior |
|------|----------|
| `locate_query.rs:307–309` | Three nested `if let Ok` discard prepare/fingerprint/align errors → "no match" |
| `scan_gaps.rs:348–355` | B-side scan `let _ = …` → later gaps `b_has_energy = false` with no warn |
| `align_videos.rs` (`resolve_mode` / extent) | Probe failures → `Ok(None)` → wrong alignment mode |
| Harness `read_corpus_json` | Parse error indistinguishable from missing file |
| CLI config `#[serde(flatten)]` | Typos silently get defaults (`deny_unknown_fields` blocked) |

**Fix:** `tracing::warn!` + report flags at each site; treat parse errors as hard
failures in measurement tools; warn unknown TOML keys via a pre-pass table.

### Related P1 items

| ID | Issue | Fix |
|----|-------|-----|
| M-RESAMPLE | `decoded_sample_count` not scaled after rubato; group delay uncompensated | Scale or clear count; trim delay or always resample both sides |
| M-HARNESS-CAST | `i16 as u16` on `gap_interior_peak_max` wraps negatives → check always passes | Use `u16` / `try_from` at load |
| M-FDK-RESET | FDK `reset()` is empty no-op after seeks | Call FDK flush; reuse packet buffers on `self` |
| M-AC3-DRAIN | Single `receive_frame` per packet | Drain until `NeedMore` |

---

## P2 — Performance / maintainability

### M-FFT. Injected correlator ignored

**File:** `offset_refinement.rs:238` — `_correlator: &dyn PcmCorrelator` unused;
hand-rolled O(n·m) Pearson. Tests annotated "slow: minutes".

**Fix:** wire `slide_template_scores` or delete the dead parameter.

### M-CLONE. Hot-path allocation

- Full multi-MB clip clones per iteration (`align_videos.rs:684–690`)
- `FftPlanner` rebuilt every correlation (`correlation.rs:69`)
- FDK decode: two `Vec` allocs per packet

**Fix:** `Cow`/refs for truncate; store planner in correlator; reusable buffers.

### M-CFG. Four-layer config field copying

`RepairConfig` → `PatchRequestSettings` → `PatchAudioRequest` → `SeamGateConfig`
(~50 knobs hand-copied). Missed copy compiles and silently uses a default.

**Fix:** shared sub-structs (`SeamGateParams`, `FillSearchParams`) embedded by value.

### M-MOD. Oversized modules

| File | ~Lines |
|------|--------|
| `gap_fingerprint.rs` | 4,000 |
| `policies.rs` | 3,900 |
| `patch_audio.rs` | 3,600 |
| `align_videos.rs` | 2,900 |
| harness `gap_fingerprint_corpus.rs` | 2,300 |

Also: repair `lib.rs` is four bare `pub mod` (vs curated `clip-sync` facade).

### M-HARNESS. Drift from production

- ~30 production defaults re-hardcoded as literals
- Seam window formula collapsed to a constant while claiming production fidelity
- `NeverCalledAligner` / alignment builders duplicated 3–4×
- Floor vs dual-fit oracle validators already diverged on the H6 clamp
- Unescaped CSV fields in calibration renderers

**Fix:** build options from `RepairConfig::default()`; share production helpers;
one exported stub/builder; RFC 4180 CSV / `csv` crate.

### Other P2

| ID | Issue |
|----|-------|
| M-GAPKEY | Float bit-pattern `HashMap` keys for gaps — prefer gap index |
| M-FRAMES | Inconsistent floor vs `.round()` in secs→frames |
| M-EPS | `f64::EPSILON` used as wall-clock time tolerance |
| M-HOUND | String-match on `hound::Error` Display instead of enum variants |
| M-DEAD | `anchor_bracket_matchability_doomed` + `MATCHABILITY_PREGATE_EPSILON` unused (clippy); align with `TEMP-anchor-pregate-plan.md` — wire or remove |

---

## P3 — Hygiene (selected)

| ID | Issue |
|----|-------|
| L-CLI-DEP | Unused `thiserror` in `clip-sync-cli` |
| L-PIPE | `println!` panics on broken pipe |
| L-QV | `--quiet --verbose` compose incoherently |
| L-EXIT | Redundant `NoAudioTracks` exit-code arm |
| L-MSG | Fingerprinter error text "greater than 1001" vs check `< MIN` |
| L-PUBLISH | CLI missing `publish = false` unlike sibling internal crates |

---

## What's notably good (keep doing)

- Real domain/infrastructure split; ports make fakes easy
- ffmpeg path: scoped threads drain all pipes; `file:` prefix + tests for
  `concat:` / `http:` / leading `-`
- Production almost free of `unwrap`; FFT gated by naive-equivalence tests
- Corpus fixtures, golden CLI output, codec regression tests
- Exit-code mapping and stderr/stdout discipline in the CLI

---

## Suggested milestones

1. ~~**Codec hardening (P0 H2/H3/H5)**~~ / ~~**Panic clamps (P0 H1/H6)**~~ — **done 2026-07-23**.
2. ~~**Config honesty (P1 M-CLI / M-NaN)**~~ / ~~**M-MUX / harness cast / resample count / silent warns**~~ — **done 2026-07-23**.
3. **Codec follow-ups (P1 M-HE / M-FDK-RESET / M-AC3-DRAIN)** — HE-AAC rate cross-check; FDK flush; drain AC-3 frames.
4. **Observability remainder (P1 M-SILENT)** — report flags; unknown TOML keys; `align_videos` `Ok(None)` logging.
5. **Perf (P2 M-FFT / M-CLONE)** — wire correlator; stop cloning; reuse planner.
6. **Structure (P2 M-CFG / M-MOD / M-HARNESS)** — can land incrementally alongside
   feature work; see also `TEMP-policies-module-split-plan.md`.

---

## Sources

Review date: 2026-07-23. Findings synthesized from a full-workspace read-only
pass (clippy default + pedantic sample, `cargo test --workspace`, and targeted
source verification of every P0 item). Earlier chat-only detail lived only in
agent transcripts and was never checked into the repo — this file is the
canonical ledger going forward. Update **status** columns (or move rows to a
"Fixed" section) as items land; archive when the open set is closed.
