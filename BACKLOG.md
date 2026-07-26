# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/pipeline.md](docs/pipeline.md) for the repair pipeline (phase by phase), [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling. Shipped work is recorded in `docs/dev/archive/*` and git history.

Last updated: 2026-07-26.

**How this doc works**

- **Open** — actionable items below (problem / direction kept for open work only).
- **Plans** — active drafts under `docs/dev/TEMP-*.md`; archive when shipped.

**Next:** [Repair R6](#repair-r6-follow-ups); [Residual gate](#residual-gate-follow-ups); [Narrow fill-length slack](#narrow-fill-length-slack).

---

## Active plans

| Plan | Covers |
|------|--------|
| [TEMP-gap-selection-plan.md](docs/dev/TEMP-gap-selection-plan.md) | Gap selection (subset patching) — draft, not started |
| [TEMP-nway-donor-alignment-plan.md](docs/dev/TEMP-nway-donor-alignment-plan.md) | N-way donor alignment: repair one damaged copy from multiple donors — draft, not started |
| [TEMP-rust-review-findings.md](docs/dev/TEMP-rust-review-findings.md) | Workspace Rust review ledger (P0–P3) — open, 2026-07-23 |

**Recently archived:** [TEMP-fill-placement-axis-plan.md](docs/dev/archive/TEMP-fill-placement-axis-plan.md) — Phase A armed, Phase B slack exit → Phase C NO-GO (2026-07-26).

## Open work

### Narrow fill-length slack

From [archive/TEMP-fill-placement-axis-plan.md](docs/dev/archive/TEMP-fill-placement-axis-plan.md) Phase B
residue; tracked as [repair-perf.md](docs/dev/repair-perf.md) §5 #3.

**One knob, two jobs — split the config (no hardcode).** `fill_length_slack_secs` and
`fill_extract_tail_slack_secs` are separate knobs (both default **5.0** today — pure fan-out,
byte-identical to the old single dial). (1) end-search range (`gap ± slack` in `gap_fill_fit`);
(2) B haystack tail (`length_slack = extract_tail.max(margin)` → `b_extract_end_secs` /
fingerprint `pad_tail`). Those are different risks: (1) only drops far end candidates; (2)
shortens `total_frames`, which can invalidate late *start* candidates
(`start + gap + post_span > total_frames`) and move seam/gate outcomes. Corpus max
`|fill − span|` is 388 ms (nothing ≥1 s; ±5 s is ~13× that max), so narrowing **search**
to 1.0 s keeps every observed winner inside a strict subset of today's end range
(`search_coarse_step` still saturates at `bin_frames`). Shrinking the extract is what creates
the wide blast radius — and it does **not** cut the #2 full-track decode cost (extract is a
slice). **Why decouple:** the corpus answers "how far may fill end slide?"; the extract only
needs enough tail for search radius + post context + that slide.

| Item | Direction |
|------|-----------|
| **Config split** | **Done** — `fill_extract_tail_slack_secs` (CLI `--fill-extract-tail-slack-secs`) wires extract / `pad_tail`; `fill_length_slack_secs` is end-sweep / `max_fill` only. Both default 5.0. |
| **Phase 1 — narrow search** | Narrow `fill_length_slack_secs` 5.0 → **1.0 s** (leave extract-tail at 5.0). Expected near byte-identical if corpus bound holds; exit: golden A/B on `fill_*` + outcome (patch/skip) + spot-listen largest-excursion patches. Then consider 0.5 s on the **search** key only if clean. |
| **Phase 2 — extract shrink (optional)** | Lower `fill_extract_tail_slack_secs` only if Phase 1 is clean *and* shorter timelines are still worth chasing. Independent A/B from Phase 1. |

### Dual-fit confidence axis

**Fingerprint / analysis only** — not a production dual-fit scope change. Do not wire
into `try_dual_fit` / rescue gating (seam ledger A5/D8: uniqueness stays diagnostic until a
labeled false positive). From [archive/TEMP-fill-placement-axis-plan.md](docs/dev/archive/TEMP-fill-placement-axis-plan.md)
Phase B residue (the `gate_pass` / end-search correlation). Axis semantics:
[gap-fingerprint.md](docs/dev/gap-fingerprint.md).

