use std::path::{Path, PathBuf};

use clip_sync::{
    select_best_track, select_track_for_reference, AlignConfig, AlignVideosRequest,
    AlignmentResult, AudioTrack, DomainError, InterleavedScanBucket, MediaError, MediaReader,
    MediaSession, MediaSource, ProgressReporter,
};

use crate::application::align_bridge::{
    audio_timeline_skew_from_clip_sync, scan_alignment_from_result,
};
use crate::application::error::RepairError;
use crate::application::ports::Aligner;
use crate::domain::cross_check::{
    b_has_energy_from_levels, b_range_fully_scanned, check_gap_offset_agreement_in_overlap,
    mutual_silence_intervals_from_gaps, SilenceInterval,
};
use crate::domain::diagnostics::TIME_EPS_SECS;
use crate::domain::gap::{Gap, GapReport};
use crate::domain::gap_fill::{format_b_scan_truncation_note, format_scan_fillable_followup};
use crate::domain::policies;
use crate::domain::track_match::{assess_track_compatibility, TrackDescriptor};

pub struct ScanGapsRequest {
    pub video_a: PathBuf,
    pub video_b: PathBuf,
    pub align: AlignConfig,
    /// Decode chunk size (seconds) for sequential PCM scan.
    pub decode_chunk_secs: u64,
    /// Scan knobs that determine which gaps are detected (`PartialEq` ⇔ same gap list).
    /// Build via [`crate::domain::ScanRecipe::with_hold_blocks`] so effective hold and scanner
    /// blocks stay one fact.
    pub recipe: crate::domain::ScanRecipe,
    /// When true, also scan B's native timeline for silence and compute `gap_offset_agreement`.
    pub scan_both: bool,
    /// Tolerance (seconds) for the silence-based vs alignment offset agreement check.
    pub gap_offset_tolerance_secs: f64,
    /// When query-reference alignment is used, only gaps inside the mapped clip coverage are fillable.
    pub limit_fill_to_mapped_region: bool,
    /// Classify the donor window at the registered lag ([`DonorRegistrationMode::Apply`]) rather than
    /// the nominal offset map ([`Observe`](DonorRegistrationMode::Observe)). Production default is
    /// `true` since 2026-08-04; see [`RepairConfig::apply_donor_registration`].
    ///
    /// [`DonorRegistrationMode::Apply`]: crate::domain::gap_equivalence::DonorRegistrationMode::Apply
    /// [`RepairConfig::apply_donor_registration`]: crate::infrastructure::config::RepairConfig::apply_donor_registration
    pub apply_donor_registration: bool,
}

/// Gap scan product: domain report plus the full aligner DTO for CLI/JSON output.
#[derive(Debug, Clone)]
pub struct ScanGapsOutcome {
    pub report: GapReport,
    pub alignment_detail: AlignmentResult,
}

impl std::ops::Deref for ScanGapsOutcome {
    type Target = GapReport;

    fn deref(&self) -> &Self::Target {
        &self.report
    }
}

/// One-line stderr summary after gap detection (thresholds + count).
pub(crate) fn format_scan_summary(request: &ScanGapsRequest, gap_count: usize) -> String {
    let recipe = request.recipe;
    let silence_pct = recipe.silence_peak_fraction() * 100.0;
    let scan_both = if request.scan_both { "on" } else { "off" };
    let mut line = format!(
        "Gap scan: {gap_count} silent run(s) ≥{}ms — block {}ms, silence {silence_pct:.1}% peak, hold {}ms, decode {}s chunks, scan-both {scan_both}",
        recipe.min_gap_ms(),
        recipe.scan_block_ms(),
        recipe.silence_hold_ms(),
        request.decode_chunk_secs,
    );
    if recipe.absolute_silence_rms() > 0.0 {
        let i16_units = recipe.absolute_silence_rms() * 32767.0;
        let db = 20.0 * f64::from(recipe.absolute_silence_rms()).log10();
        line.push_str(&format!(
            ", rms floor {:.0} (at {db:.0} dBFS)",
            i16_units.round()
        ));
    }
    line
}

/// Result of the sequential B silence/level scan (report-only safe on mid-file decode errors).
struct BSilenceScan {
    intervals: Vec<SilenceInterval>,
    levels: Vec<policies::BlockLevel>,
    scanned_end_secs: Option<f64>,
    truncated: bool,
}

pub struct ScanGaps<'r, MR: MediaReader> {
    media_reader: &'r MR,
    progress: &'r dyn ProgressReporter,
    aligner: &'r dyn Aligner,
}

impl<'r, MR: MediaReader> ScanGaps<'r, MR> {
    pub fn new(
        media_reader: &'r MR,
        progress: &'r dyn ProgressReporter,
        aligner: &'r dyn Aligner,
    ) -> Self {
        Self {
            media_reader,
            progress,
            aligner,
        }
    }

    pub fn execute(&self, request: ScanGapsRequest) -> Result<ScanGapsOutcome, RepairError> {
        let alignment = self.aligner.align(
            AlignVideosRequest {
                video_a: request.video_a.clone(),
                video_b: request.video_b.clone(),
                config: request.align.clone(),
            },
            self.progress,
        )?;

        self.scan_after_alignment(request, alignment)
    }

