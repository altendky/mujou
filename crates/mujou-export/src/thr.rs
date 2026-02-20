//! THR (Theta-Rho) export serializer.
//!
//! Converts polylines (in normalized space) into a `.thr` text file for
//! polar sand tables (Sisyphus, Oasis, Dune Weaver, and DIY polar builds).
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
//! direction (Cartesian upward / physical-table upward).
//!
//! Normalized space is +Y up, which matches the Sisyphus `atan2(x, y)`
//! convention directly — no Y negation is needed.
//!
//! ## Polar Origin
//!
//! In normalized space the origin is the center and rho = `sqrt(x² + y²)`
//! directly.  For a circular mask (radius = 1.0 in normalized space),
//! rho is naturally in [0, 1].  For rectangular masks or no mask, rho is
//! clamped to [0, 1].
//!
//! This is a pure function with no I/O — it returns a `String`.

use std::f64::consts::PI;
use std::fmt::Write;

use mujou_pipeline::Polyline;

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

/// Serialize polylines (in normalized space) into a THR (Theta-Rho) text
/// string.
///
/// In normalized space the origin is the polar center and
/// `rho = sqrt(x² + y²)` directly.  Theta uses the ecosystem's
/// `atan2(x, y)` convention (theta=0 points along +Y).  Normalized
/// space is +Y up, which matches directly — no Y negation is needed.
///
/// Theta accumulates continuously (no wrapping at 2π) and rho is
/// clamped to [0.0, 1.0].
///
/// ## Precision
///
/// Coordinates are formatted to 5 decimal places, matching the
/// convention established by [Sandify](https://sandify.org/).
///
/// # Examples
///
/// ```
/// use mujou_pipeline::{Point, Polyline};
/// use mujou_export::thr::{ThrMetadata, to_thr};
///
/// let polylines = vec![
///     Polyline::new(vec![
///         Point::new(0.0, 0.0),   // center → rho = 0
///         Point::new(0.5, 0.0),   // right of center → rho = 0.5
///     ]),
/// ];
/// let thr = to_thr(&polylines, &ThrMetadata::default());
/// assert!(thr.contains("# mujou"));
/// ```
#[must_use]
pub fn to_thr(polylines: &[Polyline], metadata: &ThrMetadata<'_>) -> String {
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
            // In normalized space: origin = center, +Y = up (mathematical convention).
            // rho = distance from origin, clamped to [0, 1].
            let dist = point.x.hypot(point.y);
            let rho = dist.clamp(0.0, 1.0);

            // At the exact polar origin (rho=0), theta is geometrically
            // undefined.  Reuse the previous theta to avoid a spurious
            // angular jump; default to 0.0 for the very first point.
            let theta = if dist == 0.0 {
                prev_theta.unwrap_or(0.0)
            } else {
                // Theta: atan2(x, y) convention (theta=0 points up / +Y).
                // Normalized space is +Y up, matching the Sisyphus
                // ecosystem directly.
                let raw_theta = point.x.atan2(point.y);

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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::suboptimal_flops)]
mod tests {
    use mujou_pipeline::Point;

