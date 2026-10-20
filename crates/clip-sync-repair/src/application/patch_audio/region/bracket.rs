//! Bracket-level fill assembly and execution: build the fill sample vector for one gap
//! bracket, then splice it into A and turn the result into a patch + outcome pair.
//!
//! Depends on `types` (patch / outcome records) and `pcm` (B-side frame math).
//! Must not import `dual_fit`, `execute`, or `characterize` — see the plan's cycle guards.

use crate::domain::{
    fill_mode::FillMode,
    gap_fill_fit::{
        fit_fill_length_for_gap, fit_fill_to_gap_frames_padded, FillConfidence, FittedFill,
        UnifiedFitBreakdown,
    },
    policies::{self, GapBorderSpec, RefinedGapFrames},
};

use super::types::{RegionPatch, RegionPatchOutcome};

/// Inputs to [`assemble_bracket_fill`] — the sliced B fill (`b_fill_raw` + contiguous `b_extension`), the
/// A-border templates for Fit-mode length fitting, and the seam context.
pub(in crate::application::patch_audio::region) struct BracketFillAssembly<'a> {
    pub(in crate::application::patch_audio::region) b_fill_raw: Vec<f32>,
    pub(in crate::application::patch_audio::region) b_extension: &'a [f32],
    pub(in crate::application::patch_audio::region) channels: usize,
    pub(in crate::application::patch_audio::region) gap_frames: usize,
    pub(in crate::application::patch_audio::region) fill_mode: FillMode,
    pub(in crate::application::patch_audio::region) a_pre_border: &'a [f64],
    pub(in crate::application::patch_audio::region) a_post_border: &'a [f64],
    pub(in crate::application::patch_audio::region) a_pre_ch: &'a [Vec<f64>],
    pub(in crate::application::patch_audio::region) a_post_ch: &'a [Vec<f64>],
    pub(in crate::application::patch_audio::region) pre_gate_frames: usize,
    pub(in crate::application::patch_audio::region) post_gate_frames: usize,
    pub(in crate::application::patch_audio::region) seam_ctx: policies::SpliceSeamContext<'a>,
}

/// The assembled fill plus the one reportable fact about how it was built. Returning the fact instead of
/// logging it (L0) is what makes [`assemble_bracket_fill`] safe to call twice: a pure function called from
/// both passes yields the same value twice, and only the reporting call site speaks. Reporting from inside
/// the primitive would double-emit once the executor re-derives — and would describe characterize's fill,
/// which is discarded, rather than the executor's, which is written.
pub(in crate::application::patch_audio::region) struct BracketFill {
    pub(in crate::application::patch_audio::region) pcm: Vec<f32>,
    /// Gate mode only: frames appended from contiguous B because the bracket was shorter than the A gap.
    /// `None` in Fit mode (length fitting handles it) and when no extension was needed or available.
    pub(in crate::application::patch_audio::region) extended_frames: Option<usize>,
    /// Fit mode only: frames cut from the head of the B slice when the trim-head anchor won
    /// ([`FittedFill::head_trim_frames`]). The written fill starts this much later in B than
    /// `FillAlignment::start_frame`, so a placement reader must add it to the slide. `0` in Gate mode.
    pub(in crate::application::patch_audio::region) head_trim_frames: usize,
    /// Tail frames the fill fell short of `gap_frames` by, because contiguous B ran out. The primitive
    /// still pads them so every fill is exactly gap-length; `execute_bracket_output` then shortens the
    /// destination so they are not written. Non-zero therefore means that much of the gap is left
    /// unrepaired. Returned rather than logged for the same reason as the siblings above: this
    /// primitive runs twice.
    pub(in crate::application::patch_audio::region) zero_pad_frames: usize,
}