    /// Gap scan after alignment — used by fixture corpus builders (not the public scan API).
    #[doc(hidden)]
    pub fn scan_after_alignment(
        &self,
        request: ScanGapsRequest,
        alignment: AlignmentResult,
    ) -> Result<ScanGapsOutcome, RepairError> {
        let alignment_detail = alignment;
        let scan_alignment = scan_alignment_from_result(&alignment_detail);
        let source_a = MediaSource::new(request.video_a.clone());
        let mut session_a = self
            .media_reader
            .open(&source_a)
            .map_err(RepairError::Media)?;
        let tracks_a = session_a.list_tracks().map_err(RepairError::Media)?;
        let track_a = select_best_track(&tracks_a)?.clone();

        if track_a.duration.is_none() {
            return Err(RepairError::Domain(DomainError::InvalidDuration));
        }

        // Step 3: best-effort open of video B for track compatibility + energy probing.
        // A missing or undecodable B never fails the scan — A's gaps are still reported, just
        // marked unfillable with no compatibility. Energy probing additionally requires an offset.
        let offset_secs = alignment_detail.recommended_offset_secs;
        let mut b_session = self.open_best_track(&request.video_b, &track_a);
        let track_compatibility = b_session.as_ref().map(|(_, track_b)| {
            assess_track_compatibility(
                TrackDescriptor {
                    channels: track_a.channels,
                    sample_rate: track_a.sample_rate,
                },
                TrackDescriptor {
                    channels: track_b.channels,
                    sample_rate: track_b.sample_rate,
                },
            )
        });

        // Step 4: sequential decode + block-level silence-run detection on A.
        let decode_chunk_secs = request.decode_chunk_secs as f64;
        let silence_peak_fraction = request.recipe.silence_peak_fraction();
        let min_gap_secs = request.recipe.min_gap_secs();
        let progress = self.progress;

        let absolute_silence_rms = request.recipe.absolute_silence_rms();
        let silence_hold_blocks = request.recipe.silence_hold_blocks();

        let mut scanner_a = policies::SilenceRunScanner::new(
            request.recipe.scan_block_secs(),
            silence_peak_fraction,
            min_gap_secs,
            silence_hold_blocks,
            absolute_silence_rms,
        )
        .retain_block_levels();
        let mut last_fed_end_secs: Option<f64> = None;

        let mut scan_a = |bucket: InterleavedScanBucket| -> Result<(), MediaError> {
            if last_fed_end_secs
                .is_some_and(|prev_end| bucket.start_secs > prev_end + TIME_EPS_SECS)
            {
                scanner_a.note_pcm_discontinuity();
            }
            scanner_a.feed(&bucket.pcm, bucket.start_secs);
            last_fed_end_secs = Some(bucket.end_secs);
            Ok(())
        };

        progress.phase("Scanning video A for gaps...");
        let mut audio_timeline_skew = None;
        session_a
            .scan_interleaved_buckets(
                &track_a,
                decode_chunk_secs,
                progress,
                "scan-a",
                &mut scan_a,
                &mut audio_timeline_skew,
            )
            .map_err(RepairError::Media)?;
        if let Some(skew) = audio_timeline_skew {
            if skew.delta_secs > crate::domain::diagnostics::TIMELINE_SKEW_WARN_SECS {
                tracing::warn!(
                    pts_secs = skew.pts_secs,
                    sample_clock_secs = skew.sample_clock_secs,
                    delta_secs = skew.delta_secs,
                    "audio timeline mismatch during gap scan on video A"
                );
            }
        }

        // Step 5: scan B's native timeline sequentially to build its silence map.
        // Used for both per-gap energy lookup (replaces per-gap seeks) and the cross-check.
        // Only meaningful when we have a B session and an alignment offset.
        let b_scan = match (&mut b_session, offset_secs) {
            (Some((session_b, track_b)), Some(_)) => self.scan_silence_intervals(
                session_b,
                track_b,
                decode_chunk_secs,
                policies::SilenceRunScanner::new(
                    request.recipe.scan_block_secs(),
                    request.recipe.silence_peak_fraction(),
                    request.recipe.min_gap_secs(),
                    silence_hold_blocks,
                    absolute_silence_rms,
                )
                .retain_block_levels(),
            ),
            _ => BSilenceScan {
                intervals: vec![],
                levels: vec![],
                scanned_end_secs: None,
                truncated: false,
            },
        };

        let (a_runs, a_levels) = scanner_a.finish_with_levels();
        // Gap-equivalence (advisory; `docs/dev/gap-vocabulary.md` § Silence-character pre-gate) is built
        // index-parallel to `gaps`. Classification uses each run's block-confirmed silent core — not
        // the refined `[start, end]` — so sub-block edge refinement (which can widen a gap into
        // fade-shoulder blocks) never inflates the A-side dropout-depth measurement and flips a real
        // dropout to `ambient-quiet`. B's donor-silence window is the same core offset-mapped so it
        // matches A. Always computed and reported; the `skip_equivalent_gaps` drop happens later in
        // `build_gap_fill_plan`.
        // `donor_registration` is **Apply** by default since 2026-08-04: the donor envelope is
        // registered against A's and the classification runs at the registered lag, abstaining below
        // `min_envelope_r`. `--no-apply-donor-registration` restores `Observe`, which records the
        // registration but classifies at the nominal map, changing no class, fraction or span.
        // Abstention under Apply is NotEvaluated (keep) — never a fallback to the nominal window.
        // The §6.10.3 head/tail exclusion recommended alongside Apply is not implemented (§7.4a).
        //
        // The rate question that kept this on `Observe` is answered:
        // `docs/dev/TEMP-equivalence-band/07-corpus-rates.md` §6.10 ran 39 pairs / 829 gaps / 782
        // registrations. Nonzero lag is 67.8 % but systematic per pair (23/39 have a modal lag ≠ 0;
        // residual scatter about own mode 13.0 %), abstention is 4.3 %, and `Apply` moves 16 gaps
        // (2.05 %) while touching none of the 236 dropouts at the digital-zero rail. Three patches
        // stop being applied; all three were listened to (§6.10.11) and all three were audible
        // degradations of undamaged periodic material — drum beats, clock ticks and speech pauses
        // where one 100 ms bin of map error puts B's loud bin over A's silent bin.
        //
        // What this does NOT fix: gaps that register correctly and still misclassify, because the
        // donor test asks "is B non-silent?" rather than "is B non-silent *where A is silent*?"
        // (§6.10.12). Registration cannot reach those; the fill-level check does.
        let equivalence_params = crate::domain::gap_equivalence::GapEquivalenceParams {
            enabled: true,
            donor_registration: Some(crate::domain::gap_equivalence::DonorRegistrationParams {
                mode: if request.apply_donor_registration {
                    crate::domain::gap_equivalence::DonorRegistrationMode::Apply
                } else {
                    crate::domain::gap_equivalence::DonorRegistrationMode::Observe
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        let b_levels_for_eq = (!b_scan.levels.is_empty()).then_some(b_scan.levels.as_slice());
        let mut gaps = Vec::with_capacity(a_runs.len());
        let mut gap_equivalence = Vec::with_capacity(a_runs.len());
        for run in a_runs {
            let pos = run.start_secs;
            let end = run.end_secs;
            let b_positions = offset_secs.map(|delta| (pos + delta, end + delta));

            // Absolute occupancy from B's per-block levels over the mapped *core* (same window
            // equivalence uses) — not hold-bridged silence-run coverage, which can span real audio.
            // Fail-closed when the mapped core is not fully inside the reviewed B scan prefix.
            let b_has_energy = match (offset_secs, b_levels_for_eq) {
                (Some(delta), Some(levels)) => {
                    let b_start = run.core_start_secs + delta;
                    let b_end = run.core_end_secs + delta;
                    b_range_fully_scanned(b_start, b_end, b_scan.scanned_end_secs)
                        && b_has_energy_from_levels(levels, b_start, b_end)
                }
                _ => false,
            };

            gaps.push(Gap {
                video_a_start_secs: pos,
                video_a_end_secs: end,
                video_b_start_secs: b_positions.map(|(s, _)| s),
                video_b_end_secs: b_positions.map(|(_, e)| e),
                b_has_energy,
            });

            let b_mapped = offset_secs
                .map(|delta| (run.core_start_secs + delta, run.core_end_secs + delta))
                .filter(|&(b_start, b_end)| {
                    b_levels_for_eq.is_some()
                        && b_range_fully_scanned(b_start, b_end, b_scan.scanned_end_secs)
                });
            let verdict = crate::domain::gap_equivalence::derive_gap_equivalence(
                &a_levels,
                run.core_start_secs,
                run.core_end_secs,
                b_levels_for_eq,
                b_mapped,
                &equivalence_params,
            );
            // F7: after F1 the two B-silent signals must agree when both are present.
            // Does not reorder NotFillable vs equivalence — only surfaces inconsistency.
            let occupancy_agrees =
                crate::domain::gap_equivalence::occupancy_agrees_with_donor_silence(
                    b_has_energy,
                    verdict.donor_silence_fraction,
                    equivalence_params.donor_silence_thresh,
                );
            debug_assert!(
                occupancy_agrees,
                "occupancy/donor disagreement at A {pos:.3}–{end:.3}: b_has_energy={b_has_energy} donor_silence={:?}",
                verdict.donor_silence_fraction
            );
            if !occupancy_agrees {
                tracing::warn!(
                    a_start = pos,
                    a_end = end,
                    b_has_energy,
                    donor_silence_fraction = ?verdict.donor_silence_fraction,
                    "absolute occupancy says B silent but donor_silence_fraction is below thresh"
                );
            }
            gap_equivalence.push(verdict);
        }

        // Step 6: mutual-silence cross-check — only meaningful when alignment produced an offset.
        // SharedSilence (donor metric), not !b_has_energy — keep fillability out of offset agreement.
        let a_intervals = mutual_silence_intervals_from_gaps(&gaps, &gap_equivalence);
        let gap_offset_agreement = if request.scan_both {
            alignment_detail.recommended_offset_secs.and_then(|offset| {
                check_gap_offset_agreement_in_overlap(
                    &a_intervals,
                    &b_scan.intervals,
                    alignment_detail
                        .start_overlap
                        .as_ref()
                        .map(|ov| crate::domain::align::TimelineOverlap {
                            video_a_start_secs: ov.video_a_start_secs,
                            video_a_end_secs: ov.video_a_end_secs,
                            video_b_start_secs: ov.video_b_start_secs,
                            video_b_end_secs: ov.video_b_end_secs,
                            shared_length_secs: ov.shared_length_secs,
                        })
                        .as_ref(),
                    offset,
                    request.gap_offset_tolerance_secs,
                )
            })
        } else {
            None
        };

        let gap_count = gaps.len();
        progress.phase(&format_scan_summary(&request, gap_count));
        if let Some(line) = format_b_scan_truncation_note(b_scan.truncated, b_scan.scanned_end_secs)
        {
            progress.phase(&line);
        }

        let report = GapReport {
            video_a: request.video_a,
            video_b: request.video_b,
            track_compatibility,
            alignment: scan_alignment,
            gaps,
            gap_equivalence,
            gap_offset_agreement,
            decode_chunk_secs: request.decode_chunk_secs,
            recipe: request.recipe,
            limit_fill_to_mapped_region: request.limit_fill_to_mapped_region,
            b_scanned_end_secs: b_scan.scanned_end_secs,
            b_scan_truncated: b_scan.truncated,
            audio_timeline_skew: audio_timeline_skew.map(audio_timeline_skew_from_clip_sync),
        };
        if let Some(line) = format_scan_fillable_followup(&report) {
            progress.phase(&line);
        }

        Ok(ScanGapsOutcome {
            report,
            alignment_detail,
        })
    }

    /// Sequential sample-bucket silence scan on a session's native timeline.
    ///
    /// The caller supplies a configured [`policies::SilenceRunScanner`]; this method only
    /// drives it over the session's decoded buckets. Mid-file decode/seek errors are report-only
    /// safe: return what was scanned and mark truncation with [`BSilenceScan::scanned_end_secs`].
    fn scan_silence_intervals(
        &self,
        session: &mut MR::Session,
        track: &AudioTrack,
        decode_chunk_secs: f64,
        mut scanner: policies::SilenceRunScanner,
    ) -> BSilenceScan {
        let progress = self.progress;
        let mut last_fed_end_secs: Option<f64> = None;

        let mut on_bucket = |bucket: InterleavedScanBucket| -> Result<(), MediaError> {
            if last_fed_end_secs
                .is_some_and(|prev_end| bucket.start_secs > prev_end + TIME_EPS_SECS)
            {
                scanner.note_pcm_discontinuity();
            }
            scanner.feed(&bucket.pcm, bucket.start_secs);
            last_fed_end_secs = Some(bucket.end_secs);
            Ok(())
        };

        let mut b_timeline_skew = None;
        let scan_err = session
            .scan_interleaved_buckets(
                track,
                decode_chunk_secs,
                progress,
                "scan-b",
                &mut on_bucket,
                &mut b_timeline_skew,
            )
            .err();

        // Truncation is a hard scan error. A walk that ends >2s before declared duration may be
        // container over-report (common) or a soft early stop — warn, but do not set
        // `b_scan_truncated` from that alone: occupancy fail-closes on `scanned_end_secs`
        // (last PCM fed), not the truncated flag.
        const NEAR_END_TOLERANCE_SECS: f64 = 2.0;
        let declared_end_secs = track.duration.map(|d| d.as_secs_f64());
        let incomplete_prefix = match (last_fed_end_secs, declared_end_secs) {
            (Some(end), Some(total)) => end + NEAR_END_TOLERANCE_SECS < total,
            (None, Some(_)) => true,
            _ => false,
        };
        let truncated = scan_err.is_some();

        if truncated {
            match (scan_err.as_ref(), last_fed_end_secs) {
                (Some(err), Some(t)) => tracing::warn!(
                    error = %err,
                    b_scanned_end_secs = t,
                    "B-side silence scan truncated mid-file; gaps mapping past that point are unfillable (not reviewed)"
                ),
                (Some(err), None) => tracing::warn!(
                    error = %err,
                    "B-side silence scan failed before any audio; donor occupancy not reviewed"
                ),
                (None, _) => {}
            }
        } else if incomplete_prefix {
            match last_fed_end_secs {
                Some(t) => tracing::warn!(
                    b_scanned_end_secs = t,
                    declared_end_secs = ?declared_end_secs,
                    "B-side silence scan ended >2s before declared duration (container over-report or soft EOF); occupancy still uses scanned_end only"
                ),
                None => tracing::warn!(
                    declared_end_secs = ?declared_end_secs,
                    "B-side silence scan produced no audio before declared duration ended"
                ),
            }
        }

        let (runs, levels) = scanner.finish_with_levels();
        let intervals = runs
            .into_iter()
            .map(|run| SilenceInterval {
                start_secs: run.start_secs,
                end_secs: run.end_secs,
            })
            .collect();
        BSilenceScan {
            intervals,
            levels,
            scanned_end_secs: last_fed_end_secs,
            truncated,
        }
    }

    /// Open `path` and select its best decodable track. Returns `None` (never an error) when the
    /// file is missing, unreadable, or has no decodable audio — keeps the scan report-only safe.
    fn open_best_track(
        &self,
        path: &Path,
        track_a: &AudioTrack,
    ) -> Option<(MR::Session, AudioTrack)> {
        let source = MediaSource::new(path.to_path_buf());
        let session = self.media_reader.open(&source).ok()?;
        let tracks = session.list_tracks().ok()?;
        let track = select_track_for_reference(track_a, &tracks).ok()?.clone();
        Some((session, track))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use clip_sync::testing::fakes::FakeProgressReporter;
    use clip_sync::{
        AlignmentResult, AudioTrack, ClipWindow, MediaError, MediaSession, MediaSource,
        MonoPcmClip, MultiChannelPcm,
    };

    use super::*;

    fn mono_clip_to_multichannel(clip: MonoPcmClip, channels: u16) -> MultiChannelPcm {
        let channels = channels.max(1);
        if channels == 1 {
            return MultiChannelPcm {
                sample_rate: clip.sample_rate,
                channels: 1,
                samples: clip.samples.iter().map(|&s| s as f32 / 32767.0).collect(),
                decode_error_skips: clip.decode_error_skips,
                decoded_frame_count: clip.decoded_sample_count,
                compressed_bytes: None,
                source_bit_depth: None,
            };
        }

        let mut samples = Vec::with_capacity(clip.samples.len().saturating_mul(channels as usize));
        for sample in clip.samples {
            let s = sample as f32 / 32767.0;
            for _ in 0..channels {
                samples.push(s);
            }
        }
        MultiChannelPcm {
            sample_rate: clip.sample_rate,
            channels,
            samples,
            decode_error_skips: clip.decode_error_skips,
            decoded_frame_count: clip
                .decoded_sample_count
                .map(|frames| frames * channels as usize),
            compressed_bytes: None,
            source_bit_depth: None,
        }
    }

    // --- minimal fakes ---

    struct LoudSession(Duration);
    struct SilentSession(Duration);

    impl MediaSession for LoudSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![loud_track(self.0)])
        }

        fn extract_mono(
            &mut self,
            _track: &AudioTrack,
            window: &ClipWindow,
            _progress: &dyn clip_sync::ProgressReporter,
            _label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            let rate = 11_025u32;
            let secs = (window.end - window.start).as_secs_f64();
            let count = (rate as f64 * secs) as usize;
            let samples: Vec<i16> = (0..count)
                .map(|i| (f32::sin(i as f32 * 0.3) * 8_000.0) as i16)
                .collect();
            Ok(MonoPcmClip {
                sample_rate: rate,
                samples,
                decode_error_skips: 0,
                decoded_sample_count: None,
            })
        }

        fn extract_interleaved(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MultiChannelPcm, MediaError> {
            let clip = self.extract_mono(track, window, progress, label)?;
            Ok(mono_clip_to_multichannel(clip, track.channels))
        }
    }

    impl MediaSession for SilentSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![loud_track(self.0)])
        }

        fn extract_mono(
            &mut self,
            _track: &AudioTrack,
            window: &ClipWindow,
            _progress: &dyn clip_sync::ProgressReporter,
            _label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            let rate = 11_025u32;
            let secs = (window.end - window.start).as_secs_f64();
            Ok(MonoPcmClip {
                sample_rate: rate,
                samples: vec![0i16; (rate as f64 * secs) as usize],
                decode_error_skips: 0,
                decoded_sample_count: None,
            })
        }

        fn extract_interleaved(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MultiChannelPcm, MediaError> {
            let clip = self.extract_mono(track, window, progress, label)?;
            Ok(mono_clip_to_multichannel(clip, track.channels))
        }
    }

    fn loud_track(duration: Duration) -> AudioTrack {
        AudioTrack {
            index: 0,
            codec: "pcm".into(),
            channels: 1,
            sample_rate: 11_025,
            duration: Some(duration),
            decodable: true,
            bit_depth: None,
        }
    }

    enum SessionKind {
        Loud,
        Silent,
        /// Loud until `window.start >= fail_from_secs`, then `SeekFailed`.
        TailSeekFail {
            fail_from_secs: f64,
        },
        /// Silent except `DecodeFailed` when `skip_start <= window.start < skip_end`.
        SkipWindow {
            skip_start: f64,
            skip_end: f64,
        },
        /// A **registrable** program: a 440 Hz tone under an aperiodic per-100 ms amplitude envelope,
        /// with a digital-silence hole. Unlike [`Loud`](Self::Loud) its block levels *vary*, which is
        /// what donor registration correlates on — a constant tone has a flat envelope and nothing to
        /// align. `shift_secs` delays the envelope, so a B built with a shift is the same program
        /// arriving late: the local drift the registration exists to find.
        /// `envelope_salt` picks *which* program. Two sessions sharing a salt are the same material
        /// (optionally shifted, which is what the registration searches for); two with different
        /// salts are unrelated programs whose envelopes do not correlate at any lag, which is the
        /// only way to reach `Apply`'s abstain arm from here.
        Program {
            hole_start_secs: f64,
            hole_end_secs: f64,
            shift_secs: f64,
            envelope_salt: u64,
        },
    }

    struct FixedReader(HashMap<PathBuf, (SessionKind, Duration)>);

    impl FixedReader {
        fn new() -> Self {
            Self(HashMap::new())
        }

        fn with(mut self, path: &str, kind: SessionKind, dur: Duration) -> Self {
            self.0.insert(PathBuf::from(path), (kind, dur));
            self
        }
    }

    struct TailSeekFailSession {
        duration: Duration,
        fail_from_secs: f64,
    }

    struct SkipWindowSession {
        duration: Duration,
        skip_start: f64,
        skip_end: f64,
    }

    struct ProgramSession {
        duration: Duration,
        hole: std::ops::Range<f64>,
        shift_secs: f64,
        envelope_salt: u64,
    }

    impl ProgramSession {
        /// Amplitude in `0.15..1.0`, constant across each 100 ms bucket so it lands one value per
        /// scan block. Aperiodic: a smooth sweep would let the lag search lock onto a harmonic and
        /// peak at the wrong offset for the right-looking reason. The `0.15` floor keeps every
        /// content block well clear of the silence predicate, so the only silence is the hole.
        fn envelope(secs: f64, salt: u64) -> f32 {
            let bucket = (secs * 10.0).floor() as i64;
            // XOR with a scrambled salt, not an addition: adding would make salt `n` the same
            // sequence `n` buckets over, which is precisely a shift — and the registration would
            // find it. The two programs have to be unrelated, not offset.
            let h = ((bucket as u64) ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15))
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            0.15 + 0.85 * ((h >> 33) % 1000) as f32 / 1000.0
        }
    }

