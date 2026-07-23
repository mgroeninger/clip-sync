# Rust review findings — prioritized recommendations

> **Status:** Active ledger (updated 2026-07-23). Workspace review of `clip-sync`,
> `clip-sync-repair`, `clip-sync-cli`, `clip-sync-repair-harness`, and
> `clip-sync-repair-fixtures`. Findings were verified in source where marked
> **confirmed**.
>
> **P0 status:** All five P0 items **fixed** (2026-07-23).
> **P1 status:** Mechanical P1s **fixed** (2026-07-23): M-CLI, M-NaN, M-MUX,
> M-HARNESS-CAST, M-RESAMPLE (count clear), M-SILENT (warn sites). **M-HE fixed**
> (2026-07-23): extract sinks + coarse search now rebase to the decoded rate.
> **M-FDK-RESET fixed + regression-tested** (2026-07-23): `reset()` now recreates the
> FDK decoder; verified red→green with an in-process HE-AAC SBR fixture.
> **M-AC3-DRAIN fixed + unit-tested** (2026-07-23): `decode_ref` now drains every frame
> a packet produces (extracted into a testable `drain_packet` helper).
> **M-SILENT effectively closed** (2026-07-23): `align_videos` `Ok(None)` now `warn!`s the
> suppressed error; CLI + repair loaders warn on unknown/misspelled TOML keys via a shared
> `clip_sync::unknown_toml_keys` round-trip diff. Only the **optional** machine-readable
> report flags remain deferred.
> **M-FFT closed as hygiene** (2026-07-23): unused `&dyn PcmCorrelator` arg removed from
> discover search only; the port stays for lag refine / holdout / anchors. Pearson discover
> left alone (do **not** restore GCC-PHAT `slide_template_scores` there).
> **Recommendations** for each remaining item (approach, tests, sequencing) are
> in the P1–P3 sections and **Suggested sequencing** below.
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
| **M-HE** | Extract sinks gate `metadata_ready` on a new `rate_validated` flag so the first decoded packet always reaches `on_first_decode`, which rebases `target_samples`/`target_frames` to the decoded rate and `warn!`s on hint mismatch; `locate_query` coarse search sizes `l_samples`/`stride` from the first bucket's decoded rate, not the probe hint | `extract_loop::tests::{mono_first_decode_rebases_window_math_when_decoded_rate_exceeds_hint, mono_first_decode_preserves_rate_when_hint_matches_decoded, interleaved_first_decode_rebases_target_frames_when_decoded_rate_exceeds_hint}`; `-Tier pr` green |
| **M-FDK-RESET** | `AacDecoder::reset()` recreates `Decoder::new(Transport::Adts)` + clears `m4a_info_validated` (fdk-aac 0.8 has no flush API), so post-seek frames no longer inherit pre-seek SBR/overlap state; ADTS header is rebuilt per-packet from base-rate `m4a_info`, so first post-reset frame re-configures cleanly | `extract_window_regression::fdk_reset_backward_seek_reprimes_to_identical_steady_state` — **verified red→green** (pre-fix: 5261 divergent steady-state samples; fixed: 0). Runs on **stock ffmpeg** via an in-process FDK-encoder HE-AAC fixture (see below). Full `he-aac,ffmpeg-tests,test-utils` suite green (341 pass). |
| **M-AC3-DRAIN** | `decode_ref` drains **every** frame per packet: send-and-drain extracted into a free `drain_packet(&mut dyn OxideDecoder, &Packet, &mut Vec<i16>)` that loops `receive_frame` to `NeedMore`, accumulating interleaved S16 into a reusable `pcm_scratch` (no per-packet alloc). Channel count taken from frame 0; a mismatch across frames is a hard `DecodeError`; empty frames skipped; buffer `grow_capacity`s so many-substream packets can't overflow `render_uninit` | `oxideav_ac3::decoder::tests::{drain_packet_accumulates_every_frame_until_needmore, drain_packet_returns_zero_when_decoder_still_buffering, drain_packet_skips_empty_frames, drain_packet_rejects_inconsistent_channel_count}` via a scripted mock decoder; full `ac3,ffmpeg-tests` lib suite green (333 pass, incl. real `probe_and_extract_eac3_surround_mp4` + AC-3 chirp decode) |

---

## Priority order (remaining)