/// Assemble the bracket fill PCM to exactly `gap_frames` from the sliced B: **Fit** length-fits against the A
/// borders ([`fit_fill_length_for_gap`]), **Gate** extends from contiguous B then tail-trims
/// ([`fit_fill_to_gap_frames`]). Extracted verbatim from `prepare_region_patch` (6b.3a) so the current inline
/// path and the future `execute_region_spec` share ONE PCM primitive — byte-identical by construction. The
/// executor reconstructs these inputs deterministically from the spec: `b_fill_raw`/`b_extension` re-sliced
/// from the decode buffer via `FillAlignment`, and (Fit mode only) the A-border templates + `seam_ctx` rebuilt
/// from A geometry — never re-deciding.
///
/// **Side-effect-free (L0).** No logging, no spans: see [`BracketFill`].
pub(in crate::application::patch_audio::region) fn assemble_bracket_fill(
    inp: BracketFillAssembly<'_>,
) -> BracketFill {
    let BracketFillAssembly {
        b_fill_raw,
        b_extension,
        channels,
        gap_frames,
        fill_mode,
        a_pre_border,
        a_post_border,
        a_pre_ch,
        a_post_ch,
        pre_gate_frames,
        post_gate_frames,
        seam_ctx,
    } = inp;
    // `source_frames` is derived here (not a field) so a caller can't pass one inconsistent with `b_fill_raw`.
    let source_frames = b_fill_raw.len() / channels;
    if fill_mode == FillMode::Fit {
        let borders = policies::BorderSeamTemplates {
            a_pre: a_pre_border,
            a_post: a_post_border,
            a_pre_ch,
            a_post_ch,
            pre_window: pre_gate_frames,
            post_window: post_gate_frames,
        };
        let FittedFill {
            pcm,
            head_trim_frames,
            zero_pad_frames,
        } = fit_fill_length_for_gap(
            &b_fill_raw,
            b_extension,
            channels,
            gap_frames,
            &borders,
            seam_ctx,
        );
        BracketFill {
            pcm,
            extended_frames: None,
            head_trim_frames,
            zero_pad_frames,
        }
    } else {
        let mut gate_fill = b_fill_raw;
        let mut extended_frames = None;
        if source_frames < gap_frames {
            let need_samples = (gap_frames - source_frames) * channels;
            let extend_to = need_samples.min(b_extension.len());
            if extend_to > 0 {
                gate_fill.extend_from_slice(&b_extension[..extend_to]);
                extended_frames = Some(extend_to / channels);
            }
        }
        let (pcm, zero_pad_frames) =
            fit_fill_to_gap_frames_padded(&gate_fill, channels, gap_frames);
        BracketFill {
            pcm,
            extended_frames,
            head_trim_frames: 0,
            zero_pad_frames,
        }
    }
}

/// Everything the executor needs to reconstruct a bracket fill from the spec — the `FillAlignment` indices,
/// the decode buffers, and the A geometry/config to rebuild the border templates.
pub(in crate::application::patch_audio::region) struct ExecuteBracketFillCtx<'a> {
    pub(in crate::application::patch_audio::region) alignment: policies::FillAlignment,
    pub(in crate::application::patch_audio::region) b_samples: &'a [f32],
    pub(in crate::application::patch_audio::region) a_samples: &'a [f32],
    pub(in crate::application::patch_audio::region) a_frames: usize,
    pub(in crate::application::patch_audio::region) refined: RefinedGapFrames,
    pub(in crate::application::patch_audio::region) channels: usize,
    pub(in crate::application::patch_audio::region) gap_frames: usize,
    pub(in crate::application::patch_audio::region) fill_mode: FillMode,
    pub(in crate::application::patch_audio::region) border_frames: usize,
    pub(in crate::application::patch_audio::region) border_standoff_frames: usize,
    pub(in crate::application::patch_audio::region) silence_peak_fraction: f32,
    pub(in crate::application::patch_audio::region) absolute_silence_rms: f32,
    pub(in crate::application::patch_audio::region) seam_gate_frames: usize,
    pub(in crate::application::patch_audio::region) sample_rate: u32,
    pub(in crate::application::patch_audio::region) crossfade_secs: f64,
}

