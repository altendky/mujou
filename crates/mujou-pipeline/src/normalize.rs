//! Pixel-to-normalized coordinate transform.
//!
//! Converts pixel-space contour points into a center-origin normalized
//! coordinate system where the mask edge corresponds to 1.0.
//!
//! The transform is:
//!
//! ```text
//! norm_x =  (pixel_x - center_x) × 2 × zoom / shorter_pixel_dim
//! norm_y = -(pixel_y - center_y) × 2 × zoom / shorter_pixel_dim
//! ```
//!
//! The Y-axis is **flipped** so that normalized space uses the
//! mathematical convention of +Y pointing upward.  Export formats that
//! need +Y-down (SVG) handle the flip at the export boundary.
//!
//! After this transform, a unit circle at the origin (radius = 1.0)
//! corresponds to the canvas mask boundary when `zoom = 1.0`.
//!
//! This transform is folded into the contour tracing stage (stage 5)
//! so that all subsequent pipeline stages operate in normalized space.

use crate::types::{Dimensions, Point, Polyline};

/// Normalize a set of pixel-space contour polylines into center-origin
/// coordinates where the mask edge = 1.0.
///
/// The Y-axis is flipped so that normalized space uses the mathematical
/// convention of +Y pointing upward.
///
/// `dimensions` provides the pixel image size (used to compute the
/// center and shorter dimension).  `zoom` controls magnification:
/// values > 1 map a smaller region of the image to the unit circle.
#[must_use]
pub fn normalize_contours(
    contours: Vec<Polyline>,
    dimensions: Dimensions,
    zoom: f64,
) -> Vec<Polyline> {
    let center_x = f64::from(dimensions.width) / 2.0;
    let center_y = f64::from(dimensions.height) / 2.0;
    let shorter = dimensions.shorter_dim();
    // scale_factor = 2 * zoom / shorter_pixel_dim
    let scale_factor = 2.0 * zoom / shorter;

    contours
        .into_iter()
        .map(|polyline| {
            let points: Vec<Point> = polyline
                .into_points()
                .into_iter()
                .map(|p| {
                    Point::new(
                        (p.x - center_x) * scale_factor,
                        (center_y - p.y) * scale_factor,
                    )
                })
                .collect();
            Polyline::new(points)
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn dims(w: u32, h: u32) -> Dimensions {
        Dimensions {
            width: w,
            height: h,
        }
    }

    #[test]
    fn center_maps_to_origin() {
        let contours = vec![Polyline::new(vec![Point::new(50.0, 50.0)])];
        let result = normalize_contours(contours, dims(100, 100), 1.0);
        let p = result[0].points()[0];
        assert!((p.x).abs() < 1e-10, "center x should be 0, got {}", p.x);
        assert!((p.y).abs() < 1e-10, "center y should be 0, got {}", p.y);
    }

    #[test]
    fn edge_maps_to_one() {
        // Right edge of shorter dimension: pixel (100, 50) on a 100x100 image.
        // norm_x = (100 - 50) * 2 * 1.0 / 100 = 1.0
        let contours = vec![Polyline::new(vec![Point::new(100.0, 50.0)])];
        let result = normalize_contours(contours, dims(100, 100), 1.0);
        let p = result[0].points()[0];
        assert!(
            (p.x - 1.0).abs() < 1e-10,
            "right edge should map to x=1.0, got {}",
            p.x,
        );
        assert!((p.y).abs() < 1e-10, "center y should be 0, got {}", p.y);
    }

    #[test]
    fn y_flips_direction() {
        // Pixel below center (larger Y) should map to negative normalized Y.
        // pixel (50, 100) on 100x100: norm_y = (50 - 100) * 2 / 100 = -1.0
        let contours = vec![Polyline::new(vec![Point::new(50.0, 100.0)])];
        let result = normalize_contours(contours, dims(100, 100), 1.0);
        let p = result[0].points()[0];
        assert!(
            (p.y - (-1.0)).abs() < 1e-10,
            "bottom edge should map to y=-1.0, got {}",
            p.y,
        );
    }

    #[test]
    fn top_maps_to_positive_y() {
        // Pixel above center (smaller Y) should map to positive normalized Y.
        // pixel (50, 0) on 100x100: norm_y = (50 - 0) * 2 / 100 = 1.0
        let contours = vec![Polyline::new(vec![Point::new(50.0, 0.0)])];
        let result = normalize_contours(contours, dims(100, 100), 1.0);
        let p = result[0].points()[0];
        assert!(
            (p.y - 1.0).abs() < 1e-10,
            "top should map to y=1.0, got {}",
            p.y,
        );
    }

    #[test]
    fn zoom_scales() {
        // zoom=2 means 2x magnification: right edge maps to 2.0.
        let contours = vec![Polyline::new(vec![Point::new(100.0, 50.0)])];
        let result = normalize_contours(contours, dims(100, 100), 2.0);
        let p = result[0].points()[0];
        assert!(
            (p.x - 2.0).abs() < 1e-10,
            "zoom=2 should double the coordinate, got {}",
            p.x,
        );
    }

    #[test]
    fn nonsquare_uses_shorter_dim() {
        // 200x100 image: shorter dim = 100.
        // pixel (150, 50) → norm_x = (150-100) * 2 * 1.0 / 100 = 1.0
        let contours = vec![Polyline::new(vec![Point::new(150.0, 50.0)])];
        let result = normalize_contours(contours, dims(200, 100), 1.0);
        let p = result[0].points()[0];
        assert!(
            (p.x - 1.0).abs() < 1e-10,
            "should use shorter dim, got x={}",
            p.x,
        );
    }
}
