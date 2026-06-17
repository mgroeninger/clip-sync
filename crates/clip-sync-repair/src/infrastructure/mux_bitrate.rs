//! Resolve ffmpeg `-b:a` for mux from measured source bitrates.

/// How to pick the AAC bitrate when muxing patched output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuxAudioBitratePolicy {
    /// Use the lower measured bitrate of video A and B (default for mux).
    MatchMin,
    /// Use video A's measured audio bitrate only.
    MatchA,
    /// Omit `-b:a` and let ffmpeg pick its encoder default (~128 kb/s stereo).
    Default,
    /// Fixed target in kilobits per second (e.g. `256` for `256k`).
    ExplicitKbps(u32),
}

pub fn parse_mux_audio_bitrate_policy(raw: &str) -> Result<MuxAudioBitratePolicy, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("must not be empty".into());
    }
    match raw.to_ascii_lowercase().as_str() {
        "match_min" | "match-min" => Ok(MuxAudioBitratePolicy::MatchMin),
        "match_a" | "match-a" => Ok(MuxAudioBitratePolicy::MatchA),
        "default" => Ok(MuxAudioBitratePolicy::Default),
        explicit => {
            let kbps = explicit
                .strip_suffix('k')
                .or_else(|| explicit.strip_suffix('K'))
                .unwrap_or(explicit);
            let value: u32 = kbps
                .parse()
                .map_err(|_| format!("invalid mux audio bitrate policy: {raw}"))?;
            if value == 0 {
                return Err(format!("invalid mux audio bitrate policy: {raw}"));
            }
            Ok(MuxAudioBitratePolicy::ExplicitKbps(value))
        }
    }
}

/// Format a bits-per-second rate for ffmpeg `-b:a` (nearest kilobit, minimum 8k).
pub fn format_ffmpeg_audio_bitrate(bps: u32) -> String {
    let kb = ((u64::from(bps) + 500) / 1000).max(8);
    format!("{kb}k")
}

/// Resolve ffmpeg `-b:a` from policy and patch-time measurements.
pub fn resolve_mux_audio_bitrate(
    policy: MuxAudioBitratePolicy,
    source_a_bps: Option<u32>,
    source_b_bps: Option<u32>,
) -> Option<String> {
    let bps = match policy {
        MuxAudioBitratePolicy::Default => return None,
        MuxAudioBitratePolicy::ExplicitKbps(kbps) => kbps.saturating_mul(1000),
        MuxAudioBitratePolicy::MatchA => source_a_bps?,
        MuxAudioBitratePolicy::MatchMin => match (source_a_bps, source_b_bps) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => return None,
        },
    };
    if bps == 0 {
        return None;
    }
    Some(format_ffmpeg_audio_bitrate(bps))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_policy_keywords() {
        assert_eq!(
            parse_mux_audio_bitrate_policy("match_min").unwrap(),
            MuxAudioBitratePolicy::MatchMin
        );
        assert_eq!(
            parse_mux_audio_bitrate_policy("match-a").unwrap(),
            MuxAudioBitratePolicy::MatchA
        );
        assert_eq!(
            parse_mux_audio_bitrate_policy("default").unwrap(),
            MuxAudioBitratePolicy::Default
        );
        assert_eq!(
            parse_mux_audio_bitrate_policy("256k").unwrap(),
            MuxAudioBitratePolicy::ExplicitKbps(256)
        );
    }

    #[test]
    fn resolve_match_min_uses_lower_source() {
        let resolved = resolve_mux_audio_bitrate(
            MuxAudioBitratePolicy::MatchMin,
            Some(255_000),
            Some(247_000),
        );
        assert_eq!(resolved.as_deref(), Some("247k"));
    }

    #[test]
    fn resolve_default_omits_bitrate() {
        assert!(resolve_mux_audio_bitrate(
            MuxAudioBitratePolicy::Default,
            Some(255_000),
            Some(247_000),
        )
        .is_none());
    }

    #[test]
    fn format_rounds_to_nearest_kilobit() {
        assert_eq!(format_ffmpeg_audio_bitrate(255_400), "255k");
        assert_eq!(format_ffmpeg_audio_bitrate(247_083), "247k");
    }
}
