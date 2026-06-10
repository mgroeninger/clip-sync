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

/// Inclusive timeline range for tables and reports (`start – end`).
pub fn format_time_range(start_secs: f64, end_secs: f64) -> String {
    format!(
        "{} – {}",
        format_timestamp(start_secs),
        format_timestamp(end_secs)
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
}
