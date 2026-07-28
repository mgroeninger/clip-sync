# Rust review findings — prioritized recommendations

> # ARCHIVED 2026-07-27 — closed, do not update
>
> The open set is empty: **every finding is fixed, withdrawn as refuted, or closed
> will-not-fix.** This file is retained as a historical record, not a live ledger —
> its value now is the *rationale*, which does not survive anywhere else:
>
> - **What was fixed** and how it actually landed (several items differed from how they
>   were filed — see the "As landed" notes).
> - **M-FRAMES — withdrawn, premise refuted** by a blast-radius check. Do not re-open it
>   from the original one-line framing.
> - **M-RESAMPLE — P3, will not fix.** Real, exactly measured delay with zero corpus
>   exposure. Its section carries the retained concern, the pre-derived fix, and three
>   explicit re-open triggers. That section is the reason this file was kept.
> - **Withdrawn recommendations** that would be plausible to re-derive and are wrong —
>   most notably M-RESAMPLE's "run both sides through the same resample path."
>
> Elective residue that outlived the ledger, none of it a defect: optional M-SILENT
> machine-readable report flags, the deferred `align_videos` module split, the
> prepare-clone perf stretch, and optional hard-fail parse in the harness measurement
> bins. Live backlog: [../../../BACKLOG.md](../../../BACKLOG.md).
>
> New review findings belong in a **new** document.

> **Status:** Closed ledger (final update 2026-07-27; archived same day). Workspace review of `clip-sync`,
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
> left alone (do **not** reintroduce a GCC-PHAT slide there without retune).
> **M-DEAD §2 fixed** (2026-07-23): `PcmCorrelator::slide_template_scores` +
> `gcc_phat_slide_scores` removed; port keeps `cross_correlate_lag` /
> `segment_similarity`.
> **M-DEAD §1 B2 fixed** (2026-07-23): pregate measurement stack removed (predicate,
> `CLIP_SYNC_BRACKET_STATS`, retired measure script → archive).
> **M-CLONE complete 2026-07-25** (#1+#3 2026-07-23; **#2 done** — thread-local
> `FftPlanner` reuse). Perf: corpus `local_anchor_xcorr` **846 s / 9.5% → 358 s / 4.2%**
> (−58%; same 1335 calls). Root 8937 s → 8449 s. Remaining material repair wall-time is
> `unified_refine_*` / lever 1c (~61% combined start+end). Prepare-clone stretch stays deferred.
> **M-CFG fixed 2026-07-23** (P2): `PatchAudioRequest` collapsed onto embedded
> `PatchRequestSettings` (58 fields → 3), `SeamGateConfig` near-twin deleted in favor of a
> settings borrow + frames-only `SeamGateDerived`, harness/test literals seeded from
> `RepairConfig::default().patch_settings()` — three conversion lists → one. The proposed policy
> bundles were **not** built and are declined.
> **M-MOD planned splits closed 2026-07-24** (P2): policies + M-MOD-DEPS + harness corpus +
> production `gap_fingerprint` + `patch_audio` (re-verified in source); `align_videos`
> deferred with no plan.
> **M-HARNESS complete 2026-07-24** (P2): all five recommended items done — shared interior
> oracle + NeverCalledAligner/builders + RFC 4180 CSV + production `FillWindowFrames` for
> harness geometry. No open M-HARNESS work remains.
> **P3 CLI hygiene: CLOSED 2026-07-27.** L-CLI-DEP, L-PIPE, L-QV, L-EXIT, L-MSG landed as a
> batch; a review follow-up then tightened L-PIPE so non-`BrokenPipe` stdout failures exit
> non-zero instead of warning (new `AppError::Output`, shared `clip_sync::write_report_to_stdout`).
> **L-PUBLISH** closed the same day by owner decision — never publishing to crates.io, so
> `publish = false` went on all three publishable crates, not the CLI alone.
> **M-EPS fixed 2026-07-27** (P2): `TIME_EPS_SECS = 1e-9` added to `domain/diagnostics.rs` and
> applied to all 3 genuine wall-clock sites (`scan_gaps.rs` ×2, `repair_profile.rs`); the ~10
> normalized-quantity `f64::EPSILON` guards deliberately untouched. Behavior-preserving; full
> `cargo test --workspace` green.
> **M-GAPKEY + M-HOUND blast radius checked *and* fixed 2026-07-27** (both previously
> unchecked). **M-GAPKEY**: `gap_index: usize` added to `FillRegion`/`GapFillSkipped`;
> `outcomes_in_report_order`'s three float-bit-pattern `HashMap`s rekeyed to the index and the
> `position()` search in `format_skip_gap_fill_log` deleted along with the `gap_key` helper;
> a characterization test was landed green against the old join first. **M-HOUND**: the real
> string match was at `finalize()`, not the lines cited, and the branch was **unreachable**
> (pre-empted by `validate_pcm_for_wav`) — removed rather than swapped to an enum match.
> Neither was a latent bug; both were fragility/dead code.
> **M-RESAMPLE measured and DOWNGRADED P1 → P3 by owner decision 2026-07-27; WILL NOT FIX at this
> time.**
> The standing recommendation was **withdrawn** (it short-circuits at `rubato.rs:27` and cannot do
> anything); reach is wider than filed (4 sites, one trigger: A and B differing in source sample
> rate) and includes `patch_audio/decode.rs:89`, where the shift lands on *spliced audio*. Then the
> two gating measurements split the verdict: the delay is **real and exact** (2.7–4.0 ms, equal to
> `output_delay()` to the sample) but the asymmetric path has **zero corpus occurrences** (4008
> tracks across 1055 artifacts, all 48 kHz). So: no production change, severity down, and the
> measurement landed as `tests/resample_group_delay.rs`. Also recorded: FFT and linear-fallback
> paths have **different** group delays, so a rubato failure silently moves the offset.
> **No open defect work remains at any severity.** M-EPS / M-GAPKEY / M-HOUND fixed 2026-07-27;
> M-FRAMES withdrawn (premise refuted); M-RESAMPLE downgraded to P3 / will-not-fix. What is left
> is elective, not outstanding: optional M-SILENT machine-readable report flags, the deferred
> `align_videos` module split, and the prepare-clone perf stretch.
>
> **Recommendations** for each remaining item (approach, tests, sequencing) are
> in the P1–P3 sections and **Suggested sequencing** below.
>
> **Context:** Default `cargo clippy --workspace --all-targets` clean on correlator /
> pregate paths. Pedantic clippy produces ~900 mostly cast/docs noise.

Legend: **sev** = P0 (correctness / panic / decode corruption) · P1 (silent wrong
behavior / user-intent override) · P2 (perf / maintainability) · P3 (hygiene).

---

## Executive summary

Architecture quality is high: hexagonal layering is real, ports/adapters are
testable, production paths avoid `unwrap`, and process I/O (ffmpeg) is carefully
threaded. Remaining open defects cluster in:

1. **Silent degradation** (errors swallowed → plausible but wrong outcomes)
2. **Structural debt** (~~3–5 kloc modules~~ **planned M-MOD splits done 2026-07-24** —
   policies / corpus / fingerprint / `patch_audio`; `align_videos` deferred; ~~four-layer config
   field copying~~ **fixed 2026-07-23** — see M-CFG)
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
| ~~1~~ | ~~M-RESAMPLE~~ | ~~P1~~ → **P3, will not fix** | Resampler group delay uncompensated. **Measured 2026-07-27**: delay real (2.7–4.0 ms, = `output_delay()` exactly) but **zero corpus occurrences** (all tracks 48 kHz). Latent-only; **downgraded and closed by owner decision** — characterization test landed, fix pre-derived for if a cross-rate pair ever appears | `resample/rubato.rs`, `offset_refinement.rs:420-430`, `patch_audio/decode.rs:89` |

**The priority list is now empty.** Every row above is fixed, withdrawn, or downgraded-and-closed.
| ~~2~~ | ~~M-GAPKEY~~ | — | **fixed 2026-07-27** — `gap_index` on `FillRegion`/`GapFillSkipped`; 3 maps rekeyed, `gap_key` + `position()` search deleted | — |
| ~~3~~ | ~~M-FRAMES~~ | — | **withdrawn 2026-07-27** — three correct conversion classes, not one inconsistency | — |
| ~~4~~ | ~~M-EPS~~ | — | **fixed 2026-07-27** — `TIME_EPS_SECS` in `domain/diagnostics.rs`, 3 sites | — |
| ~~5~~ | ~~M-HOUND~~ | — | **fixed 2026-07-27** — unreachable 4 GiB branch removed from `wav_writer.rs` `finalize()` | — |

