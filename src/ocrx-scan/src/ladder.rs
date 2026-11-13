//! Degeneration-ladder support primitives (plan §8.10): column-band
//! detection for the split retry (Q1.4a rung 3). The ladder table itself
//! lands with the windowed OCR orchestration.

/// Q1.1c candidate C (quality plan §4.1, D-10): column-band detection by
/// vertical whitespace-valley projection on the rendered page - the classic
/// layout-analysis pass, no model and no prompt change. Returns x bounds in
/// pixels for 2..=4 columns left to right, or `None` when no reliable
/// valley set exists (callers fall back to a single full-width band).
pub fn detect_column_bands(width: u32, height: u32, rgb8: &[u8]) -> Option<Vec<(u32, u32)>> {
    if width == 0 || height == 0 || rgb8.len() != width as usize * height as usize * 3 {
        return None;
    }
    // Downscale by horizontal stride sampling to ~900 analysis columns.
    let target_w = 900_u32;
    let stride_x = (width / target_w).max(1) as usize;
    let stride_y = ((height / width * target_w).max(1) / 2).max(1) as usize;
    let columns = (width as usize / stride_x).max(1);
    let row_lo = (height as usize * 5 / 100 / stride_y) * stride_y;
    let row_hi = (height as usize * 95 / 100 / stride_y) * stride_y;
    let mut ink = vec![0.0_f32; columns];
    for (cx, ink_slot) in ink.iter_mut().enumerate() {
        let x = cx * stride_x;
        let mut dark = 0_usize;
        let mut sampled = 0_usize;
        let mut y = row_lo;
        while y < row_hi {
            let base = (y * width as usize + x) * 3;
            let luminance = u32::from(rgb8[base]) * 299
                + u32::from(rgb8[base + 1]) * 587
                + u32::from(rgb8[base + 2]) * 114;
            if luminance < 48_000 {
                dark += 1;
            }
            sampled += 1;
            y += stride_y;
        }
        *ink_slot = dark as f32 / sampled.max(1) as f32;
    }
    let smoothed: Vec<f32> = (0..columns)
        .map(|i| {
            let lo = i.saturating_sub(3);
            let hi = (i + 4).min(columns);
            ink[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
        })
        .collect();
    let band_lo = columns * 8 / 100;
    let band_hi = columns * 92 / 100;
    let median = {
        let mut band = smoothed[band_lo..band_hi].to_vec();
        band.sort_by(f32::total_cmp);
        band[band.len() / 2]
    };
    let threshold = (median * 0.45).max(0.015);
    // Interior near-empty runs = valley candidates.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut run_start: Option<usize> = None;
    for (i, value) in smoothed[band_lo..band_hi].iter().enumerate() {
        let i = band_lo + i;
        if *value <= threshold {
            if run_start.is_none() {
                run_start = Some(i);
            }
        } else if let Some(start) = run_start.take() {
            runs.push((start, i));
        }
    }
    if let Some(start) = run_start.take() {
        runs.push((start, band_hi));
    }
    if runs.len() < 3 {
        return None;
    }
    let min_col = columns * 12 / 100;
    let mut cuts: Vec<usize> = Vec::new();
    for &(start, end) in &runs[1..runs.len() - 1] {
        let center = usize::midpoint(start, end);
        let _ = (start, end);
        let mut bounds = vec![band_lo];
        bounds.extend_from_slice(&cuts);
        bounds.push(band_hi);
        let fits = bounds
            .windows(2)
            .all(|pair| center >= pair[0] + min_col && pair[1] >= center + min_col);
        if fits {
            cuts.push(center);
        }
    }
    if cuts.len() + 1 < 2 {
        return None;
    }
    let mut bands = Vec::new();
    let stride_x = stride_x as u32;
    let mut left = band_lo as u32 * stride_x;
    for cut in &cuts {
        let right = *cut as u32 * stride_x;
        bands.push((left, right));
        left = right;
    }
    bands.push((left, width));
    Some(bands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_column_bands_finds_two_columns_on_synthetic_page() {
        // Synthetic 600x200 page: white, with black text-like stripes in
        // x=60..240 and x=360..540, empty gutter in between.
        let width = 600_u32;
        let height = 200_u32;
        let mut rgb8 = vec![255_u8; (width * height * 3) as usize];
        for stripe_x in 60..240 {
            for y in (10..190).step_by(6) {
                for dy in 0..3 {
                    let base = ((y + dy) * width as usize + stripe_x as usize) * 3;
                    rgb8[base] = 20;
                    rgb8[base + 1] = 20;
                    rgb8[base + 2] = 20;
                }
            }
        }
        for stripe_x in 360..540 {
            for y in (10..190).step_by(6) {
                for dy in 0..3 {
                    let base = ((y + dy) * width as usize + stripe_x as usize) * 3;
                    rgb8[base] = 20;
                    rgb8[base + 1] = 20;
                    rgb8[base + 2] = 20;
                }
            }
        }
        let bands = detect_column_bands(width, height, &rgb8).expect("two columns detected");
        assert_eq!(bands.len(), 2, "bands: {bands:?}");
        assert!(bands[0].0 <= 60);
        assert!(bands[0].1 >= 240 && bands[0].1 <= 360);
        assert!(bands[1].0 >= 240 && bands[1].0 <= 360);
        assert!(bands[1].1 >= 540);
    }

    #[test]
    fn detect_column_bands_returns_none_for_uniform_page() {
        let width = 600_u32;
        let height = 200_u32;
        let rgb8 = vec![255_u8; (width * height * 3) as usize];
        assert_eq!(detect_column_bands(width, height, &rgb8), None);
    }
}
