//! THR (Theta-Rho) export serializer.
//!
//! Converts polylines into a `.thr` text file for polar sand tables
//! (Sisyphus, Oasis, Dune Weaver, and DIY polar builds).
//!
//! Each line contains a `theta rho` pair (space-separated), where:
//! - **theta**: continuous radians (accumulating, does NOT wrap at 2π)
//! - **rho**: 0.0 (center) to 1.0 (edge), normalized
//!
//! Lines beginning with `#` are metadata comments, ignored by table
//! firmware.
//!
//! ## Coordinate Convention
//!
//! The Sisyphus ecosystem uses `atan2(x, y)` — **not** the standard
//! math `atan2(y, x)`.  This means theta=0 corresponds to the **+Y**
//! direction (image-downward / physical-table upward), i.e., `atan2(0, +dy) = 0`.
//! Confirmed by both [Sandify](https://github.com/jeffeb3/sandify)
//! and [jsisyphus](https://github.com/markyland/SisyphusForTheRestOfUs).
//!
//! ## Polar Origin
//!
//! When a circular mask is present, its center and radius define the
//! polar coordinate system.  Otherwise, the image center is used as
//! the origin and the circumscribing circle (half the diagonal) as
//! the radius.
//!
//! This is a pure function with no I/O — it returns a `String`.

use std::f64::consts::PI;
use std::fmt::Write;

use mujou_pipeline::{Dimensions, MaskShape, Polyline};

/// Metadata to embed as `#`-prefixed comment lines at the top of the
/// `.thr` file.
///
/// All fields are optional.  When present, the corresponding comment
/// line is emitted.  Parsers should skip any line beginning with `#`.
#[derive(Debug, Clone, Default)]
pub struct ThrMetadata<'a> {
    /// Source image filename — emitted as `# Source: <filename>`.
    pub title: Option<&'a str>,

    /// Human-readable pipeline parameters — emitted as a `#` comment.
    pub description: Option<&'a str>,

    /// Export timestamp — emitted as `# Exported: <timestamp>`.
    pub timestamp: Option<&'a str>,

    /// Full `PipelineConfig` JSON — emitted as `# Config: <json>`.
    ///
    /// Allows re-importing settings to reproduce the exact same output.
    pub config_json: Option<&'a str>,
}

