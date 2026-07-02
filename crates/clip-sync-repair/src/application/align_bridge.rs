//! Map clip-sync alignment DTOs to repair-domain [`ScanAlignment`].

use clip_sync::{AlignmentResult, AudioTimelineSkew as ClipAudioTimelineSkew, ClipLabel, TimelineOverlap};

use crate::domain::align::{
    AlignedClip, AudioTimelineSkew, ClipRole, ScanAlignment, TimelineOverlap as DomainOverlap,
};

pub fn scan_alignment_from_result(result: &AlignmentResult) -> ScanAlignment {
    ScanAlignment {
        clips: result.clips.iter().map(aligned_clip_from_match).collect(),
        start_aligned: result.start_aligned,
        end_aligned: result.end_aligned,
        recommended_offset_secs: result.recommended_offset_secs,
        offsets_consistent: result.offsets_consistent,
        offset_drift_secs: result.offset_drift_secs,
        start_overlap: result.start_overlap.map(timeline_overlap_from_clip_sync),
        query_reference_mode: result.query_localization.is_some(),
    }
}

pub fn audio_timeline_skew_from_clip_sync(skew: ClipAudioTimelineSkew) -> AudioTimelineSkew {
    AudioTimelineSkew {
        pts_secs: skew.pts_secs,
        sample_clock_secs: skew.sample_clock_secs,
        delta_secs: skew.delta_secs,
    }
}

fn aligned_clip_from_match(clip: &clip_sync::ClipMatch) -> AlignedClip {
    AlignedClip {
        role: clip_role_from_label(clip.label),
        window_start_secs: clip.window_start_secs,
        window_end_secs: clip.window_end_secs,
        aligned: clip.aligned,
        offset_secs: clip.offset_secs,
        confidence: clip.confidence,
        video_a_decode_skips: clip.video_a_decode_skips,
        video_b_decode_skips: clip.video_b_decode_skips,
        video_b_window_start_secs: clip.video_b_window_start_secs,
        video_b_window_end_secs: clip.video_b_window_end_secs,
    }
}

fn clip_role_from_label(label: ClipLabel) -> ClipRole {
    match label {
        ClipLabel::Start => ClipRole::Start,
        ClipLabel::Interior => ClipRole::Interior,
        ClipLabel::End => ClipRole::End,
    }
}

fn timeline_overlap_from_clip_sync(overlap: TimelineOverlap) -> DomainOverlap {
    DomainOverlap {
        video_a_start_secs: overlap.video_a_start_secs,
        video_a_end_secs: overlap.video_a_end_secs,
        video_b_start_secs: overlap.video_b_start_secs,
        video_b_end_secs: overlap.video_b_end_secs,
        shared_length_secs: overlap.shared_length_secs,
    }
}
