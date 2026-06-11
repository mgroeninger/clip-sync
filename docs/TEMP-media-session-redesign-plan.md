# Temporary plan: `MediaSession` redesign + media extent

> **Status:** Draft (2026-06-10). Plan 3 of 4 — see [BACKLOG.md](../BACKLOG.md). **One** breaking port change batching every `MediaSession` surface decision, so fakes and both CLI crates churn once. Plan 1 ([archive/output-error-contract-plan.md](archive/output-error-contract-plan.md)) shipped 2026-06-10 — the `MediaError` surface is settled (source-carrying struct variants + `MediaError::open_failed`-style constructors), so this plan is unblocked.

**Problem:** The `MediaSession` port presents a stateless `&self` facade over a stateful seekable decoder (`RefCell<Option<MediaIoState>>` + five `expect("session io initialized")`). Consequences already in the backlog: `reset_io` is forgettable (high-rate refinement discards its result), the session is not `Sync` (parallel A/B decode foreclosed by accident), scan-loop policy (`NEAR_TRACK_END_TOLERANCE_SECS`, error swallowing) lives in trait default methods, and three separate items trace back to trusting container metadata — duration-less files, hold-out placement on container duration, and dead `decoded_extent_*` fields.

**Goal:** Honest port semantics (`&mut self`, no `RefCell`, no `expect`), adapter-internal seek recovery that deletes `reset_io` from the port, scan policy in one named place, and a first-class `MediaExtent` (declared vs decodable duration) consumed by clip planning, hold-out placement, and tail scans. Decide — without implementing — the parallel-decode and streaming questions so this port shape doesn't foreclose them again.

**Workspace split:** Port + adapter + use-case changes in **`crates/clip-sync`**. **`clip-sync-cli`** unaffected beyond facade recompile. **`clip-sync-repair`** updates its 6 test fakes and the `scan_gaps` / `patch_audio` call sites to `&mut`.

---

## Current codebase baseline

