use std::io::{BufRead, BufReader, ErrorKind};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use clip_sync::{MultiChannelPcm, ProgressReporter};

use crate::application::error::RepairError;
use crate::application::ports::{MediaMuxer, MuxOptions, PatchedAudioWriter};
use crate::infrastructure::wav_writer::WavPatchedAudioWriter;

/// Build ffmpeg argv for remuxing `source_video` with audio from `replacement_audio_wav`.
pub fn build_ffmpeg_mux_args(
    source_video: &Path,
    replacement_audio_wav: &Path,
    output: &Path,
    options: &MuxOptions,
) -> Vec<String> {
    let mut args = vec![
        "-y".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        source_video.display().to_string(),
        "-i".into(),
        replacement_audio_wav.display().to_string(),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "1:a:0".into(),
        "-c:v".into(),
        options.video_codec.clone(),
        "-c:a".into(),
        options.audio_codec.clone(),
        "-shortest".into(),
    ];

    if let Some(ext) = output.extension().and_then(|e| e.to_str()) {
        if matches!(ext.to_lowercase().as_str(), "mp4" | "m4v" | "mov") {
            args.extend(["-movflags".into(), "+faststart".into()]);
        }
    }

    args.push(output.display().to_string());
    args
}

fn append_mux_progress_args(args: &mut Vec<String>) {
    args.extend([
        "-nostdin".into(),
        "-nostats".into(),
        "-progress".into(),
        "pipe:1".into(),
    ]);
}

fn trim_ffmpeg_stderr(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    if lines.is_empty() {
        return "ffmpeg failed with no stderr output".into();
    }

    let tail: Vec<&str> = lines.into_iter().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect();
    let message = tail.join("; ");
    const MAX_LEN: usize = 500;
    if message.len() <= MAX_LEN {
        message
    } else {
        format!("{}…", &message[message.len() - MAX_LEN..])
    }
}

fn parse_progress_out_time_ms(line: &str) -> Option<u64> {
    line.strip_prefix("out_time_ms=")
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn probe_media_duration_ms(path: &Path) -> Option<u64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path.to_str()?,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let secs: f64 = String::from_utf8_lossy(&output.stdout).trim().parse().ok()?;
    if !secs.is_finite() || secs <= 0.0 {
        return None;
    }
    Some((secs * 1000.0).round() as u64)
}

fn probe_wav_duration_ms(path: &Path) -> Option<u64> {
    let reader = hound::WavReader::open(path).ok()?;
    let frames = u64::from(reader.duration());
    let rate = u64::from(reader.spec().sample_rate);
    if rate == 0 {
        return None;
    }
    Some(frames.saturating_mul(1000) / rate)
}

fn mux_duration_ms(source_video: &Path, replacement_audio_wav: &Path) -> Option<u64> {
    probe_media_duration_ms(source_video)
        .or_else(|| probe_media_duration_ms(replacement_audio_wav))
        .or_else(|| probe_wav_duration_ms(replacement_audio_wav))
}

fn run_ffmpeg_mux_with_progress(
    args: &[String],
    progress: &dyn ProgressReporter,
    duration_ms: Option<u64>,
) -> Result<(), RepairError> {
    let mut child = match Command::new("ffmpeg")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(RepairError::Mux("ffmpeg not found on PATH".into()));
        }
        Err(err) => {
            return Err(RepairError::Mux(format!("failed to run ffmpeg: {err}")));
        }
    };

    let stderr = child.stderr.take().expect("stderr piped");
    let stderr_handle = thread::spawn(move || {
        BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
            .collect::<Vec<_>>()
            .join("\n")
    });

    if let Some(total_ms) = duration_ms.filter(|ms| *ms > 0) {
        progress.progress("mux", 0, total_ms);
    }

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        let mut last_reported_ms = 0u64;
        for line in reader.lines() {
            let line = line.map_err(RepairError::Io)?;
            if let Some(ms) = parse_progress_out_time_ms(&line) {
                if let Some(total_ms) = duration_ms.filter(|total| *total > 0) {
                    if ms > last_reported_ms {
                        progress.progress("mux", ms.min(total_ms), total_ms);
                        last_reported_ms = ms;
                    }
                }
            }
            if line.trim() == "progress=end" {
                break;
            }
        }
    }

    let status = child.wait().map_err(RepairError::Io)?;
    let stderr = stderr_handle.join().unwrap_or_default();

    if status.success() {
        if let Some(total_ms) = duration_ms.filter(|ms| *ms > 0) {
            progress.progress("mux", total_ms, total_ms);
        }
        return Ok(());
    }

    let detail = trim_ffmpeg_stderr(&stderr);
    Err(RepairError::Mux(detail))
}

pub struct FfmpegMediaMuxer;

