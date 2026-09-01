//! Synthetic A/B → fingerprint-corpus driver for the Fingerprint-unification harness (no media).
//!
//! Runs the from-decode dump pipeline (`characterize_gaps_from_decode`) on an in-memory synthetic A/B pair, so a
//! harness/test can exercise the live dump + projection end-to-end with **no media**. Same-master speech bursts,
//! decorrelated collars, B carrying fill across the gap.

use clip_sync::MultiChannelPcm;
use clip_sync_repair::application::gap_fingerprint::{
    characterize_gaps_from_decode, CharacterizeAbPcm, DumpOptions, GapCorpus,
};
use clip_sync_repair::application::PatchAudioRequest;
use clip_sync_repair::domain::gap::Gap;
use clip_sync_repair::domain::{GapReport, GapSignatureMode, ScanAlignment};
use clip_sync_repair::infrastructure::config::RepairConfig;

use crate::NoOpProgressReporter;

/// splitmix64 finalizer → deterministic noise in [-1, 1).
fn noise(seed: u64, i: usize) -> f64 {
    let mut z = ((seed << 32) | (i as u64 & 0xffff_ffff)).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let u = (z ^ (z >> 31)) >> 11;
    (u as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
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

/// Characterize the synthetic A/B pair through the **from-decode** dump pipeline
/// (`characterize_gaps_from_decode`) — the live `--gap-fingerprints` path. `diagnostics` toggles the X-set
/// (`seam_probe`/`wide_envelope`/`b_levels`/`lag`).
pub fn synth_ab_from_decode_corpus(diagnostics: bool) -> GapCorpus {
    synth_ab_from_decode_corpus_with_dump(DumpOptions {
        diagnostics,
        ..DumpOptions::default()
    })
}

/// Same synthetic pair as [`synth_ab_from_decode_corpus`], with a caller-chosen [`DumpOptions`].
pub fn synth_ab_from_decode_corpus_with_dump(dump: DumpOptions) -> GapCorpus {
    characterize_synth_ab(
        dump,
        SynthAbOptions {
            b_delay_secs: 0.0,
            keep_path_shift_fixture: false,
        },
    )
}

/// Like [`synth_ab_from_decode_corpus_with_dump`], but B is delayed so its content lands later in
/// the file. The gap report still maps A onto the **unshifted** B times — the keep path searches
/// the wrong map until a positive shift registers it.
///
/// Uses keep-path-shift fixture knobs: `max_anchor_bracket_secs` covers the 1.5 s throat, and
/// `gap_signature_context_secs` is long enough that the 1 s `lag_decision` pre window fits in the
/// extract. Gate search stays ±20 ms so a 100 ms delay is a miss until the map shifts.
pub fn synth_ab_from_decode_corpus_with_dump_and_b_delay(
    dump: DumpOptions,
    b_delay_secs: f64,
) -> GapCorpus {
    characterize_synth_ab(
        dump,
        SynthAbOptions {
            b_delay_secs,
            keep_path_shift_fixture: true,
        },
    )
}

fn characterize_synth_ab(dump: DumpOptions, opts: SynthAbOptions) -> GapCorpus {
    let (a_pcm, b, report, request) = synth_ab_inputs(opts);
    characterize_gaps_from_decode(
        &report,
        &CharacterizeAbPcm {
            a_pcm: &a_pcm,
            b_samples: &b,
            // Synthetic PCM — no container was probed, so there is no source provenance to record.
            sources: None,
        },
        &request,
        &[],
        dump,
        &NoOpProgressReporter,
    )
}

struct SynthAbOptions {
    b_delay_secs: f64,
    /// Throat-feasible `max_anchor_bracket_secs` and a lead-in long enough for the 1 s pre lag.
    keep_path_shift_fixture: bool,
}

/// The synthetic A/B pair + report/request the two `synth_ab_*` helpers share: same-master speech bursts,
/// decorrelated collars, B carrying fill across the gap. `b_delay_secs` > 0 copies B later in the
/// file (`B_new[i] = B_old[i - delay]`) so a positive `frac_lag_ms` is the register that finds it.
fn synth_ab_inputs(opts: SynthAbOptions) -> (MultiChannelPcm, Vec<f32>, GapReport, PatchAudioRequest) {
    let rate = 48_000u32;
    let ch = 1usize;
    let secs = |s: f64| (s * f64::from(rate)) as usize;
    let total = secs(if opts.keep_path_shift_fixture {
        8.0
    } else {
        5.0
    });
    let (sp1, n1, gap, n2, sp2) = (
        (secs(0.50), secs(0.85)),
        (secs(0.85), secs(1.85)),
        (secs(1.85), secs(3.35)),
        (secs(3.35), secs(4.35)),
        (secs(4.35), secs(4.70)),
    );
    let mut a = vec![0f32; total];
    let mut b = vec![0f32; total];
    if opts.keep_path_shift_fixture {
        // Live unique shoulders so structure and `lag_decision` can lock; B carries a donor through
        // the gap. Quiet decorrelated collars walk the 1 s lag off to empty.
        write_speech(&mut a, sp1.0, n1.1, 330.0, 0.063);
        write_speech(&mut b, sp1.0, n1.1, 330.0, 0.063);
        write_speech(&mut a, n2.0, sp2.1, 440.0, 0.079);
        write_speech(&mut b, n2.0, sp2.1, 440.0, 0.079);
        write_speech(&mut b, gap.0, gap.1, 370.0, 0.063);
        a[gap.0..gap.1].fill(0.0);
    } else {
        write_speech(&mut a, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut b, sp1.0, sp1.1, 330.0, 0.063);
        write_speech(&mut a, sp2.0, sp2.1, 440.0, 0.079);
        write_speech(&mut b, sp2.0, sp2.1, 440.0, 0.079);
        write_noise(&mut a, n1.0, n1.1, 1, 0.0056);
        write_noise(&mut b, n1.0, n1.1, 11, 0.0056);
        write_noise(&mut a, n2.0, n2.1, 3, 0.0056);
        write_noise(&mut b, n2.0, n2.1, 13, 0.0056);
        write_noise(&mut b, gap.0, gap.1, 5, 0.0056);
    }
    if opts.b_delay_secs > 0.0 {
        let delay = secs(opts.b_delay_secs);
        let mut delayed = vec![0f32; total];
        if delay < total {
            delayed[delay..].copy_from_slice(&b[..total - delay]);
        }
        b = delayed;
    }

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
        recipe: clip_sync_repair::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
        limit_fill_to_mapped_region: false,
        b_scanned_end_secs: None,
        b_scan_truncated: false,
        audio_timeline_skew: None,
        program_levels: None,
    };
    let (gap_signature_context_secs, max_anchor_bracket_secs, gap_signature_mode) =
        if opts.keep_path_shift_fixture {
            (2.0, 2.0, GapSignatureMode::Bool)
        } else {
            (1.5, 0.2, GapSignatureMode::Energy)
        };
    let repair = RepairConfig {
        gap_signature_mode,
        gap_signature_context_secs,
        // The 5 s synthetic fixture is far smaller than production defaults assume; keep every search
        // radius inside the fixture timeline so the unified fit search stays cheap. The keep-path-shift
        // fixture uses ±20 ms so a 100 ms B delay is outside the unshifted overlay.
        fill_border_search_secs: if opts.keep_path_shift_fixture {
            0.02
        } else {
            0.05
        },
        fill_align_margin_secs: 0.02,
        fill_length_slack_secs: 0.1,
        fill_extract_tail_slack_secs: 0.1,
        fill_seam_search_secs: 0.05,
        border_standoff_secs: 0.0,
        max_anchor_bracket_secs,
        max_anchors_per_side: 2,
        ..RepairConfig::default()
    };
    let request: PatchAudioRequest = repair
        .patch_settings()
        .into_request(report.clone())
        .expect("default All gap selection");
    (a_pcm, b, report, request)
}