**M-RESAMPLE was the last remaining ledger item; as of 2026-07-27 it is measured, downgraded to
P3, and closed will-not-fix.** No P0/P1/P2/P3 defect work remains open.

*(**All P3 CLI hygiene is now closed** — L-CLI-DEP / L-PIPE / L-QV / L-EXIT / L-MSG landed
2026-07-27, and L-PUBLISH closed the same day by owner decision. See the Fixed (P3) table.)*

*(M-HE + M-FDK-RESET + M-AC3-DRAIN fixed 2026-07-23 — all codec P1s closed. M-SILENT
effectively closed 2026-07-23 — only optional report flags deferred. **M-FFT closed
2026-07-23** as hygiene. **M-DEAD closed 2026-07-23**. See Fixed tables. **All P1s are
now closed: M-EPS, M-GAPKEY and M-HOUND landed 2026-07-27, closing the P2 set, and
M-RESAMPLE measured out to latent-only and downgraded to P3/will-not-fix the same day —
leaving only optional report flags.** Material *repair*
wall-time is `unified_refine_*` / lever 1c (~61% of root post–M-CLONE #2), not the
closed M-CLONE bites. **M-CFG** / **M-MOD** / **M-HARNESS** closed earlier — see
sections below.)*

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

### M-RESAMPLE. Group delay (count clear done) — **P1 → P3, WILL NOT FIX (2026-07-27)**

> **Status: MEASURED, DOWNGRADED P1 → P3, and CLOSED WILL-NOT-FIX by owner decision 2026-07-27.**
>
> Both gating measurements are done and they split: the delay is **real and exactly characterized**
> (2.7–4.0 ms, matching `output_delay()` to the sample), but the asymmetric path **never runs on
> the corpus** — all 4008 recorded tracks across 1055 artifacts are 48 kHz, zero cross-rate pairs.
> Per the plan's step 4 this is the M-FRAMES outcome: record the numbers, downgrade the severity,
> keep the measurement as a characterization test.
>
> **The concern is deliberately retained, not dismissed.** It is documented in full below —
> mechanism, all four affected sites, exact magnitudes, and a pre-derived fix — because the
> premise is sound and only the *exposure* is absent. This is a bet on the input distribution,
> and the bet is stated so it can be revisited rather than rediscovered.
>
> **Re-open this entry if any of the following becomes true:**
> - a media pair with **differing source audio sample rates** enters the corpus or is reported by
>   a user (the single trigger for all four sites);
> - `tests/resample_group_delay.rs` starts failing (a rubato bump changed the delay or broke the
>   `output_delay()` equality the fix depends on);
> - cross-rate repair output is reported as misaligned at fill seams — the
>   `patch_audio/decode.rs:89` symptom, which is audible rather than merely metrological.
>
> Until then: **no production change.** Details in *Measurements* below.

**The previous recommendation was a no-op and is withdrawn.** It read: *"when one side is already
at the target rate, still run both through the same resample path."* `resample_mono_pcm`
short-circuits on rate equality (`infrastructure/resample/rubato.rs:27` — `if clip.sample_rate ==
target_rate { return clip.clone() }`), so routing the already-at-rate side "through the same path"
returns it unchanged. Making that recommendation real would require forcing a degenerate N→N FFT
resample purely to incur a *matching* delay — paying an FFT to manufacture an error in order to
cancel it. Wrong direction; do not re-open from that framing.

**One trigger condition, four sites.** The asymmetry is `offset_refinement.rs:420-430`:
`target_rate = left.sample_rate.max(right.sample_rate)`, so exactly one side takes `clone()`
(zero delay) and the other takes the FFT path (nonzero delay). Every instance in the workspace
reduces to the same precondition — **A and B have differing source audio sample rates**:

| Site | Consequence if delay is uncompensated |
|------|----------------------------------------|
| `application/offset_refinement.rs:424,429` (`pcm_cross_correlate_lag`, feeding `refine_holdout_segment_lag`) | constant bias in the refined lag — the finding as originally filed |
| `application/align_videos.rs:991`; `application/offset_verification.rs:208,212,597,601` | `clip.target_sample_rate` resamples *both* sides, but each short-circuits **independently** — so mismatched sources still land asymmetric here. Setting `target_sample_rate` does **not** imply symmetry |
| `clip-sync-repair` `application/patch_audio/decode.rs:89` (`resample_interleaved`) | B's spliced **audio content** is time-shifted on every cross-rate fill |

**Re-frame around that last row.** It is not metrology bias — it is a real constant time shift on
patched output whenever the pair's rates differ, and the seam gate would read it as lag, plausibly
declining otherwise-good fills on cross-rate pairs. That is a higher-value framing than the
alignment-only one this entry has carried.

**Second defect, not previously recorded: the two resample paths disagree on delay.**
`linear_resample_fallback` (`rubato.rs:83-119`, and `linear_resample_f32:191`) is plain linear
interpolation — essentially zero group delay — while the `FftFixedIn` path is not. A rubato
init/process failure therefore *silently moves the measured offset*, on a path whose own `warn!`
presents it as a transparent substitute. This is the argument for compensating **to zero** rather
than for equalizing delay across the two sides.

**The fix is smaller than "full delay query/trim" assumed.** rubato 0.16.2 exposes
`Resampler::output_delay()` (delay in output frames); the code never calls it. Dropping that many
leading output frames inside `resample_mono_pcm` and `resample_f32_plane` makes resampling
delay-neutral — which fixes all four sites at once, makes symmetric-vs-asymmetric irrelevant, and
brings the FFT path into agreement with the linear fallback. The prior "more work for little gain"
judgement was made without this API in view.

### Measurements — 2026-07-27

**1. Delay magnitude — real, and `output_delay()` is exact.** Chirp through
`RubatoResampler::resample_mono`, cross-correlated against a zero-delay linear reference:

| Rate pair | Measured lag | As time | Peak corr | `output_delay()` |
|-----------|--------------|---------|-----------|------------------|
| 44100 → 48000 | 160 samples | 3.333 ms | 0.9993 | **160** |
| 48000 → 44100 | 147 samples | 3.333 ms | 0.9995 | **147** |
| 32000 → 48000 | 192 samples | 4.000 ms | 0.9970 | **192** |
| 48000 → 96000 | 256 samples | 2.667 ms | 0.9994| **256** |

Rubato's reported `output_delay()` equals the measured lag **exactly** at every pair — it is not an
estimate. That is the load-bearing fact for the fix: dropping `output_delay()` leading frames would
remove the lag completely, with no residual to tune.

Note the delay is **not constant in time** across pairs (2.667–4.000 ms). Consequence: the
`clip.target_sample_rate` path (default `Some(11_025)`, `config.rs:12`) resamples *both* sides, but
the two delays only cancel when the sources share a rate. Mismatched sources leave a **differential**
delay even there — so it is still the same single trigger, not a second one.

**Materiality if it ever fires:** 3.3 ms against `high_rate_refine_max_adjustment_secs` = 0.1 s is
~3% of the adjustment budget — a real bias, well above noise, but not budget-breaking. Against
`fill_seam_search_secs` = 0.25 s it is ~1.3%. The sharper concern remains `patch_audio/decode.rs:89`,
where it is a fixed shift on spliced audio rather than a metrology error.

**2. Trigger census — the asymmetric path never runs.** Across the full gap-files corpus:
**1055 artifact files, 4008 recorded `sample_rate` values, every one 48000.** No other rate appears
under any field name. Since nothing resamples unless the pair's rates differ, the asymmetric path
has **zero occurrences** in the corpus. The bug is latent, not active.

**Verdict: downgraded to P3 and closed will-not-fix (owner decision 2026-07-27).** The premise
survives (unlike M-FRAMES) but the exposure does not. Fixing would mean changing four call sites
and every consumer's sample alignment to correct a bias that no observed input can produce. The
cheap fix stays documented and ready.

**Caveat on the census — this is a bet, and it is on the input distribution.** The 48 kHz result
describes *this corpus*, not the space of inputs the tool accepts. A 44.1 kHz web rip paired
against a 48 kHz broadcast source is entirely plausible in the wild and would trigger every site in
the table above on first contact. The downgrade says the bug is not worth pre-emptive work, **not**
that it cannot happen.

**Landed instead:** `crates/clip-sync/tests/resample_group_delay.rs` — two characterization tests
pinning (a) measured delay == `output_delay()` at four rate pairs, within a 1–10 ms band, and (b)
the FFT-vs-linear-fallback disagreement. A rubato bump that changes either now fails loudly instead
of silently shifting offsets.

**If a cross-rate pair ever shows up**, the fix is pre-derived: drop `output_delay()` leading output
frames in `resample_mono_pcm` and `resample_f32_plane`, then flip
`fft_and_linear_paths_disagree_on_group_delay` to assert agreement at ~0. Re-read the coupling note
below before landing it.

**Coupling to know before landing the compensation (deferred).** Trimming `output_delay()` frames shortens output, and
`holdout_extract_sufficient` / `min_end_clip_decode_fraction` gate on clip length. The delay is
small against second-scale segments, so this is a footnote rather than a blocker — but a marginal
clip could cross a threshold.

**Already done:** the "count clear" half — `decoded_sample_count: None` after both resample paths
(`rubato.rs:79,117`). What remains is purely the delay half.

**Test:** `crates/clip-sync/tests/resample_group_delay.rs` (landed 2026-07-27, 2 tests, green).
If the compensation is ever implemented, add the asymmetric case (one side at target rate, one not)
with offset bias asserted at ~0 rather than merely "shrunk".

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

**Perf note:** Do **not** conflate this with repair's gate-search FFT work
(`char_gate_search` / `TEMP-production-repair-perf-plan.md`). That is a different call
site and already landed large wins; remaining *material* repair wall-time → **lever 1c**
(`k`-reduction), not M-CLONE. Unused `slide_template_scores` removed under **M-DEAD §2**
(2026-07-23); do not reintroduce a PHAT slide into discover without retune.

### M-CLONE. Optional alloc hygiene — **complete 2026-07-25**

Three **independent** PR-sized bites. Do not bundle. #1+#3 done 2026-07-23; #2 done
2026-07-25 (code + 17-pair perf verify). Prepare-clone stretch remains deferred.

1. **Align full-clip clones** — **done 2026-07-23.** `truncate_padded_tail` takes
   `&MonoPcmClip` → `Cow` (allocates only when padding must be dropped). Align loop
   borrows End/non-End refs into `select_aligned_subclip_pair` / prepare; no redundant
   pre-clone. Extracted `raw_clips` stay owned for repetition re-align.
2. **`FftPlanner` reuse on GCC-PHAT path** — **done 2026-07-25.**

   **As landed:** `thread_local!` + `RefCell<FftPlanner<f64>>` in
   `crates/clip-sync/src/infrastructure/correlation.rs`; core in
   `fft_cross_correlation_with_planner`. `FftCorrelator` remains a ZST. Module docs cite
   M-CLONE #2. Lag refine (`cross_correlate`) and `seam_local` planner untouched.

   **Correctness:** `gcc_phat_thread_local_planner_matches_fresh_planner_oracle`
   (bit-identical vs fresh planner); `gcc_phat_repeated_calls_same_lengths_are_stable`;
   correlator module tests 5/5.

   **Perf** (`measure-repair-perf`, 17-pair, release, `--wav`, `CLIP_SYNC_SPAN_TIMING=1`;
   same call counts on the xcorr path — n=1335):

   | Metric | Before (2026-07-25 GO) | After (#2) | Δ |
   |--------|-----------------------:|-----------:|--:|
   | root (`patch_audio`) | 8937 s | 8449 s | −5.5% |
   | `local_anchor_xcorr` | **846 s / 9.5%** | **358 s / 4.2%** | **−58%** |
   | `anchor_matchability` | 848 s / 9.5% | 360 s / 4.3% | −58% |
   | xcorr share of matchability | 99.7% | 99.3% | — |

   Post-change exclusive hotspots (ALL PAIRS): `unified_refine_start`+`_end` ≈ **61.5%**
   of root (Level F: Repeat ≈99.6% of refine-start — lever 1c / `fill_repeat_correlations`);
   decode A+B ≈25%; `local_anchor_xcorr` now **4.2%**. No Instant planner-vs-FFT split
   needed — memo delivered a material slice of the envelope.

   **Instrumentation** (2026-07-24): `anchor_matchability` + `local_anchor_xcorr` spans;
   harness always rolls both up.

   **Out of scope (still):** share memo with `seam_local` / `unified_refine`; multi-lag
   rewrite of `local_anchor_xcorr_peak`; threshold changes; prepare-clone stretch.
3. **FDK ADTS+payload scratch** — **done 2026-07-23.** `AacDecoder` holds reusable
   `adts_scratch: Vec<u8>`; `construct_adts_header` returns `[u8; 7]` (no heap);
   `decode_ref` clears/extends the scratch then `fill(&self.adts_scratch)`. Capacity
   survives `reset()`. AC-3 half already landed via `pcm_scratch` under M-AC3-DRAIN.
   Feature-gated `he-aac` only.

**Verified (#1):** `truncate_padded_tail_*` (borrow vs owned) +
`application::align_videos::tests` green.

**Verified (#3):** `he-aac,ffmpeg-tests,test-utils` lib suite green (346 pass / 15
ignored), including all `construct_adts_header_*` unit tests and
`fdk_reset_backward_seek_reprimes_to_identical_steady_state`.

**Verified (#2):** correlator oracle tests + 17-pair before/after table above.

**Do not (prepare-clone stretch):** Changing `prepare_clip_for_fingerprint` to take
owned `MonoPcmClip` (or dropping its internal clone) is **not** a worthwhile follow-up
to #1. Align’s common path must keep borrowing `raw_clips` for repetition re-align, so
by-value prepare would just `.clone()` at the call site. Defaults are
`normalize_loudness` + `trim_silence` **on**; after the prepare clone,
`trim_trailing_silence` / `peak_normalize` often allocate again — owning into prepare
without rewriting those helpers does not remove the dominant allocs. Most other callers
(`offset_verification`, `locate_query`, tests) also prefer `&`.

**If ever reopened (profile-gated only):** Rewrite `trim_trailing_silence` /
`peak_normalize` to mutate `&mut MonoPcmClip` (or take `Cow<'_, MonoPcmClip>`), *then*
consider `prepare_clip_for_fingerprint(Cow<'_, MonoPcmClip>, …)`. Do not flip prepare’s
API on its own; only pursue after a profile shows prepare/trim/normalize allocs matter
on a realistic align/query workload.

### M-CFG. Four-layer config field copying — **fixed 2026-07-23**

**Record:** [`archive/TEMP-repair-config-bundles-plan.md`](archive/TEMP-repair-config-bundles-plan.md)
(closed; as-landed notes + standing invariants).

**What fixed it — collapsing layers, *not* the proposed policy bundles.** The bundles were
never built and are explicitly declined: once the duplicated layers were gone, grouping the
remaining flat fields was organization rather than correctness.

- `PatchAudioRequest` 58 fields → **3** (`report` + embedded `PatchRequestSettings` +
  `measure_residual`), read-only `Deref`, no `DerefMut`; `into_request` 56 lines → 4 (`92571bc`,
  guards `e02dfad`).
- `SeamGateConfig` (53 `pub` fields — a near-twin of settings, not a projection) **deleted**;
  `SeamGateParams` borrows `&PatchRequestSettings` alongside a 13-field `SeamGateDerived` that is
  frame math only (`abb4bd2`).
- Both surviving `PatchRequestSettings` literals (harness `patch_request_with_options`,
  `query_reference_integration`) seeded from `..RepairConfig::default().patch_settings()`
  (`47cef0e`) — this also closes **M-HARNESS item 1**.

Net: three hand-written conversion lists → **one** (`RepairConfig::patch_settings`). A new knob
has one definition and one seed, so it cannot drift.

**Standing invariants:** `PatchAudioRequest { … }` literal appears exactly once (in
`into_request`); no `DerefMut`; every `PatchRequestSettings` literal seeds from
`patch_settings()`; `SeamGateDerived` carries no policy.

**Test:** `config_roundtrip` + `-Tier pr-repair` (byte-preservation) + `patch_audio_integration`
/ `anchor_seam_oracle`. Guards: `deref_reads_reach_embedded_settings_and_are_not_shadowed`,
`into_request_defaults_measure_residual_off`, `gate_mode_ignores_fill_fit_knobs`.

**Deliberately left open (optional, undated):** eight test-local default overrides in the two
seeded literals await a justify-or-drop comment audit. Not a coverage hole — production settings
are exercised separately via `patch_request_from_repair`. See the archived record §4.

### M-MOD. Oversized modules — **closed**

**Recommendation:** All planned M-MOD bites are complete. `align_videos` stays deferred
(no plan; test-inflated orchestrator already decomposed via sibling application modules).
Do not open an `align_videos` split unless that changes.

1. ~~**Harness corpus first** (mechanical warm-up) — `gap_fingerprint_corpus` → schema /
   analysis / report~~ **DONE (2026-07-23)** —
   [`TEMP-gap-fingerprint-corpus-module-split-plan.md`](archive/TEMP-gap-fingerprint-corpus-module-split-plan.md)
   P1–P4 complete: `gap_fingerprint_corpus/{mod,schema,analysis,report}.rs`, thin facade, public
   path + byte-preserving verified, harness lib 13/13 + `clip-sync-repair --all-targets` green.
   Re-verified 2026-07-24 (success criteria checked against source).
2. ~~**Production `gap_fingerprint` second** (judgment-heavy) — schema / measure / project~~
   **DONE (2026-07-23)** —
   [`TEMP-gap-fingerprint-module-split-plan.md`](archive/TEMP-gap-fingerprint-module-split-plan.md)
   P1–P3 complete: `application/gap_fingerprint/{mod,schema,project,measure}.rs`, thin facade,
   public path `crate::application::gap_fingerprint::*` unchanged, `schema ← project ← measure`
   DAG (`tags_from_measurements` in `measure`). Re-verified 2026-07-24 (success criteria
   checked against source).
3. ~~**Optional `patch_audio`** — request / decode / geometry / log / region / anchor_retry~~
   **DONE (2026-07-24)** —
   [`TEMP-patch-audio-module-split-plan.md`](archive/TEMP-patch-audio-module-split-plan.md)
   P1–P6 complete: `application/patch_audio/{mod,request,decode,geometry,log,region,anchor_retry}.rs`;
   facade holds `PatchAudio` orchestration only; contested helpers and DAG verified in source
   2026-07-24; public `patch_audio::` / `application::{PatchAudio,…}` paths unchanged.
4. ~~**Analyzer `clip-sync` `policies.rs`** — track selection / clip planning / extract quality /
   hold-out~~ **DONE (2026-07-24)** —
   [`TEMP-clip-sync-policies-module-split-plan.md`](archive/TEMP-clip-sync-policies-module-split-plan.md)
   P1–P4 complete: `domain/policies/{mod,track_selection,clip_planning,extract_quality,holdout}.rs`;
   thin facade; public `crate::domain::policies::*` unchanged; `holdout ← clip_planning`
   (`secs_to_duration` only); 46/46 policies lib tests + `-Tier pr-align` green.
   Re-verified 2026-07-24 (success criteria checked against source; plan archived).

Pure moves + `pub(crate)` — no behavior change. Optionally curate repair `lib.rs` like
`clip-sync` (not opened).

**Progress (2026-07-24):** repair policies **P1–P5 done** (`7dd0978`) + **M-MOD-DEPS** (`16b06e3`);
harness corpus **done**; production fingerprint **done**; **`patch_audio` P1–P6 done**;
**analyzer `clip-sync` policies P1–P4 done**. Remaining optional M-MOD target: `align_videos`
only (deferred — no plan).

**Test:** `-Tier pr` after each split (historical); plans carry per-phase green notes.

| File | ~Lines |
|------|--------|
| ~~harness `gap_fingerprint_corpus.rs`~~ (split 2026-07-23) | 2,300 |
| ~~`gap_fingerprint.rs`~~ (split 2026-07-23) | 4,000 |
| ~~repair `policies/`~~ (split) | facade + 5 modules (~0.3–1.4 kloc each) |
| ~~`patch_audio.rs`~~ (split 2026-07-24) | 3,600 → facade + 6 submodules (`region` ~2.5 kloc accepted) |
| ~~analyzer `policies.rs`~~ (split 2026-07-24) | 1,725 → facade + 4 modules |
| `align_videos.rs` (optional; no plan) | 2,900 |

#### M-MOD-DEPS. Policies helper placement — **fixed 2026-07-23**

Deliberately **not** done in `7dd0978`, which was byte-preserving by rule. Three misplacements
the split exposed, landed as a separate commit:

| Item | Was | Now | Why |
|------|-----|-----|-----|
| `interleaved_to_mono`, `interleaved_to_channels` | `policies/gap_borders.rs` | `domain/pcm.rs` | Generic interleaved→f64 converters, not border logic. **Imports swept, no facade re-export kept** — 8 files across 3 crates now say `domain::pcm::`, so the honest path is the only path. |
| `seam_score_channel_indices` (+ its `template_mean_square` helper) | `policies/gap_borders.rs` | `policies/seam_scoring.rs` | Energy-based **scoring** channel selection; its unit test already lived in `seam_scoring.rs`. `template_mean_square` moved with it and is `pub(crate)` for `gap_borders::loudest_seam_channel`. |
| `effective_repeat_window_frames` | `pub(crate)` in `seam_scoring.rs` | private `fn` | Used only inside `seam_scoring`; the facade never re-exported it, so the visibility was provably dead. Pre-existing — the split surfaced it. |

**Payoff (realized):** the `seam_scoring → gap_borders` edge is gone. Final DAG —
`silence` / `seam_splice` leaves; `seam_scoring → domain::pcm`;
`gap_borders → {silence, seam_splice, seam_scoring}`; `seam_residual → seam_scoring`.
Both moves had to land together: moving `seam_score_channel_indices` alone would have *flipped*
the edge (via `gap_borders::selected_seam_channels`) rather than removing it.

**Verified:** normalized line-set diff of the five affected files against `7dd0978` — the only
5 differing lines are re-wrapped `pub use` continuations in `mod.rs`; **zero function bodies
changed**. Public API delta is exactly the two intended removals from `policies::`. `cargo
clippy --workspace --all-targets` clean; policies tests still 55/55 with unchanged names; full
`cargo test --workspace` green.

**Docs:** `docs/seam-scoring.md` § Code map re-pointed at the split submodules (it still cited
`domain/policies.rs`, a file that no longer exists) and gained a downmix row.

**Declined (recorded so it isn't re-proposed):** the `interleave_a` test helper duplicated in
`gap_borders` and `seam_residual` test modules (identical 12-line bodies) — a shared
`#[cfg(test)] mod test_support` costs about what it saves. Likewise the 8 double-blank-line
seams left at extraction boundaries: the crate is not rustfmt-managed (752 `Diff in` hunks
workspace-wide), so fixing these 8 adds noise without changing the baseline.

> **Stale as of 2026-07-27:** the workspace *did* adopt rustfmt (commit `7906c8ea`, now CI-enforced),
> so the "not rustfmt-managed" rationale above no longer holds — the double-blank-line seams were
> swept up by that adoption. Kept for the record of why they were declined at the time.

### M-HARNESS. Drift from production — **complete 2026-07-24**

**Recommended order (all done):**

1. ~~`PatchTestOptions` from `RepairConfig::default()` with overrides only~~ — **done
   2026-07-23** (`47cef0e`): both harness and `query_reference_integration` literals now seed
   from `..RepairConfig::default().patch_settings()`. See
   [archive/TEMP-repair-config-bundles-plan.md](archive/TEMP-repair-config-bundles-plan.md).
   Eight surviving overrides are deliberate synthetic-fixture settings pending an undated
   justify-or-drop comment audit — do **not** extend `PatchTestOptions` field-by-field to chase
   them; that is the mechanism that let five of them drift unseen.
2. ~~One shared `gap_interior_range` / oracle validator~~ — **done 2026-07-24**:
   `floor_oracle::{gap_interior_range, validate_gap_interior_peak}`; dual-fit validator calls the
   shared peak check (H6 clamp stays on the range helper).
3. ~~One exported `NeverCalledAligner` + alignment builder in fixtures~~ — **done 2026-07-24**:
   `NeverCalledAligner` + alignment builders live in repair `application::test_support`
   (so unit tests and fixtures share one crate); fixtures `test_align` is a thin re-export.
4. ~~`csv` crate or RFC 4180 quoting for calibration CSV~~ — **done 2026-07-24**: `csv` dep on
   harness + fixtures; `CorpusReport::csv`, W5 sweep CSV, and `w5_timing_offset_csv` use
   `csv::Writer` (regression: `csv_quotes_commas_in_pair_and_ids`). Sweep header is
   [`SWEEP_CSV_FIELDS`] / `sweep_csv_header()` (single source; no parallel string).
5. ~~Delete the collapsed window formula or call production’s window helper~~ — **done
   2026-07-24**: harness `geometry_for` / `correlate_frames_for_gap` delegate to public
   `FillWindowFrames::for_gap` (standoff stays 0 by design); `geometry_tests` locks parity.

**Test:** harness lib (incl. CSV + geometry parity) + `scan_gaps` unit tests.

### Other P2

| ID | Issue | Recommendation |
|----|-------|----------------|
| M-GAPKEY | ~~Float bit-pattern `HashMap` keys for gaps~~ | **done 2026-07-27** — keyed by `gap_index`; `gap_key` + `position()` search deleted, see below |
| M-FRAMES | ~~Inconsistent floor vs `.round()` in secs→frames~~ | **WITHDRAWN 2026-07-27** — premise refuted, see below |
| M-EPS | ~~`f64::EPSILON` as wall-clock tolerance~~ | **done 2026-07-27** — `TIME_EPS_SECS = 1e-9` in `domain/diagnostics.rs`, all 3 sites |
| M-HOUND | ~~String-match on `hound::Error` Display~~ | **done 2026-07-27** — the match was unreachable dead code; branch removed rather than swapped to an enum match, see below |
| M-DEAD | ~~Dead / measurement leftovers~~ | **done 2026-07-23** (§1 B2 + §2) |

### M-FRAMES — **withdrawn 2026-07-27** (premise refuted by blast-radius check)

"Inconsistent floor vs `.round()`" conflates **three distinct and individually correct** conversion
classes. There is no inconsistency to standardize away:

1. **Duration → frame count** — `.round()`. `lag_window_secs`, `fill_seam_search_secs`,
   `border_secs`, `max_refine_secs`, `gap_signature_context_secs` (`measure.rs` 770/978/1070/
   1362-64/2129-30). Nearest whole frame is the right rounding for a window length.
2. **Frame → secs → frame round-trip** — `.round()` is **mandatory**, not stylistic.
   `measure.rs:1380` stores `a_refined_start_secs = refined.start_frame as f64 / rate`;
   2232-33 and `project.rs:537` recover the integer with `.round()`. Flooring here would be a
   *bug* (`480/48000*48000` can land on `479.999…` → 479).
3. **External wall-clock timestamp → frame index** — floor (bare `as usize`).
   `measure.rs` 2156-57 / 2346-47 and production `patch_audio/region.rs:1484-85`. Floor is the
   conventional "frame containing time *t*" semantic, and **the diagnostic and production sites
   already agree** — which is the opposite of what the finding claimed.

**Blast-radius experiment** (applied `.round()` to all six class-3 sites — `measure.rs`
2156/2157/2346/2347 plus production `region.rs` 1484/1485): `golden_baseline_invariance`, `gap_cell_fixtures`, `patch_audio_integration` — **all pass,
29 tests**. That is *not* reassurance. It means the change is invisible to the entire committed
suite because fixture timestamps are frame-aligned, so on real media it would be an unverifiable
±1-frame (~21 µs at 48 kHz) shift in `reported_start_frame` — which feeds `refine_gap_frames`
search windows and can tip a boundary decision — with **zero** test coverage either way.
Not mechanical. Reverted.

### M-EPS — **fixed 2026-07-27**

49 `f64::EPSILON` occurrences, but most must **not** be touched: ~10 are divide-by-zero / energy
guards on normalized quantities (`metrics.rs:25`, `seam_residual.rs:25,65`, `gap_energy.rs:84`,
`seam_scoring.rs:33`, `seam_splice.rs:29,46`, `gap_borders.rs:248`, `offset_refinement.rs:584`,
`seam_local.rs:109`, `patch_anchor.rs:200,207`) where `EPSILON` is correct. `patch_anchor.rs:200`
looks time-valued but guards the `(t1 - t0)` division — leave it. The rest are test assertions.

**Genuine wall-clock sites: 3.** `scan_gaps.rs:170` and `:340`
(`bucket.start_secs > prev_end + f64::EPSILON`) and `repair_profile.rs:158`.

The sharper statement: `f64::EPSILON` is the ULP at 1.0, so **for any `t ≥ 2.0`, `t + f64::EPSILON
== t` exactly** (verified numerically; ULP at 3600 s ≈ 4.5e-13, ~2000× larger than EPSILON). The
tolerance is not merely too small — it is a **no-op**, and both comparisons degenerate to exact
float `>` / `!=`.

**Why that is currently harmless.** `bucket.end_secs` and the next `bucket.start_secs` come from
two different formulas (`extract.rs:429-430`: `n*bucket_secs + len/rate` vs `(n+1)*bucket_secs`),
so a 1-ULP overshoot would spuriously fire `note_pcm_discontinuity()` → `close_open_run()`,
splitting a silence run across a bucket edge and potentially dropping a real gap. Simulated over
40 000 buckets: **21% spurious trip rate at `bucket_secs = 0.1`, but 0% at 0.25/0.5/1/2/5.**
`decode_chunk_secs` is typed `u64` (default 10), so `bucket_secs` is always an exact integer and
the failure is **unreachable today** — the type, not the epsilon, is what protects this.
`repair_profile.rs:158` only gates a human-readable override *note* string.

**Verdict: mechanical, effectively zero blast radius** — a no-op today that documents the intent
and survives someone widening `decode_chunk_secs` to a float. Worth doing; do not oversell it as
a bug fix, and do not let a blanket find/replace touch the guard sites.

**As landed (2026-07-27).** `TIME_EPS_SECS = 1e-9` added to
`crates/clip-sync-repair/src/domain/diagnostics.rs` — the module that already owns the
wall-clock `*_SECS` constants — with a doc comment recording the ULP reasoning and an
explicit "time comparisons only; the divide-by-zero guards keep `f64::EPSILON`" warning so
the next reader does not generalize it. Consumers use the full path
`crate::domain::diagnostics::TIME_EPS_SECS`, matching how `scan_gaps.rs:192` already reaches
`TIMELINE_SKEW_WARN_SECS`; no `domain::` re-export was added.

All three sites converted, no others touched: `scan_gaps.rs` (both bucket-continuity
predicates) and `repair_profile.rs` (the override-note comparison). The ~10 normalized-quantity
guard sites and the test assertions were deliberately left on `f64::EPSILON`.

**Verified:** behavior-preserving by construction (`1e-9` and `f64::EPSILON` both round-trip to
the same result at every reachable input, since `decode_chunk_secs` is `u64`). `cargo fmt --all
--check` clean — note the longer name pushed both `scan_gaps` predicates past the width limit, so
rustfmt re-wrapped them; that is the whole diff at those two sites. `cargo clippy --workspace
--all-targets` clean. `cargo test -p clip-sync-repair --lib` 391 pass / 1 ignored;
`cargo test --workspace` fully green (0 failures across all 33 targets).

### M-GAPKEY — blast radius checked **and fixed** 2026-07-27

Unlike M-FRAMES, this one survives the check. Two corrections to the original framing, then the plan.

**Correction 1 — the finding under-counts the sites.** `gap_key` (`patch_audio/log.rs:12-14`,
`(start.to_bits(), end.to_bits())`) feeds **three** `HashMap<(u64,u64), _>` in
`patch_audio/region.rs:167-169` — `status_by_gap`, `tags_by_gap`, `residual_by_gap` — written at
`region.rs:172,199,222,229-231` and drained at `:236-246`. It is *also* used at `log.rs:148` for
a `gaps.iter().position(…)` **equality search**, not a map lookup, to number the gap in
`format_skip_gap_fill_log`. That fourth site is the one an index change actually simplifies.

**Correction 2 — the current code is not broken.** Float keys are only sound if every producer
holds a bit-identical copy of `gap.video_a_start_secs`, and traced end to end, they do:
- `build_gap_fill_plan` (`gap_fill.rs:62-64,83-85,92-94,104-106,112-114`) copies
  `g.video_a_start_secs` verbatim into both `GapFillSkipped` and `FillRegion` — no arithmetic.
- `FillRegion.a_start_secs` is **never mutated** after construction (only `gain` is, per its doc).
- `region_results` tuples (`patch_audio/mod.rs:218,285`) push `region.a_start_secs` verbatim.
- The lookup side (`mod.rs:221`) is `&request.report.gaps`, the *same* `GapReport` that
  `mod.rs:90` handed to `build_gap_fill_plan`.

The trap that would have made this a live bug is real but avoided: `region.rs:1496-1500` computes
a **refined** `a_start_secs` (`refined.start_frame as f64 / sample_rate`) and shadows the name.
Every logging call site nonetheless passes `region.a_start_secs`, not the shadow
(`region.rs:358, 1641, 1908, 1862`). Had one passed the refined value, its `position()` search
would silently miss and the warn line would lose its `gap N/M` number. So this is **fragility, not
a defect** — which is the honest case for fixing it.

**The change is mechanical because the index already exists.** `build_gap_fill_plan` already
iterates `for (index, g) in report.gaps.iter().enumerate()` (`gap_fill.rs:81`) — it computes
`index` today for `gap_equivalence_at(index)`. So:
1. Add `gap_index: usize` to `GapFillSkipped` and `FillRegion`; populate from the in-scope `index`.
   The one early-return path (`gap_fill.rs:59-71`) needs `.enumerate()` added — trivial.
2. Key the three maps `HashMap<usize, _>`; drain via `gaps.iter().enumerate()`.
3. Pass `gap_index` into the log helpers and **delete** `gap_key` plus the `position()` search
   entirely — `format_skip_gap_fill_log` becomes a direct index format.

**Blast radius is small and fully contained.** `GapFillSkipped` / `FillRegion` are
`clip-sync-repair`-internal: no `serde` derive (only `Debug, Clone, PartialEq`), no use in
`clip-sync`, the harness, or the fixtures. Total footprint is `domain/gap_fill.rs`,
`domain/gap_repair_spec.rs:270` (holds a `Vec<GapFillSkipped>`), `patch_audio/{mod,region,log}.rs`,
and two test literals (`region.rs:2475`, `log.rs` gap fixtures). Public API impact: two struct
literals gain a field — a compile error at every site, never a silent one.

**Bonus correctness the swap buys.** Two gaps with identical `(start, end)` currently **collide**:
the maps are drained with `remove()`, so the second gap would take `NotPlanned/NotFillable`.
Silence-run scanning emits disjoint ordered runs, so this is unreachable today, but index keying
removes the class outright rather than relying on that invariant.

**Coverage gap to close while doing it.** `outcomes_in_report_order` has **no direct unit test** —
it is only exercised transitively through `patch_audio_integration`. Add a table-driven unit test
(mixed skipped/patched, report order ≠ plan order) *before* the refactor, so the change is
verified rather than merely compiled.

**Verdict: do it.** Mechanical, compiler-enforced, contained in one crate, and it deletes code
(`gap_key` + a linear search) rather than adding any. Land the unit test first.

**As landed (2026-07-27)** — followed the plan above exactly, in that order:

1. **Characterization test first.** `region.rs::outcomes_in_report_order_joins_every_gap_by_identity_not_position`
   — four gaps, plan and region-result orders both deliberately disagreeing with the report,
   covering all four provenance classes (plan skip / patched / region skip / absent-from-both).
   Verified **green against the old float-keyed join** before any production change.
2. `gap_index: usize` added to `FillRegion` and `GapFillSkipped`, populated at all five
   construction sites in `gap_fill.rs`; the early-return path gained the `.enumerate()` it lacked.
3. The three maps in `outcomes_in_report_order` are now `HashMap<usize, _>`, drained via
   `gaps.iter().enumerate()`. `region_results` changed shape from
   `(f64, f64, RegionPatchOutcome, GapTags)` to `(usize, RegionPatchOutcome, GapTags)` — the two
   float fields existed only to build the key, so the join no longer carries redundant identity.
4. `gap_key` **deleted**. `format_skip_gap_fill_log` takes `gap_index` and formats directly (an
   out-of-range index drops the `N/M` prefix rather than mislabelling); `log_skip_gap_fill` now
   takes `&FillRegion` instead of a loose start/end pair, so the refined-shadow trap described
   above is no longer expressible at a call site. `MarginalGapFillLog` gained `gap_index`.

Touched beyond the predicted footprint: `patch_audio/anchor_retry.rs` (tuple arity in
`build_patch_anchor_candidates`, `anchored_retry_gap_indices`, `AnchoredRetryState`, and its
`.2`/`.3` field writes) — mechanical, and the compiler found every one. Note
`PatchAnchorCandidate::gap_index` there is a *region* index, unrelated to the new report index;
left as-is deliberately.

`cargo fmt --all` / `cargo clippy --workspace --all-targets` clean; `cargo test --workspace` green,
with the new characterization test still passing after the rekey.

### M-HOUND — blast radius checked **and fixed** 2026-07-27: the branch it targeted was UNREACHABLE

The ledger's hedge was right about the wrong lines, and the real finding is bigger than the one
proposed.

**`wav_writer.rs:38-44` is not a string match** — as suspected, those two closures only *format*
the error into `io::Error::other`. Nothing branches there. That half of the finding is void.

**But a genuine string match does exist**, at `finalize()` (`wav_writer.rs:62-72`):
`message.contains("not a multiple of the number of channels")`, which drives a "you probably hit
the 4 GiB WAV limit — use --mux" hint. Verified against hound 3.5.1: that Display string belongs
uniquely to `hound::Error::UnfinishedSample` (`lib.rs:405-407`), and `Error` is a plain
`#[derive(Debug)]` enum — **not** `#[non_exhaustive]`. So `matches!(e, hound::Error::UnfinishedSample)`
is exactly equivalent and can never drift on a locale or wording change. The mechanical swap is safe.

**The larger result: the branch cannot fire.** `WavPatchedAudioWriter::write` calls
`validate_pcm_for_wav` (`wav_writer.rs:17`) *before* writing anything, and that function already
rejects both preconditions for `UnfinishedSample`:
- `validate_pcm_layout` (`pcm.rs:18-28`) rejects `samples.len() % channels != 0` — the only way
  hound's `(data_bytes_written / bytes_per_sample) % channels != 0` check (`write.rs:501-505`)
  can trip on an honest write.
- `validate_pcm_for_wav` (`pcm.rs:37-44`) rejects `data_bytes > u32::MAX` — precisely the overflow
  that would make hound's `u32` `data_bytes_written` wrap and produce a *spurious*
  `UnfinishedSample`. This is the actual 4 GiB case, and it already errors with a **better**
  message ("...exceeds the 4.00 GiB classic WAV limit; use --mux ... instead of --wav").

The writer emits exactly `audio.samples.len()` samples, so `data_bytes_written` is validated
`<= u32::MAX` *and* a multiple of `channels` — `UnfinishedSample` is unreachable. The `finalize()`
special case is **dead code duplicating two earlier guards with an inferior message**.

**Coverage:** zero. No test touches the `finalize()` error path (`wav_bit_depth_integration.rs` and
`patch_audio_integration.rs` only exercise success). The two guards that *do* fire are tested
(`validate_pcm_layout_rejects_partial_frame`, `validate_pcm_for_wav_rejects_payload_over_limit_{16,24}bit`).

**Verdict: reframe, don't just swap.** The enum match is a safe 1-line improvement, but landing
only that would preserve dead code and imply the hint is load-bearing. Preferred fix: collapse the
`finalize()` handler to the plain `write_err` mapping and drop the 4 GiB special case, since
`validate_pcm_for_wav` owns that message and owns it better. If the hint is kept as
belt-and-braces, use `matches!(e, hound::Error::UnfinishedSample)` and add a comment saying it is
unreachable-by-construction — do **not** leave it looking like a live path. Either way this is
hygiene with no behavioral effect, so it is the lowest-value of the three; the string match is
not a latent bug.

**As landed (2026-07-27):** took the preferred fix — `writer.finalize().map_err(write_err)?`, with
a comment recording *why* there is no special case (both trigger conditions are already rejected by
`validate_pcm_for_wav`, with a better message). The string comparison is gone entirely, so the
enum-match question is moot. No behavior change on any reachable input; `cargo test --workspace`
green.

### M-DEAD. Dead symbols — **fixed 2026-07-23** (both bites)

**1. Anchor pre-gate measurement stack** — **removed (B2)**

Lever was NO-GO (0/~4939 brackets doomed vs 46% ceiling). Deleted the measurement-only
stack rather than leaving env-gated scaffolding:

| Removed | Where |
|---------|-------|
| `anchor_bracket_matchability_doomed` + `MATCHABILITY_PREGATE_EPSILON` + 3 unit tests | `gap_anchor_seam.rs` |
| `pregate_doomed`, `CLIP_SYNC_BRACKET_STATS` helpers, emit/timer path | `patch_region.rs` |
| `scripts/measure-anchor-brackets.ps1` | moved to `docs/dev/archive/measure-anchor-brackets.ps1` (retired) |

NO-GO numbers remain in `archive/TEMP-anchor-pregate-plan.md`. Production
`anchor_bracket_both_matchable*` gate path is unchanged.

**2. `PcmCorrelator::slide_template_scores`** — **fixed 2026-07-23**

Removed the unused trait method, `gcc_phat_slide_scores`, repair forwarder, and test
stubs. `PcmCorrelator` keeps `cross_correlate_lag` / `segment_similarity` (lag refine,
holdout, `local_anchor_xcorr_peak`). Discover stays Pearson — do not reintroduce a
PHAT slide without a corpus pass + threshold retune.

| File | Change |
|------|--------|
| `clip-sync/.../application/ports.rs` | Dropped method (+ rustdoc) |
| `clip-sync/.../infrastructure/correlation.rs` | Dropped method impl + `gcc_phat_slide_scores`; kept GCC-PHAT similarity / FFT lag path; module docs updated |
| `clip-sync/.../testing/fakes.rs` | Dropped `FakePcmCorrelator` stub |
| `clip-sync/.../offset_verification.rs` | Dropped `SequencePcmCorrelator` stub |
| `clip-sync-repair/.../domain/ports.rs` | Dropped method |
| `clip-sync-repair/.../infrastructure/correlation.rs` | Dropped forwarder |
| `offset_refinement.rs` + this ledger | Softened wording (method gone) |

**Verified (§2):** `cargo test -p clip-sync --lib` (320 pass / 15 ignored) +
`cargo test -p clip-sync-repair --lib` (383 pass / 1 ignored).
**Verified (§1 B2):** `cargo test -p clip-sync-repair --lib` (380 pass / 1 ignored;
−3 pregate unit tests removed); clippy clean on `--lib`.

---

## P3 — Hygiene (selected)

Batch in a cleanup PR whenever touching CLI / `Cargo.toml`.

| ID | Issue | Recommendation |
|----|-------|----------------|
| L-CLI-DEP | ~~Unused `thiserror` in `clip-sync-cli`~~ | **done 2026-07-27** — dropped from `Cargo.toml` (zero `src` references) |
| L-PIPE | ~~`println!` panics on broken pipe~~ | **done 2026-07-27** — shared `clip_sync::write_report_to_stdout`; non-`BrokenPipe` failures exit 4 |
| L-QV | ~~`--quiet --verbose` compose incoherently~~ | **done 2026-07-27** — clap `conflicts_with` on both CLIs |
| L-EXIT | ~~Redundant `NoAudioTracks` exit-code arm~~ | **done 2026-07-27** — redundant arm deleted (it did match, but produced the same 3 as the catch-all; code 3 unchanged) |
| L-MSG | ~~Fingerprinter "greater than 1001" vs check `< MIN`~~ | **done 2026-07-27** — now "at least 1001 Hz" |
| L-PUBLISH | ~~CLI missing `publish = false`~~ | **done 2026-07-27** — applied to all three publishable crates, not the CLI alone. See below |
| M-DEAD / L-pregate | ~~Pregate measurement stack~~ | **done 2026-07-23** (B2 full remove) |
| M-DEAD / L-slide | ~~Unused `slide_template_scores`~~ | **done 2026-07-23** |

### L-PUBLISH — closed 2026-07-27

The original "match sibling internal crates" framing was wrong. Only `clip-sync-repair-harness`
and `clip-sync-repair-fixtures` carried `publish = false`, and both are genuinely internal test
crates. `clip-sync`, `clip-sync-repair`, and `clip-sync-cli` all carry full
`description` / `license` / `repository` metadata — they read as publishable *by intent*, so
adding the flag to the CLI alone would have made the set **less** consistent, not more.

**Resolved by decision (owner, 2026-07-27): these crates are never going to crates.io.**
`publish = false` therefore lands on **all three**, at the same position as the sibling crates
(after `edition`, before `description`). All five workspace crates now carry it.

The descriptive metadata stays — it is useful in `cargo metadata`, `--help`, and the repo itself
regardless of registry intent. `publish = false` is what makes intent unambiguous.

**Verified:** `cargo publish -p clip-sync-cli --dry-run` now refuses with
`` `clip-sync-cli` cannot be published `` — the guard bites rather than merely being present.
`cargo build`/`test`/`clippy` are unaffected: path dependencies inside a workspace never consult
the registry, and no crate here was ever consumed from one.

### Fixed (P3 CLI hygiene) — 2026-07-27

| ID | What | Evidence |
|----|------|----------|
| **L-CLI-DEP** | `thiserror = "1"` removed from `clip-sync-cli/Cargo.toml` — it was declared but never referenced in `src` | `cargo clippy --workspace --all-targets` clean; workspace tests green |
| **L-PIPE** | Both CLIs render to a `String`, then call the **shared** `clip_sync::write_report_to_stdout` (`infrastructure/stdout.rs`), which does `write_all` + `flush` on a locked stdout and returns `io::Result`: `BrokenPipe` → `Ok(())`, **any other error propagates**. Analyzer maps it to a new `AppError::Output` (exit **4**); repair to the existing `RepairError::Io` (exit **4**). Replaces 1 `println!` + 1 `print!` (analyzer) and 1 `println!` + 2 `print!` (repair). **Output bytes unchanged** — the JSON path's `println!` newline is now an explicit `\n` in the rendered string | `infrastructure::stdout::tests::{broken_pipe_is_success, other_write_failures_propagate, successful_write_emits_exact_bytes}`; existing golden output tests (`clip-sync-cli/tests/cli_output.rs`) green unchanged; measurement bin `equivalence_calibration.rs` deliberately left on `println!` (not user-facing) |
| **L-QV** | `#[arg(short, long, conflicts_with = "verbose")]` on `quiet` in **both** CLIs. Analyzer was the incoherent case (verbose set `show_diagnostics = true`, then quiet overrode only `progress`); repair had the same shape via `args.verbose` reaching `print_repair_output` independently of `progress` | `quiet_and_verbose_together_are_rejected` in both `cli/args.rs` test modules — asserts `ErrorKind::ArgumentConflict` for **both** `--quiet --verbose` and `-q -v`, plus all four spellings still parsing alone |
| **L-EXIT** | `AppError::Domain(DomainError::NoAudioTracks) => 3` deleted. It was **redundant, not shadowed** — being first, it did match, but it produced the same `3` as the `Domain(_)` catch-all behind it, so removing it cannot change any exit code. Now-unused `DomainError` import dropped | `cli_output.rs` exit-code tests green |
| **L-MSG** | `map_reset_error`'s `SampleRateTooLow` message now reads "must be at least 1001 Hz", matching the `sample_rate < MIN_SAMPLE_RATE` guard (1001 itself is *accepted*). The sibling message in `validate_clip` ("below minimum") was already correct and is untouched | `fingerprinter.rs`; no test asserted the old string |

**Not covered by a test:** the broken-pipe path *end to end*. The `ErrorKind` classification is
now unit-tested against a synthetic failing writer (`broken_pipe_is_success` /
`other_write_failures_propagate`), but no test spawns a real process into a closed pipe —
that needs a harness neither CLI suite has, and the Windows failure mode differs from Unix
`EPIPE`. Byte-identity of normal output *is* covered by the existing goldens.

#### Review follow-up (2026-07-27): stdout failures are no longer silent

The first cut of L-PIPE warned via `tracing::warn!` and returned `Ok(())` for **every** non-pipe
write failure, so `clip-sync … > out.json` on a full disk exited **0** next to a truncated file.
That is the one combination a script cannot defend against, so it was tightened:

- `write_report_to_stdout` returns `io::Result`; only `BrokenPipe` is swallowed.
- Analyzer gained `AppError::Output(#[source] io::Error)` → exit **4**.
- Repair reuses `RepairError::Io` → exit **4** (its doc comment already read "Output or report
  I/O failure").
- Both map to **4**, matching each CLI's existing I/O bucket. A distinct code was considered and
  declined: it would fork the two binaries over one failure mode without telling a script
  anything it could act on differently.
- `docs/error-mapping.md` updated — including the previously absolute claim that no report is
  printed on failure, which `Output` makes untrue (stdout may hold a partial report; that is
  precisely why it exits non-zero).

The helper was also **de-duplicated** into `clip_sync::write_report_to_stdout` rather than living
twice — with the error path now load-bearing, two copies could drift into two different
exit-code behaviors for the same failure.

**Reviewed and deliberately not changed:**

- **`--quiet` + TOML `show_diagnostics = true` still compose.** This is correct, not a leftover.
  `quiet` governs *progress noise on stderr*; `show_diagnostics` governs *report detail on
  stdout*. They are orthogonal, and the original L-QV defect was precisely that `--verbose`
  **conflated** them. Forcing `--quiet` to clear an explicitly configured `show_diagnostics`
  would re-introduce the conflation in the opposite direction.
- **Repair's `--verbose` help text.** "Show diagnostics and verbose progress" is accurate: the
  progress half goes through `apply_cli_overrides`, and the diagnostics half through
  `args.verbose` reaching `print_repair_output` directly. Two paths, both real — the help
  describes the observable effect correctly.

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

All codec P1s are done, so the structural work is unblocked. M-CFG is closed; planned M-MOD
module splits are closed (`align_videos` deferred only).

1. ~~**M-HE**~~ (done 2026-07-23) → ~~**M-FDK-RESET**~~ (done + regression-tested 2026-07-23, recreate-decoder) → ~~**M-AC3-DRAIN**~~ (done + unit-tested 2026-07-23; reusable `pcm_scratch` covers M-CLONE #3 for AC-3). **All codec P1s closed.**
2. ~~**M-SILENT** remainder (unknown TOML keys + `align_videos` warn)~~ — **done 2026-07-23** (only optional report flags deferred)
3. ~~**M-FFT**~~ (done 2026-07-23) — unused discover correlator *arg* removed; port kept;
   Pearson discover unchanged. Anchor pre-gate was NO-GO (2026-07-23); repair
   gate-search FFT already landed separately — do not reopen discover PHAT.
   ~~**M-DEAD**~~ (done 2026-07-23) — §2 `slide_template_scores` deleted; §1 B2 pregate
   measurement stack removed.
4. ~~**M-CFG**~~ (closed 2026-07-23 — layers collapsed; bundles declined).
   ~~**M-MOD-DEPS**~~ (done 2026-07-23, `16b06e3` — policies helpers relocated; `seam_scoring →
   gap_borders` edge removed).
   ~~**M-MOD** planned splits~~ (policies → harness corpus → production fingerprint →
   `patch_audio` P1–P6 done 2026-07-24). `align_videos` deferred (no plan).
   ~~**M-HARNESS**~~ **complete 2026-07-24** (items 1–5; no open remainder).
5. ~~**M-CLONE**~~ — **complete 2026-07-25** (#1+#3 2026-07-23; #2 TLS `FftPlanner` +
   17-pair verify: `local_anchor_xcorr` 846 s/9.5% → 358 s/4.2%). Prepare-clone stretch
   deferred only.
6. ~~**P3** CLI hygiene~~ — **fully closed 2026-07-27** (broken-pipe / quiet-verbose / unused dep /
   redundant exit arm / fingerprinter message / `publish = false` on all three publishable crates).
7. ~~**M-RESAMPLE**~~ — **measured 2026-07-27; downgraded P1 → P3 and closed will-not-fix.** Both
   gating measurements done: delay is real and exact (2.7–4.0 ms = `output_delay()`), corpus
   occurrences are **zero** (all tracks 48 kHz). Characterization test landed
   (`tests/resample_group_delay.rs`); the `output_delay()` compensation is pre-derived and
   deferred until a cross-rate pair appears. Re-open triggers are listed in the section. Do
   **not** re-open from the old "run both sides through the same resample path" recommendation —
   withdrawn as unimplementable (`rubato.rs:27` short-circuits).
   *(larger remaining repair wall-time is `unified_refine_*` / lever 1c — Repeat-dominated
   per Level F.)*
8. ~~**Other P2 remainder**~~ — **fully closed 2026-07-27**. ~~M-EPS~~, ~~M-GAPKEY~~ (unit test
   first, then `gap_index` through `GapFillSkipped`/`FillRegion`, three maps rekeyed, `gap_key`
   and the `position()` search deleted) and ~~M-HOUND~~ (unreachable `finalize()` branch removed)
   all landed. **M-FRAMES is withdrawn, not deferred**: do not re-open it from the original
   one-line framing.

### Milestone checklist

1. ~~**Codec hardening (P0 H2/H3/H5)**~~ / ~~**Panic clamps (P0 H1/H6)**~~ — **done 2026-07-23**.
2. ~~**Config honesty (P1 M-CLI / M-NaN)**~~ / ~~**M-MUX / harness cast / resample count / silent warns**~~ — **done 2026-07-23**.
3. ~~**Codec follow-ups (P1 M-AC3-DRAIN)**~~ — **done 2026-07-23**: drain all AC-3/E-AC-3 frames per packet (`drain_packet` helper + 4 unit tests; real E-AC-3 surround path green). *(M-HE HE-AAC rate cross-check + M-FDK-RESET recreate-decoder also done 2026-07-23; M-FDK-RESET has a verified red→green backward-seek regression test running on stock ffmpeg.)* **All codec P1s closed.**
4. ~~**Observability remainder (P1 M-SILENT)**~~ — **done 2026-07-23**: unknown TOML keys (shared `unknown_toml_keys` in analyzer + repair loaders) and `align_videos` `Ok(None)` warn logging. Optional machine-readable report flags deferred.
5. ~~**M-FFT hygiene**~~ — **done 2026-07-23**: drop unused discover correlator arg; keep
   `PcmCorrelator` for lag refine; leave Pearson. ~~**M-DEAD**~~ — **done 2026-07-23**:
   §2 drop `slide_template_scores`; §1 B2 remove pregate measurement stack.
6. ~~**Structure (P2)**~~ — ~~**M-CFG**~~ **done 2026-07-23**; ~~**M-MOD** planned splits~~ **done**
   2026-07-24 (`align_videos` deferred); ~~**M-HARNESS**~~ **complete 2026-07-24** (all five
   items; no open remainder).
7. ~~**M-CLONE**~~ — **complete 2026-07-25** (#1+#2+#3; prepare-clone stretch deferred).
8. ~~**P3 CLI hygiene**~~ — **done 2026-07-27**, all six items including L-PUBLISH. Nothing open.
9. ~~**Other P2 remainder**~~ — **done 2026-07-27**: ~~M-EPS~~ (`TIME_EPS_SECS`, 3 sites),
   ~~M-GAPKEY~~ (index-keyed join + characterization test), ~~M-HOUND~~ (dead 4 GiB branch
   removed); workspace green after each.
   ~~M-FRAMES~~ **withdrawn 2026-07-27** (premise refuted). These were absent from this checklist
   and from the header's open-set line until 2026-07-27; they were never actioned, only overlooked.
10. ~~**M-RESAMPLE group delay**~~ — **closed 2026-07-27 as P3 / will not fix.** Measured rather
    than fixed: delay real and exact (2.7–4.0 ms = `output_delay()`), corpus occurrences zero
    (4008 recorded track rates across 1055 artifacts, all 48 kHz). Characterization test
    `crates/clip-sync/tests/resample_group_delay.rs` landed; fix pre-derived; re-open triggers
    listed in the M-RESAMPLE section. **This was the last open ledger item.**

---

## Sources

Review date: 2026-07-23. Findings synthesized from a full-workspace read-only
pass (clippy default + pedantic sample, `cargo test --workspace`, and targeted
source verification of every P0 item). Recommendations for remaining work added
2026-07-23. This file **was** the canonical ledger — Fixed tables and the priority
list were updated as items landed, with the standing instruction to archive once the
open set closed. That happened **2026-07-27**: the last item (M-RESAMPLE) was measured,
downgraded to P3 and closed will-not-fix, and this file moved to `docs/dev/archive/`.
It is closed to further edits; file new findings in a new document.
