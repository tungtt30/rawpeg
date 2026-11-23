//! Image processing pipeline: exposure, contrast, highlights/shadows and white
//! balance applied on linear RGB buffers.
//!
//! Fixed operation order: white balance → exposure → highlights/shadows →
//! contrast. All math runs in linear space; the buffer may exceed `1.0` after
//! edits and is only rolled off / clipped at sRGB encode time.

use rayon::prelude::*;

use crate::models::EditParams;
use crate::raw_engine::decoder::DecodedImage;

/// sRGB piecewise curve threshold (`12.92*c` below, `1.055*c^(1/2.4)-0.055` above).
const SRGB_CUTOFF: f32 = 0.003_130_8;
/// Start of the highlight soft shoulder, as a fraction of full scale.
const SHOULDER_START: f32 = 0.8;
/// Shoulder width that makes the knee C1-continuous with the linear segment.
const SHOULDER_WIDTH: f32 = 0.2;
/// Mid-gray anchor for the contrast S-curve.
const MID_GRAY: f32 = 0.18;

/// Linear luminance (Rec. 709 weights) of one pixel.
pub fn luminance(pixel: [f32; 3]) -> f32 {
    0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2]
}

/// Applies the full edit chain to a linear image, returning an edited linear
/// buffer (values may exceed `1.0`; clipping happens at encode time).
pub fn apply(image: &DecodedImage, params: &EditParams) -> Vec<[f32; 3]> {
    let wb = white_balance_gains(params);
    let ev = params.exposure;
    let highlights = params.highlights;
    let shadows = params.shadows;
    let gamma = 1.0 + params.contrast as f32 * 0.01;

    image
        .data
        .par_iter()
        .map(|p| edit_pixel(*p, wb, ev, highlights, shadows, gamma))
        .collect()
}

/// Applies the edit chain and encodes the result to sRGB 8-bit.
pub fn apply_srgb(image: &DecodedImage, params: &EditParams) -> Vec<u8> {
    let edited = apply(image, params);
    encode_srgb(&edited)
}

/// Encodes a linear `0.0..=` buffer to sRGB 8-bit.
///
/// Values above `SHOULDER_START` pass through a soft exponential shoulder
/// (C1-continuous, asymptotic to `1.0`) so over-exposed highlights roll off
/// gradually instead of clipping at a hard threshold.
pub fn encode_srgb(data: &[[f32; 3]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 3);
    for pixel in data {
        for channel in pixel {
            out.push(encode_channel(*channel));
        }
    }
    out
}

fn edit_pixel(
    p: [f32; 3],
    wb: [f32; 3],
    ev: f32,
    highlights: i32,
    shadows: i32,
    gamma: f32,
) -> [f32; 3] {
    // 1. White balance (relative to as-shot: identity at 6500K / tint 0).
    let mut c = [p[0] * wb[0], p[1] * wb[1], p[2] * wb[2]];

    // 2. Exposure.
    let gain = 2f32.powf(ev);
    c[0] *= gain;
    c[1] *= gain;
    c[2] *= gain;

    // 3. Highlights / shadows through squared luminance masks.
    let y = luminance(c);
    let shadow_mask = (1.0 - (y / 0.5)).clamp(0.0, 1.0).powi(2);
    let highlight_mask = ((y - 0.5) / 0.5).clamp(0.0, 1.0).powi(2);
    let lift = (shadows as f32 / 100.0) * 0.3 * shadow_mask;
    let compress = (highlights as f32 / 100.0) * 0.3 * highlight_mask;
    for channel in &mut c {
        *channel = (*channel + lift - compress * *channel).max(0.0);
    }

    // 4. Contrast S-curve around mid-gray (skipped when neutral).
    if gamma != 1.0 {
        let inv = 1.0 / MID_GRAY;
        for channel in &mut c {
            *channel = MID_GRAY * (*channel * inv).powf(gamma);
        }
    }

    c
}

/// Per-channel linear gains from temperature/tint (relative to as-shot).
///
/// Warmer temperature (above 6500K) raises red and lowers blue; positive tint
/// shifts toward magenta (red/blue up, green down).
fn white_balance_gains(params: &EditParams) -> [f32; 3] {
    let temp_shift = (params.temperature as f32 - 6500.0) / 3500.0;
    let tint_shift = params.tint as f32 / 100.0;
    [
        1.0 + 0.3 * temp_shift + 0.1 * tint_shift,
        1.0 - 0.2 * tint_shift,
        1.0 - 0.3 * temp_shift + 0.1 * tint_shift,
    ]
}

