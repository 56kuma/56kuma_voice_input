//! Pure rasteriser for the top-bar (tray) icon: one [`Glyph`] in, ARGB32
//! pixels out. No OS types here so it is unit-testable; `tray` is the thin
//! adapter that hands these pixels to the desktop.
//!
//! Shapes are signed-distance functions, so edges are anti-aliased at any
//! size without an image dependency.

use super::state_view::Glyph;

// Brighter than the capsule palette: the panel is dark and its own icons are
// near-white, so Idle should blend in and only Recording should stand out.
const IDLE: [u8; 3] = [222, 222, 226];
const RECORDING: [u8; 3] = [255, 69, 58];
const WORKING: [u8; 3] = [235, 235, 240];
const SUCCESS: [u8; 3] = [48, 209, 88];
const ERROR: [u8; 3] = [255, 159, 10];

/// Short label for menus and tooltips.
pub fn label(glyph: Glyph) -> &'static str {
    match glyph {
        Glyph::Ring => "待機中",
        Glyph::Dot => "録音中",
        Glyph::Dots => "文字起こし中",
        Glyph::Check => "入力完了",
        Glyph::Bang => "エラー",
    }
}

/// Renders `glyph` into a `size`×`size` icon: 4 bytes per pixel in A, R, G, B
/// order (ARGB32, network byte order — what StatusNotifierItem expects).
pub fn render(glyph: Glyph, size: u32) -> Vec<u8> {
    let s = size as f32;
    let (cx, cy) = (s / 2.0, s / 2.0);
    let r = s / 2.0;
    let rgb = match glyph {
        Glyph::Ring => IDLE,
        Glyph::Dot => RECORDING,
        Glyph::Dots => WORKING,
        Glyph::Check => SUCCESS,
        Glyph::Bang => ERROR,
    };

    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (x, y) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
            let distance = match glyph {
                Glyph::Ring => (x.hypot(y) - r * 0.6).abs() - s / 14.0,
                Glyph::Dot => x.hypot(y) - r * 0.72,
                Glyph::Dots => (-1..=1)
                    .map(|i| (x - i as f32 * s * 0.3).hypot(y) - s * 0.11)
                    .fold(f32::MAX, f32::min),
                Glyph::Check => {
                    let a = (-r * 0.6, r * 0.05);
                    let b = (-r * 0.15, r * 0.5);
                    let c = (r * 0.65, -r * 0.45);
                    segment(x, y, a, b).min(segment(x, y, b, c)) - s / 12.0
                }
                Glyph::Bang => {
                    let stem = segment(x, y, (0.0, -r * 0.7), (0.0, r * 0.15)) - s / 11.0;
                    let dot = x.hypot(y - r * 0.65) - s / 10.0;
                    stem.min(dot)
                }
            };
            // One pixel of linear falloff across the edge = anti-aliasing.
            let coverage = (0.5 - distance).clamp(0.0, 1.0);
            data.extend_from_slice(&[(coverage * 255.0).round() as u8, rgb[0], rgb[1], rgb[2]]);
        }
    }
    data
}

/// Distance from `(x, y)` to the segment `a`–`b`.
fn segment(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((x - a.0) * dx + (y - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (x - (a.0 + t * dx)).hypot(y - (a.1 + t * dy))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Glyph; 5] = [
        Glyph::Ring,
        Glyph::Dot,
        Glyph::Dots,
        Glyph::Check,
        Glyph::Bang,
    ];

    fn alpha(data: &[u8], size: u32, x: u32, y: u32) -> u8 {
        data[((y * size + x) * 4) as usize]
    }

    #[test]
    fn every_glyph_is_a_full_argb_buffer_with_clear_corners() {
        for glyph in ALL {
            for size in [22, 32, 64] {
                let data = render(glyph, size);
                assert_eq!(data.len(), (size * size * 4) as usize, "{glyph:?}");
                assert_eq!(alpha(&data, size, 0, 0), 0, "{glyph:?} corner");
                assert!(
                    data.chunks(4).any(|p| p[0] == 255),
                    "{glyph:?} must draw something opaque"
                );
            }
        }
    }

    #[test]
    fn idle_is_hollow_and_recording_is_solid_red() {
        let size = 32;
        let ring = render(Glyph::Ring, size);
        let dot = render(Glyph::Dot, size);

        assert_eq!(alpha(&ring, size, 16, 16), 0, "ring centre is empty");
        assert_eq!(alpha(&dot, size, 16, 16), 255, "dot centre is filled");
        let centre = ((16 * size + 16) * 4) as usize;
        assert_eq!(&dot[centre + 1..centre + 4], &RECORDING);
    }

    #[test]
    fn every_glyph_looks_different() {
        let icons: Vec<_> = ALL.iter().map(|g| render(*g, 32)).collect();
        for i in 0..icons.len() {
            for j in i + 1..icons.len() {
                assert_ne!(icons[i], icons[j], "{:?} vs {:?}", ALL[i], ALL[j]);
            }
        }
    }
}