    impl MediaSession for ProgramSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![loud_track(self.duration)])
        }

        fn extract_mono(
            &mut self,
            _track: &AudioTrack,
            window: &ClipWindow,
            _progress: &dyn clip_sync::ProgressReporter,
            _label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            let rate = 11_025u32;
            let start = window.start.as_secs_f64();
            let secs = (window.end - window.start).as_secs_f64();
            let count = (rate as f64 * secs) as usize;
            // Absolute time, not sample index: the envelope has to be a property of the timeline so
            // it survives whatever windows the scanner asks for.
            let samples: Vec<i16> = (0..count)
                .map(|i| {
                    let t = start + i as f64 / f64::from(rate);
                    if t >= self.hole.start && t < self.hole.end {
                        return 0;
                    }
                    let amp = Self::envelope(t - self.shift_secs, self.envelope_salt);
                    let phase = (t as f32) * 2.0 * std::f32::consts::PI * 440.0;
                    (phase.sin() * amp * 8_000.0) as i16
                })
                .collect();
            Ok(MonoPcmClip {
                sample_rate: rate,
                samples,
                decode_error_skips: 0,
                decoded_sample_count: None,
            })
        }

        fn extract_interleaved(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MultiChannelPcm, MediaError> {
            let clip = self.extract_mono(track, window, progress, label)?;
            Ok(mono_clip_to_multichannel(clip, track.channels))
        }
    }

    impl MediaSession for SkipWindowSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![loud_track(self.duration)])
        }

        fn extract_mono(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            let start = window.start.as_secs_f64();
            if start >= self.skip_start && start < self.skip_end {
                return Err(MediaError::decode_failed(
                    track.index,
                    "skipped scan window",
                ));
            }
            SilentSession(self.duration).extract_mono(track, window, progress, label)
        }

        fn extract_interleaved(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MultiChannelPcm, MediaError> {
            let clip = self.extract_mono(track, window, progress, label)?;
            Ok(mono_clip_to_multichannel(clip, track.channels))
        }
    }

    impl MediaSession for TailSeekFailSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![loud_track(self.duration)])
        }

        fn extract_mono(
            &mut self,
            _track: &AudioTrack,
            window: &ClipWindow,
            _progress: &dyn clip_sync::ProgressReporter,
            _label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            if window.start.as_secs_f64() >= self.fail_from_secs {
                return Err(MediaError::seek_failed("tail seek"));
            }
            let rate = 11_025u32;
            let secs = (window.end - window.start).as_secs_f64();
            let count = (rate as f64 * secs) as usize;
            let samples: Vec<i16> = (0..count)
                .map(|i| (f32::sin(i as f32 * 0.3) * 8_000.0) as i16)
                .collect();
            Ok(MonoPcmClip {
                sample_rate: rate,
                samples,
                decode_error_skips: 0,
                decoded_sample_count: None,
            })
        }

        fn extract_interleaved(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MultiChannelPcm, MediaError> {
            let clip = self.extract_mono(track, window, progress, label)?;
            Ok(mono_clip_to_multichannel(clip, track.channels))
        }
    }

    // FakeMediaSession from clip-sync test-utils doesn't let us control silence per-window,
    // so we implement a local reader that dispatches to LoudSession / SilentSession.

    impl clip_sync::MediaReader for FixedReader {
        type Session = DispatchSession;

        fn open(&self, source: &MediaSource) -> Result<DispatchSession, MediaError> {
            let (kind, dur) = self
                .0
                .get(source.path())
                .ok_or_else(|| MediaError::FileNotFound(source.path().to_path_buf()))?;
            Ok(match kind {
                SessionKind::Loud => DispatchSession::Loud(LoudSession(*dur)),
                SessionKind::Silent => DispatchSession::Silent(SilentSession(*dur)),
                SessionKind::TailSeekFail { fail_from_secs } => {
                    DispatchSession::TailSeekFail(TailSeekFailSession {
                        duration: *dur,
                        fail_from_secs: *fail_from_secs,
                    })
                }
                SessionKind::SkipWindow {
                    skip_start,
                    skip_end,
                } => DispatchSession::SkipWindow(SkipWindowSession {
                    duration: *dur,
                    skip_start: *skip_start,
                    skip_end: *skip_end,
                }),
                SessionKind::Program {
                    hole_start_secs,
                    hole_end_secs,
                    shift_secs,
                    envelope_salt,
                } => DispatchSession::Program(ProgramSession {
                    duration: *dur,
                    hole: *hole_start_secs..*hole_end_secs,
                    shift_secs: *shift_secs,
                    envelope_salt: *envelope_salt,
                }),
            })
        }
    }

    enum DispatchSession {
        Loud(LoudSession),
        Silent(SilentSession),
        TailSeekFail(TailSeekFailSession),
        SkipWindow(SkipWindowSession),
        Program(ProgramSession),
    }

    impl MediaSession for DispatchSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            match self {
                Self::Loud(s) => s.list_tracks(),
                Self::Silent(s) => s.list_tracks(),
                Self::TailSeekFail(s) => s.list_tracks(),
                Self::SkipWindow(s) => s.list_tracks(),
                Self::Program(s) => s.list_tracks(),
            }
        }

        fn extract_mono(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            match self {
                Self::Loud(s) => s.extract_mono(track, window, progress, label),
                Self::Silent(s) => s.extract_mono(track, window, progress, label),
                Self::TailSeekFail(s) => s.extract_mono(track, window, progress, label),
                Self::SkipWindow(s) => s.extract_mono(track, window, progress, label),
                Self::Program(s) => s.extract_mono(track, window, progress, label),
            }
        }

        fn extract_interleaved(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MultiChannelPcm, MediaError> {
            match self {
                Self::Loud(s) => s.extract_interleaved(track, window, progress, label),
                Self::Silent(s) => s.extract_interleaved(track, window, progress, label),
                Self::TailSeekFail(s) => s.extract_interleaved(track, window, progress, label),
                Self::SkipWindow(s) => s.extract_interleaved(track, window, progress, label),
                Self::Program(s) => s.extract_interleaved(track, window, progress, label),
            }
        }
    }

    // --- helpers ---

    fn aligned_result(offset: Option<f64>) -> AlignmentResult {
        crate::application::start_clip_alignment(60.0, offset)
    }

    fn scan_request(a: &str, b: &str, decode_chunk_secs: u64) -> ScanGapsRequest {
        ScanGapsRequest {
            video_a: PathBuf::from(a),
            video_b: PathBuf::from(b),
            align: AlignConfig::default(),
            decode_chunk_secs,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 250, 0.01, 0.0),
            scan_both: false,
            gap_offset_tolerance_secs: 0.5,
            limit_fill_to_mapped_region: true,
            apply_donor_registration: true,
        }
    }

    use crate::application::NeverCalledAligner;

    struct NoDurationSession;

    impl MediaSession for NoDurationSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![AudioTrack {
                index: 0,
                codec: "pcm".into(),
                channels: 1,
                sample_rate: 11_025,
                duration: None,
                decodable: true,
                bit_depth: None,
            }])
        }

        fn extract_mono(
            &mut self,
            _track: &AudioTrack,
            _window: &ClipWindow,
            _progress: &dyn clip_sync::ProgressReporter,
            _label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            Err(MediaError::decode_failed(0, "not reached"))
        }
    }

    struct NoDurationReader;

    impl clip_sync::MediaReader for NoDurationReader {
        type Session = NoDurationSession;

        fn open(&self, _source: &MediaSource) -> Result<NoDurationSession, MediaError> {
            Ok(NoDurationSession)
        }
    }

    #[test]
    fn loud_pcm_is_not_classified_as_silent() {
        let loud_samples_i16: Vec<i16> = (0..11_025)
            .map(|i| (f32::sin(i as f32 * 0.3) * 8_000.0) as i16)
            .collect();
        let loud_clip = MonoPcmClip {
            sample_rate: 11_025,
            samples: loud_samples_i16.clone(),
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        let loud_samples_f32: Vec<f32> = loud_clip
            .samples
            .iter()
            .map(|&s| s as f32 / 32767.0)
            .collect();
        assert!(!policies::is_silent(&loud_samples_f32, 0.01, 0.0));

        let dur = Duration::from_secs(120);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Loud, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let source_a = MediaSource::new(PathBuf::from("a.wav"));
        let source_b = MediaSource::new(PathBuf::from("b.wav"));
        assert!(reader.open(&source_a).is_ok());
        assert!(reader.open(&source_b).is_ok());
    }

    #[test]
    fn scan_after_alignment_detects_silent_gap_with_fillable_b() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let report = scan
            .scan_after_alignment(
                scan_request("a.wav", "b.wav", 60),
                aligned_result(Some(0.0)),
            )
            .expect("scan should succeed");

        assert_eq!(report.gaps.len(), 1);
        assert!(report.gaps[0].b_has_energy);
        let compat = report
            .track_compatibility
            .as_ref()
            .expect("compatibility should be present when B opens");
        assert_eq!(
            compat.verdict,
            crate::domain::CompatibilityVerdict::Identical
        );
        assert!((report.gaps[0].video_a_start_secs - 0.0).abs() < 0.001);
        assert!((report.gaps[0].video_a_end_secs - 60.0).abs() < 0.001);
        assert!((report.gaps[0].video_b_start_secs.unwrap() - 0.0).abs() < 0.001);
        assert!((report.gaps[0].video_b_end_secs.unwrap() - 60.0).abs() < 0.001);
        assert!(report.gaps[0].is_fillable());
    }

    #[test]
    fn scan_after_alignment_loud_a_finds_no_gaps() {
        let dur = Duration::from_secs(120);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Loud, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let report = scan
            .scan_after_alignment(
                scan_request("a.wav", "b.wav", 60),
                aligned_result(Some(0.0)),
            )
            .expect("scan should succeed");

        assert!(report.gaps.is_empty());
    }

    #[test]
    fn scan_after_alignment_with_failed_alignment_marks_b_unfillable() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new().with("a.wav", SessionKind::Silent, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let report = scan
            .scan_after_alignment(scan_request("a.wav", "b.wav", 60), aligned_result(None))
            .expect("scan should succeed");

        assert_eq!(report.gaps.len(), 1);
        assert!(!report.gaps[0].b_has_energy);
        assert!(report.gaps[0].video_b_start_secs.is_none());
        assert!(report.gaps[0].video_b_end_secs.is_none());
        assert!(!report.gaps[0].is_fillable());
        assert!(report.track_compatibility.is_none());
        assert!(report.alignment.start_overlap.is_none());
    }

    #[test]
    fn scan_after_alignment_applies_offset_to_b_timeline() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let report = scan
            .scan_after_alignment(
                scan_request("a.wav", "b.wav", 60),
                aligned_result(Some(3.0)),
            )
            .expect("scan should succeed");

        assert_eq!(report.gaps.len(), 1);
        assert!((report.gaps[0].video_b_start_secs.unwrap() - 3.0).abs() < 0.001);
        assert!((report.gaps[0].video_b_end_secs.unwrap() - 63.0).abs() < 0.001);
    }

    #[test]
    fn scan_after_alignment_unknown_duration_returns_invalid_duration() {
        use crate::infrastructure::cli::exit_code::exit_code_for;
        use std::process::ExitCode;

        let reader = NoDurationReader;
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let err = scan
            .scan_after_alignment(
                scan_request("a.wav", "b.wav", 60),
                aligned_result(Some(0.0)),
            )
            .expect_err("missing duration should fail");

        assert!(matches!(
            err,
            RepairError::Domain(DomainError::InvalidDuration)
        ));
        assert_eq!(exit_code_for(&err), ExitCode::from(3));
    }

    #[test]
    fn gap_report_fillable_count_counts_b_energy_gaps() {
        use crate::domain::gap::{Gap, GapReport};

        let report = GapReport {
            video_a: PathBuf::from("a.wav"),
            video_b: PathBuf::from("b.wav"),
            track_compatibility: None,
            alignment: scan_alignment_from_result(&aligned_result(Some(0.0))),
            gaps: vec![
                Gap {
                    video_a_start_secs: 0.0,
                    video_a_end_secs: 60.0,
                    video_b_start_secs: Some(0.0),
                    video_b_end_secs: Some(60.0),
                    b_has_energy: true,
                },
                Gap {
                    video_a_start_secs: 120.0,
                    video_a_end_secs: 180.0,
                    video_b_start_secs: Some(120.0),
                    video_b_end_secs: Some(180.0),
                    b_has_energy: false,
                },
            ],
            gap_equivalence: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: 60,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 250, 0.01, 0.0),
            limit_fill_to_mapped_region: true,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
        };

        assert_eq!(report.gaps.len(), 2);
        assert_eq!(report.fillable_count(), 1);
        assert!(report.gaps[0].is_fillable());
        assert!(!report.gaps[1].is_fillable());
        assert!((report.gaps[0].duration_secs() - 60.0).abs() < 0.001);
    }

    #[test]
    fn scan_both_skips_cross_check_when_only_a_dropouts() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let mut request = scan_request("a.wav", "b.wav", 60);
        request.scan_both = true;

        let report = scan
            .scan_after_alignment(request, aligned_result(Some(0.0)))
            .expect("scan should succeed");

        assert_eq!(report.gaps.len(), 1);
        assert!(report.gaps[0].b_has_energy);
        assert!(
            report.gap_offset_agreement.is_none(),
            "A-only silences must not produce cross-check"
        );
    }

    #[test]
    fn scan_both_produces_agreement_when_silence_colocated() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Silent, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let mut request = scan_request("a.wav", "b.wav", 60);
        request.scan_both = true;
        request.gap_offset_tolerance_secs = 0.5;

        let report = scan
            .scan_after_alignment(request, aligned_result(Some(0.0)))
            .expect("scan should succeed");

        let agreement = report
            .gap_offset_agreement
            .as_ref()
            .expect("agreement should be present when both timelines have silence");
        assert!(agreement.agrees, "colocated silence should agree");
        assert!(agreement.delta_secs < 0.001, "delta should be ~0");
    }

    #[test]
    fn scan_both_absent_when_scan_both_disabled() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Silent, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let report = scan
            .scan_after_alignment(
                scan_request("a.wav", "b.wav", 60),
                aligned_result(Some(0.0)),
            )
            .expect("scan should succeed");

        assert!(report.gap_offset_agreement.is_none());
    }

    #[test]
    fn scan_both_absent_when_alignment_failed() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Silent, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let mut request = scan_request("a.wav", "b.wav", 60);
        request.scan_both = true;

        let report = scan
            .scan_after_alignment(request, aligned_result(None))
            .expect("scan should succeed");

        assert!(report.gap_offset_agreement.is_none());
    }

    #[test]
    fn scan_both_stops_b_scan_at_tail_seek_failure() {
        let dur = Duration::from_secs(125);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Loud, dur)
            .with(
                "b.wav",
                SessionKind::TailSeekFail {
                    fail_from_secs: 118.0,
                },
                dur,
            );
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let mut request = scan_request("a.wav", "b.wav", 60);
        request.scan_both = true;

        let report = scan
            .scan_after_alignment(request, aligned_result(Some(0.0)))
            .expect("tail seek on B should not fail the scan");

        assert!(report.gaps.is_empty());
        assert!(report.b_scan_truncated);
        // Chunk [60,120) starts before fail_from=118 so it feeds; [120,125) SeekFailed
        // propagates (not within near-end soft-EOF tolerance of 2s) → scanned end ≈ 120s.
        assert!(
            report
                .b_scanned_end_secs
                .is_some_and(|t| (t - 120.0).abs() < 0.01),
            "expected scanned end ≈ 120s after mid-tail seek abort; got {:?}",
            report.b_scanned_end_secs
        );
    }

    #[test]
    fn truncated_b_scan_fail_closes_gaps_past_scanned_end() {
        // A fully silent → one long gap. B is loud but seek-fails from 20s; without fail-closed
        // levels in [0,20) would still report b_has_energy for a core spanning to 60s.
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with(
                "b.wav",
                SessionKind::TailSeekFail {
                    fail_from_secs: 20.0,
                },
                dur,
            );
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let mut request = scan_request("a.wav", "b.wav", 10);
        request.scan_both = true;

        let report = scan
            .scan_after_alignment(request, aligned_result(Some(0.0)))
            .expect("truncated B scan must not abort the report");

        assert_eq!(report.gaps.len(), 1);
        assert!(report.b_scan_truncated);
        assert!(
            report
                .b_scanned_end_secs
                .is_some_and(|t| (t - 20.0).abs() < 0.01),
            "expected scanned end ≈ 20s, got {:?}",
            report.b_scanned_end_secs
        );
        assert!(
            !report.gaps[0].b_has_energy,
            "mapped core past scanned end must be unfillable (not reviewed)"
        );
        assert!(
            format_b_scan_truncation_note(report.b_scan_truncated, report.b_scanned_end_secs)
                .expect("note")
                .contains("truncated at 20.000s")
        );
    }

    #[test]
    fn scan_a_propagates_midfile_extract_failure() {
        // Seek-loop fallback (test fakes) now propagates mid-file DecodeFailed instead of
        // skipping the bucket. Symphonia's production sequential scan still skips individual
        // corrupt packets with its own consecutive-error limit.
        let dur = Duration::from_secs(180);
        let reader = FixedReader::new()
            .with(
                "a.wav",
                SessionKind::SkipWindow {
                    skip_start: 60.0,
                    skip_end: 120.0,
                },
                dur,
            )
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let err = scan
            .scan_after_alignment(
                scan_request("a.wav", "b.wav", 60),
                aligned_result(Some(0.0)),
            )
            .expect_err("A mid-file decode failure must fail the scan");
        assert!(
            matches!(err, RepairError::Media(_)),
            "expected Media error, got {err:?}"
        );
    }

    #[test]
    fn format_scan_summary_includes_thresholds_and_count() {
        let request = ScanGapsRequest {
            video_a: PathBuf::from("a.wav"),
            video_b: PathBuf::from("b.wav"),
            align: AlignConfig::default(),
            decode_chunk_secs: 10,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 2, 250, 0.01, 33.0 / 32767.0),
            scan_both: true,
            gap_offset_tolerance_secs: 0.5,
            limit_fill_to_mapped_region: false,
            apply_donor_registration: true,
        };
        let line = format_scan_summary(&request, 30);
        assert!(line.contains("30 silent run(s)"));
        assert!(line.contains("≥1000ms"));
        assert!(line.contains("block 250ms"));
        assert!(line.contains("1.0% peak"));
        assert!(line.contains("hold 500ms"));
        assert!(line.contains("decode 10s"));
        assert!(line.contains("scan-both on"));
        assert!(
            line.contains("rms floor 33 (at -60 dBFS)"),
            "header must show i16-scale floor + dBFS, got {line}"
        );
    }

    #[test]
    fn scan_report_recipe_round_trips_from_request() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let recipe =
            crate::domain::ScanRecipe::with_hold_blocks(1000, 2, 250, 0.01, production_abs_floor());
        let request = ScanGapsRequest {
            video_a: PathBuf::from("a.wav"),
            video_b: PathBuf::from("b.wav"),
            align: AlignConfig::default(),
            decode_chunk_secs: 60,
            recipe,
            scan_both: false,
            gap_offset_tolerance_secs: 0.5,
            limit_fill_to_mapped_region: true,
            apply_donor_registration: true,
        };

        let report = scan
            .scan_after_alignment(request, aligned_result(Some(0.0)))
            .expect("scan should succeed")
            .report;
        assert_eq!(report.recipe, recipe);
        assert_eq!(report.recipe.silence_hold_ms(), 500);
        assert_eq!(report.recipe.silence_hold_blocks(), 2);

        let corpus = crate::application::gap_fingerprint::CorpusScanRecipe::from_report(&report);
        assert_eq!(corpus.min_gap_ms, Some(1000));
        assert_eq!(corpus.silence_hold_ms, Some(500));
        assert_eq!(corpus.scan_block_ms, Some(250));
        assert_eq!(corpus.silence_peak_fraction, 0.01);
        assert_eq!(corpus.absolute_silence_rms, Some(production_abs_floor()));
    }

    /// Production default floor (`33/32767`), not the fixture habit of `0.0`.
    fn production_abs_floor() -> f32 {
        33.0 / 32767.0
    }

    fn assert_occupancy_agrees_with_donor(report: &GapReport) {
        for (i, gap) in report.gaps.iter().enumerate() {
            let ds = report
                .gap_equivalence_at(i)
                .and_then(|v| v.donor_silence_fraction);
            assert!(
                crate::domain::gap_equivalence::occupancy_agrees_with_donor_silence(
                    gap.b_has_energy,
                    ds,
                    0.5,
                ),
                "gap {i}: b_has_energy={} donor_silence={ds:?} (F7)",
                gap.b_has_energy
            );
        }
    }

    #[test]
    fn scan_with_production_floor_silent_pair_agrees_and_labels_both_silent() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Silent, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let mut request = scan_request("a.wav", "b.wav", 60);
        request.recipe = request
            .recipe
            .with_absolute_silence_rms(production_abs_floor());
        request.scan_both = true;

        let report = scan
            .scan_after_alignment(request, aligned_result(Some(0.0)))
            .expect("scan should succeed");

        assert!(!report.gaps.is_empty(), "silent A should yield gaps");
        for gap in &report.gaps {
            assert!(gap.video_b_start_secs.is_some());
            assert!(!gap.b_has_energy);
            assert_eq!(gap.unfillable_label(), "both sides silent");
        }
        assert_occupancy_agrees_with_donor(&report);
    }

    #[test]
    fn scan_with_production_floor_a_silent_b_loud_keeps_occupancy_donor_agreement() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let mut request = scan_request("a.wav", "b.wav", 60);
        request.recipe = request
            .recipe
            .with_absolute_silence_rms(production_abs_floor());
        request.scan_both = true;

        let report = scan
            .scan_after_alignment(request, aligned_result(Some(0.0)))
            .expect("scan should succeed");

        assert_eq!(report.gaps.len(), 1);
        assert!(report.gaps[0].b_has_energy);
        assert_occupancy_agrees_with_donor(&report);
        let ds = report.gap_equivalence[0].donor_silence_fraction;
        assert!(
            ds.is_some_and(|f| f < 0.5),
            "loud donor must score occupied, got {ds:?}"
        );
    }

    /// The class-flipping geometry both halves of the `Observe`/`Apply` split are pinned on.
    ///
    /// A holes 20.0–20.6 s; B is the same program 500 ms late, so B's own hole is 20.5–21.1 s. At
    /// the nominal offset (0.0) the donor window reads five content blocks and one silent one — B
    /// looks occupied and the gap is a repairable dropout. Move the window the 500 ms registration
    /// finds and it lands on B's hole, reads 6/6 silent, and the gap drops as `shared_silence`.
    /// This is the shape §6.4 found on real media, in miniature.
    fn registration_flip_report(apply_donor_registration: bool) -> ScanGapsOutcome {
        registration_report(apply_donor_registration, 0)
    }

    /// `b_envelope_salt` of `0` makes B the same program as A, 500 ms late — the flip geometry
    /// above. Any other salt makes B an unrelated program, which is how the abstain arm is reached.
    fn registration_report(
        apply_donor_registration: bool,
        b_envelope_salt: u64,
    ) -> ScanGapsOutcome {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with(
                "a.wav",
                SessionKind::Program {
                    hole_start_secs: 20.0,
                    hole_end_secs: 20.6,
                    shift_secs: 0.0,
                    envelope_salt: 0,
                },
                dur,
            )
            .with(
                "b.wav",
                SessionKind::Program {
                    hole_start_secs: 20.5,
                    hole_end_secs: 21.1,
                    shift_secs: 0.5,
                    envelope_salt: b_envelope_salt,
                },
                dur,
            );
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let mut request = scan_request("a.wav", "b.wav", 60);
        // 100 ms blocks so the 500 ms shift is a whole number of them, no hold bridging.
        request.recipe = crate::domain::ScanRecipe::with_hold_blocks(500, 0, 100, 0.01, 0.0);
        request.scan_both = true;
        request.apply_donor_registration = apply_donor_registration;

        scan.scan_after_alignment(request, aligned_result(Some(0.0)))
            .expect("scan should succeed")
    }

    /// Asserts the registration itself — computed and emitted end-to-end, decoded PCM → block
    /// levels → verdict — independently of which mode consumes it.
    fn assert_registration_found_the_shift(
        v: &crate::domain::gap_equivalence::GapEquivalenceVerdict,
    ) {
        let reg = v
            .donor_registration
            .as_ref()
            .expect("scan must record the registration");
        assert_eq!(reg.lag_blocks, 5, "B is 500 ms late: {reg:?}");
        // `lag_ms` is `lag_blocks` × the level stream's **own** bin width, which is the block in
        // samples (1103 at 11025 Hz) and not the 100 ms the recipe asked for. That ~0.05 % is the
        // real quantization and the field is right to report it, so the tolerance is a millisecond.
        assert!((reg.lag_ms - 500.0).abs() < 1.0, "{reg:?}");
        assert!(
            reg.peak_r > reg.nominal_r,
            "the registered lag beats the nominal map: {reg:?}"
        );
    }

    /// Shipped default (2026-08-04): the donor window is classified at the **registered** lag, so
    /// the registration is allowed to move the verdict. Here it moves it the whole way — off a
    /// `repairable_dropout` the nominal map only sees because the window is misregistered.
    #[test]
    fn scan_classifies_the_donor_window_at_the_registered_lag() {
        let report = registration_flip_report(true);

        assert_eq!(report.gaps.len(), 1, "only the hole is silent");
        let v = &report.gap_equivalence[0];
        assert_registration_found_the_shift(v);

        assert_eq!(
            v.class,
            crate::domain::gap_equivalence::GapEquivalenceClass::SharedSilence
        );
        assert!(v.drop, "nothing to fill from: {v:?}");
        assert!(v.not_evaluated_reason.is_none());
        assert!(
            v.donor_silence_fraction.is_some_and(|f| f >= 0.5),
            "measured at the registered window, which is B's own hole: {:?}",
            v.donor_silence_fraction
        );
        // The offset is 0.0, so an *unshifted* donor window would be the A core itself. Comparing
        // the two spans rather than a literal keeps this about the registration and not about
        // where block quantization happens to put the core.
        assert_ne!(
            v.donor_span_secs, v.a_span_secs,
            "the window measured is the registered one, not the nominal one"
        );
        assert_occupancy_agrees_with_donor(&report);
    }

    /// `--no-apply-donor-registration` restores the pre-2026-08-04 behaviour: the registration is
    /// still computed and emitted, but it is inert — the verdict is the nominal map's.
    #[test]
    fn scan_with_registration_not_applied_keeps_the_nominal_map_verdict() {
        let report = registration_flip_report(false);

        assert_eq!(report.gaps.len(), 1, "only the hole is silent");
        let v = &report.gap_equivalence[0];
        assert_registration_found_the_shift(v);

        assert_eq!(
            v.class,
            crate::domain::gap_equivalence::GapEquivalenceClass::RepairableDropout
        );
        assert!(!v.drop);
        assert!(v.not_evaluated_reason.is_none());
        assert!(
            v.donor_silence_fraction.is_some_and(|f| f < 0.5),
            "measured at the nominal window, where B has content: {:?}",
            v.donor_silence_fraction
        );
        assert_eq!(
            v.donor_span_secs, v.a_span_secs,
            "the window measured is the nominal one, not the registered one"
        );
        assert_occupancy_agrees_with_donor(&report);
    }

    /// `Apply`'s third arm, and the only one that can cost anything: when the donor envelope will
    /// not register against A's, no statement about B's occupancy is defensible, so the gate
    /// **abstains** instead of classifying — 4.3 % of the corpus (§6.10).
    ///
    /// The cost is bounded in the safe direction, which is what this pins: an abstention is
    /// `not_evaluated` and does **not** drop, so the gap goes on to the repair path and the worst
    /// case is a patch attempt on material that did not need one. It can never be a hole.
    #[test]
    fn scan_abstains_when_the_donor_will_not_register() {
        // B is an unrelated program: same tone, same silence floor, uncorrelated envelope.
        let report = registration_report(true, 0x5EED);

        assert_eq!(report.gaps.len(), 1, "only A's hole is silent");
        let v = &report.gap_equivalence[0];
        let reg = v
            .donor_registration
            .as_ref()
            .expect("the registration is still computed and recorded");
        assert!(
            reg.peak_r
                < crate::domain::gap_equivalence::DonorRegistrationParams::default().min_envelope_r,
            "unrelated programs must not register: {reg:?}"
        );

        assert_eq!(
            v.class,
            crate::domain::gap_equivalence::GapEquivalenceClass::NotEvaluated
        );
        assert_eq!(
            v.not_evaluated_reason,
            Some(crate::domain::gap_equivalence::NotEvaluatedReason::DonorRegistrationUnreliable)
        );
        assert!(
            !v.drop,
            "an abstention costs a patch attempt, never a hole: {v:?}"
        );
    }
}
