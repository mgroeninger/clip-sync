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
use clip_sync::resolve_output_bit_depth;

use crate::infrastructure::pcm::{validate_pcm_layout, write_pcm_le};

/// Format a filesystem path for ffmpeg/ffprobe so it is always treated as a
/// local file — never as a URL/protocol (`concat:`, `http:`, …) — and so an
/// output name that starts with `-` is not parsed as an option.
///
/// On Windows, separators are normalized to `/` (the form ffmpeg's `file:`
/// protocol expects alongside drive letters, e.g. `file:C:/Videos/a.mp4`).
fn ffmpeg_path_arg(path: &Path) -> String {
    let mut s = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        s = s.replace('\\', "/");
    }
    format!("file:{s}")
}

/// Build ffmpeg argv for remuxing `source_video` with replacement audio from stdin (`pipe:0`).
pub fn build_ffmpeg_mux_args(
    source_video: &Path,
    output: &Path,
    options: &MuxOptions,
    sample_rate: u32,
    channels: u16,
    pcm_format: &str,
) -> Vec<String> {
    let mut args = vec![
        "-y".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        ffmpeg_path_arg(source_video),
        "-f".into(),
        pcm_format.into(),
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

    args.push(ffmpeg_path_arg(output));
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
        // ffmpeg stderr can contain multibyte characters (e.g. non-ASCII file
        // names); advance to a char boundary so the slice cannot panic.
        let mut start = message.len() - MAX_LEN;
        while !message.is_char_boundary(start) {
            start += 1;
        }
        format!("{}…", &message[start..])
    }
}

fn parse_progress_out_time_ms(line: &str) -> Option<u64> {
    // Despite the "_ms" suffix, ffmpeg's -progress output emits microseconds here.
    line.strip_prefix("out_time_ms=")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|us| us / 1000)
}

