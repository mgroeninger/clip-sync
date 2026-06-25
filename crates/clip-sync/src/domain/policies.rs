use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::domain::alignment::{clip_with_label, AlignmentResult, TimelineOverlap};
use crate::domain::audio_track::AudioTrack;
use crate::domain::clip_plan::ClipPlan;
use crate::domain::clip_window::{ClipLabel, ClipWindow};
use crate::domain::error::DomainError;
use crate::domain::media_extent::MediaExtent;
use crate::domain::mono_pcm_clip::MonoPcmClip;
use crate::domain::query_localization::AlignmentModeUsed;

/// Pick the first decodable audio track in container order.
///
/// Multi-track files often mux the main program before commentary or effects tracks.
/// Higher sample rate on a secondary track is not a reliable signal for "best" audio.
pub fn select_best_track(tracks: &[AudioTrack]) -> Result<&AudioTrack, DomainError> {
    if tracks.is_empty() {
        return Err(DomainError::NoAudioTracks);
    }

    tracks
        .iter()
        .find(|track| track.decodable)
        .ok_or(DomainError::NoDecodableAudioTracks)
}

/// Pick the best decodable B track to use as a reference for A.
///
/// Prefers a track whose channel count matches A's exactly; falls back to
/// `select_best_track` (first decodable in container order) when no channel
/// match exists. This matters for dual-track containers (e.g. 2ch AAC + 6ch
/// AC-3) where the surround track is the correct repair source.
pub fn select_track_for_reference<'a>(
    a: &AudioTrack,
    tracks: &'a [AudioTrack],
) -> Result<&'a AudioTrack, DomainError> {
    if tracks.is_empty() {
        return Err(DomainError::NoAudioTracks);
    }

    tracks
        .iter()
        .find(|t| t.decodable && t.channels == a.channels)
        .or_else(|| tracks.iter().find(|t| t.decodable))
        .ok_or(DomainError::NoDecodableAudioTracks)
}

/// Order decodable A×B track pairs for `try_all_tracks`: channel-matched layouts first.
pub fn order_track_pairs_for_alignment<'a>(
    decodable_a: &[&'a AudioTrack],
    decodable_b: &[&'a AudioTrack],
) -> Vec<(&'a AudioTrack, &'a AudioTrack)> {
    let mut pairs: Vec<(&'a AudioTrack, &'a AudioTrack)> = decodable_a
        .iter()
        .flat_map(|track_a| decodable_b.iter().map(move |track_b| (*track_a, *track_b)))
        .collect();
    pairs.sort_by(|(a1, b1), (a2, b2)| {
        let matched1 = a1.channels == b1.channels;
        let matched2 = a2.channels == b2.channels;
        matched2
            .cmp(&matched1)
            .then_with(|| a1.index.cmp(&a2.index))
            .then_with(|| b1.index.cmp(&b2.index))
    });
    pairs
}

/// How symmetric multi-clip planning anchors the end window (and interior when `num_clips > 2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndClipAnchor {
    /// Each file's last `clip_length` before its own effective timeline end.
    FileTail,
    /// Shorter effective duration defines shared absolute times on both timelines.
    #[default]
    #[serde(alias = "anchored")]
    SharedTimeline,
}

/// Interior window overlapping start/end by more than this is omitted (paired planning).
pub const INTERIOR_OVERLAP_TOLERANCE: Duration = Duration::from_secs(1);

/// Optional bounds when placing multi-clip windows near the file tail.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClipPlanningOptions {
    /// Inset before [`MediaExtent::effective`] for the end clip window.
    pub end_tail_inset: Duration,
    /// End (and interior when `num_clips > 2`) anchoring for paired symmetric planning.
    pub end_clip_anchor: EndClipAnchor,
}

pub fn effective_timeline_end(extent: &MediaExtent, end_tail_inset: Duration) -> Duration {
    extent
        .effective()
        .saturating_sub(end_tail_inset)
        .max(Duration::from_secs(1))
        .min(extent.declared)
}

pub fn clip_windows_with_options(
    extent: &MediaExtent,
    plan: &ClipPlan,
    options: ClipPlanningOptions,
) -> Result<Vec<ClipWindow>, DomainError> {
    if extent.declared.is_zero() {
        return Err(DomainError::InvalidDuration);
    }

    let duration = extent.declared;
    let clip_length = plan.clip_length;
    let effective_num_clips = if duration < clip_length {
        1
    } else {
        plan.num_clips
    };

    if effective_num_clips == 1 {
        let end = duration.min(clip_length);
        if end.is_zero() {
            return Err(DomainError::EmptyClip);
        }
        return Ok(vec![ClipWindow::new(
            Duration::ZERO,
            end,
            ClipLabel::Start,
        )]);
    }

    let timeline_end = effective_timeline_end(extent, options.end_tail_inset);
    if timeline_end < clip_length {
        let end = timeline_end.min(clip_length);
        if end.is_zero() {
            return Err(DomainError::EmptyClip);
        }
        return Ok(vec![ClipWindow::new(
            Duration::ZERO,
            end,
            ClipLabel::Start,
        )]);
    }

    let n = effective_num_clips;
    let mut windows = Vec::with_capacity(n as usize);

    windows.push(ClipWindow::new(
        Duration::ZERO,
        clip_length,
        ClipLabel::Start,
    ));

    if n > 2 {
        windows.extend(interior_windows_along_timeline(
            timeline_end.as_secs_f64(),
            clip_length,
            n,
        ));
    }

    let end_start = timeline_end.saturating_sub(clip_length);
    windows.push(ClipWindow::new(
        end_start,
        timeline_end,
        ClipLabel::End,
    ));

    for window in &windows {
        if window.duration().is_zero() {
            return Err(DomainError::EmptyClip);
        }
    }

    Ok(windows)
}

/// Interior windows for segment indices `1..n-1` along `timeline_secs`.
pub fn interior_windows_along_timeline(
    timeline_secs: f64,
    clip_length: Duration,
    n: u32,
) -> Vec<ClipWindow> {
    if n <= 2 {
        return Vec::new();
    }
    let clip_secs = clip_length.as_secs_f64();
    let mut windows = Vec::with_capacity((n.saturating_sub(2)) as usize);
    for i in 1..(n - 1) {
        let seg_start_secs = timeline_secs * f64::from(i) / f64::from(n);
        let seg_end_secs = timeline_secs * f64::from(i + 1) / f64::from(n);
        let center_secs = (seg_start_secs + seg_end_secs) / 2.0;
        let half = clip_secs / 2.0;
        let start_secs = (center_secs - half).max(0.0);
        let end_secs = (start_secs + clip_secs).min(timeline_secs);
        windows.push(ClipWindow::new(
            secs_to_duration(start_secs),
            secs_to_duration(end_secs),
            ClipLabel::Interior,
        ));
    }
    windows
}