| Item | Direction |
|------|-----------|
| **Dual-fit `gate_pass` is a production mirror, not a discriminator — add a fingerprint confidence axis** | `gate_pass = min(pre_seam_r, post_seam_r) ≥ max(0.35, 0.12)` passes **263/263** on the 17-pair corpus. That is faithful, not broken: it reproduces the production gate exactly, and production's threshold sits far below the observed distribution (p05 of `smin` = 0.892) because `smin` is a ±600 ms argmax with no uniqueness term. Its value is provenance — "what did production decide" — so it should **not** be tightened. What's missing is a *separate fingerprint read* on whether the seam lag was unambiguous (analyzer / corpus roll-up strata — not a repair gate). The validators that discriminate are already emitted but ungated: `pre/post_seam_z` (p05 3.14, p25 4.91, median 7.86) and `pre/post_seam_prom` (p25 0.123). **Direction:** leave `gate_pass` alone (the goldens and corpus history read it); add derived fingerprint field `dualfit_confident` from min-z + min-prominence. **Do not** consolidate end-search length into dual-fit: on the throat cohort (n=65, where `span == gap`) the two disagree genuinely, not by dilution — `corr(fill−span, bridge−gap)` = +0.06, and tightening the amplitude floor drives it to −0.10; stratifying by min z reaches r = +0.68 only at n=8 / p=0.053, one of ~25 strata swept. In the z ≥ 6 cohort, 8 of 19 gaps show an end excursion of exactly 0 ms while dual-fit trims 4–28 ms. |

### Repair R6 follow-ups

From [archive/repair-write-path-plan.md](docs/dev/archive/repair-write-path-plan.md) post-ship gaps.