    use super::*;

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
        let thr = to_thr(&[], &no_meta());
        assert!(thr.starts_with("# mujou\n"));
    }

    #[test]
    fn metadata_title_emitted() {
        let meta = ThrMetadata {
            title: Some("cherry-blossoms.jpg"),
            ..ThrMetadata::default()
        };
        let thr = to_thr(&[], &meta);
        assert!(thr.contains("# Source: cherry-blossoms.jpg\n"));
    }

    #[test]
    fn metadata_description_emitted() {
        let meta = ThrMetadata {
            description: Some("blur=1.4, canny=15/40"),
            ..ThrMetadata::default()
        };
        let thr = to_thr(&[], &meta);
        assert!(thr.contains("# blur=1.4, canny=15/40\n"));
    }

    #[test]
    fn metadata_timestamp_emitted() {
        let meta = ThrMetadata {
            timestamp: Some("2026-02-14_12-30-45"),
            ..ThrMetadata::default()
        };
        let thr = to_thr(&[], &meta);
        assert!(thr.contains("# Exported: 2026-02-14_12-30-45\n"));
    }

    #[test]
    fn metadata_config_json_emitted() {
        let meta = ThrMetadata {
            config_json: Some(r#"{"blur_sigma":1.4}"#),
            ..ThrMetadata::default()
        };
        let thr = to_thr(&[], &meta);
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
        let thr = to_thr(&[], &meta);
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
        let thr = to_thr(&[], &no_meta());
        let pairs = parse_pairs(&thr);
        assert!(pairs.is_empty());
    }

    #[test]
    fn single_point_produces_one_pair() {
        // In normalized space: point at (0.5, 0.0)
        let polylines = vec![Polyline::new(vec![Point::new(0.5, 0.0)])];
        let thr = to_thr(&polylines, &no_meta());
        let pairs = parse_pairs(&thr);
        assert_eq!(pairs.len(), 1);
    }

    // --- Coordinate conversion (normalized space) ---

    #[test]
    fn origin_has_zero_rho() {
        // Point at origin → rho = 0.
        let polylines = vec![Polyline::new(vec![Point::new(0.0, 0.0)])];
        let thr = to_thr(&polylines, &no_meta());
        let pairs = parse_pairs(&thr);
        assert_eq!(pairs.len(), 1);
        assert!((pairs[0].1 - 0.0).abs() < 1e-4, "rho at origin should be 0");
    }

    #[test]
    fn unit_distance_has_rho_one() {
        // Point at (1.0, 0.0) → rho = 1.0.
        let polylines = vec![Polyline::new(vec![Point::new(1.0, 0.0)])];
        let thr = to_thr(&polylines, &no_meta());
        let pairs = parse_pairs(&thr);
        assert_eq!(pairs.len(), 1);
        assert!(
            (pairs[0].1 - 1.0).abs() < 1e-4,
            "rho at unit distance should be 1.0, got {}",
            pairs[0].1,
        );
    }

    #[test]
    fn rho_is_clamped_to_one() {
        // Point beyond unit circle → rho clamped to 1.0.
        let polylines = vec![Polyline::new(vec![Point::new(1.5, 0.0)])];
        let thr = to_thr(&polylines, &no_meta());
        let pairs = parse_pairs(&thr);
        assert!(
            (pairs[0].1 - 1.0).abs() < 1e-4,
            "rho should be clamped to 1.0, got {}",
            pairs[0].1,
        );
    }

    #[test]
    fn atan2_xy_convention_theta_zero_points_up() {
        // atan2(x, y) convention: theta=0 points "up" (+Y in
        // normalized +Y-up space).
        // Point at (0, +1): atan2(0, 1) = 0.
        let polylines = vec![Polyline::new(vec![Point::new(0.0, 1.0)])];
        let thr = to_thr(&polylines, &no_meta());
        let pairs = parse_pairs(&thr);
        assert!(
            pairs[0].0.abs() < 1e-4,
            "theta for point directly 'up' (y=+1) should be ~0, got {}",
            pairs[0].0,
        );
    }

    #[test]
    fn point_right_has_positive_theta() {
        // Point at (1, 0): atan2(1, -0) = atan2(1, 0) = π/2.
        let polylines = vec![Polyline::new(vec![Point::new(1.0, 0.0)])];
        let thr = to_thr(&polylines, &no_meta());
        let pairs = parse_pairs(&thr);
        let expected = std::f64::consts::FRAC_PI_2;
        assert!(
            (pairs[0].0 - expected).abs() < 1e-4,
            "theta for point along +X should be π/2, got {}",
            pairs[0].0,
        );
    }

    // --- Continuous theta unwinding ---

    #[test]
    fn theta_accumulates_counterclockwise() {
        // Counterclockwise path in normalized space (+Y up).
        // With atan2(x, y): "up"→+X→"down"→-X traces 0→π/2→π→3π/2→2π.
        // "up" = positive Y, "down" = negative Y in +Y-up convention.
        let r = 1.0;
        let polylines = vec![Polyline::new(vec![
            Point::new(0.0, r),  // "up"    → theta=0
            Point::new(r, 0.0),  // +X      → theta=π/2
            Point::new(0.0, -r), // "down"  → theta=π
            Point::new(-r, 0.0), // -X      → theta=3π/2
            Point::new(0.0, r),  // "up"    → theta=2π (unwound)
        ])];
        let thr = to_thr(&polylines, &no_meta());
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
        // Clockwise path: "up"→-X→"down"→+X→"up". Theta should decrease.
        let r = 1.0;
        let polylines = vec![Polyline::new(vec![
            Point::new(0.0, r),  // "up"    → theta=0
            Point::new(-r, 0.0), // -X      → theta=-π/2 (clockwise)
            Point::new(0.0, -r), // "down"  → theta=-π
            Point::new(r, 0.0),  // +X      → theta=-3π/2
            Point::new(0.0, r),  // "up"    → theta=-2π (unwound)
        ])];
        let thr = to_thr(&polylines, &no_meta());
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
            Point::new(0.5, 0.0),
            Point::new(0.75, 0.0),
        ])];
        let thr = to_thr(&polylines, &no_meta());
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

    // --- Multiple polylines ---

    #[test]
    fn multiple_polylines_concatenated() {
        let polylines = vec![
            Polyline::new(vec![Point::new(0.5, 0.0), Point::new(0.6, 0.0)]),
            Polyline::new(vec![Point::new(0.7, 0.0), Point::new(0.8, 0.0)]),
        ];
        let thr = to_thr(&polylines, &no_meta());
        let pairs = parse_pairs(&thr);
        assert_eq!(pairs.len(), 4);
    }

    #[test]
    fn theta_continues_across_polylines() {
        // Counterclockwise: "up" → +X → "down" → -X.
        let r = 1.0;
        let polylines = vec![
            Polyline::new(vec![
                Point::new(0.0, r), // "up"   → theta=0
                Point::new(r, 0.0), // +X     → theta=π/2
            ]),
            Polyline::new(vec![
                Point::new(0.0, -r), // "down" → theta=π
                Point::new(-r, 0.0), // -X     → theta=3π/2
            ]),
        ];
        let thr = to_thr(&polylines, &no_meta());
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

        // Use zoom=0.5 so the canvas covers the full 40×40 test image.
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

        let config = PipelineConfig {
            zoom: 0.5,
            ..PipelineConfig::default()
        };
        let result = process_staged(&buf, &config).unwrap();
        let thr = to_thr(std::slice::from_ref(result.final_polyline()), &no_meta());

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