| # | ID | Sev | One-line | Where |
|---|----|-----|----------|-------|
| 1 | M-CLONE | P2 | Full-clip clones + planner rebuild + per-packet alloc | hot paths |
| 2 | M-CFG | P2 | ~50 knobs copied across 4 struct layers | repair config → patch |
| 3 | M-MOD | P2 | Split 3–5 kloc modules | fingerprint / policies / patch |
| 4 | M-HARNESS | P2 | Harness drifts from production defaults / formulas | harness crate |
| 5 | L-* / M-DEAD | P3 | Dead pregate + unused `slide_template_scores`; CLI hygiene | misc |

*(M-HE + M-FDK-RESET + M-AC3-DRAIN fixed 2026-07-23 — all codec P1s closed. M-SILENT
effectively closed 2026-07-23 — only optional report flags deferred. **M-FFT closed
2026-07-23** as hygiene (unused discover correlator *arg* removed; `PcmCorrelator` kept
for lag refine). See Fixed tables. **All P1s are now done except optional report flags;
remaining work is P2/P3. Next blessed perf step is M-CLONE / remaining repair gate-search
work — not discover PHAT restore.**)*

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

### Fixed in this tranche *(see table above)*

M-CLI, M-NaN, M-MUX, M-HARNESS-CAST, M-RESAMPLE (count clear), M-SILENT warn/debug
sites — **done 2026-07-23**.

### M-HE. Container sample-rate hint trusted; HE-AAC / SBR mismatch — **fixed 2026-07-23**

**Files:** `extract_loop.rs`, `locate_query.rs`.

Was: the container hint seeded `resolved_rate`, and `metadata_ready` gated on
`resolved_rate.is_some()`, so `on_first_decode` (the only place the decoded rate was
read) never ran when a hint was present. With `he-aac` the container rate is often
half the decoder output, corrupting seconds↔samples for the whole extract / query.

Fix: both `MonoExtractSink` and `InterleavedExtractSink` gained a `rate_validated`
flag; `metadata_ready` now gates on it (plus channels for interleaved), so the first
decoded packet always reaches `on_first_decode`. There the decoded rate is compared
to the hint — on mismatch it `warn!`s and rebases `target_samples` / `target_frames`
(and hence all downstream window math, which reads `resolved_rate`) to the decoded
rate. `locate_query::coarse_search` now derives `l_samples` / `stride_samples` lazily
from the first bucket's decoded `sample_rate` instead of `reference_track.sample_rate`
(the probe hint), and `warn!`s on mismatch. The pre-decode packet-skip path
(`packet_window_pos`) still uses the hint, which is safe: it scales PTS and window
bounds by the same rate, so the Before/Within/Past decision is rate-invariant.

**Verified:** targeted unit tests in `extract_loop::tests` drive `reset_attempt`
(hint) → `on_first_decode` (decoded rate) directly and assert the mismatch case
rebases `target_units` and only then flips `metadata_ready` — for both the mono and
interleaved sinks — plus a hint-matches-decoded control. Full `-Tier pr` green;
`cargo test -p clip-sync --lib --features he-aac,test-utils` (327 pass, incl.
`sample_count_tolerance_allows_he_aac_end_boundary_gap`). Existing HE-AAC ffmpeg
fixtures cover the integration path under `--features he-aac,ffmpeg-tests`.

### M-FDK-RESET. FDK `reset()` empty after seeks — **fixed + regression-tested 2026-07-23** (recreate-decoder)

**File:** `fdk_aac/decoder.rs` (was `reset(&mut self) {}` at line 139; now recreates
the `Decoder` and clears `m4a_info_validated`). Note `reset()` is called after **every**
seek at `extract_loop.rs:309` (the everyday path), not only the `NeedReset` arm — so it
is firmly on the hot path.

**Measured magnitude (2026-07-23, `fdk_reset_backward_seek_reprimes_to_identical_steady_state`).**
Reproduced end-to-end with a genuine HE-AAC (SBR) sweep. Comparing a reused-session
backward-seek extract of window B against a fresh-session extract of B:
- **Decoder-state effect (what `reset()` owns):** pre-fix leaves **5261** steady-state
  samples divergent — *low magnitude (≤3 LSB) but systematic and widespread*; the fix
  drives it to **0** (bit-exact). This is the part that was silently wrong.