/// Whether `interior` shares more than `tolerance` with `start` or `end`.
pub fn interior_overlaps_fixed_clip(
    interior: &ClipWindow,
    start: &ClipWindow,
    end: &ClipWindow,
    tolerance: Duration,
) -> bool {
    let tol = tolerance.as_secs_f64();
    window_overlap_secs(interior, start) > tol || window_overlap_secs(interior, end) > tol
}

fn window_overlap_secs(a: &ClipWindow, b: &ClipWindow) -> f64 {
    let start = a.start.max(b.start);
    let end = a.end.min(b.end);
    if end <= start {
        0.0
    } else {
        (end - start).as_secs_f64()
    }
}

/// Paired symmetric clip windows for two files (see anchored-end extraction plan).
pub fn clip_windows_paired(
    extent_a: &MediaExtent,
    extent_b: &MediaExtent,
    plan: &ClipPlan,
    options: ClipPlanningOptions,
) -> Result<(Vec<ClipWindow>, Vec<ClipWindow>), DomainError> {
    if extent_a.declared.is_zero() || extent_b.declared.is_zero() {
        return Err(DomainError::InvalidDuration);
    }

    if plan.num_clips < 2 {
        let windows_a = clip_windows_with_options(extent_a, plan, options)?;
        let windows_b = clip_windows_with_options(extent_b, plan, options)?;
        return Ok((windows_a, windows_b));
    }

    let clip_length = plan.clip_length;
    let timeline_end_a = effective_timeline_end(extent_a, options.end_tail_inset);
    let timeline_end_b = effective_timeline_end(extent_b, options.end_tail_inset);
    let t_anchor = timeline_end_a.min(timeline_end_b);

    if t_anchor < clip_length
        || extent_a.declared < clip_length
        || extent_b.declared < clip_length
    {
        let end = t_anchor.min(clip_length);
        if end.is_zero() {
            return Err(DomainError::EmptyClip);
        }
        let window = ClipWindow::new(Duration::ZERO, end, ClipLabel::Start);
        return Ok((vec![window], vec![window]));
    }

    let n = plan.num_clips;
    let start = ClipWindow::new(Duration::ZERO, clip_length, ClipLabel::Start);
    let end_a = end_window_for_file(timeline_end_a, t_anchor, clip_length, options.end_clip_anchor);
    let end_b = end_window_for_file(timeline_end_b, t_anchor, clip_length, options.end_clip_anchor);

    let (interiors_a, interiors_b) = match options.end_clip_anchor {
        EndClipAnchor::FileTail => {
            let interiors_a = interior_windows_along_timeline(
                timeline_end_a.as_secs_f64(),
                clip_length,
                n,
            );
            let interiors_b = interior_windows_along_timeline(
                timeline_end_b.as_secs_f64(),
                clip_length,
                n,
            );
            filter_overlapping_interiors_paired(
                interiors_a,
                interiors_b,
                &start,
                &end_a,
                &start,
                &end_b,
            )
        }
        EndClipAnchor::SharedTimeline => {
            let interiors = interior_windows_along_timeline(t_anchor.as_secs_f64(), clip_length, n)
                .into_iter()
                .filter(|interior| {
                    !interior_overlaps_fixed_clip(
                        interior,
                        &start,
                        &end_a,
                        INTERIOR_OVERLAP_TOLERANCE,
                    )
                })
                .collect::<Vec<_>>();
            (interiors.clone(), interiors)
        }
    };

    let windows_a = assemble_labeled_windows(start, interiors_a, end_a)?;
    let windows_b = assemble_labeled_windows(start, interiors_b, end_b)?;
    debug_assert_eq!(windows_a.len(), windows_b.len());
    Ok((windows_a, windows_b))
}

/// Attaches symmetric clip-planning metadata for reporting (end anchor + per-clip B windows).
pub fn attach_symmetric_planning_report_metadata(
    result: &mut AlignmentResult,
    extent_a: &MediaExtent,
    extent_b: &MediaExtent,
    plan: &ClipPlan,
    options: ClipPlanningOptions,
    num_clips_configured: u32,
) {
    if result.query_localization.is_some()
        || result.alignment_mode_used == Some(AlignmentModeUsed::QueryReference)
    {
        return;
    }
    if num_clips_configured < 2 || clip_with_label(&result.clips, ClipLabel::End).is_none() {
        return;
    }

    result.end_clip_anchor = Some(options.end_clip_anchor);

    let Ok((windows_a, windows_b)) = clip_windows_paired(extent_a, extent_b, plan, options) else {
        return;
    };
    if windows_a.len() != result.clips.len() {
        return;
    }

    for (clip, (window_a, window_b)) in result
        .clips
        .iter_mut()
        .zip(windows_a.iter().zip(windows_b.iter()))
    {
        let a_start = window_a.start.as_secs_f64();
        let a_end = window_a.end.as_secs_f64();
        let b_start = window_b.start.as_secs_f64();
        let b_end = window_b.end.as_secs_f64();
        if (b_start - a_start).abs() > 1e-9 || (b_end - a_end).abs() > 1e-9 {
            clip.video_b_window_start_secs = Some(b_start);
            clip.video_b_window_end_secs = Some(b_end);
        } else {
            clip.video_b_window_start_secs = None;
            clip.video_b_window_end_secs = None;
        }
    }
}

fn end_window_for_file(
    timeline_end: Duration,
    t_anchor: Duration,
    clip_length: Duration,
    anchor: EndClipAnchor,
) -> ClipWindow {
    let end = match anchor {
        EndClipAnchor::FileTail => timeline_end,
        EndClipAnchor::SharedTimeline => t_anchor.min(timeline_end),
    };
    let start = end.saturating_sub(clip_length);
    ClipWindow::new(start, end, ClipLabel::End)
}

fn filter_overlapping_interiors_paired(
    interiors_a: Vec<ClipWindow>,
    interiors_b: Vec<ClipWindow>,
    start_a: &ClipWindow,
    end_a: &ClipWindow,
    start_b: &ClipWindow,
    end_b: &ClipWindow,
) -> (Vec<ClipWindow>, Vec<ClipWindow>) {
    debug_assert_eq!(interiors_a.len(), interiors_b.len());
    let mut out_a = Vec::with_capacity(interiors_a.len());
    let mut out_b = Vec::with_capacity(interiors_b.len());
    for (ia, ib) in interiors_a.into_iter().zip(interiors_b) {
        if interior_overlaps_fixed_clip(&ia, start_a, end_a, INTERIOR_OVERLAP_TOLERANCE)
            || interior_overlaps_fixed_clip(&ib, start_b, end_b, INTERIOR_OVERLAP_TOLERANCE)
        {
            continue;
        }
        out_a.push(ia);
        out_b.push(ib);
    }
    (out_a, out_b)
}

fn assemble_labeled_windows(
    start: ClipWindow,
    interiors: Vec<ClipWindow>,
    end: ClipWindow,
) -> Result<Vec<ClipWindow>, DomainError> {
    let mut windows = Vec::with_capacity(2 + interiors.len());
    windows.push(start);
    windows.extend(interiors);
    windows.push(end);
    for window in &windows {
        if window.duration().is_zero() {
            return Err(DomainError::EmptyClip);
        }
    }
    Ok(windows)
}