/// Serialize polylines into a THR (Theta-Rho) text string.
///
/// Each point is converted from image XY coordinates to polar
/// coordinates using the ecosystem's `atan2(x, y)` convention.
/// Theta accumulates continuously (no wrapping at 2π) and rho is
/// normalized to [0.0, 1.0].
///
/// ## Polar Origin
///
/// - If `mask_shape` is `Some(MaskShape::Circle { center, radius })`,
///   those values define the polar coordinate system directly.
/// - Otherwise (rectangle mask or no mask), the image center is used
///   as the origin and `sqrt(w² + h²) / 2` as the radius
///   (circumscribing circle).
///
/// ## Precision
///
/// Coordinates are formatted to 5 decimal places, matching the
/// convention established by [Sandify](https://sandify.org/).
///
/// # Examples
///
/// ```
/// use mujou_pipeline::{Dimensions, Point, Polyline};
/// use mujou_export::thr::{ThrMetadata, to_thr};
///
/// let polylines = vec![
///     Polyline::new(vec![
///         Point::new(100.0, 100.0),  // center
///         Point::new(150.0, 100.0),  // right of center
///     ]),
/// ];
/// let dims = Dimensions { width: 200, height: 200 };
/// let thr = to_thr(&polylines, dims, &ThrMetadata::default(), None);
/// assert!(thr.contains("# mujou"));
/// // First point at center → rho ≈ 0.0
/// // Second point 50px right → some theta, rho > 0
/// ```
#[must_use]
pub fn to_thr(
    polylines: &[Polyline],
    dimensions: Dimensions,
    metadata: &ThrMetadata<'_>,
    mask_shape: Option<&MaskShape>,
) -> String {
    let (center_x, center_y, max_radius) = polar_params(dimensions, mask_shape);

    let mut out = String::new();

    // --- Metadata header ---
    let _ = writeln!(out, "# mujou");
    if let Some(title) = metadata.title {
        for line in title.lines() {
            let _ = writeln!(out, "# Source: {line}");
        }
    }
    if let Some(description) = metadata.description {
        for line in description.lines() {
            let _ = writeln!(out, "# {line}");
        }
    }
    if let Some(timestamp) = metadata.timestamp {
        for line in timestamp.lines() {
            let _ = writeln!(out, "# Exported: {line}");
        }
    }
    if let Some(config_json) = metadata.config_json {
        for line in config_json.lines() {
            let _ = writeln!(out, "# Config: {line}");
        }
    }

    // --- Theta-Rho data ---
    let mut prev_theta: Option<f64> = None;

    for polyline in polylines {
        let points = polyline.points();
        for point in points {
            let dx = point.x - center_x;
            let dy = point.y - center_y;

            // Rho: normalized distance from center, clamped to [0, 1].
            let dist = dx.hypot(dy);
            let rho = if max_radius > 0.0 {
                (dist / max_radius).clamp(0.0, 1.0)
            } else {
                0.0
            };

            // At the exact polar origin (rho=0), theta is geometrically
            // undefined.  Reuse the previous theta to avoid a spurious
            // angular jump; default to 0.0 for the very first point.
            let theta = if dist == 0.0 {
                prev_theta.unwrap_or(0.0)
            } else {
                // Theta: atan2(x, y) convention (theta=0 points up / +Y).
                // Note: image Y increases downward, but the atan2(dx, dy)
                // convention combined with the ecosystem's coordinate system
                // produces the correct result — Sandify and jsisyphus use
                // the same image-space Y direction.
                let raw_theta = dx.atan2(dy);

                prev_theta.map_or(raw_theta, |prev| {
                    // Continuous unwinding: choose the equivalent angle
                    // closest to the previous theta.
                    let two_pi = 2.0 * PI;
                    let mut delta = (raw_theta - prev) % two_pi;
                    if delta > PI {
                        delta -= two_pi;
                    } else if delta < -PI {
                        delta += two_pi;
                    }
                    prev + delta
                })
            };
            prev_theta = Some(theta);

            let _ = writeln!(out, "{theta:.5} {rho:.5}");
        }
    }

    out
}

