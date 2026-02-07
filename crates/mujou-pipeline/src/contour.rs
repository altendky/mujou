//! Contour tracing: extract polylines from a binary edge map.
//!
//! This module defines the [`ContourTracer`] trait for pluggable contour
//! tracing algorithms and the [`ContourTracerKind`] enum for selecting
//! which algorithm to use at runtime.
//!
//! # Strategy pattern
//!
//! Different contour tracing algorithms produce different geometry from the
//! same edge map. The trait/enum design lets the user choose which algorithm
//! to use from the UI while keeping all implementations in the core layer
//! with no I/O dependencies.

use image::GrayImage;

use crate::types::{Point, Polyline};

/// Selects which contour tracing algorithm to use.
///
/// MVP ships with [`BorderFollowing`](Self::BorderFollowing) only.
/// Additional variants (e.g. marching squares) can be added without
/// changing the `PipelineConfig` struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContourTracerKind {
    /// Suzuki-Abe border following via `imageproc::contours::find_contours`.
    ///
    /// Fast, zero custom code. On 1-pixel-wide Canny edges this produces
    /// doubled borders that RDP simplification collapses in practice.
    #[default]
    BorderFollowing,
}

/// Trait for contour tracing strategies.
///
/// Input: a binary edge map (white pixels = edges, black = background).
/// Output: a set of disconnected polylines, one per contour.
pub trait ContourTracer {
    /// Trace contours in the given binary edge map.
    fn trace(&self, edges: &GrayImage) -> Vec<Polyline>;
}

impl ContourTracer for ContourTracerKind {
    fn trace(&self, edges: &GrayImage) -> Vec<Polyline> {
        match *self {
            Self::BorderFollowing => trace_border_following(edges),
        }
    }
}

/// Suzuki-Abe border following via `imageproc::contours::find_contours`.
///
/// Converts `imageproc` contour points (integer grid coordinates) into
/// floating-point [`Point`]s.
fn trace_border_following(edges: &GrayImage) -> Vec<Polyline> {
    let contours: Vec<imageproc::contours::Contour<u32>> =
        imageproc::contours::find_contours(edges);

    contours
        .into_iter()
        .filter(|c| c.points.len() >= 2)
        .map(|c| {
            let points = c
                .points
                .into_iter()
                .map(|p| Point::new(f64::from(p.x), f64::from(p.y)))
                .collect();
            Polyline::new(points)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_border_following() {
        assert_eq!(
            ContourTracerKind::default(),
            ContourTracerKind::BorderFollowing
        );
    }

    #[test]
    fn empty_image_produces_no_contours() {
        let img = GrayImage::new(10, 10); // all black
        let result = ContourTracerKind::BorderFollowing.trace(&img);
        assert!(result.is_empty());
    }

    #[test]
    fn single_pixel_contour_is_filtered_out() {
        // A single white pixel produces a contour with only 1 point,
        // which our filter removes (need >= 2 points for a polyline).
        let mut img = GrayImage::new(10, 10);
        img.put_pixel(5, 5, image::Luma([255]));
        let result = ContourTracerKind::BorderFollowing.trace(&img);
        // Single pixel may produce a very short contour; we only keep >= 2 points.
        for polyline in &result {
            assert!(polyline.len() >= 2);
        }
    }

    #[test]
    fn rectangle_produces_contours() {
        // Draw a filled white rectangle on a black background.
        // Border following should find contour(s) around it.
        let mut img = GrayImage::new(20, 20);
        for y in 5..15 {
            for x in 5..15 {
                img.put_pixel(x, y, image::Luma([255]));
            }
        }
        let result = ContourTracerKind::BorderFollowing.trace(&img);
        assert!(
            !result.is_empty(),
            "expected at least one contour from a rectangle"
        );
        // Each contour should have meaningful point count
        for polyline in &result {
            assert!(
                polyline.len() >= 4,
                "rectangle contour should have at least 4 points"
            );
        }
    }
}