fn probe_media_duration_ms(path: &Path) -> Option<u64> {
    let file_arg = ffmpeg_path_arg(path);
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            &file_arg,
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

fn validate_mux_duration(pcm: &MultiChannelPcm, video_secs: f64) -> Result<(), RepairError> {
    let pcm_secs = pcm_duration_secs(pcm).ok_or_else(|| {
        RepairError::Mux("patched audio has no decodable duration".into())
    })?;
    if (pcm_secs - video_secs).abs() > MUX_DURATION_ERROR_SECS {
        return Err(RepairError::Mux(format_mux_duration_error(
            pcm_secs, video_secs,
        )));
    }
    Ok(())
}

fn mux_duration_ms_from_probe(video_ms: Option<u64>, pcm: &MultiChannelPcm) -> Option<u64> {
    video_ms.or_else(|| pcm_duration_ms(pcm))
}

fn run_ffmpeg_mux_with_progress(
    args: &[String],
    pcm: &MultiChannelPcm,
    depth: clip_sync::WavBitDepth,
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
            let result = write_pcm_le(&mut stdin, &pcm.samples, depth).and_then(|()| stdin.flush());
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

        let video_ms = probe_media_duration_ms(source_video).ok_or_else(|| {
            RepairError::Mux("could not probe video duration via ffprobe".into())
        })?;
        validate_mux_duration(replacement_audio, video_ms as f64 / 1000.0)?;

        // Mux to a sibling temp path, then rename on success so a failed run
        // never leaves a truncated `-y` output that looks like a good file.
        let parent = output.parent().filter(|p| !p.as_os_str().is_empty());
        let tmp = tempfile::Builder::new()
            .prefix("clip-sync-mux-")
            .suffix(".partial")
            .tempfile_in(parent.unwrap_or_else(|| Path::new(".")))
            .map_err(|err| RepairError::Mux(format!("failed to create mux temp file: {err}")))?;
        let tmp_path = tmp.path().to_path_buf();

        let depth = resolve_output_bit_depth(replacement_audio.source_bit_depth);
        let mut args = build_ffmpeg_mux_args(
            source_video,
            &tmp_path,
            options,
            replacement_audio.sample_rate,
            replacement_audio.channels,
            depth.ffmpeg_format(),
        );
        append_mux_progress_args(&mut args);

        tracing::debug!(
            source = %source_video.display(),
            output = %output.display(),
            temp = %tmp_path.display(),
            sample_rate = replacement_audio.sample_rate,
            channels = replacement_audio.channels,
            frames = replacement_audio.frames(),
            pcm_format = depth.ffmpeg_format(),
            "muxing video with patched audio via ffmpeg (stdin PCM)"
        );

        progress.phase("Muxing video with patched audio...");
        let duration_ms = mux_duration_ms_from_probe(Some(video_ms), replacement_audio);
        match run_ffmpeg_mux_with_progress(&args, replacement_audio, depth, progress, duration_ms)
        {
            Ok(()) => {
                // persist closes the handle then renames into place (Windows-safe).
                tmp.persist(output).map_err(|err| {
                    RepairError::Mux(format!(
                        "mux succeeded but failed to move temp output into place: {err}"
                    ))
                })?;
                Ok(())
            }
            Err(err) => {
                // NamedTempFile Drop removes the partial on scope exit; keep explicit
                // close so the error path is obvious.
                let _ = tmp.close();
                Err(err)
            }
        }
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

        let args = build_ffmpeg_mux_args(&source, &out, &options, 48_000, 6, "s16le");

        assert_eq!(
            args,
            vec![
                "-y",
                "-loglevel",
                "error",
                "-i",
                "file:video_a.mp4",
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
                "file:repaired.mp4",
            ]
        );
    }

    #[test]
    fn ffmpeg_path_arg_forces_file_protocol() {
        assert_eq!(ffmpeg_path_arg(Path::new("video.mp4")), "file:video.mp4");
        // Protocol-looking names must not reach ffmpeg as bare URLs.
        assert_eq!(
            ffmpeg_path_arg(Path::new("concat:a.mp4|b.mp4")),
            "file:concat:a.mp4|b.mp4"
        );
        assert_eq!(
            ffmpeg_path_arg(Path::new("http://example.com/x.mp4")),
            "file:http://example.com/x.mp4"
        );
        // Leading '-' must not be parsed as an ffmpeg option.
        assert_eq!(ffmpeg_path_arg(Path::new("-out.mp4")), "file:-out.mp4");
    }

    #[cfg(windows)]
    #[test]
    fn ffmpeg_path_arg_normalizes_windows_separators() {
        assert_eq!(
            ffmpeg_path_arg(Path::new(r"C:\Videos\a.mp4")),
            "file:C:/Videos/a.mp4"
        );
    }

    #[test]
    fn ffmpeg_arg_construction_s24le_uses_correct_format() {
        let source = PathBuf::from("video_a.mp4");
        let out = PathBuf::from("repaired.mp4");
        let options = MuxOptions {
            video_codec: "copy".into(),
            audio_codec: "aac".into(),
            audio_bitrate: None,
        };
        let args = build_ffmpeg_mux_args(&source, &out, &options, 48_000, 2, "s24le");
        let f_idx = args.iter().position(|a| a == "-f").expect("-f flag");
        assert_eq!(args[f_idx + 1], "s24le");
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
            "s16le",
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
            "s16le",
        );

        assert!(!args.contains(&"-movflags".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("file:out.mkv"));
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
    fn trim_ffmpeg_stderr_truncates_on_char_boundary() {
        // A long line of multibyte characters must not panic when the 500-byte
        // truncation point lands mid-character.
        let stderr = "é".repeat(600);
        let trimmed = trim_ffmpeg_stderr(&stderr);
        assert!(trimmed.ends_with('…'));
        assert!(trimmed.len() <= 500 + '…'.len_utf8());
    }

    #[test]
    fn validate_mux_duration_rejects_large_skew() {
        let pcm = clip_sync::MultiChannelPcm {
            sample_rate: 48_000,
            channels: 2,
            samples: vec![0.0f32; 48_000 * 100],
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };
        // 100s PCM vs 1s video → skew beyond MUX_DURATION_ERROR_SECS.
        let err = super::validate_mux_duration(&pcm, 1.0).expect_err("skew");
        assert!(err.to_string().contains("differ") || err.to_string().contains("duration"));
    }

    #[test]
    fn pcm_duration_secs_uses_frame_count() {
        let pcm = clip_sync::MultiChannelPcm {
            sample_rate: 48_000,
            channels: 6,
            samples: vec![0.0f32; 48_000 * 6],
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };
        assert_eq!(pcm_duration_ms(&pcm), Some(1000));
    }

    #[test]
    #[cfg(feature = "ffmpeg-mux")]
    #[ignore = "tier:diagnostic — needs ffmpeg and ffprobe on PATH; test-tier.ps1 -Tier diagnostic"]
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
        let samples: Vec<f32> = (0..(sample_rate * 2) as usize)
            .map(|i| f32::sin(i as f32 * 2.0 * std::f32::consts::PI * 880.0 / sample_rate as f32) * 0.488)
            .collect();
        let pcm = clip_sync::MultiChannelPcm {
            sample_rate,
            channels: 1,
            samples,
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
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
