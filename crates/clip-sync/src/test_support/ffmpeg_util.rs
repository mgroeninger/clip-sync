use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeFormat {
    Mp3,
    Mp3NoDurationTag,
    Mp4Aac,
    Mp4AacStereo,
    MkvFlac,
    MkvAac,
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

pub fn decode_to_mono_wav(
    input: &Path,
    output: &Path,
    sample_rate: u32,
    max_duration_secs: Option<u32>,
) -> bool {
    if !ffmpeg_available() {
        return false;
    }

    let mut command = Command::new("ffmpeg");
    command
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(input)
        .args(["-ar", &sample_rate.to_string(), "-ac", "1"]);
    if let Some(secs) = max_duration_secs {
        command.args(["-t", &secs.to_string()]);
    }
    command.arg(output).stdout(Stdio::null()).stderr(Stdio::null());

    command
        .status()
        .map(|status| status.success())
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
        EncodeFormat::MkvAac => {
            command.args(["-c:a", "aac", "-b:a", "128k"]);
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
        EncodeFormat::MkvFlac | EncodeFormat::MkvAac => {
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

/// Encode a WAV to MKV FLAC, then double the declared container duration in the Matroska
/// Segment Info header. The resulting file has real audio of `input_wav`'s duration but
/// reports twice that duration to the Symphonia probe. Used to exercise hold-out placement
/// and `track_decodable_extent` on fixtures where container metadata exceeds the decodable
/// extent — the scenario Phase 4 of the MediaSession redesign addresses.
///
/// Returns `false` if ffmpeg is unavailable or the duration patch cannot locate the EBML
/// Duration element (e.g. the encoder moved it, or the file is too small). Callers should
/// skip rather than fail when this returns `false`.
pub fn encode_mkv_with_padded_container_duration(input_wav: &Path, output: &Path) -> bool {
    if !encode_audio(input_wav, output, EncodeFormat::MkvFlac) {
        return false;
    }
    patch_mkv_container_duration(output, 2.0)
}

/// Locate the Matroska Segment Info Duration EBML element (`44 89 88 <f64-be>`) in the
/// first 4 KB of `path` and multiply its value by `multiplier`. Operates entirely on the
/// raw bytes — no Matroska parser needed.
///
/// The element layout is: `[44][89]` (EBML ID 0x4489) + `[88]` (1-byte size = 8) + 8-byte
/// big-endian IEEE 754 f64. Duration is in TimecodeScale units (default 1 ms).
fn patch_mkv_container_duration(path: &Path, multiplier: f64) -> bool {
    let Ok(mut data) = std::fs::read(path) else {
        return false;
    };
    let search_limit = data.len().min(4096);
    let pattern = [0x44u8, 0x89, 0x88];
    if let Some(pos) = data[..search_limit].windows(3).position(|w| w == pattern) {
        let start = pos + 3;
        if start + 8 <= data.len() {
            let old =
                f64::from_be_bytes(data[start..start + 8].try_into().unwrap());
            if old > 0.0 {
                data[start..start + 8]
                    .copy_from_slice(&(old * multiplier).to_be_bytes());
                return std::fs::write(path, &data).is_ok();
            }
        }
    }
    false
}

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

/// Build a dual-track MP4: track 0 = 2ch AAC, track 1 = 6ch AC-3 (channels duplicated from input).
/// Used to smoke-test channel-matching track selection with an AC-3 surround program alongside
/// an AAC stereo fallback.
#[cfg(all(feature = "ac3", feature = "ffmpeg-tests"))]
pub fn write_dual_track_aac_ac3_mp4(source_wav: &Path, output: &Path) -> bool {
    if !ffmpeg_available() {
        return false;
    }
    Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-i",
            source_wav.to_str().unwrap_or(""),
            "-i",
            source_wav.to_str().unwrap_or(""),
            "-map",
            "0:a",
            "-map",
            "1:a",
            "-c:a:0",
            "aac",
            "-b:a:0",
            "128k",
            "-c:a:1",
            "ac3",
            "-b:a:1",
            "384k",
            "-ac:a:1",
            "6",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
            output.to_str().unwrap_or(""),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Encode a WAV to stereo AC-3 in MP4 (production-like: 48 kHz, configurable bitrate).
#[cfg(all(feature = "ac3", feature = "ffmpeg-tests"))]
pub fn encode_wav_to_ac3_mp4(
    input_wav: &Path,
    output: &Path,
    sample_rate: u32,
    channels: u16,
    bit_rate_kbps: u32,
) -> bool {
    if !ffmpeg_available() {
        return false;
    }

    Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-i",
            input_wav.to_str().unwrap_or(""),
            "-c:a",
            "ac3",
            "-b:a",
            &format!("{bit_rate_kbps}k"),
            "-ar",
            &sample_rate.to_string(),
            "-ac",
            &channels.to_string(),
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
            output.to_str().unwrap_or(""),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Decode an audio track to interleaved S16LE PCM via ffmpeg (reference path for AC-3 characterization).
#[cfg(all(feature = "ac3", feature = "ffmpeg-tests"))]
pub fn extract_pcm_s16le_ffmpeg(
    path: &Path,
    sample_rate: u32,
    channels: u16,
    start_secs: f64,
    duration_secs: f64,
) -> Option<Vec<i16>> {
    if !ffmpeg_available() {
        return None;
    }

    let child = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-loglevel",
            "error",
            "-ss",
            &start_secs.to_string(),
            "-t",
            &duration_secs.to_string(),
            "-i",
            path.to_str()?,
            "-map",
            "0:a:0",
            "-vn",
            "-f",
            "s16le",
            "-acodec",
            "pcm_s16le",
            "-ar",
            &sample_rate.to_string(),
            "-ac",
            &channels.to_string(),
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }

    let bytes = output.stdout;
    if !bytes.len().is_multiple_of(2) {
        return None;
    }

    let samples: Vec<i16> = bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    Some(samples)
}

#[cfg(all(feature = "ac3", feature = "ffmpeg-tests"))]
pub fn write_ac3_surround_mp4_fixture(path: &Path) -> bool {
    write_lavfi_sine_container(
        path,
        &["-f", "mp4"],
        &["-c:a", "ac3", "-b:a", "384k", "-ac", "6"],
        3,
    )
}

#[cfg(all(feature = "ac3", feature = "ffmpeg-tests"))]
fn encode_oxideav_eac3_surround_es(duration_secs: u32) -> Option<Vec<u8>> {
    use std::f32::consts::TAU;

    use oxideav_ac3::eac3;
    use oxideav_core::{AudioFrame, CodecId, CodecParameters, Error, Frame, SampleFormat};

    const SAMPLE_RATE: u32 = 48_000;
    const CHANNELS: u16 = 6;
    const BIT_RATE: u64 = 384_000;

    let frame_count = SAMPLE_RATE as usize * duration_secs as usize;
    let mut s16 = Vec::with_capacity(frame_count * CHANNELS as usize * 2);
    for index in 0..frame_count {
        let t = index as f32 / SAMPLE_RATE as f32;
        let sample =
            ((TAU * 440.0 * t).sin() * (i16::MAX as f32 * 0.8)) as i16;
        for _ in 0..CHANNELS as usize {
            s16.extend_from_slice(&sample.to_le_bytes());
        }
    }

    let mut params = CodecParameters::audio(CodecId::new(eac3::CODEC_ID_STR));
    params.sample_rate = Some(SAMPLE_RATE);
    params.channels = Some(CHANNELS);
    params.sample_format = Some(SampleFormat::S16);
    params.bit_rate = Some(BIT_RATE);

    let mut enc = eac3::make_encoder(&params).ok()?;
    enc.send_frame(&Frame::Audio(AudioFrame {
        samples: frame_count as u32,
        pts: Some(0),
        data: vec![s16],
    }))
    .ok()?;
    let _ = enc.flush();

    let mut elementary = Vec::new();
    loop {
        match enc.receive_packet() {
            Ok(packet) => elementary.extend_from_slice(&packet.data),
            Err(Error::NeedMore) | Err(Error::Eof) => break,
            Err(_) => return None,
        }
    }
    (!elementary.is_empty()).then_some(elementary)
}

/// Build a 5.1 E-AC-3 MP4 fixture oxideav-ac3 can decode.
///
/// ffmpeg's native `eac3` encoder emits bitstreams (AHT, etc.) that
/// oxideav-ac3 0.0.8 does not fully implement yet — decode succeeds
/// structurally but PCM is silent. We encode with oxideav-ac3 and mux
/// with ffmpeg so the integration test exercises a real decodable stream.
#[cfg(all(feature = "ac3", feature = "ffmpeg-tests"))]
pub fn write_eac3_surround_mp4_fixture(path: &Path) -> bool {
    if !ffmpeg_available() {
        return false;
    }
    let Some(elementary) = encode_oxideav_eac3_surround_es(3) else {
        return false;
    };

    let es_path = std::env::temp_dir().join(format!(
        "clip-sync-eac3-surround-{}.ec3",
        std::process::id()
    ));
    if std::fs::write(&es_path, &elementary).is_err() {
        return false;
    }

    let ok = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "eac3",
            "-i",
            es_path.to_str().unwrap_or(""),
            "-c:a",
            "copy",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
            path.to_str().unwrap_or(""),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    let _ = std::fs::remove_file(&es_path);
    ok
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
