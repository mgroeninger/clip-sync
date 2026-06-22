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
        assert_eq!(
            format_time_range_verbose(50.0, 80.0),
            "0:50 – 1:20"
        );
    }

    #[test]
    fn format_timestamp_verbose_with_hours() {
        assert_eq!(format_timestamp_verbose(3661.5, true), "1:01:01.500");
    }
}