/// Two-tier query-mode decision for `Auto`: shorter file is treated as a query when it is
/// much shorter than the reference (Tier 1, durations only) or — for near-equal lengths —
/// when symmetric clip planning yields different window counts (Tier 2).
///
/// `query_min_duration_ratio` must be in `(0, 1]`. `windows_a` / `windows_b` are the symmetric
/// clip-window counts; callers may compute them lazily (only Tier 2 needs them).
pub fn should_use_query_mode(
    extent_a: &MediaExtent,
    extent_b: &MediaExtent,
    windows_a: usize,
    windows_b: usize,
    query_min_duration_ratio: f64,
) -> bool {
    let dur_a = extent_a.effective().as_secs_f64();
    let dur_b = extent_b.effective().as_secs_f64();
    let (short, long) = if dur_a <= dur_b {
        (dur_a, dur_b)
    } else {
        (dur_b, dur_a)
    };
    if long <= 0.0 {
        return false;
    }
    // Tier 1: duration ratio.
    if short / long < query_min_duration_ratio {
        return true;
    }
    // Tier 2: clip-window count mismatch (lengths are similar).
    windows_a != windows_b
}

/// Drop silence padding appended when a tail extract ended before the planned window end.
pub fn truncate_padded_tail(mut clip: MonoPcmClip) -> MonoPcmClip {
    if let Some(decoded) = clip.decoded_sample_count {
        if decoded < clip.samples.len() {
            clip.samples.truncate(decoded);
        }
        clip.decoded_sample_count = None;
    }
    clip
}

/// Whether a hold-out extract decoded enough samples for the requested segment length.
pub fn holdout_extract_sufficient(
    clip: &MonoPcmClip,
    segment_length: Duration,
    min_decode_fraction: f64,
    max_decode_skips: u32,
) -> bool {
    if clip.decode_error_skips > max_decode_skips {
        return false;
    }
    let rate = clip.sample_rate.max(1);
    let expected = ((segment_length.as_secs_f64() * f64::from(rate))
        .floor()
        .max(1.0)) as usize;
    if expected == 0 {
        return false;
    }
    let decoded = clip.effective_decoded_sample_count();
    let threshold = min_decode_fraction.clamp(0.0, 1.0);
    (decoded as f64) >= (expected as f64) * threshold
}

/// Whether an end-clip extract is too incomplete or corrupt for alignment.
pub fn end_clip_extract_unreliable(
    clip: &MonoPcmClip,
    window: &ClipWindow,
    min_decode_fraction: f64,
    max_decode_skips: u32,
) -> bool {
    if window.label != ClipLabel::End {
        return false;
    }
    if clip.decode_error_skips > max_decode_skips {
        return true;
    }
    let rate = clip.sample_rate.max(1);
    let expected = window.sample_count_at(rate);
    if expected == 0 {
        return true;
    }
    let decoded = clip.effective_decoded_sample_count();
    let threshold = min_decode_fraction.clamp(0.0, 1.0);
    (decoded as f64) < (expected as f64) * threshold
}

fn secs_to_duration(secs: f64) -> Duration {
    Duration::from_secs_f64(secs.max(0.0))
}

/// Pick a hold-out window on the shorter file's timeline, avoiding discovery windows when possible.
pub fn pick_holdout_window(
    duration: Duration,
    discovery_windows: &[ClipWindow],
    segment_length: Duration,
) -> Option<ClipWindow> {
    if duration < segment_length {
        return None;
    }

    let segment_secs = segment_length.as_secs_f64();
    let duration_secs = duration.as_secs_f64();

    if discovery_windows.len() <= 1 {
        let start_secs = duration_secs / 3.0;
        let end_secs = (start_secs + segment_secs).min(duration_secs);
        if end_secs - start_secs < segment_secs {
            return None;
        }
        return Some(ClipWindow::new(
            secs_to_duration(start_secs),
            secs_to_duration(end_secs),
            ClipLabel::Interior,
        ));
    }

    let gap_start = discovery_windows.first()?.end.as_secs_f64();
    let gap_end = discovery_windows.last()?.start.as_secs_f64();
    let start_secs = if gap_end - gap_start >= segment_secs {
        gap_start + (gap_end - gap_start - segment_secs) / 2.0
    } else {
        (duration_secs - segment_secs) / 2.0
    };
    let end_secs = start_secs + segment_secs;
    if end_secs > duration_secs {
        return None;
    }

    Some(ClipWindow::new(
        secs_to_duration(start_secs),
        secs_to_duration(end_secs),
        ClipLabel::Interior,
    ))
}

/// Hold-out placement candidates, most preferred first. Includes fallbacks for inflated duration metadata.
pub fn holdout_window_candidates(
    duration: Duration,
    discovery_windows: &[ClipWindow],
    segment_length: Duration,
    offset_secs: f64,
) -> Vec<ClipWindow> {
    if duration < segment_length {
        return Vec::new();
    }

    let duration_secs = duration.as_secs_f64();
    let segment_secs = segment_length.as_secs_f64();
    let mut candidates = Vec::new();

    let mut push_unique = |window: ClipWindow| {
        if window.end <= window.start {
            return;
        }
        if candidates
            .iter()
            .any(|existing: &ClipWindow| {
                (existing.start.as_secs_f64() - window.start.as_secs_f64()).abs() < 0.001
            })
        {
            return;
        }
        candidates.push(window);
    };

    // Overlap-safe near-start windows first (avoids broken mid-file seeks on some MKV tracks).
    let min_a_start = (-offset_secs).max(0.0);
    if min_a_start + segment_secs <= duration_secs {
        push_unique(ClipWindow::new(
            secs_to_duration(min_a_start),
            secs_to_duration(min_a_start + segment_secs),
            ClipLabel::Interior,
        ));
    }
    let overlap_interior = min_a_start + 30.0;
    if overlap_interior + segment_secs <= duration_secs && overlap_interior > min_a_start + segment_secs {
        push_unique(ClipWindow::new(
            secs_to_duration(overlap_interior),
            secs_to_duration(overlap_interior + segment_secs),
            ClipLabel::Interior,
        ));
    }

    if let Some(window) = pick_holdout_window(duration, discovery_windows, segment_length) {
        push_unique(window);
    }

    let early_start = duration_secs / 6.0;
    if early_start + segment_secs <= duration_secs {
        push_unique(ClipWindow::new(
            secs_to_duration(early_start),
            secs_to_duration(early_start + segment_secs),
            ClipLabel::Interior,
        ));
    }

    if let Some(first) = discovery_windows.first() {
        let after_discovery = first.end.as_secs_f64();
        if after_discovery + segment_secs <= duration_secs {
            push_unique(ClipWindow::new(
                secs_to_duration(after_discovery),
                secs_to_duration(after_discovery + segment_secs),
                ClipLabel::Interior,
            ));
        }
    }

    if segment_secs <= duration_secs {
        push_unique(ClipWindow::new(
            Duration::ZERO,
            secs_to_duration(segment_secs),
            ClipLabel::Interior,
        ));
    }

    candidates
}

