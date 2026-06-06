use std::path::PathBuf;

use crate::application::ports::MediaSession;
use crate::application::config::AppConfig;
use crate::application::error::AppError;
use crate::application::ports::{Aligner, Fingerprinter, MediaReader, ProgressReporter};
use crate::domain::{
    clip_windows, select_best_track, AlignmentResult, ClipWindow, MediaSource,
};

pub struct AlignVideosRequest {
    pub video_a: PathBuf,
    pub video_b: PathBuf,
    pub config: AppConfig,
}

pub struct AlignVideosResponse {
    pub result: AlignmentResult,
}

pub struct AlignVideos<'a, MR, FP, AL, PR> {
    media_reader: &'a MR,
    fingerprinter: &'a FP,
    aligner: &'a AL,
    progress: &'a PR,
}

impl<'a, MR, FP, AL, PR> AlignVideos<'a, MR, FP, AL, PR>
where
    MR: MediaReader,
    FP: Fingerprinter,
    AL: Aligner,
    PR: ProgressReporter,
{
    pub fn new(
        media_reader: &'a MR,
        fingerprinter: &'a FP,
        aligner: &'a AL,
        progress: &'a PR,
    ) -> Self {
        Self {
            media_reader,
            fingerprinter,
            aligner,
            progress,
        }
    }

    pub fn execute(&self, request: AlignVideosRequest) -> Result<AlignVideosResponse, AppError> {
        request.config.validate()?;

        self.progress.phase(&format!(
            "clip-sync: aligning {} with {}",
            request.video_a.display(),
            request.video_b.display()
        ));

        let source_a = MediaSource::new(request.video_a);
        let source_b = MediaSource::new(request.video_b);
        let plan = request.config.clip.as_plan();

        self.progress.phase("Opening media");
        let session_a = self.media_reader.open(&source_a)?;
        let session_b = self.media_reader.open(&source_b)?;

        let clips_a = self.extract_clips(&session_a, &plan, "video A")?;
        let clips_b = self.extract_clips(&session_b, &plan, "video B")?;

        if clips_a.len() != clips_b.len() {
            return Err(AppError::Alignment(
                crate::application::error::AlignmentError::EngineFailed(
                    "clip count mismatch between inputs".into(),
                ),
            ));
        }

        self.progress.phase(&format!("Fingerprinting {} clips...", clips_a.len() * 2));
        let fingerprints_a: Vec<_> = clips_a
            .iter()
            .map(|clip| self.fingerprinter.fingerprint(clip))
            .collect::<Result<_, _>>()?;
        let fingerprints_b: Vec<_> = clips_b
            .iter()
            .map(|clip| self.fingerprinter.fingerprint(clip))
            .collect::<Result<_, _>>()?;

        self.progress.phase("Searching for match...");
        let mut per_clip_offsets = Vec::with_capacity(fingerprints_a.len());
        let mut segments = Vec::new();
        let mut best_confidence = 0.0_f32;

        for (index, (left, right)) in fingerprints_a.iter().zip(fingerprints_b.iter()).enumerate()
        {
            let result = self.aligner.find_offset(left, right)?;
            per_clip_offsets.push(result.offset_secs);
            best_confidence = best_confidence.max(result.confidence);
            segments.extend(result.segments);

            self.progress.phase(&format!(
                "Clip pair {} offset: {:+.3}s (confidence: {:.2})",
                index + 1,
                result.offset_secs,
                result.confidence
            ));
        }

        let offset_secs = choose_offset(
            &per_clip_offsets,
            request.config.alignment.prefer_start_clip,
        );

        if best_confidence < request.config.alignment.min_match_score {
            return Err(AppError::Alignment(
                crate::application::error::AlignmentError::NoMatch,
            ));
        }

        let result = AlignmentResult {
            offset_secs,
            confidence: best_confidence,
            per_clip_offsets,
            segments,
        };

        self.progress.phase(&format!(
            "Offset: {:+.3}s (confidence: {:.2})",
            result.offset_secs, result.confidence
        ));

        Ok(AlignVideosResponse { result })
    }

    fn extract_clips(
        &self,
        session: &MR::Session,
        plan: &crate::domain::ClipPlan,
        label: &str,
    ) -> Result<Vec<crate::domain::MonoPcmClip>, AppError> {
        let tracks = session.list_tracks()?;
        let track = select_best_track(&tracks)?;
        self.progress.phase(&format!(
            "Selected track {} ({} Hz, {} channel{})",
            track.index,
            track.sample_rate,
            track.channels,
            if track.channels == 1 { "" } else { "s" }
        ));

        let duration = session.duration()?;
        let windows = clip_windows(duration, plan)?;

        self.progress.phase(&format_clip_plan(label, &windows));

        let mut clips = Vec::with_capacity(windows.len());
        for (index, window) in windows.iter().enumerate() {
            let progress_label = format!("Extracting clip {}/{} ({label})", index + 1, windows.len());
            let clip = session.extract_mono(track, window, self.progress, &progress_label)?;
            clips.push(clip);
        }

        Ok(clips)
    }
}

fn choose_offset(offsets: &[f64], prefer_start_clip: bool) -> f64 {
    if offsets.is_empty() {
        return 0.0;
    }
    if prefer_start_clip {
        offsets[0]
    } else {
        *offsets.last().unwrap()
    }
}

fn format_clip_plan(label: &str, windows: &[ClipWindow]) -> String {
    let parts: Vec<String> = windows
        .iter()
        .map(|window| {
            format!(
                "[{}–{}] {}",
                format_duration(window.start),
                format_duration(window.end),
                clip_label_name(window.label)
            )
        })
        .collect();

    format!(
        "Clip plan for {label}: {} clip(s) — {}",
        windows.len(),
        parts.join(", ")
    )
}

fn clip_label_name(label: crate::domain::ClipLabel) -> &'static str {
    use crate::domain::ClipLabel;
    match label {
        ClipLabel::Start => "start",
        ClipLabel::Interior => "interior",
        ClipLabel::End => "end",
    }
}

fn format_duration(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}
