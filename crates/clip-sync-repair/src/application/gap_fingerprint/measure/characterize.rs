//! `characterize` slice of the gap_fingerprint measure folder split.

use crate::application::gap_fingerprint::contract::*;
use crate::application::gap_fingerprint::project::*;
use crate::application::gap_fingerprint::schema::*;

use crate::domain::gap_equivalence::{ChannelReduction, EquivalenceMeasurement, SpanKind};

use crate::domain::patch_result::GapPatchSkipReason;
use crate::domain::policies::{refine_gap_frames, RefinedGapFrames};

use super::build::*;
use super::config::*;
use super::overlay::*;
use super::pcm::*;

/// Corpus envelope when A/B `native_channels` disagree: provenance filled, pairwise gaps refused.
///
/// B's `channels` / `duration_secs` / `id` use **B's** layout (not A's). Rate may still be A's after
/// `decode_ab` resample. Callers must not index `b_samples` with `a_channels`.
///
/// This is the **opposite** rule from the non-refused path, where both sides are described at A's
/// layout because that is the layout everything was measured at. The split is deliberate: there is no
/// common measurement here to describe B against, so B is described honestly against itself. Note the
/// consequence — `b_source.id` digests B's own channel count, so a refused corpus and a normal one over
/// the same media carry different `b_source.id`s. Harmless today (`gaps` is empty, so `entry_stem`
/// never runs), but a `(a_id, b_id)` join across the two would not match. Both halves of the rule are
/// asserted: `characterize_gaps_refuses_channel_layout_mismatch` and the from-decode threading test.
pub(in crate::application::gap_fingerprint::measure) fn refused_channel_mismatch_corpus(
    report: &crate::domain::GapReport,
    a_samples: &[f32],
    b_samples: &[f32],
    sample_rate: u32,
    a_channels: u16,
    sources: &crate::application::patch_audio::AbSources,
) -> GapCorpus {
    GapCorpus {
        source: SourceMeta {
            a_source: file_source(a_samples, sample_rate, a_channels, Some(&sources.a)),
            b_source: file_source(
                b_samples,
                sample_rate,
                sources.b.native_channels,
                Some(&sources.b),
            ),
            scan_recipe: CorpusScanRecipe::from_report(report),
            gap_count: report.gaps.len(),
            incomparable: Some(IncomparableReason::ChannelLayoutMismatch),
            gate_recipe: None,
            // Refusal ⇒ nothing was measured, so there is no probe geometry to report.
            probe_recipe: None,
            // Refusal ⇒ `gaps` is empty, so there is nothing to qualify.
            not_measured: Vec::new(),
            // Scan still ran; refuse is the pairwise characterize gate, not a missing map.
            alignment: stamp(
                Some(CorpusScanAlignment::from_report(report)),
                ALIGNMENT_CONTRACT,
            ),
        },
        gaps: vec![],
    }
}
pub(in crate::application::gap_fingerprint::measure) fn channel_layout_mismatch(
    sources: &crate::application::patch_audio::AbSources,
) -> bool {
    sources.a.native_channels != sources.b.native_channels
}
/// Full-timeline A/B PCM for corpus characterization, plus optional source provenance.
///
/// Groups the media trio that already travels together from `decode_ab` (`a_pcm`, resampled B,
/// `sources`) so [`characterize_gaps`] / [`characterize_gaps_from_decode`] take one argument instead of
/// three. `sources` is `None` for media-free callers (synthetic fixtures), which leaves provenance fields
/// absent rather than guessed.
#[derive(Clone, Copy)]
pub struct CharacterizeAbPcm<'a> {
    pub a_pcm: &'a clip_sync::MultiChannelPcm,
    /// B interleaved samples at A's sample rate (same layout contract as decode's `b_samples_full`).
    pub b_samples: &'a [f32],
    pub sources: Option<&'a crate::application::patch_audio::AbSources>,
}
/// Fingerprint dump computed **from decode via the shared projection** (Fingerprint-unification 8g.4a/8g.4b) —
/// the `--gap-fingerprints` dump path (the old per-bracket-oracle inline build was removed at 8g.6). Per
/// gap: the summary (geometry/levels, already in `corpus`) + [`compute_region_measurements`] (8g.3a) → `m`,
/// then a spec (verdict from `m.outcome.tier` = the fingerprint **`any_ok`** semantics; tags from
/// [`tags_from_measurements`], 8g.3b) → [`spec_to_fingerprint_summary`]. Keeps fingerprint semantics — does NOT
/// run the production patch gate (pre-flip review Finding 1). Gaps whose overlay setup is skipped (no B start /
/// zero-length / empty window) keep their summary fingerprint, exactly as the oracle leaves them.
///
/// When `pcm.sources` reports disagreeing `native_channels`, returns an empty-gap corpus with
/// [`IncomparableReason::ChannelLayoutMismatch`] and does not index B at A's channel count.
///
/// Pass-1 `silence` / `contour` / `anchors` are restored after the spec projection (the projector
/// still emits `None` for those groups). Pair-level scan alignment is stamped on `source.alignment`.
pub fn characterize_gaps_from_decode(
    report: &crate::domain::GapReport,
    pcm: &CharacterizeAbPcm<'_>,
    request: &crate::application::PatchAudioRequest,
    select: &[usize],
    dump: DumpOptions,
    progress: &dyn clip_sync::ProgressReporter,
) -> GapCorpus {
    use crate::domain::gap_repair_spec::{
        BExtractWindow, GapRepairSpec, GapRepairStrategy, GapRepairVerdict, LevelTags,
    };

    let CharacterizeAbPcm {
        a_pcm,
        b_samples: b_samples_full,
        sources,
    } = *pcm;
    let sample_rate = a_pcm.sample_rate;
    let channels = a_pcm.channels as usize;
    if let Some(src) = sources {
        if channel_layout_mismatch(src) {
            return refused_channel_mismatch_corpus(
                report,
                &a_pcm.samples,
                b_samples_full,
                sample_rate,
                a_pcm.channels,
                src,
            );
        }
    }
    let cfg = FingerprintConfig::from_request(request, report.recipe.silence_peak_fraction(), dump);
    let mut corpus = characterize_gaps(report, pcm, &cfg, select);
    // Seam-gate floors used for every bracket `failure_stage` on this dump — stamped once at corpus
    // level (same idea as equivalence `thresholds`). Summary-only `characterize_gaps` leaves this None.
    corpus.source.gate_recipe = Some(CorpusGateRecipe::from_settings(&request.settings));
    // Gaps whose `lag_decision` block came from `projected_lag_entry` rather than a real sweep —
    // the population `PROJECTED_LAG_DECISION_FIELDS` exists to disown. Zero on a dump where every
    // measured gap carried its sweep through, which is the normal from-decode case.
    let mut projected_lag_gaps = 0usize;

    let mut gate_derived = crate::application::patch_region::SeamGateDerived::from_repair(
        request,
        sample_rate,
        channels,
        report.recipe.silence_peak_fraction(),
    );
    gate_derived.measure_residual = true;
    let rate = f64::from(sample_rate).max(1.0);
    let ch = channels.max(1);
    let b_total = b_samples_full.len() / ch;
    let max_refine_frames = (cfg.max_refine_secs * rate).round() as usize;
    let context_frames = (cfg.gap_signature_context_secs * rate).round() as usize;
    let bin_frames = ((cfg.gap_signature_bin_ms as f64 / 1000.0) * rate)
        .round()
        .max(1.0) as usize;
    let search_radius_frames =
        ((cfg.fill_border_search_secs.max(cfg.fill_align_margin_secs)) * rate).round() as usize;
    let pad_lead =
        cfg.gap_signature_context_secs + cfg.fill_border_search_secs + cfg.fill_align_margin_secs;
    let pad_tail = cfg.gap_signature_context_secs
        + cfg
            .fill_extract_tail_slack_secs
            .max(cfg.fill_align_margin_secs)
        + cfg.fill_border_search_secs
        + cfg.fill_align_margin_secs;

    let total_gaps = corpus.gaps.len() as u64;
    for (gn, fp) in corpus.gaps.iter_mut().enumerate() {
        progress.progress("fingerprint-gap", gn as u64 + 1, total_gaps);
        let i = fp.index;
        let gap = &report.gaps[i];
        // Guarded span: half-mapped / degenerate / negative B coords are not fingerprintable.
        let Some((b_start, b_end)) = gap.mapped_b_span() else {
            continue;
        };
        let refined = refine_gap_frames(
            &a_pcm.samples,
            ch,
            (gap.video_a_start_secs * rate) as usize,
            (gap.video_a_end_secs * rate) as usize,
            cfg.silence_peak_fraction,
            cfg.absolute_silence_rms,
            max_refine_frames,
        );
        let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
        if gap_frames == 0 {
            continue;
        }
        let gap_offset = b_start - gap.video_a_start_secs;
        let lo = (((b_start - pad_lead).max(0.0) * rate) as usize).min(b_total);
        let hi = (((b_end + pad_tail) * rate).ceil() as usize).min(b_total);
        if hi <= lo {
            continue;
        }
        let b_slice = &b_samples_full[lo * ch..hi * ch];
        let b_extract_start_secs = lo as f64 / rate;

        let m = compute_region_measurements(RegionMeasureInput {
            a_pcm,
            ch,
            b_slice,
            b_extract_start_secs,
            gap_offset,
            refined,
            gap_frames,
            gate_settings: &request.settings,
            gate_derived,
            cfg: &cfg,
            gap_floor_db: fp.levels.gap_floor_db,
            include_diagnostics: dump.diagnostics,
            context_frames,
            bin_frames,
            search_radius_frames,
            rate,
            sample_rate,
            progress,
        });

        // Decision spec: geometry + levels from the summary fp; verdict from `m.outcome` (any_ok); tags from
        // the shared measurements. Placeholder strategy/reason carries only the `patch`/`skip` + cell
        // distinction the reader's `tier` axis needs — dump `outcome.skip_reason` is the closest
        // `FailureStage` from `m.brackets`, not this production enum.
        let levels = LevelTags {
            a_gap_floor_db: f64::from(fp.levels.gap_floor_db),
            a_noise_floor_db: f64::from(fp.levels.noise_floor_db),
        };
        let verdict = if m.outcome.tier == "skip" {
            // Inert reason for `cell` only (`Decorrelated` default); wire skip_reason comes from brackets.
            GapRepairVerdict::skip(GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation: 0.0,
                post_correlation: 0.0,
                min_correlation: 0.0,
                best_attempt: None,
            })
        } else {
            GapRepairVerdict::Patch(GapRepairStrategy::SilenceSplice {
                fill: Vec::new(),
                pre_seam_r: 0.0,
                post_seam_r: 0.0,
                pre_lag: 0,
                post_lag: 0,
                trim_frames: 0,
                residual: None,
                confidence: crate::domain::gap_fill_fit::FillConfidence::High,
                registration_residual_ms: None,
                // Placeholder strategy on a probe path — nothing here is ever spliced.
                normalize_gain: 1.0,
                local_gain: crate::domain::LocalGainDecision::OFF,
            })
        };
        let spec = GapRepairSpec {
            gap_index: fp.index,
            a_start_secs: fp.geometry.a_start_secs,
            a_end_secs: fp.geometry.a_end_secs,
            gap_offset_secs: fp.geometry.fill_offset_secs.unwrap_or(gap_offset),
            refined: RefinedGapFrames {
                start_frame: (fp.geometry.a_refined_start_secs * rate).round() as usize,
                end_frame: (fp.geometry.a_refined_end_secs * rate).round() as usize,
            },
            b_extract: BExtractWindow {
                start_frame: 0,
                end_frame: 0,
                b_mapped_start_frame: 0,
            },
            crossfade_secs: 0.0,
            verdict,
            tags_ctx: tags_from_measurements(&m, Some(levels)),
        };
        let x = FingerprintXSet {
            seam_probe: m.seam_probe,
            wide_envelope: m.wide_envelope,
            b_levels: m.b_levels,
            lag_editorial: m.lag_editorial,
        };
        // Carry the REAL per-bracket rows (8g.4b) so the flipped dump is byte-faithful to the oracle's
        // `brackets` in both modes — the oracle enumerates them unconditionally, so from-decode must too.
        //
        // And the REAL `lag_decision`: `lag_at_placement` already swept ±`lag_max_lag_ms` at `b_mapped`
        // above, and the projection used to throw that away and rebuild a row from four stored scalars —
        // zeroed search parameters, `lag0_r` a copy of `peak_r`, `verdict` hardcoded `TimingOffset`. The
        // copy is the damaging one: it reads as "this shoulder peaks exactly at zero lag" on every gap,
        // which is the opposite of what a registration study wants to know. The measurement exists; it
        // just was not plumbed. The oracle path (spec only, no PCM) still cannot recover it and still
        // projects — which is why the declaration below is conditional rather than a constant.
        let measured = MeasuredDetail {
            brackets: Some(m.brackets),
            lag_decision: m.lag_decision,
        };
        let lag_supplied = measured.lag_decision.is_some();
        let silence = fp.silence.take();
        let contour = fp.contour.take();
        let anchors = fp.anchors.take();
        *fp = spec_to_fingerprint_summary(&spec, sample_rate, channels as u16, Some(x), measured);
        fp.silence = silence;
        fp.contour = contour;
        fp.anchors = anchors;
        // Count only gaps that ended up with a *fabricated* block. A gap with no sweep and no scalars
        // has no `lag_decision` at all, and absence needs no disowning.
        if !lag_supplied && fp.lag_decision.is_some() {
            projected_lag_gaps += 1;
        }

        // Gap-equivalence classification overlay (gap-equivalence plan §7.4) — emitted for tuning/categorizing.
        // Silence-character signals: A gap RMS vs the recording's noise floor + donor silence at nominal.
        // `enabled: true` here so the dump always classifies (it never drops gaps — that's the v1 plan-time gate).
        // F15 fixes 1–3. The diagnostic equivalence read now owns its sensors instead of borrowing
        // `fp.levels.*`: those are amplitude-mean downmixes over the refined span with no silence
        // predicate, and all three properties bias this path toward `drop`. They cannot simply be fixed
        // in `level_profile` — `levels.gap_floor_db` / `levels.noise_floor_db` have other consumers
        // (`snr_db`, dual-fit's `a_gap_floor_db`) that must not move. See
        // `docs/dev/archive/TEMP-equivalence-divergence-findings.md` § *The three F15 fixes*.
        //
        // The span is the **silent core** — `equivalence_production.a_span_secs`, taken straight off the
        // index-parallel scan verdict — not `refined` and not the raw gap. Scan's block grid is
        // media-absolute and selects blocks by centre-containment, which is what fix 3 adopted and which
        // held (the grid reproduced scan's lattice on 802/802 gaps of the 39-pair corpus). The *interval*
        // did not: fix 3 read `derive_gap_equivalence`'s then-parameter names `a_start_secs`/`a_end_secs`
        // and bound `geometry.a_*`, the raw hold-bridged run, while every caller of that function passes
        // `SilentRun::core_*`. The core is narrower by 1–2 blocks on 66.9 % of gaps, and the extra blocks
        // are fade shoulders — non-silent, so they drag `donor_silence_fraction` under 0.5 and flip
        // `shared_silence` to `repairable_dropout`. That is the *dangerous* direction (10 such gaps, where
        // the 17-pair predecessor had 0), and it is why the fallback below is the raw span rather than a
        // refusal: raw is what this path measured for months, so absent scan provenance costs no accuracy
        // it previously had — but `a_span` then says `nominal`, because it is.
        //
        // Taking the interval from scan rather than recomputing it is deliberate: a second derivation
        // would have to re-run silent-run detection at this path's bin width and could disagree again.
        // Reading the number scan used makes convergence structural. See `docs/gap-scan.md` and
        // `crate::domain::gap_equivalence::GapEquivalenceVerdict::a_span_secs`.
        //
        // The noise floor is read under the same interleaved reduction as A, since a level-dependent
        // reduction error does not cancel between the two sides of `a < nf − margin`. No downmix
        // fallback when the context is empty — that would un-apply fix 2 on exactly the gaps too thin to
        // measure; `None` classifies `NotEvaluated` ⇒ keep.
        //
        // I1 (2026-07-30): the whole equivalence overlay now bins at **`scan_block_ms`**, not
        // `gap_signature_bin_ms`. That parameter is documented as the bin width for *active/silent
        // structure signatures* and every other consumer uses it for exactly that — a binary seam pattern
        // match, where 50 ms is well chosen because finer bins discriminate syllable-scale structure. This
        // overlay is a different job (level + threshold estimation for comparison against scan) and had
        // inherited the value by proximity, never by choice. The property inverts between the two jobs:
        // for `gap_floor_db` (a **max**) and `donor_silence_fraction` (a **threshold-crossing fraction**)
        // finer bins are upward-biased, measured at max ≥ coarser on 10/10 gaps and the donor fraction
        // biased up on 5/6. `gap_signature_bin_ms` itself is untouched — it has production consumers in
        // `patch_audio::geometry` / `::region`. The context window (I2) was split for that same reason
        // and is **closed 2026-08-01 by removal** — see the `noise_floor_probe` call below.
        // See `docs/dev/archive/TEMP-equivalence-instrument-convergence.md` § I1.
        let equiv_bin_ms = report.recipe.scan_block_ms();
        //
        // The donor goes in as **PCM at the nominal `b_mapped` span**, not as
        // `donor_interior_nominal.silence_fraction`. That fraction is a mono-downmix read thresholded
        // against `levels.gap_floor_db` — the unfiltered whole-span peak — so passing it here would leave
        // fix 1 half-applied: A's floor would move while the predicate that actually reaches the class
        // still tested against the old one. On the band-donor mechanism that predicate *is* the flip.
        //
        // A/donor move together or not at all. Converging A onto the core while leaving the donor on
        // `geometry.b_mapped_*` would fix one end of a comparison whose whole content is the two ends
        // agreeing, so a scan verdict that carries a core but no donor (B unscanned) falls back on both.
        let scan_verdict = report.gap_equivalence.get(fp.index);
        let scan_core = scan_verdict
            .and_then(|v| v.a_span_secs)
            .filter(|(s, e)| e > s);
        let (a_span_secs, b_span_secs, span_kind) = match scan_core {
            Some(core) => (
                core,
                scan_verdict.and_then(|v| v.donor_span_secs),
                SpanKind::Core,
            ),
            None => (
                (fp.geometry.a_start_secs, fp.geometry.a_end_secs),
                fp.geometry
                    .b_mapped_start_secs
                    .zip(fp.geometry.b_mapped_end_secs),
                SpanKind::Nominal,
            ),
        };
        let gap_frames = (a_span_secs.0 * rate).round().max(0.0) as usize
            ..(a_span_secs.1 * rate).round().max(0.0) as usize;
        let equiv_nf = noise_floor_probe(
            &a_pcm.samples,
            ch,
            gap_frames.clone(),
            sample_rate,
            // I2 closed 2026-08-01 by **removal, not convergence**: this overlay now estimates the
            // noise floor over scan's window. Note this is the *argument*, not the field —
            // `gap_signature_context_secs` keeps its 3.0 s for `build_gap_fingerprint`'s context
            // frames, the B-extract padding below, and its production consumers in
            // `patch_audio::geometry` / `::region`. Only the equivalence floor moves.
            //
            // Why removal rather than a converged value: 3.0 s was never chosen for *this* job. It is
            // the sibling field of `gap_signature_bin_ms`, which I1 found had been "inherited by
            // proximity, never by choice" for the same reason — so the split was one considered value
            // (scan's, named and documented) against one accident, not two judgements in tension. The
            // header here claimed "both values encode a real judgement"; that was true of scan's only.
            //
            // The 3.0 s reading is **not lost** — `noise_floor_probe_grid` below still carries its row,
            // which is where a context-sensitivity question belongs: a labelled axis in the provenance,
            // not an unlabelled difference inside the verdict being compared.
            crate::domain::gap_equivalence::EQUIVALENCE_CONTEXT_SECS,
            equiv_bin_ms,
            ChannelReduction::Interleaved,
        );
        // Refuse a donor window B does not contain, rather than measuring whatever part of it exists.
        //
        // `measure_gap_equivalence` clamps its frame range to the samples available, so a window running
        // past B's end silently becomes a *shorter* window — and the truncated remainder of a tail gap is
        // digital silence, which scored 99.3–100 % silent on all 20 such gaps of the 39-pair corpus and
        // produced `shared_silence`/drop from audio that does not exist. Scan already fails closed here
        // (`b_range_fully_scanned`), which is why those gaps showed up as scan-vs-diagnostic divergences:
        // the disagreement was never about silence, only about whether to answer.
        //
        // `None` ⇒ no donor fraction ⇒ `NotEvaluated` ⇒ keep, the same stated refusal scan makes. The
        // `--check` warning stays: it reports the geometry, which is still worth seeing, and now nothing
        // downstream acts on it.
        let donor_span = b_span_secs
            .filter(|&(s, e)| {
                let lo = (s * rate).round().max(0.0) as usize;
                let hi = (e * rate).round().max(0.0) as usize;
                hi <= b_total && lo < hi
            })
            .map(|(s, e)| crate::application::gap_equivalence::DonorSpan {
                samples: b_samples_full,
                frames: (s * rate).round().max(0.0) as usize..(e * rate).round().max(0.0) as usize,
            });
        // Captured before `donor_span` is moved into the call below — the provenance token has to
        // report what was measured, and after the move there is nothing left to ask.
        let donor_measured = donor_span.is_some();
        let equiv = crate::application::gap_equivalence::measure_gap_equivalence(
            &a_pcm.samples,
            ch,
            gap_frames.clone(),
            equiv_nf.floor_db,
            donor_span,
            &crate::application::gap_equivalence::SilentCoreConfig {
                bin_frames: (((equiv_bin_ms as f64) / 1000.0) * rate).round().max(1.0) as usize,
                silence_peak_fraction: cfg.silence_peak_fraction,
                absolute_silence_rms: cfg.absolute_silence_rms,
            },
            &crate::domain::gap_equivalence::GapEquivalenceParams {
                enabled: true,
                ..Default::default()
            },
        );
        // Measurement recipe — attached here, not inside `measure_gap_equivalence`: that function never
        // sees `context_secs` / `bin_ms` (noise floor arrives precomputed; `SilentCoreConfig` has frames
        // only). See `docs/dev/archive/TEMP-fingerprint-provenance-plan.md` §3a.
        //
        // Both tokens are `span_kind`, reported rather than asserted. They read `core`/`nominal` as
        // literals until 2026-08-01 — `core` was simply wrong (this path measured the raw span), and a
        // provenance field that states the thing it exists to let you check is worse than no field: the
        // calibration diff printed `core` on both sides of every one of the 10 dangerous divergences the
        // span mismatch caused. A hardcoded token cannot report a fallback, so it does not get to be one.
        let measurement = EquivalenceMeasurement {
            // Must track the `noise_floor_probe` argument above, not `cfg` — a token reporting the
            // config field after the call site stopped reading it would be the third instance this
            // month of provenance describing an intent instead of a measurement (`a_span: core` on a
            // raw-span read; `donor_span: core` with no donor). Both sides now read 2.0.
            context_secs: crate::domain::gap_equivalence::EQUIVALENCE_CONTEXT_SECS,
            bin_ms: equiv_bin_ms,
            reduction: ChannelReduction::Interleaved,
            a_span: span_kind,
            // Taken from the donor that was actually built, not from `b_span_secs`: the EOF filter
            // above can reject a mapped window, and the token has to report the measurement, not the
            // intent. `None` when nothing was measured.
            donor_span: donor_measured.then_some(span_kind),
        };

        // Candidate noise floors (F15, second axis) over the {context window} × {bin size} × {channel
        // reduction} grid the two front-ends straddle — also provenance, also classified on by nothing.
        // Built by calling `level_profile` itself rather than re-deriving the bin walk, so a probe
        // cannot drift from the measurement it characterizes. **Retained** for I2 attribution.
        //
        // The `(EQUIVALENCE_CONTEXT_SECS, scan_block_ms, Interleaved)` row is the anchor: it matches
        // scan's recipe on all three variables and so should reproduce `equivalence_production.noise_floor_db`.
        // Its `Downmix` twin was the anchor before the reduction dimension existed, and undershot by
        // 3.13–7.96 dB on every gap of the first run — the two rows now sit side by side, and their
        // difference *is* the reduction term.
        let nf_probes = noise_floor_probe_grid(
            &a_pcm.samples,
            ch,
            // **`gap_frames`, the span the live measurement used** — not `refined`, which is this
            // path's own edge-refined interval and a *third* window distinct from both the scan core
            // and the nominal span. The grid varies three axes so the rows differ by those axes and
            // nothing else; holding a different interval than the measurement it characterizes makes
            // every row carry an unlabelled fourth term. Measured cost of the mismatch on the
            // 2026-08-01 4-pair run: the anchor row (which matches scan's recipe on all three axes
            // and should therefore reproduce `equivalence_production.noise_floor_db`) missed it on 33/33
            // gaps, median 0.24 dB and max 7.49 dB, and the `gap_signature_context_secs` row missed
            // this path's own live floor by up to 1.63 dB. Both are the interval, not the axes.
            //
            // Was `refined` until 2026-08-01 and correct while it lasted: the live measurement used
            // the raw span too, and the A-span convergence moved the measurement onto the scan core
            // without bringing the probes along. I2 cannot be attributed from a grid that measures a
            // window nothing else does.
            gap_frames.clone(),
            sample_rate,
            &[
                crate::domain::gap_equivalence::EQUIVALENCE_CONTEXT_SECS,
                cfg.gap_signature_context_secs,
            ],
            &[report.recipe.scan_block_ms(), cfg.gap_signature_bin_ms],
            &[ChannelReduction::Interleaved, ChannelReduction::Downmix],
        );

        // No `with_gap_floor_db` here any more: `equivalence.gap_floor_db` is now the **silent-core**
        // floor this path measured (fix 1), carried by `with_scan_provenance` alongside the silent-bin
        // count behind it. Re-attaching `levels.gap_floor_db` would overwrite the fix with the whole-span
        // content peak it exists to replace. `levels.gap_floor_db` is still dumped in its own block for
        // anyone who wants the old number.
        fp.equivalence_diagnostic = Some(Contracted::new(
            equiv
                .with_measurement(measurement)
                .with_noise_floor_probes(nf_probes),
            EQUIVALENCE_DIAGNOSTIC_CONTRACT,
        ));
    }

    // Copy in the coarse scan-block verdict (block size = the `scan_block_ms` recipe knob, not a
    // constant — see `default_scan_block_ms`; this comment said "250 ms" until 2026-07-30, long
    // after the default moved to 100), index-parallel to report gaps, so the corpus holds both
    // verdicts per gap and the calibration diff reads them from `corpus.json` alone. They are two
    // differently-*defined* measurements, not two granularities of one — see the table in
    // `bin/equivalence_calibration.rs` before treating either as the reference.
    //
    // **A separate pass, deliberately.** This ran inside the loop above until 2026-08-01, below the
    // `mapped_b_span` / `gap_frames` / `hi <= lo` guards — so a gap that tripped any of them lost
    // scan's verdict too, even though `derive_gap_equivalence` had already produced one and
    // `report.gap_equivalence` is pushed unconditionally (`scan_gaps.rs`, index-parallel). On the
    // 39-pair v0.5.0 corpus that silently discarded 27 real `NotEvaluated` verdicts — head gaps whose
    // negative offset maps B before zero. `NotEvaluated` is a *stated* refusal ("no donor, so no
    // judgement"); dropping it degrades that to an absence, which is precisely the unreadable-null
    // defect the provenance work exists to prevent. The diagnostic side genuinely cannot measure
    // those gaps and stays `None`; scan's answer is real and is now always carried.
    //
    // It cannot simply move to the top of the loop: `*fp = spec_to_fingerprint_summary(..)` rebuilds
    // the fingerprint wholesale mid-body and would clobber an early assignment.
    for fp in corpus.gaps.iter_mut() {
        fp.equivalence_production = report
            .gap_equivalence
            .get(fp.index)
            .cloned()
            .map(|v| Contracted::new(v, EQUIVALENCE_PRODUCTION_CONTRACT));
    }

    // Declare fabricated stand-ins this path still writes — stamped **here**, not in
    // `characterize_gaps`, because it is this function that rebuilds via
    // `spec_to_fingerprint_summary`. The `levels` envelope / `seam_shape` stay omitted (`None`)
    // rather than zeroed. Pass-1 `silence` / `contour` / `anchors` are restored after the rebuild,
    // so they are not declared. Only `lag_decision.*` may still need a declaration when the sweep
    // was projected rather than measured.
    //
    // Scoped to `DetailTier::Full` — gaps that never reached the rebuild keep `characterize_gaps`'s
    // real values and stay `Summary`. Only claim anything if the rebuild actually ran.
    //
    // `lag_decision.*` is appended only when some gap actually got the fabricated row. This path
    // normally threads the real sweep through `MeasuredDetail`, so the usual answer is "not declared,
    // because measured" — the declaration tracks what happened, not what the code is capable of.
    // Re-stamp the probe recipe now that the rebuild has run: `characterize_gaps` could only report
    // the collar, because at that point no gap carried a `lag_decision` or a `seams` block. Same
    // rule, later evidence — the recipe describes what this corpus can show, not what the config
    // could have swept.
    corpus.source.probe_recipe = Some(CorpusProbeRecipe::for_corpus(
        &cfg,
        sample_rate,
        &corpus.gaps,
    ));

    if corpus.gaps.iter().any(|g| g.tier == DetailTier::Full) {
        corpus.source.not_measured = NOT_MEASURED_BY_PROJECTION
            .iter()
            .chain(if projected_lag_gaps > 0 {
                PROJECTED_LAG_DECISION_FIELDS
            } else {
                &[]
            })
            .map(|s| (*s).to_string())
            .collect();
    }
    corpus
}
/// Build A-side **summary** fingerprints for the gaps in `select` (empty ⇒ all) against decoded
/// full A/B PCM: geometry + levels + contour + anchors + a baseline structure/seam per gap. The
/// authoritative gate detail (brackets / `failure_stage` / lag / outcome) is layered on by
/// [`characterize_gaps_from_decode`]. A gap with no B mapping is characterized A-only.
///
/// When `pcm.sources` reports disagreeing `native_channels`, returns
/// [`IncomparableReason::ChannelLayoutMismatch`] with no gaps — B must not be sliced at A's channel
/// count.
pub fn characterize_gaps(
    report: &crate::domain::GapReport,
    pcm: &CharacterizeAbPcm<'_>,
    cfg: &FingerprintConfig,
    select: &[usize],
) -> GapCorpus {
    let CharacterizeAbPcm {
        a_pcm,
        b_samples,
        sources,
    } = *pcm;
    let a_samples = a_pcm.samples.as_slice();
    let sample_rate = a_pcm.sample_rate;
    let channels = a_pcm.channels as usize;
    if let Some(src) = sources {
        if channel_layout_mismatch(src) {
            return refused_channel_mismatch_corpus(
                report,
                a_samples,
                b_samples,
                sample_rate,
                a_pcm.channels,
                src,
            );
        }
    }
    let rate = f64::from(sample_rate).max(1.0);
    let ch = channels.max(1);
    let b_total = b_samples.len() / ch;
    // Per-gap B haystack pad: context + border search + margin/slack on each side (mirrors
    // `prepare_region_patch`). Bounds the unified search so it does not build a timeline over all of B.
    let pad_lead =
        cfg.gap_signature_context_secs + cfg.fill_border_search_secs + cfg.fill_align_margin_secs;
    let pad_tail = cfg.gap_signature_context_secs
        + cfg.fill_border_search_secs
        + cfg
            .fill_extract_tail_slack_secs
            .max(cfg.fill_align_margin_secs)
        + cfg.fill_align_margin_secs;

    let take_all = select.is_empty();
    let gaps: Vec<GapFingerprint> = report
        .gaps
        .iter()
        .enumerate()
        .filter(|(i, _)| take_all || select.contains(i))
        .map(|(i, gap)| {
            // One guarded read of the mapped span: the reported offset and the extract window
            // must come from the same validated coordinates (F10).
            let mapped_b = gap.mapped_b_span();
            let gap_offset_secs = mapped_b
                .map(|(b0, _)| b0 - gap.video_a_start_secs)
                .unwrap_or(0.0);

            let (b_haystack, b_extract_start_secs) = match mapped_b {
                Some((b_start, b_end)) => {
                    let extract_start = (b_start - pad_lead).max(0.0);
                    let extract_end = b_end + pad_tail;
                    let lo = ((extract_start * rate) as usize).min(b_total);
                    let hi = ((extract_end * rate).ceil() as usize).min(b_total);
                    if hi > lo {
                        (Some(&b_samples[lo * ch..hi * ch]), lo as f64 / rate)
                    } else {
                        (None, 0.0)
                    }
                }
                None => (None, 0.0),
            };

            let inputs = GapInputs {
                a_samples,
                b_haystack,
                channels,
                sample_rate,
                reported_start_frame: (gap.video_a_start_secs * rate) as usize,
                reported_end_frame: (gap.video_a_end_secs * rate) as usize,
                b_extract_start_secs,
                gap_offset_secs,
                config: *cfg,
            };
            build_gap_fingerprint(i, &inputs, DetailTier::Summary, true)
        })
        .collect();

    GapCorpus {
        source: SourceMeta {
            a_source: file_source(
                a_samples,
                sample_rate,
                channels as u16,
                sources.map(|s| &s.a),
            ),
            b_source: file_source(
                b_samples,
                sample_rate,
                channels as u16,
                sources.map(|s| &s.b),
            ),
            scan_recipe: CorpusScanRecipe::from_report(report),
            gap_count: report.gaps.len(),
            incomparable: None,
            // Summary-only path does not run the seam gate; from-decode stamps `gate_recipe`.
            gate_recipe: None,
            // Built from `gaps`, not from `cfg`: this path is summary-only, so it must not quote the
            // lag/seam windows it never swept. `characterize_gaps_from_decode` re-stamps after its
            // `DetailTier::Full` rebuild, when those measurements exist.
            probe_recipe: Some(CorpusProbeRecipe::for_corpus(cfg, sample_rate, &gaps)),
            // **No `not_measured` here.** This path measures every one of those fields —
            // `build_gap_fingerprint` fills levels/silence/contour/anchors for real. The from-decode
            // rebuild restores the three groups after projection; the remaining fabricated
            // stand-ins (`lag_decision.*` on the oracle path) are declared there, not here.
            not_measured: Vec::new(),
            alignment: stamp(
                Some(CorpusScanAlignment::from_report(report)),
                ALIGNMENT_CONTRACT,
            ),
        },
        gaps,
    }
}
#[cfg(any(feature = "calibration", test))]
pub(in crate::application::gap_fingerprint::measure) fn detail_tier_str(
    t: DetailTier,
) -> &'static str {
    match t {
        DetailTier::Summary => "summary",
        DetailTier::Full => "full",
    }
}
#[cfg(any(feature = "calibration", test))]
pub(in crate::application::gap_fingerprint::measure) fn lag_verdict_str(
    v: LagVerdict,
) -> &'static str {
    match v {
        LagVerdict::TimingOffset => "timing_offset",
        LagVerdict::Decorrelated => "decorrelated",
        LagVerdict::Ambiguous => "ambiguous",
    }
}
#[cfg(any(feature = "calibration", test))]
pub(in crate::application::gap_fingerprint::measure) fn hms(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}-{:02}-{:02}", s / 3600, (s % 3600) / 60, s % 60)
}
/// `<a8>_<b4>_t<hh-mm-ss>_g<idx>_<tier>` — non-leaking, sortable, classifiable.
///
/// **No verdict token.** The stem carried one until 2026-08-12 (the lag verdict if measured, else
/// `outcome.tier`, else `na`). It was removed because it read as production's disposition and is
/// not: `outcome.tier` is the bracket-gate result, and a `skip` gap is routinely patched by
/// production's `skip_or_dual_fit` (`schema.rs`, `outcome`). It was also not flag-stable — the same
/// gap took a different suffix with vs without `--fingerprint-diagnostics`, since the lag verdict
/// only exists on diagnostic runs. Both verdicts remain structured data: `outcome.tier` and
/// `lag_editorial` in the gap JSON, `outcome` / `lag_verdict` in `manifest.json`. The `full` /
/// `summary` detail tier stays — it is accurate, flag-stable, and says how much is in the file.
///
/// **Extension-free on purpose.** This is the single naming authority for everything a gap emits:
/// `write_corpus_dir` appends `.json`, and the `--gap-listen` export appends `_a_surround.wav` /
/// `_b_surround.wav` / `_a_patched.wav`. That shared stem is the ears ↔ JSON join, so a WAV can
/// always be traced back to the fingerprint that describes it. Baking `.json` in here (as this did
/// before 2026-08-02) would force the exporter to reconstruct the format string and let the two
/// namings drift apart silently.
///
/// Note the stem is a function of the **built** fingerprint (`tier`, refined start), not of the gap
/// alone — so an exporter must look its gap up in the corpus by `index`.
#[cfg(any(feature = "calibration", test))]
pub(crate) fn entry_stem(source: &SourceMeta, gap: &GapFingerprint) -> String {
    let a8: String = source.a_source.id.chars().take(8).collect();
    let b4: String = source.b_source.id.chars().take(4).collect();
    format!(
        "{a8}_{b4}_t{}_g{:03}_{}",
        hms(gap.geometry.a_refined_start_secs),
        gap.index,
        detail_tier_str(gap.tier),
    )
}
#[cfg(any(feature = "calibration", test))]
#[derive(serde::Serialize)]
pub(in crate::application::gap_fingerprint::measure) struct ManifestEntry {
    file: String,
    index: usize,
    a_start_secs: f64,
    tier: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lag_verdict: Option<String>,
}
#[cfg(any(feature = "calibration", test))]
#[derive(serde::Serialize)]
pub(in crate::application::gap_fingerprint::measure) struct Manifest<'a> {
    a_id: &'a str,
    b_id: &'a str,
    scan_recipe: &'a CorpusScanRecipe,
    gap_count: usize,
    entries: Vec<ManifestEntry>,
}
/// Write a self-contained corpus directory: the combined `corpus.json` (all gaps), one single-gap
/// [`GapCorpus`] JSON **per gap**, and a non-identifying `manifest.json`. Returns the gap count. No
/// titles/paths anywhere.
#[cfg(any(feature = "calibration", test))]
pub(crate) fn write_corpus_dir(
    corpus: &GapCorpus,
    dir: &std::path::Path,
) -> std::io::Result<usize> {
    let to_io = |e: serde_json::Error| std::io::Error::other(e);
    std::fs::create_dir_all(dir)?;
    // Combined corpus (all gaps) for quick inspection / scripting.
    let combined = std::fs::File::create(dir.join("corpus.json"))?;
    serde_json::to_writer_pretty(combined, corpus).map_err(to_io)?;

    let mut entries = Vec::with_capacity(corpus.gaps.len());
    for gap in &corpus.gaps {
        let file = format!("{}.json", entry_stem(&corpus.source, gap));
        let single = GapCorpus {
            source: corpus.source.clone(),
            gaps: vec![gap.clone()],
        };
        let f = std::fs::File::create(dir.join(&file))?;
        serde_json::to_writer_pretty(f, &single).map_err(to_io)?;
        entries.push(ManifestEntry {
            file,
            index: gap.index,
            a_start_secs: gap.geometry.a_refined_start_secs,
            tier: detail_tier_str(gap.tier),
            outcome: gap.outcome.as_ref().map(|o| o.tier.clone()),
            lag_verdict: gap
                .lag_editorial
                .as_ref()
                .and_then(|l| l.pre_anchor.first().or_else(|| l.post_anchor.first()))
                .map(|s| lag_verdict_str(s.verdict).to_string()),
        });
    }
    let manifest = Manifest {
        a_id: &corpus.source.a_source.id,
        b_id: &corpus.source.b_source.id,
        scan_recipe: &corpus.source.scan_recipe,
        gap_count: corpus.gaps.len(),
        entries,
    };
    let mf = std::fs::File::create(dir.join("manifest.json"))?;
    serde_json::to_writer_pretty(mf, &manifest).map_err(to_io)?;
    Ok(corpus.gaps.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::gap_equivalence::{ChannelReduction, SpanKind};

    fn noise(seed: u64, i: usize) -> f64 {
        let mut z = ((seed << 32) | (i as u64 & 0xffff_ffff)).wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 40) as f64 / (1u64 << 24) as f64) * 2.0 - 1.0
    }

    fn write_speech(buf: &mut [f32], start: usize, end: usize, freq: f64, amp: f32) {
        let n = (end - start) as f64;
        for (f, slot) in buf.iter_mut().enumerate().take(end).skip(start) {
            let t = (f - start) as f64;
            let env = 0.5 - 0.5 * (std::f64::consts::TAU * t / n).cos();
            let s = (std::f64::consts::TAU * freq * t / 48_000.0).sin();
            *slot = (env * s) as f32 * amp;
        }
    }

    fn write_noise(buf: &mut [f32], start: usize, end: usize, seed: u64, amp: f32) {
        for (f, slot) in buf.iter_mut().enumerate().take(end).skip(start) {
            *slot = noise(seed, f) as f32 * amp;
        }
    }
    /// One 2.5 s gap with speech shoulders on both sides and low-level noise filling B's donor window:
    /// enough for the full projection path to produce a `DetailTier::Full` gap with a real donor.
    fn equivalence_overlay_fixture() -> (
        crate::domain::GapReport,
        clip_sync::MultiChannelPcm,
        Vec<f32>,
    ) {
        use crate::domain::gap::Gap;
        use crate::domain::{GapReport, ScanAlignment};
        use clip_sync::MultiChannelPcm;

        let rate = 48_000u32;
        let secs = |s: f64| (s * f64::from(rate)) as usize;
        let mut a = vec![0f32; secs(6.0)];
        let mut b = vec![0f32; secs(6.0)];
        write_speech(&mut a, secs(0.5), secs(1.5), 330.0, 0.063);
        write_speech(&mut b, secs(0.5), secs(1.5), 330.0, 0.063);
        write_speech(&mut a, secs(4.0), secs(5.0), 440.0, 0.079);
        write_speech(&mut b, secs(4.0), secs(5.0), 440.0, 0.079);
        write_noise(&mut b, secs(1.5), secs(4.0), 5, 0.0056);
        let a_pcm = MultiChannelPcm {
            sample_rate: rate,
            channels: 1,
            samples: a,
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };
        let report = GapReport {
            local_pair_collar_secs: None,
            video_a: Default::default(),
            video_b: Default::default(),
            track_compatibility: None,
            alignment: ScanAlignment {
                clips: vec![],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                query_reference_mode: false,
            },
            gaps: vec![Gap {
                video_a_start_secs: 1.5,
                video_a_end_secs: 4.0,
                video_b_start_secs: Some(1.5),
                video_b_end_secs: Some(4.0),
                b_has_energy: true,
                collar_level: None,
            }],
            gap_equivalence: vec![Default::default()],
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
            program_levels: None,
        };
        (report, a_pcm, b)
    }

    /// `characterize_gaps` alone measures those fields, so it must **not** declare them unmeasured.
    ///
    /// The declaration is owned by the projection rebuild in `characterize_gaps_from_decode`, not by
    /// the summary builder that feeds it. Stamped one function too early it would make this corpus
    /// disown levels/silence/contour/anchors it genuinely holds — and `--check` treats a false
    /// declaration as an error precisely because it licenses discarding real data.
    #[test]
    fn summary_characterize_does_not_declare_fields_it_measured() {
        let (report, a_pcm, b) = equivalence_overlay_fixture();
        let corpus = characterize_gaps(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &FingerprintConfig::default(),
            &[],
        );
        assert!(
            corpus.source.not_measured.is_empty(),
            "summary path measures these fields; declaring them unmeasured is a false claim: {:?}",
            corpus.source.not_measured
        );
        // Not vacuous: prove the path really does fill the envelope it used to declare unmeasured.
        assert!(
            corpus
                .gaps
                .iter()
                .any(|g| g.levels.bin_ms.is_some_and(|b| b != 0) || !g.levels.profile_db.is_empty()),
            "fixture must exercise a measured levels envelope field"
        );
    }

    /// The probe row matching the live recipe **reproduces the live floor exactly**.
    ///
    /// That identity is what makes the grid an attribution instrument: rows differ by the three axes
    /// they vary and by nothing else, so a row-to-row delta *is* that axis's term. It held until the
    /// A-span convergence moved the live measurement onto the scan core and left the grid on this
    /// path's `refined` span — an unlabelled fourth term worth up to 1.63 dB against the live floor
    /// and 7.49 dB against scan's, which is the size of the I2 effect the grid exists to measure.
    #[test]
    fn noise_floor_probes_measure_the_span_the_equivalence_measurement_used() {
        let (report, a_pcm, b) = equivalence_overlay_fixture();
        let corpus = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &crate::infrastructure::config::RepairConfig::default()
                .patch_settings()
                .into_request(report.clone())
                .expect("default All gap selection"),
            &[],
            DumpOptions::default(),
            &clip_sync_repair_fixtures::NoOpProgressReporter,
        );
        let eq = corpus.gaps[0]
            .equivalence_diagnostic_verdict()
            .expect("overlay ran");
        let m = eq
            .measurement
            .as_ref()
            .expect("measurement recipe recorded");
        let live = eq.noise_floor_db.expect("a floor was measured");
        let row = eq
            .noise_floor_probes
            .iter()
            .find(|p| {
                p.context_secs == m.context_secs
                    && p.bin_ms == m.bin_ms
                    && p.reduction == m.reduction
            })
            .expect("the grid must contain the live recipe's own row");
        assert!(
            (row.floor_db.expect("row measured") - live).abs() < 1e-9,
            "probe row {:?} reads {:?} but the live measurement reads {live} — the grid is measuring \
             a different span, so every row carries an unlabelled term",
            (row.context_secs, row.bin_ms, row.reduction),
            row.floor_db,
        );
    }

    /// A donor window running past B's end is **refused**, not measured against a clamped remainder.
    ///
    /// `measure_gap_equivalence` clamps its frame range to what exists, so without the guard a tail
    /// gap measures a *shorter* window whose missing part reads as digital silence — 20 gaps of the
    /// 39-pair corpus scored 99.3–100 % silent that way and drew `shared_silence`/drop from audio
    /// B does not contain. Scan already fails closed here, so the divergence was never about silence,
    /// only about whether to answer. Absent donor ⇒ no fraction ⇒ keep.
    #[test]
    fn diagnostic_equivalence_refuses_a_donor_window_past_b_end() {
        let (report, a_pcm, b) = equivalence_overlay_fixture();
        // B stops at 3.0 s; the gap's donor window is 1.5–4.0 s, so 1 s of it does not exist.
        let truncated: Vec<f32> = b[..(3.0 * 48_000.0) as usize].to_vec();
        let corpus = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &truncated,
                sources: None,
            },
            &crate::infrastructure::config::RepairConfig::default()
                .patch_settings()
                .into_request(report.clone())
                .expect("default All gap selection"),
            &[],
            DumpOptions::default(),
            &clip_sync_repair_fixtures::NoOpProgressReporter,
        );
        let eq = corpus.gaps[0]
            .equivalence_diagnostic_verdict()
            .expect("diagnostic overlay still runs — the refusal is about the donor, not the gap");
        assert_eq!(
            eq.measurement.as_ref().and_then(|m| m.donor_span),
            None,
            "the provenance token must report the refusal, not the intent"
        );
        assert_eq!(
            eq.donor_silence_fraction, None,
            "nothing may be measured against samples B does not have"
        );
        assert!(
            !eq.class.drops(),
            "a refusal keeps the gap; dropping on absent audio is the defect this guards"
        );
    }

    /// Production from-decode dump omits unmeasured Option fields and leaves `not_measured` empty
    /// when the real `lag_decision` sweep was threaded through (no fabricated lag stand-ins).
    #[test]
    fn production_dump_omits_unmeasured_option_fields() {
        use crate::application::PatchAudioRequest;
        use crate::infrastructure::config::RepairConfig;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let (report, a_pcm, b) = equivalence_overlay_fixture();
        let repair = RepairConfig {
            fill_border_search_secs: 0.05,
            fill_align_margin_secs: 0.02,
            fill_extract_tail_slack_secs: 0.1,
            fill_seam_search_secs: 0.05,
            max_anchor_bracket_secs: 0.2,
            max_anchors_per_side: 2,
            ..RepairConfig::default()
        };
        let request: PatchAudioRequest = repair
            .patch_settings()
            .into_request(report.clone())
            .expect("default All gap selection");
        let corpus = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &request,
            &[],
            DumpOptions::default(),
            &NoOpProgressReporter,
        );
        assert_eq!(
            corpus.source.not_measured, NOT_MEASURED_BY_PROJECTION,
            "the production dump must leave not_measured empty when it threads the real \
             `lag_decision` through (no `PROJECTED_LAG_DECISION_FIELDS`)"
        );
        let recipe = corpus
            .source
            .gate_recipe
            .as_ref()
            .expect("from-decode dump stamps the seam-gate recipe");
        assert_eq!(
            recipe.min_fill_correlation, request.min_fill_correlation,
            "gate_recipe must echo the settings used to score brackets"
        );
        assert_eq!(
            recipe.fill_absolute_floor, request.fill_absolute_floor,
            "gate_recipe must echo the settings used to score brackets"
        );
        assert_eq!(
            recipe.residual_headroom_margin_db,
            request.residual_headroom_margin_db
        );
        // The probe's own geometry, re-stamped after the `DetailTier::Full` rebuild. This dump DID
        // run the sweeps, so unlike a summary-only corpus it must report their windows — and it must
        // report the probe's standoff, never production's `border_standoff_secs`.
        let probe = corpus
            .source
            .probe_recipe
            .as_ref()
            .expect("from-decode dump stamps the probe recipe");
        let cfg = FingerprintConfig::from_request(
            &request,
            report.recipe.silence_peak_fraction(),
            DumpOptions::default(),
        );
        assert_eq!(
            probe.lag_window_secs,
            Some(cfg.lag_window_secs),
            "a dump that swept lag_decision must report the window it swept with"
        );
        assert_eq!(probe.lag_max_lag_ms, Some(cfg.lag_max_lag_ms));
        assert_eq!(
            probe.lag_envelope_max_lag_ms,
            Some(cfg.lag_envelope_max_lag_ms),
            "a dump that swept lag_decision must record the envelope knob, including 0"
        );
        assert_eq!(probe.noise_collar_secs, request.normalize_window_secs);
        assert_eq!(
            probe.border_standoff_secs,
            probe_border_standoff_secs(corpus.gaps[0].sample_rate),
            "the probe measures at standoff 0; echoing production's 0.35 would misdescribe every \
             recorded frac_lag_ms"
        );
        let full: Vec<_> = corpus
            .gaps
            .iter()
            .filter(|g| g.tier == DetailTier::Full)
            .collect();
        assert!(!full.is_empty(), "fixture must produce a measured gap");
        for g in full {
            assert!(g.levels.bin_ms.is_none());
            assert!(g.levels.profile_db.is_empty());
            assert!(g.levels.floor_db.is_none());
            assert!(g.levels.speech_peak_db.is_none());
            assert!(
                g.silence.is_some(),
                "from-decode overlay restores pass-1 silence"
            );
            assert!(
                g.contour.is_some(),
                "from-decode overlay restores pass-1 contour"
            );
            assert!(
                g.anchors.is_some(),
                "from-decode overlay restores pass-1 anchors (empty lists stay Some)"
            );
            assert!(
                g.outcome.as_ref().is_some_and(|o| o.seam_shape.is_none()),
                "seam_shape must be omitted, not hardcoded empty"
            );
            // This path threads the real sweep through `MeasuredDetail`, so the rows must NOT carry
            // `projected_lag_entry`'s signature: a real sweep reports its search width, and `lag0_r`
            // is an independent read rather than a copy of `peak_r`. Guarding the shape here is what
            // keeps the conditional declaration below honest in the other direction — if the
            // pass-through ever regresses to projection, this fires before the declaration does.
            for e in g
                .lag_decision
                .iter()
                .flat_map(|bl| bl.pre_anchor.iter().chain(bl.post_anchor.iter()))
            {
                assert_ne!(e.max_lag_ms, 0, "real sweep reports its search width");
                assert_ne!(e.window_ms, 0, "real sweep reports its window");
            }
        }
        assert!(
            corpus.gaps.iter().any(|g| g.lag_decision.is_some()),
            "fixture must produce a lag_decision to make the assertions above non-vacuous"
        );
    }

    fn fixture_patch_request(
        report: crate::domain::GapReport,
    ) -> crate::application::PatchAudioRequest {
        use crate::infrastructure::config::RepairConfig;
        RepairConfig {
            fill_border_search_secs: 0.05,
            fill_align_margin_secs: 0.02,
            fill_extract_tail_slack_secs: 0.1,
            fill_seam_search_secs: 0.05,
            max_anchor_bracket_secs: 0.2,
            max_anchors_per_side: 2,
            ..RepairConfig::default()
        }
        .patch_settings()
        .into_request(report)
        .expect("default All gap selection")
    }

    /// Overlay copies pass-1 values; it does not re-detect.
    #[test]
    fn from_decode_restores_pass1_silence_contour_anchors() {
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let (report, a_pcm, b) = equivalence_overlay_fixture();
        let request = fixture_patch_request(report.clone());
        let cfg = FingerprintConfig::from_request(
            &request,
            report.recipe.silence_peak_fraction(),
            DumpOptions::default(),
        );
        let pcm = CharacterizeAbPcm {
            a_pcm: &a_pcm,
            b_samples: &b,
            sources: None,
        };
        let pass1 = characterize_gaps(&report, &pcm, &cfg, &[]);
        let corpus = characterize_gaps_from_decode(
            &report,
            &pcm,
            &request,
            &[],
            DumpOptions::default(),
            &NoOpProgressReporter,
        );
        let full: Vec<_> = corpus
            .gaps
            .iter()
            .filter(|g| g.tier == DetailTier::Full)
            .collect();
        assert!(!full.is_empty(), "fixture must produce a measured gap");
        for g in full {
            let p1 = pass1
                .gaps
                .iter()
                .find(|p| p.index == g.index)
                .expect("pass-1 row");
            assert_eq!(g.silence, p1.silence, "restore, not a second detector");
            assert_eq!(g.contour, p1.contour, "restore, not a second detector");
            assert_eq!(g.anchors, p1.anchors, "restore, not a second detector");
            assert!(g.silence.is_some());
            assert!(g.contour.is_some());
            assert!(g.anchors.is_some());
        }
    }

    /// `Some({pre:[], post:[]})` is measured-none, not omitted.
    #[test]
    fn empty_anchor_set_stays_some_on_the_wire() {
        let fp = mk_fp(0, false);
        assert_eq!(
            fp.anchors,
            Some(AnchorSet {
                pre: vec![],
                post: vec![]
            })
        );
        let json = serde_json::to_value(&fp).expect("json");
        assert!(
            json.get("anchors").is_some(),
            "empty lists must not drop the key"
        );
        assert_eq!(json["anchors"]["pre"], serde_json::json!([]));
        assert_eq!(json["anchors"]["post"], serde_json::json!([]));
    }

    /// Overlay assignment after a lossy projection must not turn `Some({pre:[], post:[]})` into
    /// `None`. From-decode never produces empty lists (`list_anchor_candidates_a` always inserts a
    /// scan-refined fallback); this is the caller-side restore, not a second detector.
    #[test]
    fn empty_anchor_set_stays_some_after_projection_overlay() {
        let pass1 = mk_fp(0, true);
        let spec = fingerprint_to_spec(&pass1);
        let mut fp = spec_to_fingerprint_summary(
            &spec,
            pass1.sample_rate,
            pass1.channels,
            None,
            MeasuredDetail::default(),
        );
        assert!(
            fp.anchors.is_none(),
            "projector stays lossy; overlay is the caller's job"
        );
        fp.anchors = pass1.anchors.clone();
        assert_eq!(
            fp.anchors,
            Some(AnchorSet {
                pre: vec![],
                post: vec![]
            })
        );
    }

    #[test]
    fn source_alignment_stamped_from_the_fixture_report() {
        use crate::domain::GapOffsetAgreement;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let (mut report, a_pcm, b) = equivalence_overlay_fixture();
        report.alignment = crate::domain::align::test_two_clip_alignment(-1.15, -1.10);
        report.gap_offset_agreement = Some(GapOffsetAgreement {
            silence_based_offset_secs: -1.20,
            alignment_offset_secs: -1.125,
            delta_secs: 0.075,
            agrees: true,
        });
        let request = fixture_patch_request(report.clone());
        let corpus = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &request,
            &[],
            DumpOptions::default(),
            &NoOpProgressReporter,
        );
        let align = corpus
            .source
            .alignment
            .as_ref()
            .expect("from-decode stamps scan alignment");
        assert_eq!(
            align.recommended_offset_secs,
            report.alignment.recommended_offset_secs
        );
        assert_eq!(align.offset_drift_secs, report.alignment.offset_drift_secs);
        assert_eq!(align.clips.len(), 2);
        assert_eq!(align.clips[0].role, CorpusClipRole::Start);
        let json = serde_json::to_value(align).expect("alignment json");
        assert_eq!(json["clips"][0]["role"], "start");
        assert!(json.get("clips").unwrap()[0]
            .get("video_a_decode_skips")
            .is_none());
        assert!(json.get("clips").unwrap()[0]
            .get("video_b_window_start_secs")
            .is_none());
        let xcheck = align
            .silence_cross_check
            .as_ref()
            .expect("cross-check copied");
        assert!((xcheck.delta_secs - 0.075).abs() < 1e-12);
        assert!(
            json["_contract"]["not"]
                .as_str()
                .is_some_and(|n| n.starts_with("lag_decision")),
            "alignment contract must name lag_decision first"
        );
    }

    /// The conditional list must not overlap the unconditional one.
    ///
    /// If a `lag_decision.*` path appeared in both, the declaration would be emitted even on dumps
    /// that measured the sweep — claiming a field is unmeasured when it is real, which `--check`
    /// treats as worse than no declaration at all (it licenses discarding good data). The
    /// exact-equality assertion in the test above is what proves the from-decode dump omits them;
    /// this proves the constants can't make that impossible.
    #[test]
    fn conditional_lag_fields_stay_out_of_the_unconditional_list() {
        for f in PROJECTED_LAG_DECISION_FIELDS {
            assert!(
                !NOT_MEASURED_BY_PROJECTION.contains(f),
                "{f} is conditional; it must not sit in the unconditional list too"
            );
        }
    }

    /// A gap the fingerprint cannot measure still carries scan's verdict.
    ///
    /// The head-gap shape: A silent from t=0 in a pair with a negative offset, so B's window maps before
    /// zero, `Gap::mapped_b_span` fails closed and the per-gap loop `continue`s. Scan does **not** fail
    /// here — `scan_gaps.rs` pushes a verdict unconditionally and yields `NotEvaluated`, a *stated*
    /// refusal to classify (⇒ keep). The copy that carries it into the dump used to sit at the bottom of
    /// that loop body, below the `continue`, so 27 of 829 gaps on the 39-pair v0.5.0 corpus reached
    /// `equivalence-calibration` with neither verdict and were dropped from the comparison population
    /// silently. Degrading a stated refusal into an absent key is the provenance plan's §1.1 defect.
    #[test]
    fn scan_verdict_survives_a_gap_the_fingerprint_cannot_map() {
        use crate::application::PatchAudioRequest;
        use crate::domain::gap::Gap;
        use crate::domain::gap_equivalence::{GapEquivalenceClass, GapEquivalenceVerdict};
        use crate::domain::{GapReport, ScanAlignment};
        use crate::infrastructure::config::RepairConfig;
        use clip_sync::MultiChannelPcm;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let rate = 48_000u32;
        let a_pcm = MultiChannelPcm {
            sample_rate: rate,
            channels: 1,
            samples: vec![0f32; rate as usize * 2],
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };
        let report = GapReport {
            local_pair_collar_secs: None,
            video_a: Default::default(),
            video_b: Default::default(),
            track_compatibility: None,
            alignment: ScanAlignment {
                clips: vec![],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                query_reference_mode: false,
            },
            // The head gap: A from 0.0, B mapped negative by the pair's offset ⇒ `mapped_b_span` = None.
            gaps: vec![Gap {
                video_a_start_secs: 0.0,
                video_a_end_secs: 0.8,
                video_b_start_secs: Some(-0.4),
                video_b_end_secs: Some(0.4),
                b_has_energy: true,
                collar_level: None,
            }],
            gap_equivalence: vec![GapEquivalenceVerdict {
                class: GapEquivalenceClass::NotEvaluated,
                ..Default::default()
            }],
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
            program_levels: None,
        };
        let repair = RepairConfig::default();
        let request: PatchAudioRequest = repair
            .patch_settings()
            .into_request(report.clone())
            .expect("default All gap selection");
        let corpus = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &vec![0f32; rate as usize * 2],
                sources: None,
            },
            &request,
            &[],
            DumpOptions::default(),
            &NoOpProgressReporter,
        );
        let fp = &corpus.gaps[0];
        assert!(
            fp.equivalence_diagnostic.is_none(),
            "unmappable gap: the diagnostic path genuinely cannot measure it"
        );
        assert_eq!(
            fp.equivalence_production_verdict().map(|v| v.class),
            Some(GapEquivalenceClass::NotEvaluated),
            "scan's stated refusal must reach the dump; an absent key would read as 'never asked'"
        );
    }

    /// C1: both equivalence verdicts ship a co-located `_contract`, and the two carry **different**
    /// text — that per-field difference is the reason the wrapper exists rather than a field on the
    /// shared `GapEquivalenceVerdict`. Contracts are reader metadata: their presence must never
    /// change a class, so this asserts alongside, not instead of, the verdict itself.
    #[test]
    fn from_decode_dump_stamps_a_contract_on_both_equivalence_verdicts() {
        let (report, a_pcm, b) = equivalence_overlay_fixture();
        let corpus = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &crate::infrastructure::config::RepairConfig::default()
                .patch_settings()
                .into_request(report.clone())
                .expect("default All gap selection"),
            &[],
            DumpOptions::default(),
            &clip_sync_repair_fixtures::NoOpProgressReporter,
        );
        let fp = &corpus.gaps[0];

        let diag = fp
            .equivalence_diagnostic
            .as_ref()
            .expect("overlay ran")
            .contract
            .as_ref()
            .expect("diagnostic contract stamped on write");
        let prod = fp
            .equivalence_production
            .as_ref()
            .expect("scan verdict copied in")
            .contract
            .as_ref()
            .expect("production contract stamped on write");

        assert_ne!(
            diag, prod,
            "the pair shares one type; contract text must still distinguish them"
        );
        assert!(
            prod.not
                .as_deref()
                .is_some_and(|s| s.contains("equivalence_diagnostic")),
            "the authoritative verdict must name its confusable sibling: {prod:?}"
        );
        assert!(
            diag.not
                .as_deref()
                .is_some_and(|s| s.contains("equivalence_production")),
            "the diagnostic verdict must point at the authoritative one: {diag:?}"
        );

        // `_contract` sits beside the values, not around them (flatten), and does not displace them.
        let json = serde_json::to_value(fp).expect("serialize");
        assert!(json["equivalence_production"]["_contract"]["placement"].is_string());
        assert!(json["equivalence_production"]["class"].is_string());
    }

    /// The diagnostic equivalence overlay measures the **silent core**, taken off the
    /// index-parallel scan verdict, and says so in `measurement.a_span` / `donor_span`.
    ///
    /// This is the convergence half of the 2026-08-01 fix. The overlay used to window on
    /// `geometry.a_start_secs`/`a_end_secs` — the raw hold-bridged run — while scan windows on
    /// `SilentRun::core_*`; on the 39-pair v0.5.0 corpus the two disagreed on `a_gap_total_blocks` in
    /// 66.9 % of gaps, and the extra blocks (fade shoulders, non-silent) dragged `donor_silence_fraction`
    /// under the 0.5 threshold on 10 gaps, flipping scan's `shared_silence`/drop to the diagnostic's
    /// `repairable_dropout`/keep. Both verdicts printed `a_span: core` throughout, so the provenance
    /// field concealed the divergence it existed to expose.
    ///
    /// Asserted against the *same* fixture with an empty `gap_equivalence` (the fallback arm, pinned in
    /// `characterize_gaps_from_decode_include_diagnostics_toggles_x_set`), so the block counts move for
    /// one reason only: which interval was binned.
    #[test]
    fn diagnostic_equivalence_adopts_the_scan_verdicts_core_span() {
        use crate::application::PatchAudioRequest;
        use crate::domain::gap::Gap;
        use crate::domain::gap_equivalence::{GapEquivalenceClass, GapEquivalenceVerdict};
        use crate::domain::{GapReport, GapSignatureMode, ScanAlignment};
        use crate::infrastructure::config::RepairConfig;
        use clip_sync::MultiChannelPcm;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let rate = 48_000u32;
        let ch = 1usize;
        let secs = |s: f64| (s * f64::from(rate)) as usize;
        let total = secs(5.0);
        let (sp1, n1, gap, n2, sp2) = (
            (secs(0.50), secs(0.85)),
            (secs(0.85), secs(1.85)),
            (secs(1.85), secs(3.35)),
            (secs(3.35), secs(4.35)),
            (secs(4.35), secs(4.70)),
        );
        let mut a = vec![0f32; total];
        let mut b = vec![0f32; total];
        write_speech(&mut a, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut b, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut a, sp2.0, sp2.1, 440.0, 0.079);
        write_speech(&mut b, sp2.0, sp2.1, 440.0, 0.079);
        write_noise(&mut a, n1.0, n1.1, 1, 0.0056);
        write_noise(&mut b, n1.0, n1.1, 11, 0.0056);
        write_noise(&mut a, n2.0, n2.1, 3, 0.0056);
        write_noise(&mut b, n2.0, n2.1, 13, 0.0056);
        write_noise(&mut b, gap.0, gap.1, 5, 0.0056);

        let a_pcm = MultiChannelPcm {
            sample_rate: rate,
            channels: ch as u16,
            samples: a,
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };
        let f = |x: usize| x as f64 / f64::from(rate);
        let report = |gap_equivalence: Vec<GapEquivalenceVerdict>| GapReport {
            local_pair_collar_secs: None,
            video_a: Default::default(),
            video_b: Default::default(),
            track_compatibility: None,
            alignment: ScanAlignment {
                clips: vec![],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                query_reference_mode: false,
            },
            gaps: vec![Gap {
                video_a_start_secs: f(gap.0),
                video_a_end_secs: f(gap.1),
                video_b_start_secs: Some(f(gap.0)),
                video_b_end_secs: Some(f(gap.1)),
                b_has_energy: true,
                collar_level: None,
            }],
            gap_equivalence,
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
            program_levels: None,
        };
        // Same search-radius trimming as the C3 test, and for the same reason: the 5 s fixture is far
        // smaller than the production defaults assume.
        let repair = RepairConfig {
            gap_signature_mode: GapSignatureMode::Energy,
            gap_signature_context_secs: 1.5,
            fill_border_search_secs: 0.05,
            fill_align_margin_secs: 0.02,
            fill_extract_tail_slack_secs: 0.1,
            fill_seam_search_secs: 0.05,
            max_anchor_bracket_secs: 0.2,
            max_anchors_per_side: 2,
            ..RepairConfig::default()
        };
        let run = |rep: &GapReport| {
            let request: PatchAudioRequest = repair
                .patch_settings()
                .into_request(rep.clone())
                .expect("default All gap selection");
            characterize_gaps_from_decode(
                rep,
                &CharacterizeAbPcm {
                    a_pcm: &a_pcm,
                    b_samples: &b,
                    sources: None,
                },
                &request,
                &[],
                DumpOptions::default(),
                &NoOpProgressReporter,
            )
            .gaps
            .remove(0)
            .equivalence_diagnostic
            .expect("diagnostic equivalence")
        };

        let nominal = run(&report(Vec::new()));
        assert_eq!(
            nominal.measurement.as_ref().map(|m| m.a_span),
            Some(SpanKind::Nominal)
        );

        // A core inset half a second at each end of the 1.5 s gap — the shape of the real residual
        // (shoulders trimmed), exaggerated so the block count has to move at 100 ms bins.
        let core = (f(gap.0) + 0.5, f(gap.1) - 0.5);
        let mut verdict = GapEquivalenceVerdict {
            class: GapEquivalenceClass::NotEvaluated,
            ..Default::default()
        };
        verdict.a_span_secs = Some(core);
        verdict.donor_span_secs = Some(core);
        let converged = run(&report(vec![verdict]));

        let m = converged.measurement.as_ref().expect("measurement");
        assert_eq!(
            (m.a_span, m.donor_span),
            (SpanKind::Core, Some(SpanKind::Core)),
            "adopting scan's span must be reported, not silently assumed"
        );
        assert!(
            converged.a_gap_total_blocks < nominal.a_gap_total_blocks,
            "the core is inset, so fewer A blocks fall in it: core {:?} vs nominal {:?}",
            converged.a_gap_total_blocks,
            nominal.a_gap_total_blocks
        );
        assert!(
            converged.donor_total_blocks < nominal.donor_total_blocks,
            "the donor must follow A onto the core, not stay on `b_mapped`: core {:?} vs nominal {:?}",
            converged.donor_total_blocks,
            nominal.donor_total_blocks
        );
    }

    /// **C3 — `fingerprint_diagnostics` gates the X-set:** off, the diagnostic-only fields
    /// (`seam_probe`, `wide_envelope`, `b_levels`) are absent; on, they're populated. Closes
    /// perf-plan `docs/dev/archive/TEMP-pipeline-perf-redesign-plan.md` §4.7 backlog item **C3** — the flag
    /// exists (`RepairConfig.fingerprint_diagnostics`, `characterize_gaps_from_decode`'s
    /// `include_diagnostics`) but had no regression test pinning what it actually gates.
    #[test]
    fn characterize_gaps_from_decode_include_diagnostics_toggles_x_set() {
        use crate::application::PatchAudioRequest;
        use crate::domain::gap::Gap;
        use crate::domain::{GapReport, GapSignatureMode, ScanAlignment};
        use crate::infrastructure::config::RepairConfig;
        use clip_sync::MultiChannelPcm;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let rate = 48_000u32;
        let ch = 1usize;
        let secs = |s: f64| (s * f64::from(rate)) as usize;
        let total = secs(5.0);
        let (sp1, n1, gap, n2, sp2) = (
            (secs(0.50), secs(0.85)),
            (secs(0.85), secs(1.85)),
            (secs(1.85), secs(3.35)),
            (secs(3.35), secs(4.35)),
            (secs(4.35), secs(4.70)),
        );
        let mut a = vec![0f32; total];
        let mut b = vec![0f32; total];
        write_speech(&mut a, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut b, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut a, sp2.0, sp2.1, 440.0, 0.079);
        write_speech(&mut b, sp2.0, sp2.1, 440.0, 0.079);
        write_noise(&mut a, n1.0, n1.1, 1, 0.0056);
        write_noise(&mut b, n1.0, n1.1, 11, 0.0056);
        write_noise(&mut a, n2.0, n2.1, 3, 0.0056);
        write_noise(&mut b, n2.0, n2.1, 13, 0.0056);
        write_noise(&mut b, gap.0, gap.1, 5, 0.0056);

        let a_pcm = MultiChannelPcm {
            sample_rate: rate,
            channels: ch as u16,
            samples: a,
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };

        let report = GapReport {
            local_pair_collar_secs: None,
            video_a: Default::default(),
            video_b: Default::default(),
            track_compatibility: None,
            alignment: ScanAlignment {
                clips: vec![],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                query_reference_mode: false,
            },
            gaps: vec![Gap {
                video_a_start_secs: gap.0 as f64 / f64::from(rate),
                video_a_end_secs: gap.1 as f64 / f64::from(rate),
                video_b_start_secs: Some(gap.0 as f64 / f64::from(rate)),
                video_b_end_secs: Some(gap.1 as f64 / f64::from(rate)),
                b_has_energy: true,
                collar_level: None,
            }],
            gap_equivalence: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
            program_levels: None,
        };

        let repair = RepairConfig {
            gap_signature_mode: GapSignatureMode::Energy,
            gap_signature_context_secs: 1.5,
            // The 5 s synthetic fixture is far smaller than the production defaults assume; every
            // search radius (border slide, anchor bracket span) must stay inside the fixture's own
            // timeline or the unified fit search's O(radius * window) slide becomes a multi-minute
            // brute force over a handful of anchor-bracket combinations.
            fill_border_search_secs: 0.05,
            fill_align_margin_secs: 0.02,
            fill_extract_tail_slack_secs: 0.1,
            fill_seam_search_secs: 0.05,
            max_anchor_bracket_secs: 0.2,
            max_anchors_per_side: 2,
            ..RepairConfig::default()
        };
        let request: PatchAudioRequest = repair
            .patch_settings()
            .into_request(report.clone())
            .expect("default All gap selection");
        let progress = NoOpProgressReporter;

        // Threading check: the descriptors must land on the matching side of `source`, and the two sides
        // must stay distinguishable — a swap or a copy would pass every `file_source` unit test.
        //
        // **Do not give the two sides different `native_channels` to strengthen this.** Equal counts are
        // the refuse gate's precondition (`channel_layout_mismatch`); making them differ turns this into
        // a refuse test — `gaps` empties and every assertion below stops exercising the from-decode path.
        // Per-side channel mapping is asserted where it is observable, in
        // `characterize_gaps_refuses_channel_layout_mismatch`. Here the sides are distinguished by
        // codec / bit_depth / native_sample_rate / bitrate instead.
        let sources = crate::application::AbSources {
            a: crate::application::SourceDescriptor {
                codec: "flac".into(),
                bit_depth: Some(clip_sync::BitDepth::Int24),
                native_sample_rate: rate,
                native_channels: ch as u16,
                bitrate_bps: None,
            },
            b: crate::application::SourceDescriptor {
                codec: "aac".into(),
                bit_depth: None,
                native_sample_rate: 44_100,
                native_channels: ch as u16,
                bitrate_bps: Some(192_000),
            },
        };
        let with_sources = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: Some(&sources),
            },
            &request,
            &[],
            DumpOptions::default(),
            &progress,
        );
        assert_eq!(with_sources.source.a_source.codec.as_deref(), Some("flac"));
        assert_eq!(with_sources.source.b_source.codec.as_deref(), Some("aac"));
        assert_eq!(
            with_sources.source.a_source.bit_depth.as_deref(),
            Some("s24")
        );
        assert_eq!(
            with_sources.source.b_source.native_channels,
            Some(ch as u16)
        );
        // The gate's precondition, asserted so the reason the counts match is explicit rather than
        // incidental — see the comment above before changing either side.
        assert_eq!(
            with_sources.source.a_source.native_channels,
            with_sources.source.b_source.native_channels
        );
        assert_eq!(with_sources.source.a_source.was_resampled(), Some(false));
        assert_eq!(with_sources.source.b_source.was_resampled(), Some(true));
        // Bitrate is the one per-side field that is *asymmetric* on the positive path, so it is the
        // strongest available side-mapping check here: a swap flips both of these.
        assert_eq!(with_sources.source.a_source.source_audio_bitrate_bps, None);
        assert_eq!(
            with_sources.source.b_source.source_audio_bitrate_bps,
            Some(192_000)
        );
        // Counterpart to the refuse path's B-uses-B's-layout rule: when the layouts agree, **both**
        // sides are described at the decoded/analysis layout (A's), because that is what was measured.
        // The two rules are deliberate and differ; asserting only the refuse half made this look like a
        // quirk of that path. See `refused_channel_mismatch_corpus`'s doc comment.
        assert_eq!(with_sources.source.a_source.channels, ch as u16);
        assert_eq!(with_sources.source.b_source.channels, ch as u16);
        assert_eq!(
            with_sources.source.a_source.sample_rate, with_sources.source.b_source.sample_rate,
            "both sides are described at the measurement rate (A's), not their native rates"
        );
        assert_eq!(with_sources.source.incomparable, None);
        assert!(
            !with_sources.gaps.is_empty(),
            "matched channel layout must still characterize"
        );
        // Track B fine-path attach: pin the recipe on the dumped verdict, not only on the builder.
        //
        // `report.gap_equivalence` is empty here, so this is the **fallback** arm: no scan verdict means
        // no exported silent core, and the path measures the raw span — `nominal` on both sides. Both
        // tokens said `core`/`nominal` unconditionally until 2026-08-01; `core` was false and this test
        // pinned the falsehood. The core arm is covered by
        // `diagnostic_equivalence_adopts_the_scan_verdicts_core_span`.
        {
            let equiv = with_sources.gaps[0]
                .equivalence_diagnostic
                .as_ref()
                .expect("diagnostic equivalence");
            let m = equiv
                .measurement
                .as_ref()
                .expect("measurement attached at caller");
            // Scan's window, not `repair.gap_signature_context_secs` — I2 closed by removal
            // 2026-08-01. Asserted against the constant so this fails if the call site drifts back
            // onto the config field while the token keeps saying otherwise.
            assert!(
                (m.context_secs - crate::domain::gap_equivalence::EQUIVALENCE_CONTEXT_SECS).abs()
                    < f64::EPSILON,
                "the equivalence floor must be measured over scan's context window"
            );
            assert!(
                (repair.gap_signature_context_secs
                    - crate::domain::gap_equivalence::EQUIVALENCE_CONTEXT_SECS)
                    .abs()
                    > f64::EPSILON,
                "fixture must keep the two values distinct or the assertion above is vacuous"
            );
            assert_eq!(m.bin_ms, report.recipe.scan_block_ms());
            assert_eq!(m.reduction, ChannelReduction::Interleaved);
            assert_eq!(
                (m.a_span, m.donor_span),
                (SpanKind::Nominal, Some(SpanKind::Nominal)),
                "no scan verdict ⇒ no core to adopt ⇒ both ends stay on the raw span"
            );
        }

        let off = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &request,
            &[],
            DumpOptions::default(),
            &progress,
        );
        // The descriptor-less call must leave provenance absent, not defaulted.
        assert_eq!(off.source.a_source.codec, None);
        assert_eq!(off.source.b_source.was_resampled(), None);
        let on = characterize_gaps_from_decode(
            &report,
            &CharacterizeAbPcm {
                a_pcm: &a_pcm,
                b_samples: &b,
                sources: None,
            },
            &request,
            &[],
            DumpOptions {
                diagnostics: true,
                ..DumpOptions::default()
            },
            &progress,
        );

        let fp_off = off.gaps.first().expect("one gap (off)");
        assert!(
            fp_off.seam_probe.is_none(),
            "diagnostics off: seam_probe must be absent"
        );
        assert!(
            fp_off.wide_envelope.is_none(),
            "diagnostics off: wide_envelope must be absent"
        );
        assert!(
            fp_off.b_levels.is_none(),
            "diagnostics off: b_levels must be absent"
        );

        let fp_on = on.gaps.first().expect("one gap (on)");
        assert!(
            fp_on.seam_probe.is_some(),
            "diagnostics on: seam_probe must be populated"
        );
        assert!(
            fp_on.wide_envelope.is_some(),
            "diagnostics on: wide_envelope must be populated"
        );
        assert!(
            fp_on.b_levels.is_some(),
            "diagnostics on: b_levels must be populated"
        );
    }

    /// Stereo A + mono B: refuse pairwise characterize rather than indexing B at A's channel count.
    #[test]
    fn characterize_gaps_refuses_channel_layout_mismatch() {
        use crate::application::PatchAudioRequest;
        use crate::domain::gap::Gap;
        use crate::domain::{GapReport, GapSignatureMode, ScanAlignment};
        use crate::infrastructure::config::RepairConfig;
        use clip_sync::MultiChannelPcm;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let rate = 48_000u32;
        let a_ch = 2u16;
        let b_ch = 1u16;
        // 1.0 s of frames on each side — duration must use each side's own layout.
        let a_frames = rate as usize;
        let b_frames = rate as usize;
        let a_pcm = MultiChannelPcm {
            sample_rate: rate,
            channels: a_ch,
            samples: vec![0.01f32; a_frames * a_ch as usize],
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };
        let b = vec![0.01f32; b_frames * b_ch as usize];
        let report = GapReport {
            local_pair_collar_secs: None,
            video_a: Default::default(),
            video_b: Default::default(),
            track_compatibility: None,
            alignment: ScanAlignment {
                clips: vec![],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                query_reference_mode: false,
            },
            gaps: vec![Gap {
                video_a_start_secs: 0.25,
                video_a_end_secs: 0.50,
                video_b_start_secs: Some(0.25),
                video_b_end_secs: Some(0.50),
                b_has_energy: true,
                collar_level: None,
            }],
            gap_equivalence: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
            program_levels: None,
        };
        let request: PatchAudioRequest = RepairConfig {
            gap_signature_mode: GapSignatureMode::Energy,
            ..RepairConfig::default()
        }
        .patch_settings()
        .into_request(report.clone())
        .expect("default All gap selection");

        let sources = crate::application::AbSources {
            a: crate::application::SourceDescriptor {
                codec: "flac".into(),
                bit_depth: Some(clip_sync::BitDepth::Int16),
                native_sample_rate: rate,
                native_channels: a_ch,
                bitrate_bps: None,
            },
            b: crate::application::SourceDescriptor {
                codec: "aac".into(),
                bit_depth: None,
                native_sample_rate: rate,
                native_channels: b_ch,
                bitrate_bps: Some(128_000),
            },
        };

        let pcm = CharacterizeAbPcm {
            a_pcm: &a_pcm,
            b_samples: &b,
            sources: Some(&sources),
        };
        let corpus = characterize_gaps_from_decode(
            &report,
            &pcm,
            &request,
            &[],
            DumpOptions::default(),
            &NoOpProgressReporter,
        );
        assert_eq!(
            corpus.source.incomparable,
            Some(IncomparableReason::ChannelLayoutMismatch)
        );
        assert!(
            corpus.gaps.is_empty(),
            "mismatch must not emit pairwise gap fingerprints"
        );
        assert_eq!(corpus.source.gap_count, 1);
        assert_eq!(corpus.source.a_source.native_channels, Some(a_ch));
        assert_eq!(corpus.source.b_source.native_channels, Some(b_ch));
        // Honest B layout: 1.0 s mono, not the false 0.5 s that A's channel count would invent.
        assert_eq!(corpus.source.b_source.channels, b_ch);
        assert!(
            (corpus.source.b_source.duration_secs - 1.0).abs() < 1e-9,
            "B duration must use B channels, got {}",
            corpus.source.b_source.duration_secs
        );
        assert_eq!(corpus.source.a_source.channels, a_ch);
        assert!(
            (corpus.source.a_source.duration_secs - 1.0).abs() < 1e-9,
            "A duration {}",
            corpus.source.a_source.duration_secs
        );

        // Direct characterize path must refuse the same way (defense in depth).
        let summary = characterize_gaps(&report, &pcm, &FingerprintConfig::default(), &[]);
        assert_eq!(
            summary.source.incomparable,
            Some(IncomparableReason::ChannelLayoutMismatch)
        );
        assert!(summary.gaps.is_empty());
        assert!(
            corpus.source.alignment.is_some(),
            "scan ran; channel-layout refuse is later"
        );
        assert_eq!(
            corpus
                .source
                .alignment
                .as_ref()
                .map(|a| a.recommended_offset_secs),
            Some(report.alignment.recommended_offset_secs)
        );
        assert!(
            summary.source.alignment.is_some(),
            "summary refuse path stamps the same scan alignment"
        );
        let not = corpus
            .source
            .alignment
            .as_ref()
            .and_then(|a| a.contract.as_ref())
            .and_then(|c| c.not.as_deref())
            .expect("refuse stamps the alignment contract");
        assert!(
            not.starts_with("lag_decision"),
            "alignment contract must name lag_decision first, got {not:?}"
        );
    }

    fn mk_fp(index: usize, full: bool) -> GapFingerprint {
        GapFingerprint {
            index,
            tier: if full {
                DetailTier::Full
            } else {
                DetailTier::Summary
            },
            sample_rate: 48_000,
            channels: 2,
            geometry: GapGeometry {
                a_start_secs: 832.5,
                a_end_secs: 834.0,
                a_refined_start_secs: 832.3,
                a_refined_end_secs: 834.1,
                duration_secs: 1.8,
                b_mapped_start_secs: None,
                b_mapped_end_secs: None,
                fill_offset_secs: None,
            },
            levels: LevelProfile {
                bin_ms: Some(50),
                profile_db: vec![],
                floor_db: Some(-120.0),
                speech_peak_db: Some(-40.0),
                noise_floor_db: -53.0,
                gap_floor_db: -98.0,
            },
            silence: Some(SilenceProfile {
                collar_rms_peak_ratio: 0.1,
                collar_above_relative_floor: true,
                silence_peak_fraction: 0.01,
            }),
            contour: Some(ContourInfo {
                has_anchor_seam_contour: true,
                pre_flatness: 0.0,
                post_flatness: 0.0,
            }),
            anchors: Some(AnchorSet::default()),
            brackets: vec![],
            structure: None,
            seams: None,
            lag_editorial: stamp(
                full.then(|| LagFingerprint {
                    pre_anchor: vec![LagSummary {
                        window_ms: 250,
                        max_lag_ms: 200,
                        channel: LagChannel::Mono,
                        lag0_r: Some(0.02),
                        peak_r: 0.99,
                        second_peak_r: Some(0.20),
                        peak_z: Some(12.5),
                        prominence: Some(0.79),
                        top2_spacing_ms: Some(40.0),
                        peak_lag_samples: -778,
                        frac_lag_samples: -778.0,
                        frac_lag_ms: -16.2,
                        edge_pinned: Some(false),
                        border_displacement_ms: None,
                        envelope_window_ms: None,
                        envelope_lag_ms: None,
                        envelope_peak_r: None,
                        envelope_edge_pinned: None,
                        envelope_rescue: None,
                        verdict: LagVerdict::TimingOffset,
                    }],
                    post_anchor: vec![],
                }),
                LAG_EDITORIAL_CONTRACT,
            ),
            lag_decision: None,
            residual: None,
            seam_probe: None,
            donor_interior_aligned: None,
            splice: None,
            wide_envelope: None,
            splice_dualfit: None,
            donor_interior_nominal: None,
            b_levels: None,
            outcome: None,
            equivalence_diagnostic: None,
            equivalence_production: None,
        }
    }

    #[test]
    fn write_corpus_library_emits_named_files_and_manifest() {
        let corpus = GapCorpus {
            source: SourceMeta {
                a_source: FileSource {
                    id: "aaaaaaaaaaaaaaaa".into(),
                    container: None,
                    codec: None,
                    bit_depth: None,
                    native_sample_rate: None,
                    native_channels: None,
                    source_audio_bitrate_bps: None,
                    sample_rate: 48_000,
                    channels: 2,
                    duration_secs: 1000.0,
                },
                b_source: FileSource {
                    id: "bbbbbbbbbbbbbbbb".into(),
                    container: None,
                    codec: None,
                    bit_depth: None,
                    native_sample_rate: None,
                    native_channels: None,
                    source_audio_bitrate_bps: None,
                    sample_rate: 48_000,
                    channels: 2,
                    duration_secs: 1000.0,
                },
                scan_recipe: CorpusScanRecipe::default(),
                gap_count: 2,
                incomparable: None,
                gate_recipe: None,
                probe_recipe: None,
                not_measured: Vec::new(),
                alignment: None,
            },
            gaps: vec![mk_fp(0, false), mk_fp(3, true)],
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let n = write_corpus_dir(&corpus, dir.path()).expect("write library");
        assert_eq!(n, 2);
        assert!(dir.path().join("manifest.json").exists());
        assert!(dir.path().join("corpus.json").exists());
        assert!(
            corpus.source.alignment.is_none(),
            "hand-built corpus never saw a GapReport"
        );
        let src_json = serde_json::to_value(&corpus.source).expect("source json");
        assert!(
            src_json.get("alignment").is_none(),
            "unanswerable alignment must omit the key, not emit zeros"
        );

        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        // Named by detail tier only. The verdict token was removed 2026-08-12: it read as
        // production's disposition (it is the bracket gate's) and was not flag-stable.
        assert!(
            names.iter().any(|f| f.ends_with("g003_full.json")),
            "full gap named by detail tier, no verdict token: {names:?}"
        );
        assert!(
            names.iter().any(|f| f.ends_with("g000_summary.json")),
            "summary gap named by detail tier, no verdict token: {names:?}"
        );
        // No leaking identifiers; per-gap files are opaque-id prefixed.
        assert!(names.iter().all(|f| f == "manifest.json"
            || f == "corpus.json"
            || f.starts_with("aaaaaaaa_bbbb_")));

        // Each per-gap file is a self-contained single-gap corpus.
        let full = names.iter().find(|f| f.contains("g003")).unwrap();
        let parsed: GapCorpus =
            serde_json::from_reader(std::fs::File::open(dir.path().join(full)).unwrap()).unwrap();
        assert_eq!(parsed.gaps.len(), 1);
        assert_eq!(parsed.gaps[0].index, 3);
        assert_eq!(parsed.source.a_source.id, "aaaaaaaaaaaaaaaa");
    }
}
