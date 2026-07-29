/// Format seconds as `H:MM:SS` or `M:SS` for human-readable CLI output.
pub fn format_timestamp(secs: f64) -> String {
    let total = secs.round().max(0.0) as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Parse a clock time as used by gap-selection range tokens (inverse of the formatters above).
///
/// Accepts bare seconds (`6128.25`), `M:SS`, `M:SS.mmm`, `H:MM:SS`, and `H:MM:SS.mmm`.
/// Returns `(seconds, fractional_spelling)` where `fractional_spelling` is true when the input
/// contains a `.` — that spelling drives the dual-ε matcher in gap selection (50 ms vs 500 ms).
pub fn parse_timestamp(raw: &str) -> Result<(f64, bool), String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty timestamp".to_string());
    }
    if s.contains(':') {
        return parse_colon_timestamp(s);
    }
    let fractional = s.contains('.');
    let v: f64 = s
        .parse()
        .map_err(|_| format!("invalid timestamp {raw:?}: expected seconds or H:MM:SS"))?;
    if !v.is_finite() || v < 0.0 {
        return Err(format!(
            "invalid timestamp {raw:?}: must be a non-negative finite value"
        ));
    }
    Ok((v, fractional))
}

fn parse_colon_timestamp(s: &str) -> Result<(f64, bool), String> {
    let parts: Vec<&str> = s.split(':').collect();
    let err = || format!("invalid timestamp {s:?}: expected M:SS[.mmm] or H:MM:SS[.mmm]");
    match parts.as_slice() {
        [minutes, seconds] => {
            let minutes: u64 = minutes.parse().map_err(|_| err())?;
            let (secs, fractional) = parse_seconds_component(seconds).map_err(|_| err())?;
            Ok((minutes as f64 * 60.0 + secs, fractional))
        }
        [hours, minutes, seconds] => {
            let hours: u64 = hours.parse().map_err(|_| err())?;
            let minutes: u64 = minutes.parse().map_err(|_| err())?;
            if minutes >= 60 {
                return Err(err());
            }
            let (secs, fractional) = parse_seconds_component(seconds).map_err(|_| err())?;
            Ok((
                hours as f64 * 3600.0 + minutes as f64 * 60.0 + secs,
                fractional,
            ))
        }
        _ => Err(err()),
    }
}

fn parse_seconds_component(s: &str) -> Result<(f64, bool), String> {
    if let Some((whole, frac)) = s.split_once('.') {
        if frac.is_empty() || !frac.chars().all(|c| c.is_ascii_digit()) {
            return Err("bad fractional seconds".into());
        }
        let whole: u64 = whole.parse().map_err(|_| "bad seconds")?;
        if whole >= 60 {
            return Err("seconds out of range".into());
        }
        // Interprets "750" as 750 ms, "5" as 500 ms, "50" as 500 ms — same as `0.{frac}` parse.
        let frac_val: f64 = format!("0.{frac}")
            .parse()
            .map_err(|_| "bad fractional seconds")?;
        Ok((whole as f64 + frac_val, true))
    } else {
        let whole: u64 = s.parse().map_err(|_| "bad seconds")?;
        if whole >= 60 {
            return Err("seconds out of range".into());
        }
        Ok((whole as f64, false))
    }
}

/// Span below which verbose ranges show millisecond precision.
pub const VERBOSE_SUBSECOND_SPAN_SECS: f64 = 10.0;

/// Like [`format_timestamp`] but with `M:SS.mmm` / `H:MM:SS.mmm` when `subsecond` is true.
pub fn format_timestamp_verbose(secs: f64, subsecond: bool) -> String {
    if !subsecond {
        return format_timestamp(secs);
    }
    let total_ms = (secs.max(0.0) * 1000.0).round() as u64;
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}.{millis:03}")
    } else {
        format!("{minutes}:{seconds:02}.{millis:03}")
    }
}

/// Inclusive timeline range for tables and reports (`start – end`).
pub fn format_time_range(start_secs: f64, end_secs: f64) -> String {
    format!(
        "{} – {}",
        format_timestamp(start_secs),
        format_timestamp(end_secs)
    )
}

/// Verbose patch/diagnostic range: millisecond precision when span &lt; [`VERBOSE_SUBSECOND_SPAN_SECS`].
pub fn format_time_range_verbose(start_secs: f64, end_secs: f64) -> String {
    let subsecond = (end_secs - start_secs).abs() < VERBOSE_SUBSECOND_SPAN_SECS;
    format!(
        "{} – {}",
        format_timestamp_verbose(start_secs, subsecond),
        format_timestamp_verbose(end_secs, subsecond)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_timestamp_under_one_hour() {
        assert_eq!(format_timestamp(90.0), "1:30");
    }

    #[test]
    fn format_timestamp_with_hours() {
        assert_eq!(format_timestamp(3661.0), "1:01:01");
    }

    #[test]
    fn format_time_range_joins_start_and_end() {
        assert_eq!(format_time_range(0.0, 16.25), "0:00 – 0:16");
    }

    #[test]
    fn format_time_range_verbose_uses_millis_for_short_spans() {
        assert_eq!(
            format_time_range_verbose(3301.75, 3302.75),
            "55:01.750 – 55:02.750"
        );
    }

    #[test]
    fn format_time_range_verbose_keeps_seconds_for_long_spans() {
        assert_eq!(format_time_range_verbose(50.0, 80.0), "0:50 – 1:20");
    }

    #[test]
    fn format_timestamp_verbose_with_hours() {
        assert_eq!(format_timestamp_verbose(3661.5, true), "1:01:01.500");
    }

    #[test]
    fn parse_timestamp_round_trips_formatters() {
        let (secs, frac) = parse_timestamp("1:30").unwrap();
        assert!((secs - 90.0).abs() < 1e-9);
        assert!(!frac);

        let (secs, frac) = parse_timestamp("1:01:01").unwrap();
        assert!((secs - 3661.0).abs() < 1e-9);
        assert!(!frac);

        let (secs, frac) = parse_timestamp("55:01.750").unwrap();
        assert!((secs - 3301.75).abs() < 1e-9);
        assert!(frac);

        let (secs, frac) = parse_timestamp("6128.25").unwrap();
        assert!((secs - 6128.25).abs() < 1e-9);
        assert!(frac);

        let (secs, frac) = parse_timestamp("6128").unwrap();
        assert!((secs - 6128.0).abs() < 1e-9);
        assert!(!frac);
    }
}