/// The executor's bracket-fill reconstruction (6b.3a). Re-derives everything the fill needs from the spec's
/// [`FillAlignment`](crate::domain::policies::FillAlignment) + the decode buffers + A geometry — **independently of characterize** — then assembles
/// the PCM via the shared [`assemble_bracket_fill`]. This is the "assemble twice" design: characterize
/// assembles a fill to score the report-vs-splice reconciliation and discards it; the executor re-assembles
/// from the spec for the splice (borders rebuilt from scratch — deduped later by the step-8 hoists).
///
/// **This is the live fill path** (F1). The `debug_assert_eq!` shadow in characterize still compares this
/// reconstruction against the inline assembly on every bracket gap in debug builds — that is what keeps
/// "assemble twice" honest about producing the same bytes twice.
pub(in crate::application::patch_audio::region) fn execute_bracket_fill(
    ctx: ExecuteBracketFillCtx<'_>,
) -> Vec<f32> {
    execute_bracket_fill_detail(ctx).pcm
}

/// As [`execute_bracket_fill`], but keeps the [`BracketFill`] siblings. `head_trim_frames` is what
/// the seam-margin slice needs: the written fill starts that much later in B than
/// `FillAlignment::start_frame`, so a margin cut at the unadjusted start would not be contiguous
/// with the audio actually spliced.
pub(in crate::application::patch_audio::region) fn execute_bracket_fill_detail(
    ctx: ExecuteBracketFillCtx<'_>,
) -> BracketFill {
    let ExecuteBracketFillCtx {
        alignment,
        b_samples,
        a_samples,
        a_frames,
        refined,
        channels,
        gap_frames,
        fill_mode,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_silence_rms,
        seam_gate_frames,
        sample_rate,
        crossfade_secs,
    } = ctx;
    // M0 (§3): the executor-side cost — border rebuild + assembly. In debug builds this nests under the
    // `debug_assert_eq!` shadow rather than the live path; measure on a release-profile run.
    let _s = tracing::info_span!("exec_fill_assembly").entered();
    let fill_start_sample = alignment.start_frame * channels;
    let b_fill_end_sample = fill_start_sample + alignment.fill_frames * channels;
    let b_fill_raw = b_samples[fill_start_sample..b_fill_end_sample].to_vec();
    let b_extension = if b_fill_end_sample < b_samples.len() {
        &b_samples[b_fill_end_sample..]
    } else {
        &[][..]
    };
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_rms_floor: absolute_silence_rms,
    };
    let (a_pre_border, a_post_border) =
        policies::border_templates_for_gap(a_samples, channels, &border_spec);
    let (a_pre_ch, a_post_ch) =
        policies::border_templates_per_channel_for_gap(a_samples, channels, &border_spec);
    let pre_gate_frames = seam_gate_frames.min(a_pre_border.len().max(1));
    let post_gate_frames = if a_post_border.is_empty() {
        0
    } else {
        seam_gate_frames.min(a_post_border.len()).max(1)
    };
    let seam_cf = policies::effective_seam_crossfade_frames(
        (crossfade_secs * sample_rate as f64) as usize,
        refined.start_frame,
        refined.end_frame,
        a_frames,
    );
    let seam_ctx = policies::SpliceSeamContext {
        seam_cf,
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        a_samples,
        channels,
        single_lag_alignment: true,
    };
    assemble_bracket_fill(BracketFillAssembly {
        b_fill_raw,
        b_extension,
        channels,
        gap_frames,
        fill_mode,
        a_pre_border: &a_pre_border,
        a_post_border: &a_post_border,
        a_pre_ch: &a_pre_ch,
        a_post_ch: &a_post_ch,
        pre_gate_frames,
        post_gate_frames,
        seam_ctx,
    })
}