- **Reader-position effect (orthogonal, NOT `reset()`):** the *leading* re-prime region
  (~first 3 SBR frames ≈ 6 k samples) diverges by a *large* amount (~23 k) both before
  and after the fix — because the backward seek re-primes SBR from a different reader
  landing point. This is the `reset_decode_io` domain; the fix neither helps nor should.

So the earlier "SBR-overlap on the first frame(s), within tolerance" framing was
directionally right but the effect is actually a low-level contamination smeared across
the *whole converged window*, not just the first frames — real, previously silent, and
now closed. The large leading divergence is a separate reader-position artifact,
confirming the reader-reopen analysis below with hard numbers.

**Blast radius (traced 2026-07-23).** `reset()` *is* on the hot path: sessions cache
`MediaIoState` (with its decoders) via `open_io_state` and reuse it across window
extracts, doing a per-extract seek + `decoder.reset()`. So the no-op means every
reused-decoder backward seek carries pre-seek SBR/overlap state into the first
post-seek frame(s). Why this has never been observed as wrong audio: the three
reader-reopen escape hatches recreate the decoder as a *side effect*, masking the
broken reset at exactly the roughest seek points:
- `seek_with_recovery` — reopens `MediaIoState` on seek failure.
- `track_decodable_extent` — always reopens after a tail scan (MP4 reader is broken
  past EOF).
- `reset_decode_io` — explicit full container reopen (used by high-rate refinement,
  see below).

Residual exposure = the *everyday* extract path that reuses a cached FDK decoder
across per-window seeks where none of the reopens fire; the SBR-overlap contamination
lands on the first frame(s) and is almost certainly swallowed by
`sample_count_tolerance` (~2 SBR frames) / boundary trimming — which is why it reads
as latent rather than a live bug.

**`reset_decode_io: true` is reader-motivated, not decoder-motivated (traced
2026-07-23).** The `extract_native_holdout(…, reset_decode_io: true)` calls in
`high_rate_refinement.rs` (lines 295, 436) exist to clear a *corrupted reader
position*, not to reset the decoder. Evidence: the `ports.rs` doc for the method
("…so the next window decode does not inherit a corrupted reader position (common on
MKV after backward seeks)"), the introducing commit `14170d8`, and
`media-session-redesign-plan.md` (Phase 2 `seek_with_recovery` / attempt-2 reopen;
"post-extent reopen kept for MP4"). The decoder recreation is incidental. **Consequence
for the fix:** repairing `reset()` does *not* let us delete these reader reopens — the
reader-position problem is real and orthogonal. At most, a correct cheap `reset()`
could let a future refactor swap one full-container `reset_decode_io` reopen for a
decoder-only reset *when* the reader position is known-good; the reopens themselves stay.

**Recommendation:** Implement `reset()` via FDK flush/clear **if `fdk-aac` 0.8
exposes one on `Decoder`** — but do not assume it does. Robust fallback that needs
no upstream API: recreate the decoder (`self.decoder = Decoder::new(Transport::Adts)`)
and set `m4a_info_validated = false` inside `reset()`. That guarantees SBR/overlap
state is cleared without betting on a flush API. Optionally keep a reusable
ADTS+payload `Vec` on `self` to remove per-packet allocs (pairs with M-CLONE’s FDK
slice).

**Test (done).** `fdk_reset_backward_seek_reprimes_to_identical_steady_state`
(`extract_window_regression.rs`): decode a late window A then backward-seek to earlier
window B on one session; assert the **steady-state region** (samples past the re-prime
transient) is bit-identical to a fresh-session extract of B, with a fresh-vs-fresh
determinism control. Verified red (5261 divergent) → green (0).

