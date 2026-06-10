use rusty_chromaprint::Segment;

use crate::infrastructure::chromaprint::config::{MATCH_SCORE_THRESHOLD, MIN_RELIABLE_ITEMS};

pub(crate) fn segment_offset_items(segment: &Segment) -> isize {
    segment.offset2 as isize - segment.offset1 as isize
}

/// Clusters segments by offset lag and returns the best candidate and whether it is ambiguous.
pub(crate) fn select_best_segment(segments: &[Segment]) -> Option<(&Segment, bool)> {
    if segments.is_empty() {
        return None;
    }

    let mut clusters: Vec<(isize, f64, usize, &Segment)> = Vec::new();

    for segment in segments {
        let offset_items = segment_offset_items(segment);
        if let Some(cluster) = clusters
            .iter_mut()
            .find(|(offset, _, _, _)| (*offset - offset_items).abs() <= 1)
        {
            cluster.1 += segment.items_count as f64 / (segment.score + 1.0);
            cluster.2 += segment.items_count;
            if segment.score < cluster.3.score
                || (segment.score == cluster.3.score
                    && segment.items_count > cluster.3.items_count)
            {
                cluster.3 = segment;
            }
        } else {
            clusters.push((
                offset_items,
                segment.items_count as f64 / (segment.score + 1.0),
                segment.items_count,
                segment,
            ));
        }
    }

    clusters.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| {
                left.3
                    .score
                    .partial_cmp(&right.3.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let best = clusters.first()?;
    let ambiguous = clusters.len() > 1
        && clusters[1].1 >= best.1 * 0.75
        && (clusters[1].0 - best.0).abs() > 2;

    Some((best.3, ambiguous))
}

pub(crate) fn segment_confidence(score: f64, items_count: usize, ambiguous: bool) -> f32 {
    if score >= MATCH_SCORE_THRESHOLD {
        return 0.0;
    }

    let score_conf =
        ((MATCH_SCORE_THRESHOLD - score) / MATCH_SCORE_THRESHOLD).clamp(0.0, 1.0) as f32;
    let length_conf = (items_count as f32 / MIN_RELIABLE_ITEMS as f32).clamp(0.0, 1.0);
    let mut confidence = (score_conf * length_conf).sqrt();

    if ambiguous {
        confidence *= 0.5;
    }

    confidence
}
