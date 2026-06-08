use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeFormat {
    Mp3,
    Mp3NoDurationTag,
    Mp4Aac,
    Mp4AacStereo,
    MkvFlac,
    #[cfg(feature = "he-aac")]
    Mp4HeAac,
}

pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn resample_wav(input: &Path, output: &Path, sample_rate: u32) -> bool {
    if !ffmpeg_available() {
        return false;
    }

    Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(input)
        .args(["-ar", &sample_rate.to_string()])
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn encode_audio(input_wav: &Path, output: &Path, format: EncodeFormat) -> bool {
    if !ffmpeg_available() {
        return false;
    }

    let mut command = Command::new("ffmpeg");
    command
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(input_wav);

    match format {
        EncodeFormat::Mp3 => {
            command.args(["-c:a", "libmp3lame", "-q:a", "4"]);
        }
        EncodeFormat::Mp3NoDurationTag => {
            command.args([
                "-write_xing",
                "0",
                "-id3v2_version",
                "0",
                "-c:a",
                "libmp3lame",
                "-b:a",
                "128k",
            ]);
        }
        EncodeFormat::Mp4Aac => {
            command.args(["-vn", "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"]);
        }
        EncodeFormat::Mp4AacStereo => {
            command.args([
                "-vn",
                "-ac",
                "2",
                "-c:a",
                "aac",
                "-b:a",
                "128k",
                "-movflags",
                "+faststart",
            ]);
        }
        EncodeFormat::MkvFlac => {
            command.args(["-c:a", "flac"]);
        }
        #[cfg(feature = "he-aac")]
        EncodeFormat::Mp4HeAac => {}
    }

    match format {
        EncodeFormat::Mp4Aac | EncodeFormat::Mp4AacStereo => {
            command.arg("-f").arg("mp4");
        }
        #[cfg(feature = "he-aac")]
        EncodeFormat::Mp4HeAac => {
            return encode_he_aac_mp4_from_wav(input_wav, output);
        }
        EncodeFormat::MkvFlac => {
            command.arg("-f").arg("matroska");
        }
        EncodeFormat::Mp3 | EncodeFormat::Mp3NoDurationTag => {}
    }

    command
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Mux program + decoy into a dual-track MP4. Sample rates are set so corpus cases
/// can exercise first-track selection vs higher-rate decoy on track two.
pub fn encode_dual_track_mp4(
    program_wav: &Path,
    decoy_wav: &Path,
    output: &Path,
    prefer_program_track: bool,
) -> bool {
    if !ffmpeg_available() {
        return false;
    }

    let (program_rate, decoy_rate) = if prefer_program_track {
        (48_000, 11_025)
    } else {
        (11_025, 48_000)
    };

    let parent = output.parent().expect("output parent");
    let program_resampled = parent.join("_program_resampled.wav");
    let decoy_resampled = parent.join("_decoy_resampled.wav");

    if !resample_wav(program_wav, &program_resampled, program_rate) {
        return false;
    }
    if !resample_wav(decoy_wav, &decoy_resampled, decoy_rate) {
        return false;
    }

    let ok = Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(&program_resampled)
        .arg("-i")
        .arg(&decoy_resampled)
        .args(["-map", "0:a", "-map", "1:a"])
        .args(["-vn", "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"])
        .arg("-f")
        .arg("mp4")
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    let _ = std::fs::remove_file(program_resampled);
    let _ = std::fs::remove_file(decoy_resampled);
    ok
}

#[allow(dead_code)]
pub fn delay_wav(input: &Path, output: &Path, delay_ms: u32) -> bool {
    if !ffmpeg_available() {
        return false;
    }

    Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(input)
        .arg("-af")
        .arg(format!("adelay={delay_ms}|{delay_ms}"))
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Short sine fixture for container probe tests (MKV/MP4/etc.).
#[cfg(feature = "ffmpeg-tests")]
pub fn write_lavfi_sine_container(
    path: &Path,
    format_args: &[&str],
    audio_codec_args: &[&str],
    duration_secs: u32,
) -> bool {
    if !ffmpeg_available() {
        return false;
    }

    let mut command = Command::new("ffmpeg");
    command
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg(format!("sine=frequency=440:duration={duration_secs}"))
        .args(format_args)
        .args(audio_codec_args)
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(feature = "he-aac")]
pub fn encode_he_aac_mp4_from_wav(input_wav: &Path, output: &Path) -> bool {
    if !ffmpeg_available() {
        return false;
    }

    let attempts: &[&[&str]] = &[
        &["-vn", "-c:a", "libfdk_aac", "-profile:a", "aac_he", "-b:a", "64k"],
        &["-vn", "-c:a", "aac", "-profile:a", "aac_he", "-b:a", "64k"],
    ];

    for audio_codec_args in attempts {
        if Command::new("ffmpeg")
            .arg("-y")
            .arg("-loglevel")
            .arg("error")
            .arg("-i")
            .arg(input_wav)
            .args(*audio_codec_args)
            .arg("-f")
            .arg("mp4")
            .arg(output)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            return true;
        }
    }

    false
}

#[cfg(all(feature = "he-aac", feature = "ffmpeg-tests"))]
pub fn write_he_aac_mp4_fixture(path: &Path) -> bool {
    if !ffmpeg_available() {
        return false;
    }

    let attempts: &[&[&str]] = &[
        &["-c:a", "libfdk_aac", "-profile:a", "aac_he", "-b:a", "64k"],
        &["-c:a", "aac", "-profile:a", "aac_he", "-b:a", "64k"],
    ];

    for audio_codec_args in attempts {
        if write_lavfi_sine_container(path, &["-f", "mp4"], audio_codec_args, 3) {
            return true;
        }
    }

    false
}

#[cfg(all(feature = "he-aac", feature = "ffmpeg-tests"))]
pub fn write_he_aac_surround_mp4_fixture(path: &Path) -> bool {
    if !ffmpeg_available() {
        return false;
    }

    let attempts: &[&[&str]] = &[
        &[
            "-c:a",
            "libfdk_aac",
            "-profile:a",
            "aac_he",
            "-ac",
            "6",
            "-b:a",
            "128k",
        ],
        &["-c:a", "aac", "-profile:a", "aac_he", "-ac", "6", "-b:a", "128k"],
    ];

    for audio_codec_args in attempts {
        if write_lavfi_sine_container(path, &["-f", "mp4"], audio_codec_args, 3) {
            return true;
        }
    }

    false
}