*Fixture technique worth reusing:* stock ffmpeg (incl. CI's `windows-latest`) is built
**without libfdk**, so it cannot *encode* HE-AAC — which is why the existing
`write_he_aac_mp4_fixture` tests silently **skip everywhere**. The `fdk-aac` crate,
however, bundles the FDK **encoder** (`fdk_aac::enc`), and its public `Encoder` emits
real SBR (`AudioObjectType::Mpeg4HeAac` → `frameLength == 2048`) despite the "hardcode
SBR off" source comment. So `test_support::ffmpeg_util::write_he_aac_sweep_mp4` encodes
SBR **in-process** and remuxes ADTS→MP4 with `ffmpeg -c copy` (copy needs no libfdk).
This makes HE-AAC decode tests actually **execute** on stock ffmpeg + in CI, instead of
skipping.

*Migration (done).* `probe_and_extract_he_aac_mp4_container` was moved onto
`write_he_aac_sweep_mp4` and now genuinely runs (verified: no skip line, decodes real
SBR, non-silent PCM); the dead `write_he_aac_mp4_fixture` builder was deleted. The 5.1
`probe_and_extract_he_aac_surround_mp4_container` **cannot** migrate — the fdk-aac crate's
`Encoder` wrapper is stereo-max (`EncoderHandle::alloc(0, 2)`), so surround still needs a
libfdk-enabled ffmpeg and continues to skip.

### M-AC3-DRAIN. Single `receive_frame` per packet — **fixed + unit-tested 2026-07-23**

**File:** `oxideav_ac3/decoder.rs`

Was: `decode_ref` called `send_packet` then exactly one `receive_frame`, so any
additional frames a packet produced were stranded until the next packet — silently
dropping audio and skewing sample counts on E-AC-3 (independent + dependent
substreams carried in one container packet).

Fix: the send-and-drain was extracted into a free, testable helper
`drain_packet(&mut dyn OxideDecoder, &Packet, &mut Vec<i16>) -> Result<(samples, n_ch)>`
that loops `receive_frame` until `NeedMore`, accumulating interleaved S16 into a
reusable `pcm_scratch` field on the decoder (no per-packet allocation — pairs with
M-CLONE #3). Channel count is fixed by the first non-empty frame; a differing count
on a later frame is a hard `DecodeError` rather than a mis-interleave; zero-sample
frames are skipped; a decode-nothing packet returns an empty buffer (unchanged
contract). `decode_ref` then `grow_capacity`s the output buffer to the drained total
before `render_uninit` (which panics past capacity), so a many-substream packet
cannot overflow the preallocated `BUF_CAPACITY`. Channel-layout lazy-init still
happens once, now keyed off the drained `n_ch`.

**Verified:** four unit tests drive `drain_packet` with a scripted mock decoder
(`ScriptedDecoder`) — multi-frame accumulation (the exact stranding case), still-
buffering `NeedMore` → `(0,0)` with scratch cleared, empty-frame skip, and the
channel-count-mismatch hard error. Full `-p clip-sync --features ac3,ffmpeg-tests
--lib` green (333 pass), which exercises the real `decode_ref` drain path via
`probe_and_extract_eac3_surround_mp4` (genuine E-AC-3 5.1) and the AC-3 chirp decode
characterization. Clippy clean on `--features ac3`.

### M-SILENT. Remaining swallowed-error sites — **effectively closed (only optional report flags remain)** *(2026-07-23)*

`align_videos` `Ok(None)` and CLI/repair unknown-TOML-key detection both landed
2026-07-23 (see table below). The unknown-key check is a shared, struct-derived
round-trip diff — `crates/clip-sync/src/infrastructure/config/toml_keys.rs`,
exported as `clip_sync::unknown_toml_keys`, unit-tested there and guarded against
false positives by `clip-sync-repair`'s `repair_fixture_reports_no_unknown_keys`
(full `[repair]` + nested `[repair.output]` surface). Only the **optional**
machine-readable report flags remain, deferred until an operator needs them.

| Site | Status | Recommendation |
|------|--------|----------------|
| `locate_query` prepare/fp/align | **done** (`debug!`) | — |
| `scan_gaps` B-side scan | **done** (`warn!`) | — |
| Harness `read_corpus_json` | **done** (warn on read/parse) | Optional later: hard-fail parse in measurement bins |
| `align_videos` `Ok(None)` | **done** (`warn!`) | `resolve_mode`'s two `Err(_) => Ok(None)` arms now `tracing::warn!` the suppressed per-side track/extent error before falling back to symmetric (`side`, `%error`) |
| CLI unknown TOML keys | **done** (`eprintln!`) | Shared `clip_sync::unknown_toml_keys(raw, &config)` round-trips the parsed config back to TOML and diffs key sets (no hand-kept list, no `deny_unknown_fields`); both `load_app_config` (analyzer) and `load_repair_app_config` emit `warning: unknown config key …` via `eprintln!` (tracing not yet up at load time) |
| Report flags (e.g. `b_scan_truncated`) | **open / optional** | Only if operators need machine-readable signal beyond logs |

**Test:** unit for unknown-key warning; existing align/scan tests for warn-only paths.

### M-RESAMPLE. Group delay (count clear done) — **partially open**

**Recommendation:** Prefer the cheap correct option: when one side is already at
the target rate, still run **both** through the same resample path (or document
that refined offsets are only valid when both sides resample). Full delay
query/trim is more work for little gain if paths are normalized.

**Test:** asymmetric case (one at target rate, one not) — offset bias should shrink.

---

## P2 — Performance / maintainability

### M-FFT. Unused discover correlator arg — **closed 2026-07-23** (hygiene; Pearson kept)

**File:** `application/offset_refinement.rs` (`pcm_search_near_offset` / `pcm_discover_offset`)

**History (corrected):** This was **not** an unfinished first wiring. Jun 2026 layer
purity + GCC-PHAT landed `PcmCorrelator::slide_template_scores`, and discover **did**
call it. Jul 6 (`c6241cd`) deliberately replaced that with a **local-window Pearson**
slide (`normalized_correlation`) + silence gating, renaming the unused param to
`_correlator`. Motive: keep discover on the Pearson scale that `DISCOVER_*` thresholds
expect, and shrink the haystack — not "FFT later." `slide_template_scores` is GCC-PHAT;
re-wiring it would have been a **behavior change**, not a silent speedup.

**Disposition (2026-07-23):** Removed the unused arg only —
`pcm_search_near_offset` / `pcm_discover_offset` no longer take `&dyn PcmCorrelator`.
The **`PcmCorrelator` port remains**; lag refine / holdout / anchor paths still inject
`FftCorrelator` for `cross_correlate_lag` / `segment_similarity`. Pearson discover left
alone.

**Do not:** restore PHAT for discover without a corpus pass + threshold retune.

**If discover later profiles hot:** FFT/prefix-sum **Pearson** (same family as
`lag_correlation_curve_auto`) + naive equivalence oracle — separate from this item.

**Perf note:** Do **not** conflate this with repair's blessed gate-search FFT work
(`char_gate_search` / `TEMP-production-repair-perf-plan.md`). That is a different call
site and already landed large wins; remaining repair perf → **M-CLONE**. Follow-up
hygiene for the unused `slide_template_scores` method → **M-DEAD §2** (do not restore
PHAT into discover).

### M-CLONE. Hot-path allocation — **open** (next blessed perf step)

Three independent PR-sized bites:

1. `truncate_padded_tail` / alignment loop → borrow or allocate only when truncating.
2. `FftCorrelator` holds `Mutex<FftPlanner<f64>>` (or thread-local).
3. FDK reusable buffers (pairs with M-FDK-RESET).

**Test:** existing align/refine corpus; no behavior change expected.

### M-CFG. Four-layer config field copying — **open**

**Recommendation:** Do not rewrite all four layers at once. Extract 1–2 shared
bundles first (`SeamGateParams`, `FillSearchParams`), embed by value in
`RepairConfig` and the patch request, delete the hand copies for those fields
only. Repeat. Pair with any new knob you add anyway.

**Test:** config roundtrip + one patch integration smoke.

### M-MOD. Oversized modules — **open**

**Recommendation:** Follow existing plan: [`TEMP-policies-module-split-plan.md`](TEMP-policies-module-split-plan.md)
first. Then split `gap_fingerprint` into schema / measure / project, and harness
`gap_fingerprint_corpus` into schema / analysis / report. Pure moves +
`pub(crate)` — no behavior change. Optionally curate repair `lib.rs` like
`clip-sync`.

**Test:** `-Tier pr` after each split.

| File | ~Lines |
|------|--------|
| `gap_fingerprint.rs` | 4,000 |
| `policies.rs` | 3,900 |
| `patch_audio.rs` | 3,600 |
| `align_videos.rs` | 2,900 |
| harness `gap_fingerprint_corpus.rs` | 2,300 |

### M-HARNESS. Drift from production — **open**

**Recommended order:**

1. `PatchTestOptions` from `RepairConfig::default()` with overrides only
2. One shared `gap_interior_range` / oracle validator (H6 clamp already on floor path)
3. One exported `NeverCalledAligner` + alignment builder in fixtures
4. `csv` crate or RFC 4180 quoting for calibration CSV
5. Delete the collapsed window formula or call production’s window helper

**Test:** harness lib + oracle smoke already in PR.

### Other P2

| ID | Issue | Recommendation |
|----|-------|----------------|
| M-GAPKEY | Float bit-pattern `HashMap` keys for gaps | Key by gap index (report is index-parallel elsewhere) |
| M-FRAMES | Inconsistent floor vs `.round()` in secs→frames | Standardize on `.round()` (match siblings that already do) |
| M-EPS | `f64::EPSILON` as wall-clock tolerance | Named `TIME_EPS_SECS` (e.g. `1e-9`) |
| M-HOUND | String-match on `hound::Error` Display | Match enum variants |
| M-DEAD | Dead symbols after dropped/unwound features | **Delete in a hygiene PR** — two independent bites below |

### M-DEAD. Dead symbols — **open** (batch with P3)

**1. Anchor pre-gate symbols** (`gap_anchor_seam.rs`, 2 `dead_code` warnings)

The anchor pre-gate was measured NO-GO and dropped (2026-07-23) — 0/~4939 brackets
doomed vs a 46% ceiling. The "wire it up" option is off the table; remove the dead
pregate symbols. See `archive/TEMP-anchor-pregate-plan.md` §7 and the
`anchor-pregate-greenlit` memory.

**2. `PcmCorrelator::slide_template_scores`** (zero production callers after M-FFT)

Discover used to call this (GCC-PHAT); Jul 6 switched discover to Pearson and M-FFT
removed the unused arg. The trait method + helpers remain with **no production
callers** — only trait defs, adapters, and fakes. Safe mechanical delete; **no
behavior change**. Keep `PcmCorrelator` itself (`cross_correlate_lag` /
`segment_similarity` still live).

**How to remove `slide_template_scores`:**

| File | Change |
|------|--------|
| `clip-sync/.../application/ports.rs` | Drop method from trait (+ its rustdoc) |
| `clip-sync/.../infrastructure/correlation.rs` | Drop method impl + `gcc_phat_slide_scores`; keep `gcc_phat_correlation` / `fft_cross_correlation` / `rustfft` (still used by `segment_similarity`); fix module docs |
| `clip-sync/.../testing/fakes.rs` | Drop `FakePcmCorrelator` stub |
| `clip-sync/.../offset_verification.rs` | Drop `SequencePcmCorrelator` stub |
| `clip-sync-repair/.../domain/ports.rs` | Drop method |
| `clip-sync-repair/.../infrastructure/correlation.rs` | Drop forwarder |
| `offset_refinement.rs` + this ledger | Soften “don’t restore `slide_template_scores`” wording (method gone) |

**Not required:** discover/Pearson changes, threshold retune, corpus work, new tests
beyond deleting stubs.

**Risks:** public trait break for any out-of-tree `PcmCorrelator` implementor
(`clip_sync` exports the trait) — in-workspace only today. Re-adding a slide method
later (PHAT or FFT-Pearson) is cheap if profiling ever wants it on the port.

**Test:** `cargo test -p clip-sync --lib` + repair paths that compile against the
correlator port (~15–30 min hygiene bite).

---

## P3 — Hygiene (selected)

Batch in a cleanup PR whenever touching CLI / `Cargo.toml`. Prefer bundling
**M-DEAD** (pregate + `slide_template_scores`) in the same PR.

| ID | Issue | Recommendation |
|----|-------|----------------|
| L-CLI-DEP | Unused `thiserror` in `clip-sync-cli` | Remove from `Cargo.toml` |
| L-PIPE | `println!` panics on broken pipe | `writeln!(stdout)`; treat `BrokenPipe` as success |
| L-QV | `--quiet --verbose` compose incoherently | clap `conflicts_with`, or a single verbosity enum |
| L-EXIT | Redundant `NoAudioTracks` exit-code arm | Distinct code or delete the specific arm |
| L-MSG | Fingerprinter "greater than 1001" vs check `< MIN` | Align message with the check ("at least") |
| L-PUBLISH | CLI missing `publish = false` | Match sibling internal crates |
| M-DEAD / L-pregate | Dead pregate symbols (pre-gate dropped NO-GO 2026-07-23) | **Delete** — see M-DEAD §1 |
| M-DEAD / L-slide | Unused `PcmCorrelator::slide_template_scores` (no production callers) | **Delete** — see M-DEAD §2 (file checklist + keep `segment_similarity` FFT path) |

---

## What's notably good (keep doing)

- Real domain/infrastructure split; ports make fakes easy
- ffmpeg path: scoped threads drain all pipes; `file:` prefix + tests for
  `concat:` / `http:` / leading `-`
- Production almost free of `unwrap`; FFT gated by naive-equivalence tests
- Corpus fixtures, golden CLI output, codec regression tests
- Exit-code mapping and stderr/stdout discipline in the CLI

---

## Suggested sequencing

Do **not** start M-CFG or large module splits until the codec P1s are done — those
are the remaining ways to get silently wrong audio.

1. ~~**M-HE**~~ (done 2026-07-23) → ~~**M-FDK-RESET**~~ (done + regression-tested 2026-07-23, recreate-decoder) → ~~**M-AC3-DRAIN**~~ (done + unit-tested 2026-07-23; reusable `pcm_scratch` covers M-CLONE #3 for AC-3). **All codec P1s closed.**
2. ~~**M-SILENT** remainder (unknown TOML keys + `align_videos` warn)~~ — **done 2026-07-23** (only optional report flags deferred)
3. ~~**M-FFT**~~ (done 2026-07-23) — unused discover correlator *arg* removed; port kept;
   Pearson discover unchanged. Next: **M-CLONE** planner (user-visible speed on
   repair/align hot paths). Anchor pre-gate was NO-GO (2026-07-23); repair
   gate-search FFT already landed separately — do not reopen discover PHAT.
4. **M-CFG** / **M-MOD** / **M-HARNESS** opportunistically with nearby feature work
5. **P3** / **M-DEAD** cleanup whenever convenient — dead pregate symbols + unused
   `slide_template_scores` (see M-DEAD checklist); also CLI broken-pipe / quiet /
   verbose / unused deps / publish flag
6. **M-RESAMPLE** group-delay / dual-path normalize when next touching refinement

### Milestone checklist

1. ~~**Codec hardening (P0 H2/H3/H5)**~~ / ~~**Panic clamps (P0 H1/H6)**~~ — **done 2026-07-23**.
2. ~~**Config honesty (P1 M-CLI / M-NaN)**~~ / ~~**M-MUX / harness cast / resample count / silent warns**~~ — **done 2026-07-23**.
3. ~~**Codec follow-ups (P1 M-AC3-DRAIN)**~~ — **done 2026-07-23**: drain all AC-3/E-AC-3 frames per packet (`drain_packet` helper + 4 unit tests; real E-AC-3 surround path green). *(M-HE HE-AAC rate cross-check + M-FDK-RESET recreate-decoder also done 2026-07-23; M-FDK-RESET has a verified red→green backward-seek regression test running on stock ffmpeg.)* **All codec P1s closed.**
4. ~~**Observability remainder (P1 M-SILENT)**~~ — **done 2026-07-23**: unknown TOML keys (shared `unknown_toml_keys` in analyzer + repair loaders) and `align_videos` `Ok(None)` warn logging. Optional machine-readable report flags deferred.
5. ~~**M-FFT hygiene**~~ — **done 2026-07-23**: drop unused discover correlator arg; keep
   `PcmCorrelator` for lag refine; leave Pearson. **Perf (P2 M-CLONE)** — stop cloning;
   reuse planner (next).
6. **Structure (P2 M-CFG / M-MOD / M-HARNESS)** — incremental; see `TEMP-policies-module-split-plan.md`.
7. **P3 / M-DEAD hygiene** — dead pregate + unused `slide_template_scores` (M-DEAD §1–§2);
   CLI broken-pipe, quiet/verbose, unused deps, publish flag.

---

## Sources

Review date: 2026-07-23. Findings synthesized from a full-workspace read-only
pass (clippy default + pedantic sample, `cargo test --workspace`, and targeted
source verification of every P0 item). Recommendations for remaining work added
2026-07-23. This file is the canonical ledger — update Fixed tables and the
priority list as items land; archive when the open set is closed.
