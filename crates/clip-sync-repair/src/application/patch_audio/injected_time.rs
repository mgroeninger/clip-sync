//! Injected-time pre-pass: Full-tier mono `lag_decision` at `b_mapped` for every planned gap,
//! then the sequence pass. Lives next to `PatchAudio::run_with_decoded` so production never
//! imports `gap_fingerprint::measure`.

use clip_sync::MultiChannelPcm;

use super::PatchAudioRequest;
use crate::domain::fill_offset::{resolve_gap_offset_secs, AnchoredRetryPass};
use crate::domain::gap_fill::GapFillPlan;
use crate::domain::injected_time::{
    find_injected_time_runs, stamp_injected_time_report_times, InjectedTimeReport,
    INJECTED_TIME_DELTA_MS, INJECTED_TIME_MIN_RUN,
};
use crate::domain::lag_decision::{
    b_mapped_frame_in_haystack, shoulder_lags_at_b_mapped, ShoulderLagKnobs,
};
use crate::domain::pcm::interleaved_to_mono;
use crate::domain::policies::{self, GAP_EDGE_REFINE_SECS};

pub(super) fn detect_injected_time(
    plan: &GapFillPlan,
    request: &PatchAudioRequest,
    a_pcm: &MultiChannelPcm,
    b_samples_full: &[f32],
    channels: usize,
    sample_rate: u32,
) -> Option<InjectedTimeReport> {
    if plan.regions.is_empty() {
        return None;
    }
    let knobs = ShoulderLagKnobs::production(
        sample_rate,
        request.report.recipe.silence_peak_fraction(),
        request.absolute_silence_rms,
        request.lag_envelope_max_lag_ms,
        request.lag_envelope_window_secs,
    );
    let b_mono = interleaved_to_mono(b_samples_full, channels);
    let rate = f64::from(sample_rate);
    let max_refine_frames = (GAP_EDGE_REFINE_SECS * rate).round() as usize;
    let mut measured = Vec::new();
    let mut times = Vec::new();
    for region in &plan.regions {
        times.push((region.gap_index, region.a_start_secs, region.a_end_secs));
        let gap_time_on_a = (region.a_start_secs + region.a_end_secs) / 2.0;
        let gap_offset_secs = resolve_gap_offset_secs(
            &request.report.alignment,
            request.fill_offset_mode,
            gap_time_on_a,
            None,
            AnchoredRetryPass::First,
        )
        .unwrap_or(region.b_start_secs - region.a_start_secs);
        let reported_start = (region.a_start_secs * rate) as usize;
        let reported_end = (region.a_end_secs * rate) as usize;
        let refined = policies::refine_gap_frames(
            &a_pcm.samples,
            channels,
            reported_start,
            reported_end,
            request.report.recipe.silence_peak_fraction(),
            request.absolute_silence_rms,
            max_refine_frames,
        );
        let start_frame =
            b_mapped_frame_in_haystack(refined.start_frame, rate, gap_offset_secs, 0.0);
        let (pre, post) = shoulder_lags_at_b_mapped(
            &a_pcm.samples,
            channels,
            &refined,
            &b_mono,
            start_frame,
            &knobs,
        );
        let (Some(pre), Some(post)) = (pre, post) else {
            continue;
        };
        measured.push((region.gap_index, pre.frac_lag_ms, post.frac_lag_ms));
    }
    measured.sort_by_key(|(index, _, _)| *index);
    let runs = find_injected_time_runs(&measured, INJECTED_TIME_DELTA_MS, INJECTED_TIME_MIN_RUN);
    stamp_injected_time_report_times(runs, &times)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use clip_sync::testing::fakes::FakeMediaReader;
    use clip_sync::{MultiChannelPcm, ProgressReporter};

    use crate::application::align_bridge::scan_alignment_from_result;
    use crate::application::test_support::zero_offset_alignment;
    use crate::domain::diagnostics::format_injected_time_warning;
    use crate::domain::gap::Gap;
    use crate::domain::gap_fill::build_gap_fill_plan;
    use crate::domain::{CompatibilityVerdict, GapReport, ScanRecipe, TrackCompatibility};
    use crate::infrastructure::config::RepairConfig;

    use super::super::decode::{AbSources, DecodedAb, SourceDescriptor};
    use super::super::{PatchAudio, PatchAudioRequest};

    struct RecordingProgress {
        events: RefCell<Vec<(String, String)>>,
    }

    impl ProgressReporter for RecordingProgress {
        fn phase(&self, message: &str) {
            self.events
                .borrow_mut()
                .push(("phase".into(), message.to_string()));
        }

        fn progress(&self, label: &str, _current: u64, _total: u64) {
            self.events
                .borrow_mut()
                .push(("progress".into(), label.to_string()));
        }
    }

    fn noise(seed: u64, i: usize) -> f64 {
        let mut z = ((seed << 32) | (i as u64 & 0xffff_ffff)).wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 40) as f64 / (1u64 << 24) as f64) * 2.0 - 1.0
    }

    fn unique_program(n: usize, seed: u64) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let env = 0.25
                    + 0.75
                        * ((i as f64 / n.max(1) as f64) * std::f64::consts::TAU * 5.0)
                            .sin()
                            .abs();
                (noise(seed, i) * env) as f32 * 0.6
            })
            .collect()
    }

    fn case_1_pcm(sample_rate: u32) -> (Vec<f32>, Vec<f32>, Vec<(f64, f64)>) {
        let total = (12.0 * f64::from(sample_rate)) as usize;
        let extra = (3.0 * f64::from(sample_rate)) as usize;
        let insert_at = (4.0 * f64::from(sample_rate)) as usize;
        let realign_at = (9.0 * f64::from(sample_rate)) as usize;
        let program = unique_program(total, 11);
        let mut b = Vec::with_capacity(total);
        b.extend_from_slice(&program[..insert_at]);
        b.extend(unique_program(extra, 99));
        b.extend_from_slice(&program[insert_at..realign_at - extra]);
        b.extend_from_slice(&program[realign_at..]);
        let mut a = program;
        let gaps = vec![
            (1.5, 1.65),
            (3.0, 3.15),
            (5.5, 5.65),
            (7.0, 7.15),
            (8.5, 8.65),
            (10.5, 10.65),
        ];
        for &(s, e) in &gaps {
            let lo = (s * f64::from(sample_rate)) as usize;
            let hi = (e * f64::from(sample_rate)) as usize;
            for slot in a.iter_mut().take(hi).skip(lo) {
                *slot = 0.0;
            }
        }
        (a, b, gaps)
    }

    fn pcm(samples: Vec<f32>, sample_rate: u32) -> MultiChannelPcm {
        MultiChannelPcm {
            sample_rate,
            channels: 1,
            samples,
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        }
    }

    fn case_1_request(gaps: &[(f64, f64)], duration: f64) -> PatchAudioRequest {
        let report_gaps: Vec<Gap> = gaps
            .iter()
            .map(|&(start, end)| Gap {
                video_a_start_secs: start,
                video_a_end_secs: end,
                video_b_start_secs: Some(start),
                video_b_end_secs: Some(end),
                b_has_energy: true,
                collar_level: None,
            })
            .collect();
        let report = GapReport {
            video_a: PathBuf::from("a.wav"),
            video_b: PathBuf::from("b.wav"),
            track_compatibility: Some(TrackCompatibility {
                a_channels: 1,
                b_channels: 1,
                a_sample_rate: 48_000,
                b_sample_rate: 48_000,
                channels_match: true,
                rate_match: true,
                verdict: CompatibilityVerdict::Compatible,
            }),
            alignment: scan_alignment_from_result(&zero_offset_alignment(duration)),
            gaps: report_gaps,
            recipe: ScanRecipe::with_hold_blocks(100, 0, 50, 0.01, 0.0),
            ..GapReport::default()
        };

        let repair = RepairConfig {
            dual_fit: false,
            anchor_seam_mode: crate::domain::AnchorSeamMode::Off,
            fill_border_search_secs: 0.4,
            fill_align_margin_secs: 0.15,
            gap_signature_context_secs: 0.5,
            gap_end_extend_on_post_seam_fail: false,
            gap_start_extend_on_pre_seam_fail: false,
            fill_mode: crate::domain::FillMode::Gate,
            ..RepairConfig::default()
        };
        repair
            .patch_settings()
            .into_request(report)
            .expect("request")
    }

    fn decoded(a: Vec<f32>, b: Vec<f32>, sample_rate: u32) -> DecodedAb {
        let duration = a.len() as f64 / f64::from(sample_rate);
        let descriptor = SourceDescriptor {
            codec: "pcm".into(),
            bit_depth: None,
            native_sample_rate: sample_rate,
            native_channels: 1,
            bitrate_bps: None,
        };
        DecodedAb {
            a_pcm: pcm(a, sample_rate),
            b_samples_full: b,
            b_channels: 1,
            source_audio_bitrate_a_bps: None,
            source_audio_bitrate_b_bps: None,
            container_duration_a_secs: duration,
            sources: AbSources {
                a: descriptor.clone(),
                b: descriptor,
            },
        }
    }

    #[test]
    fn injected_time_phase_fires_before_characterize() {
        const SR: u32 = 48_000;
        let (a, b, gaps) = case_1_pcm(SR);
        let duration = a.len() as f64 / f64::from(SR);
        let request = case_1_request(&gaps, duration);
        let plan = build_gap_fill_plan(&request.report, 10, false, &request.gap_selection);
        assert!(
            plan.regions.len() >= 5,
            "need a planned island, got {:?}",
            plan.regions.len()
        );

        let progress = RecordingProgress {
            events: RefCell::new(Vec::new()),
        };
        let media = FakeMediaReader::default();
        let patcher = PatchAudio::new(&media, &progress);
        let result = patcher
            .preview_with_decoded(request, plan, decoded(a, b, SR))
            .expect("preview");

        let injected = result
            .summary
            .injected_time
            .as_ref()
            .expect("A4 should flag case 1");
        let warning = format_injected_time_warning(injected, false);
        let events = progress.events.borrow();
        let warn_at = events
            .iter()
            .position(|(kind, msg)| kind == "phase" && msg == &warning)
            .expect("mid-run phase should be the A4 line");
        let char_at = events
            .iter()
            .position(|(kind, msg)| kind == "progress" && msg == "patch-characterize")
            .expect("characterize should still run");
        assert!(
            warn_at < char_at,
            "A4 must precede patch-characterize: {events:?}"
        );
        assert!(warning.contains("injected time"), "{warning}");
    }
}
