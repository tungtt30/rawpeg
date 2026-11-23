//! Auto-exposure / auto-tone engine based on histogram analysis.
//!
//! Strategy: a 256-bin linear-luminance histogram drives three adjustments —
//! exposure moves the median luminance to ~18% gray (`log2` ratio), highlights
//! compress the top 0.5% tail that would clip after the exposure correction,
//! and shadows lift the darkest 1% when it sits well below mid-gray. White
//! balance fields are left untouched.

use crate::models::EditParams;
use crate::raw_engine::decoder::DecodedImage;
use crate::raw_engine::processor::luminance;

/// Target mid-gray after auto-tone (18% reflectance gray).
const MID_GRAY: f32 = 0.18;
/// Maximum fraction of pixels allowed to clip after auto-tone.
const CLIP_FRACTION: f32 = 0.005;
const HIST_BINS: usize = 256;

/// Computes auto-tone adjustments from a linear buffer.
///
/// Returns an `EditParams` with `exposure`, `highlights` and `shadows`
/// computed; temperature/tint stay at their defaults and `is_auto_applied` is
/// set to `true` so the UI can preserve the user's own white balance.
pub fn auto_exposure(image: &DecodedImage) -> EditParams {
    let mut params = EditParams::default();
    if image.data.is_empty() {
        return params;
    }

    let mut hist = [0u64; HIST_BINS];
    for pixel in &image.data {
        let y = luminance(*pixel).clamp(0.0, 1.0);
        let bin = ((y * (HIST_BINS - 1) as f32).floor() as usize).min(HIST_BINS - 1);
        hist[bin] += 1;
    }
    let total = image.data.len() as u64;

    let median = percentile(&hist, total, 0.5);
    params.exposure = (MID_GRAY / median.max(1e-6))
        .log2()
        .clamp(-3.0, 3.0);

    let hi = percentile(&hist, total, 1.0 - CLIP_FRACTION);
    let hi_after = hi * 2f32.powf(params.exposure);
    params.highlights = ((hi_after - 1.0) * 100.0).round().clamp(0.0, 100.0) as i32;

    let lo = percentile(&hist, total, 0.01);
    params.shadows = (((MID_GRAY - lo) / MID_GRAY) * 100.0)
        .round()
        .clamp(0.0, 100.0) as i32;

    params.is_auto_applied = true;
    params
}

/// Value (in `0.0..=1.0`) of the histogram bin at the given fraction.
fn percentile(hist: &[u64; HIST_BINS], total: u64, fraction: f32) -> f32 {
    let target = (total as f64 * fraction as f64) as u64;
    let mut seen = 0u64;
    for (bin, count) in hist.iter().enumerate() {
        seen += count;
        if seen > target {
            return bin as f32 / (HIST_BINS - 1) as f32;
        }
    }
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(values: &[f32]) -> DecodedImage {
        DecodedImage {
            width: values.len(),
            height: 1,
            data: values.iter().map(|v| [*v, *v, *v]).collect(),
        }
    }

    #[test]
    fn neutral_gray_gives_zero_exposure() {
        let params = auto_exposure(&img(&[0.18; 64]));
        assert!(
            params.exposure.abs() < 0.05,
            "expected near-zero exposure, got {}",
            params.exposure
        );
        assert_eq!(params.highlights, 0);
        assert!((0..=5).contains(&params.shadows));
        assert_eq!(params.temperature, 6500);
        assert_eq!(params.tint, 0);
        assert!(params.is_auto_applied);
    }

    #[test]
    fn underexposed_image_gets_positive_exposure() {
        let params = auto_exposure(&img(&[0.09; 64]));
        assert!(
            (0.9..=1.2).contains(&params.exposure),
            "expected ~1 EV, got {}",
            params.exposure
        );
    }

    #[test]
    fn overexposed_image_gets_negative_exposure() {
        let params = auto_exposure(&img(&[0.5; 64]));
        assert!(
            (-1.6..=-1.3).contains(&params.exposure),
            "expected ~-1.4 EV, got {}",
            params.exposure
        );
        assert_eq!(params.highlights, 0);
    }

    #[test]
    fn bright_tail_triggers_highlight_compression_and_shadow_lift() {
        let mut values = vec![0.05; 90];
        values.extend_from_slice(&[5.0; 10]);
        let params = auto_exposure(&img(&values));
        assert!(params.exposure > 0.0, "dark body pushes exposure positive");
        assert_eq!(params.highlights, 100, "tail far past full scale");
        assert!(params.shadows > 30, "dark body lifts shadows");
    }

    #[test]
    fn auto_exposure_is_deterministic() {
        let image = img(&[0.02, 0.1, 0.3, 0.9, 2.0]);
        assert_eq!(auto_exposure(&image), auto_exposure(&image));
    }

    #[test]
    fn auto_exposure_empty_image_returns_default() {
        let params = auto_exposure(&img(&[]));
        assert_eq!(params, EditParams::default());
        assert!(!params.is_auto_applied);
    }
}