/// Resolved values the executor assembles a bracket `(RegionPatch, RegionPatchOutcome)` from. Everything
/// here is either read from the spec (gain, seam correlations, confidence, flags) or reconstructed (`fill`
/// via [`execute_bracket_fill`], slides recomputed from the geometry fields).
pub(in crate::application::patch_audio::region) struct ExecuteBracketOutputCtx {
    pub(in crate::application::patch_audio::region) fill: Vec<f32>,
    /// See [`super::types::SeamMargin`] — `None` unless `--seam-extend-fill` and the slice exists.
    pub(in crate::application::patch_audio::region) seam_margin: Option<super::types::SeamMargin>,
    /// The product actually multiplied into the fill.
    pub(in crate::application::patch_audio::region) gain: f32,
    /// The hole-local half of [`Self::gain`], carried whole so the reason reaches the record.
    pub(in crate::application::patch_audio::region) local_gain: crate::domain::LocalGainDecision,
    pub(in crate::application::patch_audio::region) refined: RefinedGapFrames,
    pub(in crate::application::patch_audio::region) crossfade_secs: f64,
    pub(in crate::application::patch_audio::region) final_pre: f64,
    pub(in crate::application::patch_audio::region) final_post: f64,
    pub(in crate::application::patch_audio::region) final_confidence: FillConfidence,
    pub(in crate::application::patch_audio::region) structure_trusted: bool,
    pub(in crate::application::patch_audio::region) gap_start_adjust_frames: i64,
    pub(in crate::application::patch_audio::region) gap_end_adjust_frames: i64,
    pub(in crate::application::patch_audio::region) fit_used_boundary_grid: bool,
    pub(in crate::application::patch_audio::region) fit_boundary_grid_cells: Option<u32>,
    pub(in crate::application::patch_audio::region) residual: Option<policies::SeamResidualVerdict>,
    pub(in crate::application::patch_audio::region) anchor_seam_used: bool,
    pub(in crate::application::patch_audio::region) anchor_bracket_move_frames: usize,
    pub(in crate::application::patch_audio::region) anchor_trusted: bool,
    /// [`BracketFill::zero_pad_frames`] for the fill being written — the executor's copy, which is the
    /// one that reaches the output.
    pub(in crate::application::patch_audio::region) zero_pad_frames: usize,
    // Slide geometry — recomputed here from EXACT frame values (no float round-trip). `offset_nominal_start`
    // is the nominal gap start within the B extract window, stored on the spec as `b_extract.b_mapped_start_frame`
    // (deriving it back from `b_extract_start_secs` would be lossy by up to 1/sr — a byte-parity break).
    // `structure_start_frame` was carried here until 2026-08-12 to split the slide into structure and
    // waveform halves; the unified search assigns `start_frame` once, so that split was always 0.
    pub(in crate::application::patch_audio::region) alignment_start_frame: usize,
    pub(in crate::application::patch_audio::region) offset_nominal_start: usize,
    pub(in crate::application::patch_audio::region) sample_rate: u32,
    /// Read straight off the spec — the terms are decided at characterize time with the placement and
    /// are never recomputed here, for the same single-authority reason as the seam correlations.
    pub(in crate::application::patch_audio::region) fit_terms: Option<UnifiedFitBreakdown>,
    /// S1: read straight off the spec — decided at characterize time, never recomputed here.
    pub(in crate::application::patch_audio::region) registration_residual:
        Option<crate::domain::patch_result::RegistrationResidual>,
}