fn encode_channel(c: f32) -> u8 {
    let c = c.max(0.0);
    let c = if c <= SHOULDER_START {
        c
    } else {
        SHOULDER_START + SHOULDER_WIDTH * (1.0 - (-(c - SHOULDER_START) / SHOULDER_WIDTH).exp())
    };
    let srgb = if c <= SRGB_CUTOFF {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(pixels: Vec<[f32; 3]>) -> DecodedImage {
        DecodedImage {
            width: pixels.len(),
            height: 1,
            data: pixels,
        }
    }

    fn neutral() -> EditParams {
        EditParams::default()
    }

    #[test]
    fn neutral_params_are_identity() {
        let buffer = vec![[0.0, 0.1, 0.2], [0.18, 0.5, 0.9], [1.0, 2.0, 4.0]];
        let image = img(buffer.clone());
        assert_eq!(apply(&image, &neutral()), buffer);
    }

    #[test]
    fn exposure_doubles_and_halves_linear_values() {
        let image = img(vec![[0.25, 0.5, 0.75]]);
        let plus = apply(
            &image,
            &EditParams {
                exposure: 1.0,
                ..neutral()
            },
        );
        assert_eq!(plus[0], [0.5, 1.0, 1.5]);
        let minus = apply(
            &image,
            &EditParams {
                exposure: -1.0,
                ..neutral()
            },
        );
        assert_eq!(minus[0], [0.125, 0.25, 0.375]);
    }

    #[test]
    fn contrast_spreads_values_around_mid_gray() {
        let image = img(vec![[0.05, 0.18, 0.5]]);
        let high = apply(
            &image,
            &EditParams {
                contrast: 50,
                ..neutral()
            },
        );
        assert!(high[0][0] < 0.05, "darks get darker");
        assert!(
            (high[0][1] - 0.18).abs() < 1e-6,
            "mid-gray is a fixed point"
        );
        assert!(high[0][2] > 0.5, "brights get brighter");
        let low = apply(
            &image,
            &EditParams {
                contrast: -50,
                ..neutral()
            },
        );
        assert!(low[0][0] > 0.05, "darks get lighter");
        assert!(low[0][2] < 0.5, "brights get darker");
    }

    #[test]
    fn highlights_and_shadows_only_touch_their_masked_region() {
        let image = img(vec![[0.02, 0.02, 0.02], [0.9, 0.9, 0.9]]);
        let lifted = apply(
            &image,
            &EditParams {
                shadows: 100,
                ..neutral()
            },
        );
        assert!(lifted[0][0] > 0.02, "shadows lifted");
        assert_eq!(lifted[1], [0.9, 0.9, 0.9], "highlights untouched");
        let compressed = apply(
            &image,
            &EditParams {
                highlights: 100,
                ..neutral()
            },
        );
        assert_eq!(compressed[0], [0.02, 0.02, 0.02], "shadows untouched");
        assert!(compressed[1][0] < 0.9, "highlights compressed");
    }

    #[test]
    fn highlights_and_shadows_are_symmetric_for_negative_values() {
        let image = img(vec![[0.02, 0.02, 0.02], [0.9, 0.9, 0.9]]);
        let dark = apply(
            &image,
            &EditParams {
                shadows: -100,
                ..neutral()
            },
        );
        assert_eq!(dark[0], [0.0, 0.0, 0.0], "negative shadows crush to zero");
        let bright = apply(
            &image,
            &EditParams {
                highlights: -100,
                ..neutral()
            },
        );
        assert!(bright[1][0] > 0.9, "negative highlights brighten");
    }

    #[test]
    fn white_balance_direction_follows_temperature_and_tint() {
        let image = img(vec![[0.5, 0.5, 0.5]]);
        let warm = apply(
            &image,
            &EditParams {
                temperature: 10000,
                ..neutral()
            },
        );
        assert!(
            warm[0][0] > 0.5 && warm[0][2] < 0.5,
            "warm: red up, blue down"
        );
        let cool = apply(
            &image,
            &EditParams {
                temperature: 2000,
                ..neutral()
            },
        );
        assert!(
            cool[0][2] > 0.5 && cool[0][0] < 0.5,
            "cool: blue up, red down"
        );
        let magenta = apply(
            &image,
            &EditParams {
                tint: 100,
                ..neutral()
            },
        );
        assert!(
            magenta[0][0] > 0.5 && magenta[0][1] < 0.5,
            "positive tint: magenta shift"
        );
        assert_eq!(apply(&image, &neutral())[0], [0.5, 0.5, 0.5]);
    }

    #[test]
    fn encode_srgb_endpoints_and_monotonicity() {
        assert_eq!(encode_srgb(&[[0.0, 0.0, 0.0]])[0], 0);
        assert_eq!(encode_srgb(&[[100.0, 100.0, 100.0]])[0], 255);
        let mut prev = 0u8;
        let mut c = 0.0;
        while c <= 3.0 {
            let v = encode_srgb(&[[c, 0.0, 0.0]])[0];
            assert!(v >= prev, "non-decreasing at c={c}");
            prev = v;
            c += 0.01;
        }
    }

    #[test]
    fn encode_srgb_soft_shoulder_rolls_off_gradually() {
        let full = encode_srgb(&[[1.0, 0.0, 0.0]])[0];
        assert!(
            (240..255).contains(&full),
            "full-scale passes through the shoulder, got {full}"
        );
        assert!(
            encode_srgb(&[[2.0, 0.0, 0.0]])[0] >= full,
            "shoulder is monotonic beyond 1.0"
        );
        let at_knee = encode_srgb(&[[0.8, 0.0, 0.0]])[0];
        let just_below = encode_srgb(&[[0.7999, 0.0, 0.0]])[0];
        let just_above = encode_srgb(&[[0.8001, 0.0, 0.0]])[0];
        assert!((at_knee - just_below) <= 2 && (just_above - at_knee) <= 2);
    }

    #[test]
    fn apply_srgb_outputs_valid_bytes() {
        let image = img(vec![[0.0, 0.18, 1.0], [2.0, 0.5, 0.01]]);
        let out = apply_srgb(
            &image,
            &EditParams {
                exposure: 0.5,
                ..neutral()
            },
        );
        assert_eq!(out.len(), 6);
    }
}
