use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use clip_sync::{MultiChannelPcm, ProgressReporter};

use crate::application::error::RepairError;
use crate::application::ports::{MediaMuxer, MuxOptions};
use crate::domain::diagnostics::format_mux_duration_error;
use crate::domain::diagnostics::MUX_DURATION_ERROR_SECS;
use crate::infrastructure::pcm::{validate_pcm_layout, write_pcm_s16le};

/// Build ffmpeg argv for remuxing `source_video` with replacement audio from stdin (`pipe:0`).
pub fn build_ffmpeg_mux_args(
    source_video: &Path,
    output: &Path,
    options: &MuxOptions,
    sample_rate: u32,
    channels: u16,
) -> Vec<String> {
    let mut args = vec![
        "-y".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        source_video.display().to_string(),
        "-f".into(),
        "s16le".into(),
        "-ar".into(),
        sample_rate.to_string(),
        "-ac".into(),
        channels.to_string(),
        "-i".into(),
        "pipe:0".into(),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "1:a:0".into(),
        "-c:v".into(),
        options.video_codec.clone(),
        "-c:a".into(),
        options.audio_codec.clone(),
    ];

    if let Some(bitrate) = &options.audio_bitrate {
        args.push("-b:a".into());
        args.push(bitrate.clone());
    }

    args.push("-shortest".into());

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
    // Despite the "_ms" suffix, ffmpeg's -progress output emits microseconds here.
    line.strip_prefix("out_time_ms=")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|us| us / 1000)
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

fn pcm_duration_ms(pcm: &MultiChannelPcm) -> Option<u64> {
    let rate = u64::from(pcm.sample_rate);
    if rate == 0 {
        return None;
    }
    Some(pcm.frames() as u64 * 1000 / rate)
}

fn pcm_duration_secs(pcm: &MultiChannelPcm) -> Option<f64> {
    if pcm.sample_rate == 0 {
        return None;
    }
    Some(pcm.frames() as f64 / f64::from(pcm.sample_rate))
}

fn validate_mux_duration(pcm: &MultiChannelPcm, source_video: &Path) -> Result<(), RepairError> {
    let pcm_secs = pcm_duration_secs(pcm).ok_or_else(|| {
        RepairError::Mux("patched audio has no decodable duration".into())
    })?;
    let video_secs = probe_media_duration_ms(source_video)
        .map(|ms| ms as f64 / 1000.0)
        .ok_or_else(|| RepairError::Mux("could not probe video duration via ffprobe".into()))?;
    if (pcm_secs - video_secs).abs() > MUX_DURATION_ERROR_SECS {
        return Err(RepairError::Mux(format_mux_duration_error(
            pcm_secs, video_secs,
        )));
    }
    Ok(())
}

fn mux_duration_ms(source_video: &Path, pcm: &MultiChannelPcm) -> Option<u64> {
    probe_media_duration_ms(source_video).or_else(|| pcm_duration_ms(pcm))
}

