//! Throwaway diagnostic: confirm whether the stepped jump-cut fixture's true offsets actually
//! correlate well, bypassing the full production search (which takes ~13 min per run).
use clip_sync::testing::corpus_sources::source_ready;
use clip_sync_repair_harness::dual_fit_oracle::{load_manifest, require_case_sources};
use clip_sync_repair_harness::floor_oracle::{decode_to_mono_wav_at, read_mono_wav};

fn normalized_correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let a = &a[..n];
    let b = &b[..n];
    let mean_a = a.iter().sum::<f64>() / n as f64;
    let mean_b = b.iter().sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut da = 0.0;
    let mut db = 0.0;
    for i in 0..n {
        let x = a[i] - mean_a;
        let y = b[i] - mean_b;
        num += x * y;
        da += x * x;
        db += y * y;
    }
    if da <= 0.0 || db <= 0.0 {
        return 0.0;
    }
    num / (da.sqrt() * db.sqrt())
}

fn main() {
    let repair_tests_dir = std::path::Path::new(
        "C:/tools/clip-sync/crates/clip-sync-repair",
    );
    let manifest = load_manifest(repair_tests_dir);
    let case = manifest
        .case
        .iter()
        .find(|c| c.id == "cc_speech_dual_fit_jump_cut_wav")
        .expect("case");
    if !source_ready(&case.source_id) {
        eprintln!("source not ready, skipping");
        return;
    }
    require_case_sources(case);

    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().join(&case.id);
    let built = clip_sync_repair_harness::dual_fit_oracle::build_stepped_floor_oracle_pair(
        &dir,
        case,
        &manifest.defaults,
    );

    let rate = built.meta.sample_rate;
    let decoded_a = dir.join("diag_a.wav");
    let decoded_b = dir.join("diag_b.wav");
    assert!(decode_to_mono_wav_at(&built.path_a, &decoded_a, rate, None));
    assert!(decode_to_mono_wav_at(&built.path_b, &decoded_b, rate, None));
    let (_, a_i16) = read_mono_wav(&decoded_a);
    let (_, b_i16) = read_mono_wav(&decoded_b);
    let a: Vec<f64> = a_i16.iter().map(|&s| s as f64).collect();
    let b: Vec<f64> = b_i16.iter().map(|&s| s as f64).collect();

    let gap_start = built.meta.gap_start_frame;
    let gap_end = built.meta.gap_end_frame;
    let step_frames = ((case.step_ms / 1000.0) * rate as f64).round() as usize;

    println!("gap_start={gap_start} gap_end={gap_end} step_frames={step_frames}");
    println!("a.len()={} b.len()={}", a.len(), b.len());

    let w = (0.25 * rate as f64) as usize; // 250ms window, matches fill_seam_search_secs default

    // PRE seam: A[gap_start-w..gap_start] vs B[gap_start-w..gap_start] (should be ~identical, lag 0)
    let pre_a = &a[gap_start - w..gap_start];
    let pre_b = &b[gap_start - w..gap_start];
    println!(
        "PRE  (lag0)      corr = {:.4}",
        normalized_correlation(pre_a, pre_b)
    );

    // POST seam at naive lag0 (rigid, WRONG placement): A[gap_end..gap_end+w] vs B[gap_end..gap_end+w]
    let post_a = &a[gap_end..gap_end + w];
    let post_b_lag0 = &b[gap_end..gap_end + w];
    println!(
        "POST (lag0, wrong) corr = {:.4}",
        normalized_correlation(post_a, post_b_lag0)
    );

    // POST seam at TRUE offset: A[gap_end..gap_end+w] vs B[gap_end+step..gap_end+step+w]
    let post_b_true = &b[gap_end + step_frames..gap_end + step_frames + w];
    println!(
        "POST (true step) corr = {:.4}",
        normalized_correlation(post_a, post_b_true)
    );

    // Now call the real try_dual_fit with production-shaped inputs.
    use clip_sync_repair::domain::dual_fit::{try_dual_fit, DualFitParams};
    let b_samples_f32: Vec<f32> = b.iter().map(|&x| x as f32).collect();
    let a_pre_mono = pre_a.to_vec();
    let a_post_mono = post_a.to_vec();
    let p = DualFitParams {
        channels: 1,
        sample_rate: rate,
        gap_frames: gap_end - gap_start,
        seam_window_frames: w,
        max_lag_frames: (0.6 * rate as f64) as usize,
        min_fill_correlation: 0.12,
        fill_absolute_floor: 0.12,
        step_real_margin: 0.15,
        a_gap_floor_db: -60.0,
    };
    let b_mapped_start = gap_start; // lag-0 nominal, matches oracle_injected_alignment offset=0
    match try_dual_fit(&a_pre_mono, &a_post_mono, &b, &b_samples_f32, b_mapped_start, &p) {
        Some(r) => println!(
            "try_dual_fit: Some(pre_seam_r={:.4}, post_seam_r={:.4}, trim_frames={})",
            r.pre_seam_r, r.post_seam_r, r.trim_frames
        ),
        None => println!("try_dual_fit: None (declined)"),
    }

    // Check refine_gap_frames: does it snap the reported boundaries somewhere unexpected?
    use clip_sync_repair::domain::policies::refine_gap_frames;
    let a_f32: Vec<f32> = a.iter().map(|&x| x as f32).collect();
    let max_refine_frames = (0.75 * rate as f64).round() as usize;
    let refined = refine_gap_frames(
        &a_f32,
        1,
        gap_start,
        gap_end,
        0.01,
        0.0,
        max_refine_frames,
    );
    println!(
        "refine_gap_frames: reported=({gap_start},{gap_end}) -> refined=({},{})",
        refined.start_frame, refined.end_frame
    );
}