/// The executor's bracket `(RegionPatch, RegionPatchOutcome)` assembly. Recomputes the geometry slides
/// independently and builds the structs from the resolved/spec values — `dual_fit_used` is always `false`
/// for a bracket. **The authoritative bracket output path since 6b.3b** (was shadow-validated byte-identical
/// across the full suite in 6b.3a before the inline construction was removed).
pub(in crate::application::patch_audio::region) fn execute_bracket_output(
    ctx: ExecuteBracketOutputCtx,
) -> (RegionPatch, RegionPatchOutcome) {
    let align_adjustment_secs = (ctx.alignment_start_frame as f64
        - ctx.offset_nominal_start as f64)
        / ctx.sample_rate as f64;
    // Don't write the pad. A fill that B could not fill out to the gap used to be zero-padded to the
    // destination length and spliced in full, which overwrote A's own gap tail with digital silence.
    // That is not a no-op: the gap is "below the silence threshold", not literally zero, so the pad
    // cuts A's noise floor to black for the padded span, flush against the post seam. Shortening the
    // destination writes the same repair minus the black stripe and leaves A's tail byte-identical —
    // an honestly unrepaired sliver rather than a hole. `gained_fill` cuts `b_samples` to the
    // destination, so the padded tail never reaches the splice or the fill-level measurement, and
    // `zero_pad_frames` still reports the shortfall on the record.
    //
    // `covered_frames == 0` (nothing was filled at all) is a geometry defect upstream, not a short
    // fill: left alone so `splice_into_a`'s own bails report it rather than manufacturing an empty
    // destination here.
    let covered_frames = ctx
        .refined
        .end_frame
        .saturating_sub(ctx.refined.start_frame)
        .saturating_sub(ctx.zero_pad_frames);
    let short_write = ctx.zero_pad_frames > 0 && covered_frames > 0;
    let patch = RegionPatch {
        b_samples: ctx.fill,
        // The tail margin is cut `zero_pad_frames` past where the fill now ends, so it is no longer
        // contiguous with the audio written. A short write splices via the shipped seam path.
        seam_margin: if short_write { None } else { ctx.seam_margin },
        gain: ctx.gain,
        a_start_frame: ctx.refined.start_frame,
        a_end_frame: if short_write {
            ctx.refined.start_frame + covered_frames
        } else {
            ctx.refined.end_frame
        },
        crossfade_secs: ctx.crossfade_secs,
    };
    let outcome = RegionPatchOutcome::Patched {
        pre_correlation: ctx.final_pre,
        post_correlation: ctx.final_post,
        align_adjustment_secs,
        structure_trusted: ctx.structure_trusted,
        confidence: ctx.final_confidence,
        gap_start_adjust_frames: ctx.gap_start_adjust_frames,
        gap_end_adjust_frames: ctx.gap_end_adjust_frames,
        fit_used_boundary_grid: ctx.fit_used_boundary_grid,
        fit_boundary_grid_cells: ctx.fit_boundary_grid_cells,
        residual: ctx.residual,
        anchor_seam_used: ctx.anchor_seam_used,
        anchor_bracket_move_frames: ctx.anchor_bracket_move_frames,
        anchor_trusted: ctx.anchor_trusted,
        zero_pad_frames: ctx.zero_pad_frames,
        dual_fit_used: false,
        dual_fit_step_real: None,
        dual_fit_post_window_ms: None,
        fit_terms: ctx.fit_terms,
        normalize_gain: ctx.gain,
        local_gain: ctx.local_gain,
        registration_residual: ctx.registration_residual.map(Box::new),
    };
    (patch, outcome)
}

#[cfg(test)]
mod tests {
    // The padded variant is what this module calls in production; these tests exercise the thin
    // unpadded wrapper, so it is imported directly rather than through `super::*`.
    use crate::domain::gap_fill_fit::fit_fill_to_gap_frames;

    fn short_write_ctx(zero_pad_frames: usize) -> super::ExecuteBracketOutputCtx {
        use crate::domain::policies::RefinedGapFrames;
        use crate::domain::{FillConfidence, LocalGainDecision, LocalGainReason};

        super::ExecuteBracketOutputCtx {
            // Padded to the full ten-frame gap, as every fill path still is.
            fill: {
                let mut v = vec![0.5f32; 10 - zero_pad_frames];
                v.extend(std::iter::repeat_n(0.0f32, zero_pad_frames));
                v
            },
            seam_margin: Some(super::super::types::SeamMargin {
                head: vec![0.1f32],
                tail: vec![0.1f32],
            }),
            gain: 1.0,
            local_gain: LocalGainDecision::Refused {
                reason: LocalGainReason::FlagOff,
            },
            refined: RefinedGapFrames {
                start_frame: 100,
                end_frame: 110,
            },
            crossfade_secs: 0.0,
            final_pre: 0.9,
            final_post: 0.9,
            final_confidence: FillConfidence::High,
            structure_trusted: true,
            gap_start_adjust_frames: 0,
            gap_end_adjust_frames: 0,
            fit_used_boundary_grid: false,
            fit_boundary_grid_cells: None,
            residual: None,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            anchor_trusted: false,
            zero_pad_frames,
            alignment_start_frame: 0,
            offset_nominal_start: 0,
            sample_rate: 44_100,
            fit_terms: None,
            registration_residual: None,
        }
    }