fn run_ffmpeg_mux_with_progress(
    args: &[String],
    pcm: &MultiChannelPcm,
    progress: &dyn ProgressReporter,
    duration_ms: Option<u64>,
) -> Result<(), RepairError> {
    validate_pcm_layout(pcm)?;

    let mut child = match Command::new("ffmpeg")
        .args(args)
        .stdin(Stdio::piped())
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

    let stdin = child.stdin.take().expect("stdin piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let stdout = child.stdout.take();

    let (pcm_tx, pcm_rx) = mpsc::channel();
    thread::scope(|scope| {
        scope.spawn(|| {
            let mut stdin = stdin;
            let result = write_pcm_s16le(&mut stdin, &pcm.samples).and_then(|()| stdin.flush());
            let _ = pcm_tx.send(result);
        });

        let stderr_handle = scope.spawn(move || {
            BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
                .collect::<Vec<_>>()
                .join("\n")
        });

        if let Some(total_ms) = duration_ms.filter(|ms| *ms > 0) {
            progress.progress("mux", 0, total_ms);
        }

        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            let mut last_reported_ms = 0u64;
            for line in reader.lines() {
                let line = match line {
                    Ok(line) => line,
                    Err(err) => {
                        let _ = pcm_rx.recv();
                        let _ = child.wait();
                        let stderr = stderr_handle.join().unwrap_or_default();
                        let detail = trim_ffmpeg_stderr(&stderr);
                        return Err(RepairError::Mux(format!(
                            "failed to read ffmpeg progress: {err}; ffmpeg: {detail}"
                        )));
                    }
                };
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

        let pcm_write_err = pcm_rx
            .recv()
            .map_err(|_| RepairError::Mux("PCM writer thread exited before reporting".into()))?;
        let status = child.wait().map_err(RepairError::Io)?;
        let stderr = stderr_handle.join().unwrap_or_default();
        let ffmpeg_suffix = {
            let detail = trim_ffmpeg_stderr(&stderr);
            if detail.is_empty() {
                String::new()
            } else {
                format!("; ffmpeg: {detail}")
            }
        };

        if let Err(err) = pcm_write_err {
            return Err(RepairError::Mux(format!(
                "failed to write replacement audio to ffmpeg stdin: {err}{ffmpeg_suffix}"
            )));
        }

        if status.success() {
            if let Some(total_ms) = duration_ms.filter(|ms| *ms > 0) {
                progress.progress("mux", total_ms, total_ms);
            }
            return Ok(());
        }

        Err(RepairError::Mux(trim_ffmpeg_stderr(&stderr)))
    })
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
        validate_pcm_layout(replacement_audio)?;
        validate_mux_duration(replacement_audio, source_video)?;

        let mut args = build_ffmpeg_mux_args(
            source_video,
            output,
            options,
            replacement_audio.sample_rate,
            replacement_audio.channels,
        );
        append_mux_progress_args(&mut args);

        tracing::debug!(
            source = %source_video.display(),
            output = %output.display(),
            sample_rate = replacement_audio.sample_rate,
            channels = replacement_audio.channels,
            frames = replacement_audio.frames(),
            "muxing video with patched audio via ffmpeg (stdin PCM)"
        );

        progress.phase("Muxing video with patched audio...");
        let duration_ms = mux_duration_ms(source_video, replacement_audio);
        run_ffmpeg_mux_with_progress(&args, replacement_audio, progress, duration_ms)
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
        let out = PathBuf::from("repaired.mp4");
        let options = MuxOptions {
            video_codec: "copy".into(),
            audio_codec: "aac".into(),
            audio_bitrate: Some("247k".into()),
        };

        let args = build_ffmpeg_mux_args(&source, &out, &options, 48_000, 6);

        assert_eq!(
            args,
            vec![
                "-y",
                "-loglevel",
                "error",
                "-i",
                "video_a.mp4",
                "-f",
                "s16le",
                "-ar",
                "48000",
                "-ac",
                "6",
                "-i",
                "pipe:0",
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "copy",
                "-c:a",
                "aac",
                "-b:a",
                "247k",
                "-shortest",
                "-movflags",
                "+faststart",
                "repaired.mp4",
            ]
        );
    }

    #[test]
    fn ffmpeg_arg_construction_omits_bitrate_when_unset() {
        let options = MuxOptions {
            video_codec: "copy".into(),
            audio_codec: "aac".into(),
            audio_bitrate: None,
        };
        let args = build_ffmpeg_mux_args(
            Path::new("a.mp4"),
            Path::new("out.mp4"),
            &options,
            48_000,
            2,
        );
        assert!(!args.contains(&"-b:a".to_string()));
    }

    #[test]
    fn ffmpeg_arg_construction_mkv_omits_faststart() {
        let options = MuxOptions {
            video_codec: "copy".into(),
            audio_codec: "flac".into(),
            audio_bitrate: None,
        };
        let args = build_ffmpeg_mux_args(
            Path::new("a.mkv"),
            Path::new("out.mkv"),
            &options,
            44_100,
            2,
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
        // ffmpeg emits microseconds; we convert to ms
        assert_eq!(parse_progress_out_time_ms("out_time_ms=12345000"), Some(12345));
        assert_eq!(parse_progress_out_time_ms("out_time_ms=500"), Some(0));
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
    fn validate_mux_duration_rejects_large_skew() {
        let pcm = clip_sync::MultiChannelPcm {
            sample_rate: 48_000,
            channels: 2,
            samples: vec![0i16; 48_000 * 100],
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
        };
        let err = super::validate_mux_duration(&pcm, Path::new("missing-probe.mp4"))
            .expect_err("should fail without ffprobe or on skew");
        assert!(err.to_string().contains("ffprobe") || err.to_string().contains("differ"));
    }

    #[test]
    fn pcm_duration_secs_uses_frame_count() {
        let pcm = clip_sync::MultiChannelPcm {
            sample_rate: 48_000,
            channels: 6,
            samples: vec![0i16; 48_000 * 6],
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
        };
        assert_eq!(pcm_duration_ms(&pcm), Some(1000));
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
            compressed_bytes: None,
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
                    audio_bitrate: None,
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