/// Derive polar coordinate system parameters from dimensions and mask.
///
/// Returns `(center_x, center_y, max_radius)`.
fn polar_params(dimensions: Dimensions, mask_shape: Option<&MaskShape>) -> (f64, f64, f64) {
    if let Some(MaskShape::Circle { center, radius }) = mask_shape {
        (center.x, center.y, *radius)
    } else {
        // Circumscribe the full image.
        let w = f64::from(dimensions.width);
        let h = f64::from(dimensions.height);
        (w / 2.0, h / 2.0, w.hypot(h) / 2.0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::suboptimal_flops)]
mod tests {
    use mujou_pipeline::Point;

    use super::*;

    fn dims(width: u32, height: u32) -> Dimensions {
        Dimensions { width, height }
    }

    fn no_meta() -> ThrMetadata<'static> {
        ThrMetadata::default()
    }

    /// Parse theta-rho pairs from THR output (skipping comments).
    fn parse_pairs(thr: &str) -> Vec<(f64, f64)> {
        thr.lines()
            .filter(|line| !line.starts_with('#'))
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let mut parts = line.split_whitespace();
                let theta: f64 = parts.next().unwrap().parse().unwrap();
                let rho: f64 = parts.next().unwrap().parse().unwrap();
                (theta, rho)
            })
            .collect()
    }

    // --- Header / metadata ---

    #[test]
    fn header_always_contains_mujou_identifier() {
        let thr = to_thr(&[], dims(100, 100), &no_meta(), None);
        assert!(thr.starts_with("# mujou\n"));
    }

    #[test]
    fn metadata_title_emitted() {
        let meta = ThrMetadata {
            title: Some("cherry-blossoms.jpg"),
            ..ThrMetadata::default()
        };
        let thr = to_thr(&[], dims(100, 100), &meta, None);
        assert!(thr.contains("# Source: cherry-blossoms.jpg\n"));
    }

    #[test]
    fn metadata_description_emitted() {
        let meta = ThrMetadata {
            description: Some("blur=1.4, canny=15/40"),
            ..ThrMetadata::default()
        };
        let thr = to_thr(&[], dims(100, 100), &meta, None);
        assert!(thr.contains("# blur=1.4, canny=15/40\n"));
    }

    #[test]
    fn metadata_timestamp_emitted() {
        let meta = ThrMetadata {
            timestamp: Some("2026-02-14_12-30-45"),
            ..ThrMetadata::default()
        };
        let thr = to_thr(&[], dims(100, 100), &meta, None);
        assert!(thr.contains("# Exported: 2026-02-14_12-30-45\n"));
    }

    #[test]
    fn metadata_config_json_emitted() {
        let meta = ThrMetadata {
            config_json: Some(r#"{"blur_sigma":1.4}"#),
            ..ThrMetadata::default()
        };
        let thr = to_thr(&[], dims(100, 100), &meta, None);
        assert!(thr.contains("# Config: {\"blur_sigma\":1.4}\n"));
    }

    #[test]
    fn all_metadata_emitted_in_order() {
        let meta = ThrMetadata {
            title: Some("test"),
            description: Some("params"),
            timestamp: Some("2026"),
            config_json: Some("{}"),
        };
        let thr = to_thr(&[], dims(100, 100), &meta, None);
        let mujou_pos = thr.find("# mujou").unwrap();
        let source_pos = thr.find("# Source:").unwrap();
        let params_pos = thr.find("# params").unwrap();
        let exported_pos = thr.find("# Exported:").unwrap();
        let config_pos = thr.find("# Config:").unwrap();
        assert!(mujou_pos < source_pos);
        assert!(source_pos < params_pos);
        assert!(params_pos < exported_pos);
        assert!(exported_pos < config_pos);
    }

    // --- Empty / degenerate inputs ---

    #[test]
    fn empty_polylines_produces_header_only() {
        let thr = to_thr(&[], dims(100, 100), &no_meta(), None);
        let pairs = parse_pairs(&thr);
        assert!(pairs.is_empty());
    }

    #[test]
    fn single_point_produces_one_pair() {
        let polylines = vec![Polyline::new(vec![Point::new(50.0, 50.0)])];
        let thr = to_thr(&polylines, dims(100, 100), &no_meta(), None);
        let pairs = parse_pairs(&thr);
        assert_eq!(pairs.len(), 1);
    }

    // --- Coordinate conversion ---

    #[test]
    fn center_point_has_zero_rho() {
        // Point at exact center of 200x200 image → rho = 0.
        let polylines = vec![Polyline::new(vec![Point::new(100.0, 100.0)])];
        let shape = MaskShape::Circle {
            center: Point::new(100.0, 100.0),
            radius: 100.0,
        };
        let thr = to_thr(&polylines, dims(200, 200), &no_meta(), Some(&shape));
        let pairs = parse_pairs(&thr);
        assert_eq!(pairs.len(), 1);
        assert!((pairs[0].1 - 0.0).abs() < 1e-4, "rho at center should be 0");
    }

    #[test]
    fn edge_point_has_rho_one() {
        // Point at radius distance from center → rho = 1.0.
        let polylines = vec![Polyline::new(vec![Point::new(200.0, 100.0)])];
        let shape = MaskShape::Circle {
            center: Point::new(100.0, 100.0),
            radius: 100.0,
        };
        let thr = to_thr(&polylines, dims(200, 200), &no_meta(), Some(&shape));
        let pairs = parse_pairs(&thr);
        assert_eq!(pairs.len(), 1);
        assert!(
            (pairs[0].1 - 1.0).abs() < 1e-4,
            "rho at edge should be 1.0, got {}",
            pairs[0].1,
        );
    }

    #[test]
    fn rho_is_clamped_to_one() {
        // Point beyond the radius → rho clamped to 1.0.
        let polylines = vec![Polyline::new(vec![Point::new(250.0, 100.0)])];
        let shape = MaskShape::Circle {
            center: Point::new(100.0, 100.0),
            radius: 100.0,
        };
        let thr = to_thr(&polylines, dims(300, 200), &no_meta(), Some(&shape));
        let pairs = parse_pairs(&thr);
        assert!(
            (pairs[0].1 - 1.0).abs() < 1e-4,
            "rho should be clamped to 1.0, got {}",
            pairs[0].1,
        );
    }

    #[test]
    fn atan2_xy_convention_theta_zero_points_up() {
        // atan2(x, y) convention: a point at larger Y (image-down direction)
        // relative to center has theta = 0, because atan2(0, +dy) = 0.
        // The ecosystem treats +Y (image down) as the theta=0 reference direction.
        //
        // Point at (100, 200) relative to center (100, 100):
        //   dx = 0, dy = 100 → atan2(0, 100) = 0.
        let polylines = vec![Polyline::new(vec![Point::new(100.0, 200.0)])];
        let shape = MaskShape::Circle {
            center: Point::new(100.0, 100.0),
            radius: 100.0,
        };
        let thr = to_thr(&polylines, dims(200, 200), &no_meta(), Some(&shape));
        let pairs = parse_pairs(&thr);
        assert!(
            pairs[0].0.abs() < 1e-4,
            "theta for point directly below center should be ~0, got {}",
            pairs[0].0,
        );
    }

    #[test]
    fn point_right_of_center_has_positive_theta() {
        // Point to the right: dx > 0, dy = 0 → atan2(dx, 0) = π/2.
        let polylines = vec![Polyline::new(vec![Point::new(200.0, 100.0)])];
        let shape = MaskShape::Circle {
            center: Point::new(100.0, 100.0),
            radius: 100.0,
        };
        let thr = to_thr(&polylines, dims(200, 200), &no_meta(), Some(&shape));
        let pairs = parse_pairs(&thr);
        let expected = std::f64::consts::FRAC_PI_2;
        assert!(
            (pairs[0].0 - expected).abs() < 1e-4,
            "theta for point right of center should be π/2, got {}",
            pairs[0].0,
        );
    }

    // --- Continuous theta unwinding ---

    #[test]
    fn theta_accumulates_counterclockwise() {
        // Trace a counterclockwise spiral: points at 0, π/2, π, 3π/2, 2π.
        // In atan2(x,y) convention with center (0,0), radius 1:
        //   (0, 1) → theta=0
        //   (1, 0) → theta=π/2
        //   (0, -1) → theta=π
        //   (-1, 0) → theta=3π/2
        //   (0, 1) → theta=2π (unwound from 0)
        let center = Point::new(0.0, 0.0);
        let r = 100.0;
        let polylines = vec![Polyline::new(vec![
            Point::new(0.0, r),  // theta=0
            Point::new(r, 0.0),  // theta=π/2
            Point::new(0.0, -r), // theta=π
            Point::new(-r, 0.0), // theta=3π/2
            Point::new(0.0, r),  // theta=2π (same position, unwound)
        ])];
        let shape = MaskShape::Circle { center, radius: r };
        let thr = to_thr(&polylines, dims(200, 200), &no_meta(), Some(&shape));
        let pairs = parse_pairs(&thr);
        assert_eq!(pairs.len(), 5);

        let eps = 0.02;
        assert!((pairs[0].0 - 0.0).abs() < eps, "theta[0] ≈ 0");
        assert!(
            (pairs[1].0 - std::f64::consts::FRAC_PI_2).abs() < eps,
            "theta[1] ≈ π/2, got {}",
            pairs[1].0,
        );
        assert!(
            (pairs[2].0 - PI).abs() < eps,
            "theta[2] ≈ π, got {}",
            pairs[2].0,
        );
        assert!(
            (pairs[3].0 - 3.0 * std::f64::consts::FRAC_PI_2).abs() < eps,
            "theta[3] ≈ 3π/2, got {}",
            pairs[3].0,
        );
        assert!(
            (pairs[4].0 - 2.0 * PI).abs() < eps,
            "theta[4] ≈ 2π, got {}",
            pairs[4].0,
        );

        // Theta should be monotonically increasing for counterclockwise.
        for i in 1..pairs.len() {
            assert!(
                pairs[i].0 > pairs[i - 1].0,
                "theta should increase: theta[{}]={} <= theta[{}]={}",
                i,
                pairs[i].0,
                i - 1,
                pairs[i - 1].0,
            );
        }
    }

    #[test]
    fn theta_accumulates_clockwise() {
        // Trace a clockwise spiral: theta should decrease.
        let center = Point::new(0.0, 0.0);
        let r = 100.0;
        let polylines = vec![Polyline::new(vec![
            Point::new(0.0, r),  // theta=0
            Point::new(-r, 0.0), // theta=-π/2 (clockwise)
            Point::new(0.0, -r), // theta=-π
            Point::new(r, 0.0),  // theta=-3π/2
            Point::new(0.0, r),  // theta=-2π (same position, unwound)
        ])];
        let shape = MaskShape::Circle { center, radius: r };
        let thr = to_thr(&polylines, dims(200, 200), &no_meta(), Some(&shape));
        let pairs = parse_pairs(&thr);
        assert_eq!(pairs.len(), 5);

        // Theta should be monotonically decreasing for clockwise.
        for i in 1..pairs.len() {
            assert!(
                pairs[i].0 < pairs[i - 1].0,
                "theta should decrease: theta[{}]={} >= theta[{}]={}",
                i,
                pairs[i].0,
                i - 1,
                pairs[i - 1].0,
            );
        }

        let eps = 0.02;
        assert!(
            (pairs[4].0 - (-2.0 * PI)).abs() < eps,
            "theta[4] ≈ -2π, got {}",
            pairs[4].0,
        );
    }

    // --- Precision ---

    #[test]
    fn output_has_five_decimal_places() {
        let polylines = vec![Polyline::new(vec![
            Point::new(50.0, 50.0),
            Point::new(75.0, 50.0),
        ])];
        let thr = to_thr(&polylines, dims(100, 100), &no_meta(), None);
        let data_lines: Vec<&str> = thr
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .collect();
        for line in &data_lines {
            let parts: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(parts.len(), 2, "each line should have 2 values");
            // Each value should have a decimal point and 5 digits after it.
            for part in parts {
                let dot_pos = part.find('.').expect("should have decimal point");
                let decimals = &part[dot_pos + 1..];
                assert_eq!(
                    decimals.len(),
                    5,
                    "expected 5 decimal places, got {decimals} in {part}",
                );
            }
        }
    }

    // --- Fallback polar params (no circular mask) ---

    #[test]
    fn no_mask_uses_circumscribing_circle() {
        // 100x100 image, no mask → center=(50,50), radius=sqrt(100²+100²)/2 ≈ 70.71.
        // Point at (100, 50) is 50px right of center → rho = 50/70.71 ≈ 0.707.
        let polylines = vec![Polyline::new(vec![Point::new(100.0, 50.0)])];
        let thr = to_thr(&polylines, dims(100, 100), &no_meta(), None);
        let pairs = parse_pairs(&thr);
        let expected_rho = 50.0 / (100.0_f64.hypot(100.0) / 2.0);
        assert!(
            (pairs[0].1 - expected_rho).abs() < 1e-4,
            "rho should be ~{expected_rho}, got {}",
            pairs[0].1,
        );
    }

    #[test]
    fn rectangle_mask_falls_back_to_circumscribing_circle() {
        // Rectangle mask should be treated the same as no mask.
        let shape = MaskShape::Rectangle {
            center: Point::new(50.0, 50.0),
            half_width: 40.0,
            half_height: 30.0,
        };
        let polylines = vec![Polyline::new(vec![Point::new(100.0, 50.0)])];
        let thr_rect = to_thr(&polylines, dims(100, 100), &no_meta(), Some(&shape));
        let thr_none = to_thr(&polylines, dims(100, 100), &no_meta(), None);
        let pairs_rect = parse_pairs(&thr_rect);
        let pairs_none = parse_pairs(&thr_none);
        assert!(
            (pairs_rect[0].0 - pairs_none[0].0).abs() < 1e-10,
            "theta should be identical for rect mask vs no mask",
        );
        assert!(
            (pairs_rect[0].1 - pairs_none[0].1).abs() < 1e-10,
            "rho should be identical for rect mask vs no mask",
        );
    }

    // --- Multiple polylines ---

    #[test]
    fn multiple_polylines_concatenated() {
        let polylines = vec![
            Polyline::new(vec![Point::new(50.0, 50.0), Point::new(60.0, 50.0)]),
            Polyline::new(vec![Point::new(70.0, 50.0), Point::new(80.0, 50.0)]),
        ];
        let thr = to_thr(&polylines, dims(100, 100), &no_meta(), None);
        let pairs = parse_pairs(&thr);
        assert_eq!(pairs.len(), 4);
    }

    #[test]
    fn theta_continues_across_polylines() {
        // Theta unwinding should continue across polyline boundaries.
        let center = Point::new(0.0, 0.0);
        let r = 100.0;
        let polylines = vec![
            Polyline::new(vec![
                Point::new(0.0, r), // theta=0
                Point::new(r, 0.0), // theta=π/2
            ]),
            Polyline::new(vec![
                Point::new(0.0, -r), // theta=π (continues from π/2)
                Point::new(-r, 0.0), // theta=3π/2
            ]),
        ];
        let shape = MaskShape::Circle { center, radius: r };
        let thr = to_thr(&polylines, dims(200, 200), &no_meta(), Some(&shape));
        let pairs = parse_pairs(&thr);

        // All thetas should monotonically increase.
        for i in 1..pairs.len() {
            assert!(
                pairs[i].0 > pairs[i - 1].0,
                "theta should increase across polylines: theta[{}]={} <= theta[{}]={}",
                i,
                pairs[i].0,
                i - 1,
                pairs[i - 1].0,
            );
        }
    }

    // --- End-to-end: process() -> to_thr() ---

    #[test]
    fn end_to_end_image_to_thr() {
        use mujou_pipeline::{PipelineConfig, process_staged};

        let img = image::RgbaImage::from_fn(40, 40, |x, _y| {
            if x < 20 {
                image::Rgba([0, 0, 0, 255])
            } else {
                image::Rgba([255, 255, 255, 255])
            }
        });
        let mut buf = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut buf);
        image::ImageEncoder::write_image(
            encoder,
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();

        let result = process_staged(&buf, &PipelineConfig::default()).unwrap();
        let mask_shape = result.canvas.as_ref().map(|mr| &mr.shape);
        let thr = to_thr(
            std::slice::from_ref(result.final_polyline()),
            result.dimensions,
            &no_meta(),
            mask_shape,
        );

        // Should have the header and some data lines.
        assert!(thr.starts_with("# mujou"));
        let pairs = parse_pairs(&thr);
        assert!(
            pairs.len() >= 2,
            "expected at least 2 theta-rho pairs, got {}",
            pairs.len(),
        );

        // All rho values should be in [0, 1].
        for (i, &(_, rho)) in pairs.iter().enumerate() {
            assert!(
                (0.0..=1.0001).contains(&rho),
                "rho[{i}] = {rho} is outside [0, 1]",
            );
        }
    }
}
