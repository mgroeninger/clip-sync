use std::io::ErrorKind;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::application::error::RepairError;
use crate::application::ports::{MediaMuxer, MuxOptions};

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

pub struct FfmpegMediaMuxer;

impl MediaMuxer for FfmpegMediaMuxer {
    fn mux_video_with_replaced_audio(
        &self,
        source_video: &Path,
        replacement_audio_wav: &Path,
        output: &Path,
        options: &MuxOptions,
    ) -> Result<(), RepairError> {
        let args = build_ffmpeg_mux_args(source_video, replacement_audio_wav, output, options);

        tracing::info!(
            source = %source_video.display(),
            audio = %replacement_audio_wav.display(),
            output = %output.display(),
            "muxing video with patched audio via ffmpeg"
        );

        let output_result = Command::new("ffmpeg")
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output();

        let output = match output_result {
            Ok(output) => output,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                return Err(RepairError::Mux("ffmpeg not found on PATH".into()));
            }
            Err(err) => {
                return Err(RepairError::Mux(format!("failed to run ffmpeg: {err}")));
            }
        };

        if output.status.success() {
            return Ok(());
        }

        let detail = trim_ffmpeg_stderr(&String::from_utf8_lossy(&output.stderr));
        Err(RepairError::Mux(detail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
}
