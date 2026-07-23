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
> a packet produces (extracted into a testable `drain_packet` helper). Still open:
> non-mechanical M-SILENT pieces (report flags / unknown TOML keys /
> `align_videos` `Ok(None)`).
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
| 1 | M-SILENT | P1 | Remaining: report flags, unknown TOML keys, `align_videos` `Ok(None)` | several |
| 2 | M-FFT | P2 | Injected FFT correlator ignored; O(n·m) search — **blessed perf lever** | `offset_refinement.rs:238` |
| 3 | M-CLONE | P2 | Full-clip clones + planner rebuild + per-packet alloc | hot paths |
| 4 | M-CFG | P2 | ~50 knobs copied across 4 struct layers | repair config → patch |
| 5 | M-MOD | P2 | Split 3–5 kloc modules | fingerprint / policies / patch |
| 6 | M-HARNESS | P2 | Harness drifts from production defaults / formulas | harness crate |
| 7 | L-* | P3 | Delete dead pregate, unused dep, broken-pipe, quiet/verbose | misc |

*(M-HE + M-FDK-RESET + M-AC3-DRAIN fixed 2026-07-23 — see Fixed (P1) table. All codec
P1s are now closed.)*

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

### M-SILENT. Remaining swallowed-error sites — **partially open**

| Site | Status | Recommendation |
|------|--------|----------------|
| `locate_query` prepare/fp/align | **done** (`debug!`) | — |
| `scan_gaps` B-side scan | **done** (`warn!`) | — |
| Harness `read_corpus_json` | **done** (warn on read/parse) | Optional later: hard-fail parse in measurement bins |
| `align_videos` `Ok(None)` | **open** | Keep fallback; `tracing::warn!` the suppressed probe/extent error (same pattern as B-scan). No report-schema change required for v1 |
| CLI unknown TOML keys | **open** | Pre-pass: walk `toml::Table` against known key sets for `[repair]` / `[clip]` / `[alignment]`; `warn!` / `eprintln!` unknowns. Do not require `deny_unknown_fields` with `flatten` |
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

### M-FFT. Injected correlator ignored — **open (now the blessed perf lever)**

**File:** `offset_refinement.rs:238`

**Status note (2026-07-23):** The anchor pre-gate (lever #2, cut k) was measured
**NO-GO** and dropped — 0/~4939 brackets doomed over the full 17-pair fingerprint
run vs a 46% ceiling. Perf effort has been redirected to **FFT-ing the per-bracket
score sweep** (see `production-perf-gate-search-dominates`: `char_gate_search` ≈93%,
per-bracket score × k brackets is the hot path). That work overlaps directly with
this item, so M-FFT is no longer just P2 hygiene — it is the actively-blessed
perf direction and should be the first perf step once the codec P1s land.

**Recommendation:** Wire `_correlator.slide_template_scores` (or equivalent) into
`pcm_search_near_offset` — FFT the haystack sweep rather than the naive O(n·m) loop.
Prefer **wire** over delete — the port is already injected. Keep the naive path
behind `cfg(test)` as an equivalence oracle (pattern used elsewhere).

**Test:** existing slow refine tests should get faster; add a short equivalence
test at small N.

### M-CLONE. Hot-path allocation — **open**

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
| M-DEAD | Unused pregate symbols in `gap_anchor_seam` (2 `dead_code` warnings) | **Delete.** The anchor pre-gate was measured NO-GO and dropped (2026-07-23) — 0/~4939 brackets doomed vs a 46% ceiling. The "wire it up" option is off the table; remove the dead pregate symbols. See `archive/TEMP-anchor-pregate-plan.md` §7 and the `anchor-pregate-greenlit` memory. |

---

## P3 — Hygiene (selected)

Batch in a cleanup PR whenever touching CLI / `Cargo.toml`.

| ID | Issue | Recommendation |
|----|-------|----------------|
| L-CLI-DEP | Unused `thiserror` in `clip-sync-cli` | Remove from `Cargo.toml` |
| L-PIPE | `println!` panics on broken pipe | `writeln!(stdout)`; treat `BrokenPipe` as success |
| L-QV | `--quiet --verbose` compose incoherently | clap `conflicts_with`, or a single verbosity enum |
| L-EXIT | Redundant `NoAudioTracks` exit-code arm | Distinct code or delete the specific arm |
| L-MSG | Fingerprinter "greater than 1001" vs check `< MIN` | Align message with the check ("at least") |
| L-PUBLISH | CLI missing `publish = false` | Match sibling internal crates |
| M-DEAD / L-pregate | Dead pregate symbols (pre-gate dropped NO-GO 2026-07-23) | **Delete** — see M-DEAD above |

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
2. **M-SILENT** remainder (unknown TOML keys + `align_videos` warn) — half-day
3. **M-FFT** then **M-CLONE** planner (user-visible speed). M-FFT is now the
   blessed perf lever after the anchor pre-gate was dropped NO-GO (2026-07-23) —
   FFT the per-bracket score sweep. Also **delete** the dead pregate symbols
   (M-DEAD) as part of this cleanup.
4. **M-CFG** / **M-MOD** / **M-HARNESS** opportunistically with nearby feature work
5. **P3** cleanup whenever touching CLI / manifests
6. **M-RESAMPLE** group-delay / dual-path normalize when next touching refinement

### Milestone checklist

1. ~~**Codec hardening (P0 H2/H3/H5)**~~ / ~~**Panic clamps (P0 H1/H6)**~~ — **done 2026-07-23**.
2. ~~**Config honesty (P1 M-CLI / M-NaN)**~~ / ~~**M-MUX / harness cast / resample count / silent warns**~~ — **done 2026-07-23**.
3. ~~**Codec follow-ups (P1 M-AC3-DRAIN)**~~ — **done 2026-07-23**: drain all AC-3/E-AC-3 frames per packet (`drain_packet` helper + 4 unit tests; real E-AC-3 surround path green). *(M-HE HE-AAC rate cross-check + M-FDK-RESET recreate-decoder also done 2026-07-23; M-FDK-RESET has a verified red→green backward-seek regression test running on stock ffmpeg.)* **All codec P1s closed.**
4. **Observability remainder (P1 M-SILENT)** — unknown TOML keys; `align_videos` `Ok(None)` logging; optional report flags.
5. **Perf (P2 M-FFT / M-CLONE)** — wire correlator; stop cloning; reuse planner.
6. **Structure (P2 M-CFG / M-MOD / M-HARNESS)** — incremental; see `TEMP-policies-module-split-plan.md`.
7. **P3 hygiene** — CLI broken-pipe, quiet/verbose, unused deps, publish flag.

---

## Sources

Review date: 2026-07-23. Findings synthesized from a full-workspace read-only
pass (clippy default + pedantic sample, `cargo test --workspace`, and targeted
source verification of every P0 item). Recommendations for remaining work added
2026-07-23. This file is the canonical ledger — update Fixed tables and the
priority list as items land; archive when the open set is closed.