| Item | Direction |
|------|-----------|
| `--dry-run` / `--write` | Explicit CLI flags; today write mode is implied by `--wav` / `--mux` or TOML `dry_run = false` |
| Scratch-buffer regression test | Dedicated unit test for patch PCM path |
| Streaming / chunked WAV encode | Large multi-gap fills without holding full PCM |
| Adaptive gap-signature context (low priority) | Widen `gap_signature_context_secs` per-gap only when the score at the nominal map is below floor, instead of decoding wide B context for every gap. From [energy-signature-plan.md](docs/dev/archive/energy-signature-plan.md) Phase 4; low value since mode-coupled `nominal_bias` already handles drift at the 3 s default |
| `fill_repeat_correlations` channel alignment (low priority) | Repeat tier scores the mono downmix **and** every per-channel pair, taking the best — but without the ~20 dB energy **selection** (`seam_score_channel_indices`) that seam Pearson and residual now share, so the three discriminators don't measure the same channel set. Consistency cleanup, not a correctness bug (peak-normalized near-silent channels score ~0). **Fix:** restrict `ch_pre`/`ch_post` to `selected_seam_channels` and **drop the mono term**; replace its defensive role (the per-channel folds start at `NEG_INFINITY`) with an explicit **`0.0` default** for unscoreable cases — empty selection *or* no per-channel window fits. Empty selection needs no mono fallback: it means every channel is digital silence (the loudest channel is always selected when any content exists), so `0.0` ("no repeat") is the honest result. **Caveat:** mono is combined via `max`, so unlike the residual floor it only *adds* sensitivity — a repeat smeared across channels could correlate better in the downmix than in any single channel (rare; for a center-dominant repeat peak-normalization makes `mono ≈ center`). Dropping mono is therefore a deliberate, tiny sensitivity change — add a smeared-multichannel-repeat test to confirm the per-channel path still catches it. From [TEMP-residual-channel-alignment-plan.md](docs/dev/archive/TEMP-residual-channel-alignment-plan.md) §8 |
| Dual-fit oracle: unpin pre-shoulder lag from 0 (optional hardening) | `validate_dual_fit_oracle.rs` (gated `validation-tests`, needs ffmpeg + fetched corpus) only steps the **post**-shoulder (`step_ms`); the pre-shoulder is always sourced from B at lag 0 by construction (`dual_fit_oracle.rs`). The 2026-07-03/05 production bug (dual-fit's re-validation gate wrongly applying the single-rigid-lag crossfade branch, fixed via `SpliceSeamContext::single_lag_alignment`) is now covered end-to-end by a synthetic unit test (`dual_fit_result_passes_the_production_revalidation_gate`, `domain/dual_fit.rs`), so this isn't blocking. Direction if picked up: add an optional `pre_step_ms` field to `DualFitOracleCase` (default `0.0`, mirroring `step_ms`) that shifts the pre-gap portion the same way the post-gap portion is shifted, plus a manifest case with both nonzero — proving the real-codec path too, not just the synthetic gate. Not yet implemented or run (needs ffmpeg + real media, which wasn't fetched here per the licensed-media partition) |

---

### Residual gate follow-ups

From [archive/residual-gate-findings.md](docs/dev/archive/residual-gate-findings.md) and
[archive/residual-gate-wiring-plan.md](docs/dev/archive/residual-gate-wiring-plan.md). Test inventory:
[`residual_gate_catalog/`](crates/clip-sync-repair/tests/residual_gate_catalog/).

| Item | Priority | Direction |
|------|----------|-----------|
| **M3** — floor walk vs B haystack OOB | med | `walk_reference_frames` is A-energy only; B OOB → NaN at measure time today. Tighten walk only if field media hits bad geometry |
| **G1** — residual on Pearson-only skips | gap | Veto skips done (`ResidualHeadroomExceeded` + `GapPatchOutcome.residual`). Pearson/structure skips still lack residual unless we measure on last grid candidate when `measure_residual` |
| **L6** — coarse outward walk step | low | `step_frames = window` in `measure_fit_residual_verdict`; changes floors → recalibrate if touched |
| **M4** — MP3 calibration | defer | Manifest rows marked M4; gate is codec-agnostic |
| **FD-1** — fractional-delay cancellation | defer | Sub-sample lag + B resample; re-run floor calibration — see findings § FD-1 |
| **`finale_floor_nan_probe`** | optional test | Unit repro: why Grieg finale floor is NaN (M3-adjacent); catalog backlog |
| **`c1b_acoustic_echo_pipeline_veto`** | optional test | Pipeline `ResidualHeadroomExceeded` under `production_fit` on non-F4 echo fixture — optional C1b |
| **`p2_search_winner_bounds`** | optional test | Bound headroom on search winner vs truth placement — needs design |

**Explicitly not planned:** `veto_rescue` as default (G5: synthetic-only); F4 pipeline veto (M6).

---

### Defer / opportunistic

| Item | Direction |
|------|-----------|
| [Offset-mapped end placement](#offset-mapped-end-placement) | After start clip, place B end window at `A_end + Δ` when B has a long leader — see [archive/anchored-end-extraction-plan.md](docs/dev/archive/anchored-end-extraction-plan.md) follow-ups |
| [Skip overlapping end fingerprint](#skip-overlapping-end-fingerprint) | Omit end clip when `T_anchor − L` overlaps start window |
| [Weighted drift in repair warning](#weighted-drift-in-repair-warning) | Down-rank end clip in instability synthesis when end confidence is low |
| [Memory / PCM cloning](#memory-use-and-pcm-cloning-on-long-clips) | `Cow` / in-place prep when painful; parallel A/B decode when needed |
| [Committed test fixtures](#committed-test-fixtures) | Optional committed MP3; committed verify deferred — see [tests/corpus/README.md](tests/corpus/README.md) |
| [Verification cost knob](#verification-cost-knob) | `validation.max_verification_secs` — only on demonstrated friction |
| [Test tier follow-ups](docs/dev/test-tier-remainder.md) | `clip-sync` ignore cleanup (~1 h), optional `pr-repair-extended` / SP on PR, Phase 2b binary split — see [test-tier-remainder.md](docs/dev/test-tier-remainder.md) |

#### Memory use and PCM cloning on long clips

15-minute default clips; full PCM in memory per extracted window; no streaming fingerprint API yet. **Decided (2026-06-11):** future streaming should reuse `scan_*_buckets` callbacks; `MediaSession: Send` allows one session per thread when parallel decode lands — see [PLAN.md](PLAN.md) § Media session semantics and [archive/media-session-redesign-plan.md](docs/dev/archive/media-session-redesign-plan.md).

**Refs:** `application/align_videos.rs`, `domain/pcm_preparation.rs`

#### Committed test fixtures

Tier B = 3× 30 s WAV pairs; ffmpeg for encoded formats. Hold-out verify on committed tier deferred — generated-only coverage documented in [tests/corpus/README.md](tests/corpus/README.md).

**Refs:** `tests/corpus/`, `Cargo.toml` features

#### Verification cost knob

Optional `validation.max_verification_secs` — deferred in [archive/verification-hardening-plan.md](docs/dev/archive/verification-hardening-plan.md). Implement only if verify decode cost becomes painful in practice.

**Refs:** [corpus-validation.md](docs/dev/corpus-validation.md) § Hold-out verification cost

#### Offset-mapped end placement

Symmetric `SharedTimeline` places end windows at the same absolute times on A and B; when B has a long leader before shared content, offset-mapped end (`[T_a−L, T_a]` on A, shifted by Δ on B) would align fingerprints to the same story region.

**Refs:** [archive/anchored-end-extraction-plan.md](docs/dev/archive/anchored-end-extraction-plan.md)

#### Skip overlapping end fingerprint

When `T_anchor − clip_length` overlaps the start window (short shared span), skip end fingerprinting to avoid comparing redundant audio.

**Refs:** `domain/policies.rs`, `application/align_videos.rs`

#### Weighted drift in repair warning

Repair instability warning treats end − start drift equally; down-rank or de-emphasize end when end confidence is low or tail decode was unreliable.

**Refs:** `clip-sync-repair/src/infrastructure/cli/output.rs`

---

## Explicitly out of scope (initial version)

From [PLAN.md](PLAN.md) — not backlog unless scope changes:

- Video frame / visual sync
- Batch processing (> two files)
- Writing aligned output files from the **analyzer** (report offset only)
- Patched repair output is **`clip-sync-repair`** only — [archive/repair-write-path-plan.md](docs/dev/archive/repair-write-path-plan.md)
- Network or streaming sources