| Area | Path | Current state | Target phase |
|------|------|---------------|--------------|
| Port surface | `crates/clip-sync/src/application/ports.rs` ~27–173 | All methods `&self`; defaults: `extract_interleaved` → `Unsupported`, `reset_io` → `Ok(())`, `track_decodable_extent` → `None`, `scan_mono_buckets` / `scan_interleaved_buckets` = seek-loop fallbacks with `NEAR_TRACK_END_TOLERANCE_SECS = 2.0` + `DecodeFailed`/`SeekFailed` swallowing (~84–116, ~135–166) | 1, 3 |
| Symphonia session | `infrastructure/symphonia/session.rs` ~101–258 | `io: RefCell<Option<MediaIoState>>`; `expect()` at 132, 160, 189, 219, 251; lazy reopen when probe rewind failed; decoder cache `HashMap<u32, CachedTrackDecoder>` | 1–2 |
| `reset_io` callers | `high_rate_refinement.rs` 65–66 (`let _ =` — **discarded**); `offset_verification.rs` 106–110 (debug-logged only); `session.rs` 256 (propagated) | Caller-driven, forgettable | 2 |
| Hold-out extract errors | `high_rate_refinement.rs` 185–195 (`extract_native_holdout` → `Result<_, String>` via `.map_err(to_string)` — drops `MediaError`/`source()` before skip logging); `offset_verification.rs` 134–156 (matches `MediaError`, `debug!`s full error, then formats for `skip_reason`) | Match on `MediaError`; structured debug log; `Display` only at `skip_reason` boundary | 2 |
| Implementors | lib: `SymphoniaMediaSession`, `FakeMediaSession` (`testing/fakes.rs` 113–155), `BareSession` (test); repair tests: `LoudSession`, `SilentSession`, `SkipWindowSession`, `TailSeekFailSession`, `DispatchSession`, `NoDurationSession` (`scan_gaps.rs` 323–689) | 9 impls to migrate | 1 |
| Call sites | `align_videos.rs` 65–70, 583–588; `high_rate_refinement.rs` 109–127; `offset_verification.rs` 134–152; repair `scan_gaps.rs` 79–80, 130–132, 243–245, 269–274; `patch_audio.rs` 114–147 | Sessions A/B held simultaneously, **used strictly sequentially** (no threads anywhere) | 1 |
| `scan_mono_buckets` | — | **No production caller** (symphonia override exercised by tests only) | 3 (consider demotion) |
| Hold-out placement | `offset_verification.rs` 67–93 | `pick_duration = duration_a.min(duration_b)` — container duration; `decoded_extent_a/b` destructured to `_`, `#[allow(dead_code)]` at 23–26 | 4 |
| Extent today | `align_videos.rs` 513–524 (`track_decodable_extent` when `num_clips >= 2` && clamp flag), ~606 (`decoded_timeline_extent` of discovery clips) | Three uncoordinated duration notions: container, tail-scan extent, discovery-decode extent | 4 |
| Duration at open | `infrastructure/symphonia/probe.rs` 52–127; `duration.rs` 44–110 | Per-track duration, chapter fallback, packet-scan fallback (`scan_container_audio_duration`); open fails on zero duration | 4 |
| ~~Bitrate~~ | — | **Done 2026-06-10** (before this plan lands): `AudioTrack.bitrate` deleted — Symphonia doesn't expose encoding bitrate; `select_best_track` stays first-decodable. BACKLOG #8 closed | — |
| Memory model | `ports.rs` 41 (owned `MonoPcmClip`); clones in `align_videos.rs` ~285–308, `pcm_preparation.rs` 60–113 | Full-clip buffers; streaming would change `Fingerprinter` too | decision only |

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Mutability model** | **`&mut self`** on `extract_mono`, `extract_interleaved`, `scan_*`, `track_decodable_extent`. `list_tracks` stays `&self` (cached at open). A and B are separate session values used sequentially, so `&mut` borrows never conflict. *Rejected alternative:* handle/guard object — more API for no current consumer. |
| **`RefCell` removal** | `io: Option<MediaIoState>` plain field; private `fn io_state(&mut self) -> Result<&mut MediaIoState, MediaError>` performs the lazy reopen and replaces all five `expect()` sites with a real error path. |
| **`Sync`/parallel decode** | **Decide, don't implement.** After `RefCell` removal, verify `SymphoniaMediaSession: Send` (boxed `FormatReader`/decoder objects) and add `Session: Send` bound **if it holds**; if Symphonia objects aren't `Send`, document that parallel A/B requires open-per-thread, not a port change. Parallel extraction itself is follow-up work, deliberately out of scope. |
| **`reset_io`** | **Remove from the port.** The Symphonia adapter recovers internally at two points, both **below the `MediaError` mapping** where the `SymphoniaError` is still typed (no string-matching on stringly errors): (a) a `seek_with_recovery` wrapper around `seek_to_window_start` — on seek error, reopen `MediaIoState`, re-run `ensure_track_decoder`, retry the seek **once**; (b) reopen `MediaIoState` before the existing attempt-2 sequential-from-zero fallback in `run_extract_decode_loop` (today the fallback reuses the possibly-confused reader that just produced no audio). Together these subsume the trailing `reset_io` in `track_decodable_extent` (post-tail-scan reader at EOF: the next op's seek either succeeds or recovers). Call sites in `high_rate_refinement.rs` / `offset_verification.rs` are deleted — the failure mode "caller forgot reset" becomes unrepresentable. The BACKLOG item "`reset_io` ignored in high-rate refinement" closes as a side effect. *Why this shape:* per-extract decoder reset and PTS-based window classification already exist, so mispositioned seeks fail loudly (shortfall/`SeekFailed`/EOF-flavored `DecodeFailed`), not silently — recovery only needs to catch the loud cases at the layer where state is reconstructible. |
| **Scan policy location** | Default `scan_*_buckets` bodies move to `application/media_scan.rs` free functions (`scan_buckets_via_windows(...)`); trait defaults become one-line delegations. `NEAR_TRACK_END_TOLERANCE_SECS` and the swallow-`DecodeFailed`/`SeekFailed`-near-end rule live there, named, documented, and unit-tested directly (today they're only testable through a fake). |
| **`scan_mono_buckets` fate** | Keep on the port (repair's interleaved twin has production callers; mono variant is its symmetric API), but note no production caller — candidate for deletion at a future port break if still unused. |
| **`MediaExtent`** | New domain value type: `MediaExtent { declared: Duration, decodable: Option<Duration> }` with `effective() -> Duration` (= `decodable.unwrap_or(declared)`, clamped to declared). Resolved **once per video** in `AlignVideos::execute` — declared from `track.duration`, decodable from `track_decodable_extent` when end-anchored windows or `verify_offset` make the tail matter. Passed to clip planning (`clip_windows_with_options`), hold-out placement, and high-rate refinement inputs. The `#[allow(dead_code)]` `decoded_extent_*` fields are replaced by `MediaExtent`, not merely wired. |
| **Hold-out placement** | `offset_verification.rs` uses `extent_a.effective().min(extent_b.effective())` for `pick_duration` and feasibility. This is the BACKLOG "hybrid extent policy": container duration when the tail is verified decodable, decodable extent when it isn't. MKV-tail regression test required (the motivating failure). |
| **Duration-less files (BACKLOG #6)** | Open keeps failing only when **no** duration can be established (probe → chapters → packet scan). Audit remaining open-failure paths; anything decodable-but-duration-less should open and fail at **clip planning** with `InvalidDuration`. Covered by Phase 4 audit task; `mp3_no_duration_tag` corpus case is the regression anchor. |
| **Bitrate (BACKLOG #8)** | **Already resolved (2026-06-10, outside this plan):** `AudioTrack.bitrate` deleted after confirming Symphonia doesn't expose encoding bitrate. No work remains here. |
| **Streaming / memory ceiling** | **Decide, don't implement:** future streaming fingerprinting will use the bucket-callback shape (`scan_*_buckets`), not a new pull API — so this redesign must keep callbacks compatible with `&mut self` (callback cannot re-enter the session; document this re-entrancy rule on the trait). The PCM-clone reduction in `align_extracted_pair` stays in BACKLOG defer/opportunistic. |
| **Error semantics** | No new `MediaError` variants. Retry-once recovery maps the *second* failure to the original error. Recovery triggers on the typed `SymphoniaError` inside the adapter (see `reset_io` row), so the settled `MediaError` surface is untouched. |
| **Hold-out extract errors** | Match on `MediaError` in hold-out loops (high-rate + offset verification); `debug!` the full error (preserves `source()` in logs); store `{e}` in `skip_reason` for the report. Delete `extract_native_holdout`'s `Result<_, String>` wrapper — mirror `offset_verification.rs`. JSON contract unchanged (`skip_reason` stays `Option<String>` per [json-output.md](json-output.md)); no new `MediaError` variants. |
| **Fallback scan-loop duration trust** | **Document, don't change.** The trait-default `scan_*_buckets` fallbacks terminate on declared duration (`while pos < total_secs` + 2 s swallow rule) — but their only callers are test fakes. Production scans (Symphonia overrides used by repair) are **EOF-driven**: duration feeds progress estimation only, and bucket timestamps come from decoded sample counts. Threading `MediaExtent` into `scan_*_buckets` would be a port-signature change serving no production caller. Phase 3 records the trust in `media_scan.rs` rustdoc instead: over-reporting beyond the tolerance fails loudly, and that is acceptable for fallback paths. |
| **Extent clamp / under-reported duration** | **Keep the clamp, add observability.** `scan_track_decodable_extent` already returns `max_end.min(container_duration)`; `MediaExtent::effective()` keeps declared as the ceiling. Planning windows beyond declared duration is exactly the region where seeks go `OutOfRange` — high risk, no demonstrated need. Phase 4 adds a `warn!` when the tail scan observes packets past declared duration *before* clamping ("container under-reports duration"), and the `MediaExtent` rustdoc records the decision. If the warning ever fires on real media, that is the evidence to revisit. |

---

## Phases

### Phase 0 — characterization guard rails

- [ ] Regression test: distant backward seek after a long extract (the scenario `reset_io` existed for) — assert the re-extracted window is **bit-exact** against the same window from a fresh session, on **both an MP4 and an MKV** fixture. Bit-exactness (not just success) catches silent mispositioning regardless of whether the failure surfaces as an error; two containers because seek behavior differs. Must pass before and after Phase 2.
- [ ] MKV tail regression: hold-out / end-clip placement on a fixture whose container duration exceeds decodable extent (extend `tests/corpus/manifest.toml` generated tier).
- [ ] Snapshot test for scan-loop end-of-track tolerance behavior driven through a fake (pins current swallow semantics before Phase 3 moves the code).

### Phase 1 — `&mut self` port migration (mechanical)

- [ ] Change trait method receivers in `ports.rs`; drop `RefCell` from `SymphoniaMediaSession`; introduce `io_state()` and delete the five `expect()`s.
- [ ] Migrate all 9 implementors (lib fake, repair test fakes — most are `&self` field reads and migrate trivially) and all call sites (`align_videos`, `high_rate_refinement`, `offset_verification`, repair `scan_gaps` / `patch_audio`).
- [ ] Verify/decide `Send` bound per the decision above.
- [ ] `cargo test --workspace` green.

### Phase 2 — internal seek recovery, delete `reset_io`

- [ ] `seek_with_recovery` in the extract layer (typed `SymphoniaError`, pre-mapping): on seek error, reopen `MediaIoState`, re-run `ensure_track_decoder`, retry the seek once; second failure returns the original error. Unit test with an io-layer fault injector that fails the first seek.
- [ ] Reopen `MediaIoState` before the attempt-2 sequential-from-zero fallback in `run_extract_decode_loop` (fresh reader for the retry instead of the one that just produced no audio).
- [ ] Remove `reset_io` from the port; delete caller lines in `high_rate_refinement.rs` 65–66 and `offset_verification.rs` 106–110; delete the trailing `reset_io` in `track_decodable_extent` (`session.rs` 256) — subsumed by recovery.
- [ ] Align hold-out extract error handling in `high_rate_refinement.rs` and `offset_verification.rs`: match on `MediaError`, structured `debug!` before flattening, `Display` only at the `skip_reason` boundary; delete `extract_native_holdout`'s `Result<_, String>` wrapper.
- [ ] While restructuring the loop, fix the deferred `clippy::too_many_arguments` on `ExtractSink::finalize` (`extract_loop.rs`, currently `#[allow]`ed): group the per-extract identity/reporting values (`path`, `track`, `window`, `progress`, `label`) into a borrowed context struct — `ExtractLoopParams` already bundles the same five for the driver, so reuse or mirror it rather than adding a third shape.
- [ ] Phase 0 backward-seek characterization test still green (bit-exact, both containers).

### Phase 3 — scan policy extraction

- [ ] `application/media_scan.rs`: move default scan-loop bodies; named constants + rustdoc for the near-end tolerance and swallow rule; direct unit tests.
- [ ] Rustdoc records the duration-trust decision: fallback loops terminate on declared duration and fail loudly past the tolerance; production scans are EOF-driven (see Decisions).
- [ ] Trait defaults delegate; symphonia sequential overrides untouched.

### Phase 4 — `MediaExtent`

- [ ] Domain type + unit tests; resolve once per video in `AlignVideos::execute`; thread through `ClipPlanningOptions`, `HighRateRefinementInput`, `OffsetVerificationInput` (replacing `decoded_extent_*`, removing `#[allow(dead_code)]`). Rustdoc records the declared-as-ceiling clamp decision.
- [ ] `scan_track_decodable_extent`: `warn!` when packets are observed past declared duration before clamping ("container under-reports duration") — observability for the clamp decision, no behavior change.
- [ ] Hold-out placement + feasibility switch to `effective()` durations; Phase 0 MKV-tail test goes green.
- [ ] Duration-less audit (BACKLOG #6): enumerate remaining open-failure paths in `probe.rs`/`session.rs`; relax open where decodable; clip planning rejects unknown duration; corpus cases for each relaxed path.

### Phase 5 — docs

- [ ] PLAN.md: port table, session semantics, extent concept, re-entrancy rule, parallel/streaming decisions recorded. BACKLOG: close items 6, 12, "hold-out container duration", "unused decoded_extent", "reset_io ignored"; memory item stays with updated note. (Item 8 / bitrate closed 2026-06-10, independent of this plan.)

---

## Tests

| Concern | Coverage |
|---------|----------|
| Seek recovery | Phase 0 bit-exact characterization (MP4 + MKV) + Phase 2 io-layer fault injection; corpus committed tier on every phase |
| Hold-out extract errors | Existing skip-reason / JSON tests unchanged; extract-failure paths log structured `MediaError` before `skip_reason` flattening |
| Extent | MKV-tail regression; `MediaExtent::effective()` unit tests; `mp3_no_duration_tag` corpus anchor |
| Scan policy | Direct unit tests on `media_scan.rs` (previously reachable only through fakes) |
| Migration | Full workspace suite green at each phase boundary; repair fakes compile-checked under `cargo test -p clip-sync-repair` |

## Exit criteria

- No `RefCell`, no `expect()` in `session.rs`; port methods honest about mutation.
- `reset_io` gone from the port; no caller-managed IO state anywhere.
- One named home for scan policy with direct tests.
- Hold-out placement and clip planning consume `MediaExtent`; no dead extent fields.
- Hold-out extract loops match on `MediaError`; no `Result<_, String>` extract wrappers.

## Cross-plan sequencing

- ~~After the output/error contract plan~~ — **satisfied**: shipped 2026-06-10 ([archive/output-error-contract-plan.md](archive/output-error-contract-plan.md)); `MediaError` surface settled.
- Independent of [TEMP-layer-purity-plan.md](TEMP-layer-purity-plan.md) (ports.rs merge conflicts only).
- [TEMP-verification-hardening-plan.md](TEMP-verification-hardening-plan.md) rebases its hold-out work on `MediaExtent` if this lands first; both plans touch `offset_verification.rs` — coordinate.