impl MediaMuxer for FfmpegMediaMuxer {
    fn mux_video_with_replaced_audio(
        &self,
        source_video: &Path,
        replacement_audio: &MultiChannelPcm,
        output: &Path,
        options: &MuxOptions,
        progress: &dyn ProgressReporter,
    ) -> Result<(), RepairError> {
        let temp = tempfile::NamedTempFile::new().map_err(RepairError::Io)?;
        WavPatchedAudioWriter.write(replacement_audio, temp.path())?;

        let mut args = build_ffmpeg_mux_args(source_video, temp.path(), output, options);
        append_mux_progress_args(&mut args);

        tracing::info!(
            source = %source_video.display(),
            output = %output.display(),
            "muxing video with patched audio via ffmpeg"
        );

        progress.phase("Muxing video with patched audio...");
        let duration_ms = mux_duration_ms(source_video, temp.path());
        run_ffmpeg_mux_with_progress(&args, progress, duration_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct RecordingProgress {
        last_label: std::cell::Cell<Option<String>>,
        last_current: std::cell::Cell<u64>,
        last_total: std::cell::Cell<u64>,
    }

    impl RecordingProgress {
        fn new() -> Self {
            Self {
                last_label: std::cell::Cell::new(None),
                last_current: std::cell::Cell::new(0),
                last_total: std::cell::Cell::new(0),
            }
        }
    }

    impl ProgressReporter for RecordingProgress {
        fn phase(&self, _message: &str) {}

        fn progress(&self, label: &str, current: u64, total: u64) {
            self.last_label.set(Some(label.to_string()));
            self.last_current.set(current);
            self.last_total.set(total);
        }
    }

    #[test]
    fn ffmpeg_arg_construction() {
        let source = PathBuf::from("video_a.mp4");
        let wav = PathBuf::from("patched.wav");
        let out = PathBuf::from("repaired.mp4");
        let options = MuxOptions {
            video_codec: "copy".into(),
            audio_codec: "aac".into(),
        };

        let args = build_ffmpeg_mux_args(&source, &wav, &out, &options);

        assert_eq!(
            args,
            vec![
                "-y",
                "-loglevel",
                "error",
                "-i",
                "video_a.mp4",
                "-i",
                "patched.wav",
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "copy",
                "-c:a",
                "aac",
                "-shortest",
                "-movflags",
                "+faststart",
                "repaired.mp4",
            ]
        );
    }

    #[test]
    fn ffmpeg_arg_construction_mkv_omits_faststart() {
        let options = MuxOptions {
            video_codec: "copy".into(),
            audio_codec: "flac".into(),
        };
        let args = build_ffmpeg_mux_args(
            Path::new("a.mkv"),
            Path::new("patched.wav"),
            Path::new("out.mkv"),
            &options,
        );

        assert!(!args.contains(&"-movflags".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("out.mkv"));
    }

    #[test]
    fn append_mux_progress_args_adds_pipe_progress() {
        let mut args = vec!["-y".into()];
        append_mux_progress_args(&mut args);
        assert_eq!(
            args,
            vec!["-y", "-nostdin", "-nostats", "-progress", "pipe:1"]
        );
    }

    #[test]
    fn parse_progress_out_time_ms_reads_ffmpeg_progress_line() {
        assert_eq!(parse_progress_out_time_ms("out_time_ms=12345"), Some(12345));
        assert_eq!(parse_progress_out_time_ms("progress=continue"), None);
    }

    #[test]
    fn trim_ffmpeg_stderr_keeps_tail_lines() {
        let stderr = "ffmpeg version 6.0\n\
Input #0, mp4\n\
Duration: 00:01:00\n\
Stream mapping:\n\
Press [q] to stop\n\
frame= 100 fps=0.0\n\
Error: no video stream\n";
        let trimmed = trim_ffmpeg_stderr(stderr);
        assert!(trimmed.contains("no video stream"));
        assert!(!trimmed.contains("ffmpeg version"));
    }

    #[test]
    #[cfg(feature = "ffmpeg-mux")]
    #[ignore = "requires ffmpeg and ffprobe on PATH"]
    fn mux_reports_progress_for_short_fixture() {
        let ffmpeg_ok = Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        if !ffmpeg_ok {
            return;
        }

        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source.mp4");
        let output = temp.path().join("out.mp4");

        let wrote = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=320x240:d=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                source.to_str().expect("source"),
            ])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        assert!(wrote, "failed to build source fixture");

        let sample_rate = 44_100u32;
        let samples: Vec<i16> = (0..(sample_rate * 2) as usize)
            .map(|i| (f32::sin(i as f32 * 2.0 * std::f32::consts::PI * 880.0 / sample_rate as f32) * 16_000.0) as i16)
            .collect();
        let pcm = clip_sync::MultiChannelPcm {
            sample_rate,
            channels: 1,
            samples,
            decode_error_skips: 0,
            decoded_frame_count: None,
        };

        let progress = RecordingProgress::new();
        FfmpegMediaMuxer
            .mux_video_with_replaced_audio(
                &source,
                &pcm,
                &output,
                &MuxOptions {
                    video_codec: "copy".into(),
                    audio_codec: "aac".into(),
                },
                &progress,
            )
            .expect("mux should succeed");

        assert_eq!(progress.last_label.take(), Some("mux".into()));
        assert!(progress.last_total.get() > 0);
        assert_eq!(progress.last_current.get(), progress.last_total.get());
        assert!(output.exists());
    }
}