    #[test]
    fn a_short_fill_shortens_the_destination_instead_of_writing_the_pad() {
        let (patch, _outcome) = super::execute_bracket_output(short_write_ctx(3));

        assert_eq!(patch.a_start_frame, 100);
        assert_eq!(
            patch.a_end_frame, 107,
            "the three padded frames are not written; A keeps its own gap tail"
        );
        assert!(
            patch.seam_margin.is_none(),
            "the tail margin was cut past where the fill now ends, so it is no longer contiguous"
        );
        assert_eq!(
            super::super::pcm::gained_fill(&patch, 1).len(),
            7,
            "what reaches the splice is exactly the donor audio, no silence"
        );
    }

    #[test]
    fn a_full_length_fill_is_written_across_the_whole_gap() {
        let (patch, _outcome) = super::execute_bracket_output(short_write_ctx(0));

        assert_eq!(patch.a_end_frame, 110);
        assert!(patch.seam_margin.is_some(), "unchanged on the normal path");
    }

    #[test]
    fn gate_assembly_reports_the_silence_it_wrote_at_the_gap_tail() {
        use super::{assemble_bracket_fill, BracketFillAssembly};
        use crate::domain::fill_mode::FillMode;
        use crate::domain::policies::SpliceSeamContext;

        let channels = 1usize;
        let gap_frames = 10usize;
        // Six frames of bracket and two of contiguous B: eight of the ten frames can be filled, and
        // the gate arm has nothing left to reach the last two with.
        let b_fill_raw = vec![0.5f32; 6];
        let b_extension = vec![0.25f32; 2];
        let a_samples = vec![0.1f32; 40];

        let fill = assemble_bracket_fill(BracketFillAssembly {
            b_fill_raw,
            b_extension: &b_extension,
            channels,
            gap_frames,
            fill_mode: FillMode::Gate,
            a_pre_border: &[],
            a_post_border: &[],
            a_pre_ch: &[],
            a_post_ch: &[],
            pre_gate_frames: 0,
            post_gate_frames: 0,
            seam_ctx: SpliceSeamContext {
                seam_cf: 0,
                gap_start_frame: 10,
                gap_end_frame: 20,
                a_samples: &a_samples,
                channels,
                single_lag_alignment: true,
            },
        });

        assert_eq!(fill.pcm.len(), gap_frames * channels, "splice stays rigid");
        assert_eq!(fill.extended_frames, Some(2), "both extension frames taken");
        assert_eq!(
            fill.zero_pad_frames, 2,
            "the two frames the gate arm could not reach are the reported hole"
        );
        assert!(
            fill.pcm[8..].iter().all(|s| *s == 0.0),
            "the hole is exact zeros, flush to the gap end"
        );
    }

    #[test]
    fn fit_fill_trims_tail_without_resampling() {
        let channels = 2usize;
        let mut samples = Vec::new();
        for frame in 0..10i32 {
            samples.push(frame as f32 * 100.0 / 32767.0);
            samples.push(frame as f32 * 100.0 / 32767.0);
        }
        let fitted = fit_fill_to_gap_frames(&samples, channels, 6);
        assert_eq!(fitted.len(), 12);
        assert!((fitted[0] - 0.0).abs() < 1e-6);
        assert!((fitted[1] - 0.0).abs() < 1e-6);
        assert!((fitted[10] - 500.0 / 32767.0).abs() < 1e-5);
        assert!((fitted[11] - 500.0 / 32767.0).abs() < 1e-5);
    }

    #[test]
    fn fit_fill_zero_pads_short_source() {
        let v1 = 1000.0_f32 / 32767.0;
        let v2 = 2000.0_f32 / 32767.0;
        let samples = vec![v1, v1, v2, v2];
        let fitted = fit_fill_to_gap_frames(&samples, 2, 4);
        assert_eq!(fitted, vec![v1, v1, v2, v2, 0.0, 0.0, 0.0, 0.0]);
    }
}