/// Calendar-parallel hold-out windows (same `[T, T+L)` on A and B) for periodic offset recheck.
/// Edge `T = 0` is listed first.
pub fn parallel_holdout_window_candidates(
    duration_a: Duration,
    duration_b: Duration,
    segment_length: Duration,
) -> Vec<ClipWindow> {
    let segment_secs = segment_length.as_secs_f64();
    if segment_secs <= 0.0 {
        return Vec::new();
    }

    let max_start = duration_a
        .as_secs_f64()
        .min(duration_b.as_secs_f64())
        - segment_secs;
    if max_start < 0.0 {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut push_unique = |start_secs: f64| {
        if start_secs < 0.0 || start_secs > max_start + 0.001 {
            return;
        }
        if candidates.iter().any(|w: &ClipWindow| {
            (w.start.as_secs_f64() - start_secs).abs() < 0.001
        }) {
            return;
        }
        candidates.push(ClipWindow::new(
            secs_to_duration(start_secs),
            secs_to_duration(start_secs + segment_secs),
            ClipLabel::Interior,
        ));
    };

    push_unique(0.0);
    if max_start > 0.0 {
        push_unique(max_start);
    }

    candidates
}

/// Short hold-out segment centered inside a discovery clip window on A's timeline.
pub fn holdout_window_centered_in(
    window: &ClipWindow,
    segment_length: Duration,
) -> Option<ClipWindow> {
    let segment_secs = segment_length.as_secs_f64();
    if segment_secs <= 0.0 {
        return None;
    }
    let win_start = window.start.as_secs_f64();
    let win_end = window.end.as_secs_f64();
    if win_end - win_start < segment_secs {
        return None;
    }
    let center = (win_start + win_end) / 2.0;
    let start_secs = (center - segment_secs / 2.0).clamp(win_start, win_end - segment_secs);
    Some(ClipWindow::new(
        secs_to_duration(start_secs),
        secs_to_duration(start_secs + segment_secs),
        window.label,
    ))
}

/// Hold-out candidates inside one discovery clip window, overlap-safe positions first.
///
/// Prefer near the window start (and overlap floor) before mid-window seeks that often fail
/// on MKV timestamp boundaries — same policy as [`holdout_window_candidates`].
pub fn anchor_holdout_candidates(
    discovery_window: &ClipWindow,
    segment_length: Duration,
    prior_offset_secs: f64,
    duration_a_secs: f64,
    duration_b_secs: f64,
) -> Vec<ClipWindow> {
    let segment_secs = segment_length.as_secs_f64();
    if segment_secs <= 0.0 {
        return Vec::new();
    }
    let win_start = discovery_window.start.as_secs_f64();
    let win_end = discovery_window.end.as_secs_f64();
    if win_end - win_start < segment_secs {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut push = |start_secs: f64| {
        if start_secs < win_start - 0.001 || start_secs + segment_secs > win_end + 0.001 {
            return;
        }
        if !holdout_window_feasible(
            start_secs,
            segment_secs,
            prior_offset_secs,
            duration_a_secs,
            duration_b_secs,
        ) {
            return;
        }
        if candidates.iter().any(|window: &ClipWindow| {
            (window.start.as_secs_f64() - start_secs).abs() < 0.001
        }) {
            return;
        }
        candidates.push(ClipWindow::new(
            secs_to_duration(start_secs),
            secs_to_duration(start_secs + segment_secs),
            discovery_window.label,
        ));
    };

    let overlap_floor = (-prior_offset_secs).max(win_start);
    push(overlap_floor);

    let early = overlap_floor + 30.0;
    if early + segment_secs <= win_end {
        push(early);
    }

    push(win_start);

    if let Some(centered) = holdout_window_centered_in(discovery_window, segment_length) {
        push(centered.start.as_secs_f64());
    }

    push(win_end - segment_secs);

    candidates
}

pub fn holdout_window_feasible(
    window_start_secs: f64,
    segment_length_secs: f64,
    offset_secs: f64,
    duration_a_secs: f64,
    duration_b_secs: f64,
) -> bool {
    window_start_secs >= 0.0
        && window_start_secs + segment_length_secs <= duration_a_secs
        && window_start_secs + offset_secs >= 0.0
        && window_start_secs + segment_length_secs + offset_secs <= duration_b_secs
}

/// Map an A-timeline hold-out to the corresponding B window.
///
/// Returns `None` when the mapped range is negative or empty — callers must skip the candidate
/// instead of [`Duration::from_secs_f64`](Duration::from_secs_f64), which panics on negatives.
pub fn holdout_b_window_for_offset(
    holdout_on_a: &ClipWindow,
    segment_length: Duration,
    offset_secs: f64,
) -> Option<ClipWindow> {
    let segment_secs = segment_length.as_secs_f64();
    if segment_secs <= 0.0 {
        return None;
    }
    let b_start_secs = holdout_on_a.start.as_secs_f64() + offset_secs;
    let b_end_secs = b_start_secs + segment_secs;
    if b_start_secs < 0.0 || b_end_secs <= b_start_secs {
        return None;
    }
    Some(ClipWindow::new(
        secs_to_duration(b_start_secs),
        secs_to_duration(b_end_secs),
        holdout_on_a.label,
    ))
}

/// Timeline length used to decide whether a hold-out segment fits (symmetric vs query mode).
pub fn holdout_pick_duration(
    result: &AlignmentResult,
    extent_a: MediaExtent,
    extent_b: MediaExtent,
) -> Duration {
    if result.alignment_mode_used == Some(AlignmentModeUsed::QueryReference) {
        if let Some(loc) = &result.query_localization {
            if loc.skip_reason.is_none() {
                return Duration::from_secs_f64(loc.mapped_region.shared_length_secs.max(0.0));
            }
        }
    }
    extent_a.effective().min(extent_b.effective())
}

/// Hold-out candidates confined to a query-reference mapped region on A (absolute A time).
///
/// Discovery windows are rebased into `[0, shared_length)` on A, candidates are picked in
/// region-relative coordinates (offset `0` for placement), then shifted back to absolute A time.
/// Callers still validate each window with [`holdout_window_feasible`] using the global offset.
pub fn mapped_region_holdout_candidates(
    mapped: &TimelineOverlap,
    discovery_windows: &[ClipWindow],
    segment_length: Duration,
) -> Vec<ClipWindow> {
    let region_a_start = mapped.video_a_start_secs;
    let region_a_len_secs = mapped.shared_length_secs;
    let segment_secs = segment_length.as_secs_f64();
    if region_a_len_secs < segment_secs || segment_secs <= 0.0 {
        return Vec::new();
    }
    let region_duration = Duration::from_secs_f64(region_a_len_secs.max(0.0));

    let rebased: Vec<ClipWindow> = discovery_windows
        .iter()
        .map(|window| rebase_clip_window_to_region(*window, region_a_start, region_a_len_secs))
        .collect();

    let relative =
        holdout_window_candidates(region_duration, &rebased, segment_length, 0.0);

    relative
        .into_iter()
        .map(|window| shift_clip_window_on_a(window, region_a_start))
        .collect()
}

/// Hold-out placement for symmetric alignment or query-reference mapped-region mode.
pub fn resolve_holdout_candidates(
    result: &AlignmentResult,
    extent_a: MediaExtent,
    extent_b: MediaExtent,
    discovery_windows: &[ClipWindow],
    segment_length: Duration,
    offset_secs: f64,
) -> Vec<ClipWindow> {
    if result.alignment_mode_used == Some(AlignmentModeUsed::QueryReference) {
        if let Some(loc) = &result.query_localization {
            if loc.skip_reason.is_none() {
                return mapped_region_holdout_candidates(
                    &loc.mapped_region,
                    discovery_windows,
                    segment_length,
                );
            }
        }
    }

    let pick_duration = extent_a.effective().min(extent_b.effective());
    holdout_window_candidates(pick_duration, discovery_windows, segment_length, offset_secs)
}

fn rebase_clip_window_to_region(
    window: ClipWindow,
    region_a_start: f64,
    region_a_len: f64,
) -> ClipWindow {
    let rel_start = (window.start.as_secs_f64() - region_a_start).clamp(0.0, region_a_len);
    let rel_end = (window.end.as_secs_f64() - region_a_start).clamp(rel_start, region_a_len);
    ClipWindow::new(
        secs_to_duration(rel_start),
        secs_to_duration(rel_end),
        window.label,
    )
}

fn shift_clip_window_on_a(window: ClipWindow, region_a_start: f64) -> ClipWindow {
    let shift = secs_to_duration(region_a_start);
    ClipWindow::new(window.start + shift, window.end + shift, window.label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::audio_track::AudioTrack;

    fn mins(m: u64) -> Duration {
        Duration::from_secs(m * 60)
    }

    fn track(index: u32, channels: u16, decodable: bool) -> AudioTrack {
        AudioTrack {
            index,
            codec: "aac".into(),
            channels,
            sample_rate: 48_000,

            duration: Some(mins(60)),
            decodable,
            bit_depth: None,
        }
    }

    #[test]
    fn select_track_for_reference_picks_channel_match_over_first_decodable() {
        let a = track(0, 6, true);
        let tracks = vec![track(0, 2, true), track(1, 6, true)];
        assert_eq!(select_track_for_reference(&a, &tracks).unwrap().index, 1);
    }

    #[test]
    fn select_track_for_reference_falls_back_to_first_decodable_when_no_match() {
        let a = track(0, 6, true);
        let tracks = vec![track(0, 2, true), track(1, 2, true)];
        assert_eq!(select_track_for_reference(&a, &tracks).unwrap().index, 0);
    }

    #[test]
    fn select_track_for_reference_ignores_undecodable_channel_match() {
        let a = track(0, 6, true);
        let tracks = vec![track(0, 6, false), track(1, 2, true)];
        assert_eq!(select_track_for_reference(&a, &tracks).unwrap().index, 1);
    }

    #[test]
    fn select_track_for_reference_mono_a_unchanged() {
        let a = track(0, 1, true);
        let tracks = vec![track(0, 1, true), track(1, 6, true)];
        assert_eq!(select_track_for_reference(&a, &tracks).unwrap().index, 0);
    }

    #[test]
    fn select_track_for_reference_errors_when_empty() {
        let a = track(0, 2, true);
        assert_eq!(
            select_track_for_reference(&a, &[]),
            Err(DomainError::NoAudioTracks)
        );
    }

    #[test]
    fn select_track_for_reference_errors_when_none_decodable() {
        let a = track(0, 6, true);
        let tracks = vec![track(0, 6, false), track(1, 2, false)];
        assert_eq!(
            select_track_for_reference(&a, &tracks),
            Err(DomainError::NoDecodableAudioTracks)
        );
    }

    #[test]
    fn order_track_pairs_for_alignment_prefers_channel_matched_pairs() {
        let a6 = track(2, 6, true);
        let b2 = track(1, 2, true);
        let b6 = track(2, 6, true);
        let pairs = order_track_pairs_for_alignment(&[&a6], &[&b2, &b6]);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], (&a6, &b6));
        assert_eq!(pairs[1], (&a6, &b2));
    }

    #[test]
    fn select_best_track_prefers_first_decodable_in_container_order() {
        let tracks = vec![
            AudioTrack {
                index: 0,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 44_100,

                duration: Some(mins(60)),
                decodable: true,
                bit_depth: None,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: true,
                bit_depth: None,
            },
        ];

        assert_eq!(select_best_track(&tracks).unwrap().index, 0);
    }

    #[test]
    fn select_best_track_prefers_program_when_decoy_has_higher_sample_rate() {
        let tracks = vec![
            AudioTrack {
                index: 0,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 11_025,

                duration: Some(mins(60)),
                decodable: true,
                bit_depth: None,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: true,
                bit_depth: None,
            },
        ];

        assert_eq!(select_best_track(&tracks).unwrap().index, 0);
    }

    #[test]
    fn select_best_track_prefers_decodable_over_sample_rate() {
        let tracks = vec![
            AudioTrack {
                index: 0,
                codec: "ac3".into(),
                channels: 6,
                sample_rate: 44_100,

                duration: Some(mins(60)),
                decodable: true,
                bit_depth: None,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: false,
                bit_depth: None,
            },
        ];

        assert_eq!(select_best_track(&tracks).unwrap().index, 0);
    }

    #[test]
    fn select_best_track_errors_when_none_are_decodable() {
        let tracks = vec![AudioTrack {
            index: 2,
            codec: "aac".into(),
            channels: 6,
            sample_rate: 48_000,

            duration: Some(mins(60)),
            decodable: false,
            bit_depth: None,
        }];

        assert_eq!(
            select_best_track(&tracks),
            Err(DomainError::NoDecodableAudioTracks)
        );
    }

    #[test]
    fn select_best_track_prefers_first_track_when_sample_rates_match() {
        let tracks = vec![
            AudioTrack {
                index: 0,
                codec: "aac".into(),
                channels: 6,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: true,
                bit_depth: None,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: true,
                bit_depth: None,
            },
        ];

        assert_eq!(select_best_track(&tracks).unwrap().index, 0);
    }

    #[test]
    fn select_best_track_skips_undecodable_leading_tracks() {
        let tracks = vec![
            AudioTrack {
                index: 0,
                codec: "ac3".into(),
                channels: 6,
                sample_rate: 48_000,

                duration: Some(mins(60)),
                decodable: false,
                bit_depth: None,
            },
            AudioTrack {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                sample_rate: 44_100,

                duration: Some(mins(60)),
                decodable: true,
                bit_depth: None,
            },
        ];

        assert_eq!(select_best_track(&tracks).unwrap().index, 1);
    }

    #[test]
    fn clip_windows_short_media_single_start_clip() {
        let plan = ClipPlan::new(mins(15), 2);
        let windows =
            clip_windows_with_options(&MediaExtent::from_declared(mins(12)), &plan, ClipPlanningOptions::default()).unwrap();

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].start, Duration::ZERO);
        assert_eq!(windows[0].end, mins(12));
        assert_eq!(windows[0].label, ClipLabel::Start);
    }

    #[test]
    fn clip_windows_two_clips_start_and_end() {
        let plan = ClipPlan::new(mins(15), 2);
        let windows =
            clip_windows_with_options(&MediaExtent::from_declared(mins(45)), &plan, ClipPlanningOptions::default()).unwrap();

        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].start, Duration::ZERO);
        assert_eq!(windows[0].end, mins(15));
        assert_eq!(windows[1].start, mins(30));
        assert_eq!(windows[1].end, mins(45));
    }

    #[test]
    fn clip_windows_num_clips_one_on_long_media() {
        let plan = ClipPlan::new(mins(15), 1);
        let windows = clip_windows_with_options(
            &MediaExtent::from_declared(mins(60)),
            &plan,
            ClipPlanningOptions::default(),
        )
        .unwrap();

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].end, mins(15));
    }

    #[test]
    fn clip_windows_three_clips_with_interior() {
        let plan = ClipPlan::new(mins(10), 3);
        let windows = clip_windows_with_options(
            &MediaExtent::from_declared(mins(60)),
            &plan,
            ClipPlanningOptions::default(),
        )
        .unwrap();

        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].label, ClipLabel::Start);
        assert_eq!(windows[1].label, ClipLabel::Interior);
        assert_eq!(windows[2].label, ClipLabel::End);
        assert_eq!(windows[0].start, Duration::ZERO);
        assert_eq!(windows[0].end, mins(10));
        assert_eq!(windows[1].start, mins(25));
        assert_eq!(windows[1].end, mins(35));
        assert_eq!(windows[2].start, mins(50));
        assert_eq!(windows[2].end, mins(60));
    }

    #[test]
    fn clip_windows_clamps_end_to_decodable_extent_with_inset() {
        let plan = ClipPlan::new(mins(15), 2);
        let container = Duration::from_secs_f64(6180.033);
        let extent = Duration::from_secs_f64(6176.0);
        let windows = clip_windows_with_options(
            &MediaExtent::new(container, Some(extent)),
            &plan,
            ClipPlanningOptions {
                end_tail_inset: Duration::from_secs(1),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(windows.len(), 2);
        let end = &windows[1];
        assert_eq!(end.label, ClipLabel::End);
        assert!((end.end.as_secs_f64() - 6175.0).abs() < 0.01);
        assert!((end.start.as_secs_f64() - (6175.0 - 900.0)).abs() < 0.01);
    }

    #[test]
    fn end_clip_extract_unreliable_when_tail_padding_exceeds_threshold() {
        let window = ClipWindow::new(Duration::from_secs(5280), Duration::from_secs(6180), ClipLabel::End);
        let expected = window.sample_count_at(48_000);
        let decoded = (expected as f64 * 0.94) as usize;
        let clip = MonoPcmClip {
            sample_rate: 48_000,
            samples: vec![0; expected],
            decode_error_skips: 0,
            decoded_sample_count: Some(decoded),
        };
        assert!(end_clip_extract_unreliable(&clip, &window, 0.95, 8));
    }

    #[test]
    fn truncate_padded_tail_removes_synthetic_silence() {
        let clip = MonoPcmClip {
            sample_rate: 48_000,
            samples: vec![1; 100],
            decode_error_skips: 0,
            decoded_sample_count: Some(80),
        };
        let trimmed = truncate_padded_tail(clip);
        assert_eq!(trimmed.samples.len(), 80);
        assert!(trimmed.decoded_sample_count.is_none());
    }

    #[test]
    fn clip_windows_rejects_zero_duration() {
        let plan = ClipPlan::new(mins(15), 2);
        assert_eq!(
            clip_windows_with_options(
                &MediaExtent::from_declared(Duration::ZERO),
                &plan,
                ClipPlanningOptions::default(),
            ),
            Err(DomainError::InvalidDuration)
        );
    }

    #[test]
    fn pick_holdout_window_middle_for_single_clip() {
        let discovery = vec![ClipWindow::new(Duration::ZERO, Duration::from_secs(15), ClipLabel::Start)];
        let window =
            pick_holdout_window(Duration::from_secs(60), &discovery, Duration::from_secs(3))
                .unwrap();
        assert_eq!(window.start, Duration::from_secs(20));
        assert_eq!(window.end, Duration::from_secs(23));
    }

    #[test]
    fn pick_holdout_window_fits_two_clip_gap() {
        let discovery = vec![
            ClipWindow::new(Duration::ZERO, Duration::from_secs(15), ClipLabel::Start),
            ClipWindow::new(Duration::from_secs(45), Duration::from_secs(60), ClipLabel::End),
        ];
        let window =
            pick_holdout_window(Duration::from_secs(60), &discovery, Duration::from_secs(3))
                .unwrap();
        assert!((window.start.as_secs_f64() - 28.5).abs() < 0.001);
        assert!((window.end.as_secs_f64() - 31.5).abs() < 0.001);
    }

    #[test]
    fn pick_holdout_window_none_when_shorter_than_segment() {
        let discovery = vec![ClipWindow::new(Duration::ZERO, Duration::from_secs(2), ClipLabel::Start)];
        assert!(pick_holdout_window(Duration::from_secs(2), &discovery, Duration::from_secs(3)).is_none());
    }

    #[test]
    fn holdout_window_feasible_respects_offset() {
        assert!(holdout_window_feasible(10.0, 3.0, 3.0, 120.0, 120.0));
        assert!(!holdout_window_feasible(10.0, 3.0, 3.0, 120.0, 12.0));
    }

    #[test]
    fn holdout_b_window_for_offset_rejects_negative_b_start() {
        let holdout = ClipWindow::new(
            Duration::from_secs_f64(8422.029),
            Duration::from_secs_f64(8425.029),
            ClipLabel::End,
        );
        assert!(holdout_b_window_for_offset(&holdout, Duration::from_secs(3), -8423.0).is_none());
        let mapped =
            holdout_b_window_for_offset(&holdout, Duration::from_secs(3), -100.0).expect("mapped");
        assert!((mapped.start.as_secs_f64() - 8322.029).abs() < 0.001);
    }

    #[test]
    fn holdout_extract_sufficient_requires_full_segment_decode() {
        let segment = Duration::from_secs(60);
        let full = MonoPcmClip {
            sample_rate: 11_025,
            samples: vec![0_i16; 11_025 * 60],
            decode_error_skips: 0,
            decoded_sample_count: Some(11_025 * 60),
        };
        assert!(holdout_extract_sufficient(&full, segment, 0.95, 8));

        let short = MonoPcmClip {
            sample_rate: 11_025,
            samples: vec![0_i16; 11_025 * 30],
            decode_error_skips: 0,
            decoded_sample_count: Some(11_025 * 30),
        };
        assert!(!holdout_extract_sufficient(&short, segment, 0.95, 8));
    }

    #[test]
    fn holdout_window_centered_in_discovery_clip() {
        let window = ClipWindow::new(
            Duration::from_secs(100),
            Duration::from_secs(1000),
            ClipLabel::End,
        );
        let holdout = holdout_window_centered_in(&window, Duration::from_secs(3))
            .expect("hold-out should fit");
        assert!((holdout.start.as_secs_f64() - 548.5).abs() < 0.001);
        assert!((holdout.end.as_secs_f64() - 551.5).abs() < 0.001);
        assert_eq!(holdout.label, ClipLabel::End);
    }

    #[test]
    fn anchor_holdout_candidates_prefer_overlap_safe_window_start() {
        let start = ClipWindow::new(Duration::ZERO, Duration::from_secs(900), ClipLabel::Start);
        let candidates = anchor_holdout_candidates(
            &start,
            Duration::from_secs(3),
            -7.326,
            7_547.0,
            7_547.0,
        );
        assert!(!candidates.is_empty());
        let first = candidates[0].start.as_secs_f64();
        assert!(
            first < 50.0,
            "first candidate should be near window start, got {first}"
        );
        assert!(
            !candidates.iter().any(|window| {
                (window.start.as_secs_f64() - 448.5).abs() < 1.0
            }) || candidates[0].start.as_secs_f64() < 100.0,
            "centered mid-window seek should not be first choice"
        );

        let end = ClipWindow::new(
            Duration::from_secs(6_647),
            Duration::from_secs(7_547),
            ClipLabel::End,
        );
        let end_candidates = anchor_holdout_candidates(
            &end,
            Duration::from_secs(3),
            -6.674,
            7_547.0,
            7_547.0,
        );
        assert!(!end_candidates.is_empty());
        assert!((end_candidates[0].start.as_secs_f64() - 6_647.0).abs() < 1.0);
    }

    #[test]
    fn high_rate_refine_skips_when_window_infeasible() {
        let discovery = vec![ClipWindow::new(
            Duration::ZERO,
            Duration::from_secs(60),
            ClipLabel::Start,
        )];
        let candidates = holdout_window_candidates(
            Duration::from_secs(60),
            &discovery,
            Duration::from_secs(3),
            3.0,
        );
        assert!(!candidates.is_empty());
        assert!(
            candidates.iter().all(|window| {
                !holdout_window_feasible(
                    window.start.as_secs_f64(),
                    3.0,
                    3.0,
                    120.0,
                    5.0,
                )
            }),
            "short B duration should make every candidate infeasible"
        );
    }

    #[test]
    fn holdout_candidates_include_early_fallback() {
        let discovery =
            vec![ClipWindow::new(Duration::ZERO, Duration::from_secs(600), ClipLabel::Start)];
        let candidates = holdout_window_candidates(
            Duration::from_secs(203),
            &discovery,
            Duration::from_secs(3),
            0.0,
        );
        assert!(candidates.len() >= 2);
        assert!(candidates.iter().any(|window| window.start.as_secs_f64() < 50.0));
    }

    #[test]
    fn holdout_candidates_prefer_overlap_safe_start_for_negative_offset() {
        let discovery =
            vec![ClipWindow::new(Duration::ZERO, Duration::from_secs(600), ClipLabel::Start)];
        let candidates = holdout_window_candidates(
            Duration::from_secs(600),
            &discovery,
            Duration::from_secs(3),
            -11.019,
        );
        let first = candidates.first().expect("candidate");
        assert!((first.start.as_secs_f64() - 11.019).abs() < 0.001);
    }

    #[test]
    fn mapped_region_holdout_a_long_negative_offset_stays_in_region() {
        use crate::domain::query_localization::compute_mapped_region;

        let mapped = compute_mapped_region(2700.0, 480.0, extent_secs(3600.0), extent_secs(480.0));
        let discovery = vec![ClipWindow::new(
            Duration::from_secs(2640),
            Duration::from_secs(3120),
            ClipLabel::Start,
        )];
        let segment = Duration::from_secs(3);
        let candidates =
            mapped_region_holdout_candidates(&mapped, &discovery, segment);
        assert!(!candidates.is_empty());
        let region_end = mapped.video_a_start_secs + mapped.shared_length_secs;
        for window in &candidates {
            let start = window.start.as_secs_f64();
            let end = window.end.as_secs_f64();
            assert!(
                start >= mapped.video_a_start_secs - 0.001 && end <= region_end + 0.001,
                "window [{start}, {end}] outside mapped A [{}, {region_end}]",
                mapped.video_a_start_secs
            );
            assert!(holdout_window_feasible(start, 3.0, -2700.0, 3600.0, 480.0));
        }
    }

    #[test]
    fn mapped_region_holdout_b_long_positive_offset_stays_in_region() {
        use crate::domain::query_localization::compute_mapped_region;

        let pseudo = compute_mapped_region(2700.0, 480.0, extent_secs(3600.0), extent_secs(480.0));
        let mapped = TimelineOverlap {
            video_a_start_secs: pseudo.video_b_start_secs,
            video_a_end_secs: pseudo.video_b_end_secs,
            video_b_start_secs: pseudo.video_a_start_secs,
            video_b_end_secs: pseudo.video_a_end_secs,
            shared_length_secs: pseudo.shared_length_secs,
        };
        let discovery = vec![ClipWindow::new(Duration::ZERO, Duration::from_secs(420), ClipLabel::Start)];
        let segment = Duration::from_secs(3);
        let candidates =
            mapped_region_holdout_candidates(&mapped, &discovery, segment);
        assert!(!candidates.is_empty());
        let region_end = mapped.shared_length_secs;
        for window in &candidates {
            let start = window.start.as_secs_f64();
            let end = window.end.as_secs_f64();
            assert!(
                start >= -0.001 && end <= region_end + 0.001,
                "window [{start}, {end}] outside mapped A [0, {region_end}]"
            );
            assert!(holdout_window_feasible(start, 3.0, 2700.0, 480.0, 3600.0));
        }
    }

    #[test]
    fn mapped_region_holdout_empty_when_region_shorter_than_segment() {
        let mapped = TimelineOverlap {
            video_a_start_secs: 100.0,
            video_a_end_secs: 102.0,
            video_b_start_secs: 0.0,
            video_b_end_secs: 2.0,
            shared_length_secs: 2.0,
        };
        let discovery = vec![ClipWindow::new(
            Duration::from_secs(100),
            Duration::from_secs(102),
            ClipLabel::Start,
        )];
        assert!(mapped_region_holdout_candidates(
            &mapped,
            &discovery,
            Duration::from_secs(3),
        )
        .is_empty());
    }

    fn paired_options(anchor: EndClipAnchor) -> ClipPlanningOptions {
        ClipPlanningOptions {
            end_clip_anchor: anchor,
            ..Default::default()
        }
    }

    fn assert_windows_match_single(
        extent: &MediaExtent,
        plan: &ClipPlan,
        options: ClipPlanningOptions,
        paired: &[ClipWindow],
    ) {
        let single = clip_windows_with_options(extent, plan, options).unwrap();
        assert_eq!(single.len(), paired.len());
        for (expected, actual) in single.iter().zip(paired) {
            assert_eq!(expected.start, actual.start);
            assert_eq!(expected.end, actual.end);
            assert_eq!(expected.label, actual.label);
        }
    }

    #[test]
    fn clip_windows_paired_equal_45_min_matches_single_file_planner() {
        let plan = ClipPlan::new(mins(15), 2);
        let extent = MediaExtent::from_declared(mins(45));
        let options = paired_options(EndClipAnchor::SharedTimeline);
        let (a, b) = clip_windows_paired(&extent, &extent, &plan, options).unwrap();
        assert_eq!(a, b);
        assert_windows_match_single(&extent, &plan, options, &a);
    }

    #[test]
    fn clip_windows_paired_equal_45_min_three_clips_matches_single_file() {
        let plan = ClipPlan::new(mins(10), 3);
        let extent = MediaExtent::from_declared(mins(60));
        let options = paired_options(EndClipAnchor::SharedTimeline);
        let (a, b) = clip_windows_paired(&extent, &extent, &plan, options).unwrap();
        assert_eq!(a, b);
        assert_windows_match_single(&extent, &plan, options, &a);
    }

    #[test]
    fn clip_windows_paired_40_300_shared_timeline_end_on_both() {
        let plan = ClipPlan::new(mins(15), 2);
        let short = MediaExtent::from_declared(mins(40));
        let long = MediaExtent::from_declared(mins(300));
        let options = paired_options(EndClipAnchor::SharedTimeline);
        let (a, b) = clip_windows_paired(&short, &long, &plan, options).unwrap();
        assert_eq!(a.len(), b.len());
        assert_eq!(a[1].start, mins(25));
        assert_eq!(a[1].end, mins(40));
        assert_eq!(b[1].start, mins(25));
        assert_eq!(b[1].end, mins(40));
        assert_ne!(b[1].end, mins(300));
    }

    #[test]
    fn clip_windows_paired_40_300_file_tail_b_end_at_file_tail() {
        let plan = ClipPlan::new(mins(15), 2);
        let short = MediaExtent::from_declared(mins(40));
        let long = MediaExtent::from_declared(mins(300));
        let options = paired_options(EndClipAnchor::FileTail);
        let (a, b) = clip_windows_paired(&short, &long, &plan, options).unwrap();
        assert_eq!(a[1].start, mins(25));
        assert_eq!(a[1].end, mins(40));
        assert_eq!(b[1].start, mins(285));
        assert_eq!(b[1].end, mins(300));
    }

    #[test]
    fn clip_windows_paired_40_300_three_clips_shared_interior_and_end() {
        let plan = ClipPlan::new(mins(10), 3);
        let short = MediaExtent::from_declared(mins(40));
        let long = MediaExtent::from_declared(mins(300));
        let options = paired_options(EndClipAnchor::SharedTimeline);
        let (a, b) = clip_windows_paired(&short, &long, &plan, options).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 3);
        assert_eq!(a[1].label, ClipLabel::Interior);
        assert_eq!(a[1].start, mins(15));
        assert_eq!(a[1].end, mins(25));
        assert_eq!(a[2].start, mins(30));
        assert_eq!(a[2].end, mins(40));
    }

    #[test]
    fn clip_windows_paired_collapses_12_20_min_to_single_window() {
        let plan = ClipPlan::new(mins(15), 2);
        let a_extent = MediaExtent::from_declared(mins(12));
        let b_extent = MediaExtent::from_declared(mins(20));
        let options = paired_options(EndClipAnchor::SharedTimeline);
        let (a, b) = clip_windows_paired(&a_extent, &b_extent, &plan, options).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0].end, mins(12));
        assert_eq!(b[0].end, mins(12));
    }

    #[test]
    fn clip_windows_paired_end_tail_inset_before_anchor_min() {
        let plan = ClipPlan::new(mins(15), 2);
        let container = Duration::from_secs(300 * 60);
        let short_decodable = Duration::from_secs(40 * 60);
        let long_decodable = Duration::from_secs(300 * 60);
        let short = MediaExtent::new(container, Some(short_decodable));
        let long = MediaExtent::new(container, Some(long_decodable));
        let options = ClipPlanningOptions {
            end_tail_inset: Duration::from_secs(60),
            end_clip_anchor: EndClipAnchor::SharedTimeline,
        };
        let (a, b) = clip_windows_paired(&short, &long, &plan, options).unwrap();
        let t_anchor = short_decodable.saturating_sub(Duration::from_secs(60));
        assert_eq!(a[1].end, t_anchor);
        assert_eq!(b[1].end, t_anchor);
    }

    #[test]
    fn interior_windows_along_timeline_matches_three_clip_fixture() {
        let from_helper = interior_windows_along_timeline(3600.0, mins(10), 3);
        let plan = ClipPlan::new(mins(10), 3);
        let windows =
            clip_windows_with_options(&MediaExtent::from_declared(mins(60)), &plan, ClipPlanningOptions::default())
                .unwrap();
        assert_eq!(from_helper.len(), 1);
        assert_eq!(from_helper[0], windows[1]);
    }

    #[test]
    fn attach_symmetric_planning_report_metadata_sets_anchor_and_b_windows() {
        use crate::domain::alignment::{build_alignment_result, ClipMatchEstimate, ClipPairReportInput};

        let plan = ClipPlan::new(mins(15), 2);
        let short = MediaExtent::from_declared(mins(40));
        let long = MediaExtent::from_declared(mins(300));
        let windows = clip_windows_with_options(&short, &plan, ClipPlanningOptions::default()).unwrap();
        let estimates = windows
            .iter()
            .map(|_| ClipMatchEstimate {
                offset_secs: 12.0,
                confidence: 0.9,
            })
            .collect::<Vec<_>>();
        let mut result = build_alignment_result(
            ClipPairReportInput {
                windows: &windows,
                estimates: &estimates,
                decode_skips_a: &[],
                decode_skips_b: &[],
                duration_a: Some(short.declared),
                duration_b: Some(long.declared),
            },
            crate::domain::alignment::AlignmentMergePolicy {
                min_match_score: 0.5,
                prefer_start_clip: true,
                require_consistent_offsets: false,
            },
        );
        attach_symmetric_planning_report_metadata(
            &mut result,
            &short,
            &long,
            &plan,
            paired_options(EndClipAnchor::FileTail),
            2,
        );
        assert_eq!(result.end_clip_anchor, Some(EndClipAnchor::FileTail));
        let end = result
            .clips
            .iter()
            .find(|clip| clip.label == ClipLabel::End)
            .expect("end clip");
        assert_eq!(end.video_b_window_start_secs, Some(mins(285).as_secs_f64()));
        assert_eq!(end.video_b_window_end_secs, Some(mins(300).as_secs_f64()));
    }

    fn extent_secs(secs: f64) -> MediaExtent {
        MediaExtent::from_declared(Duration::from_secs_f64(secs))
    }
}
