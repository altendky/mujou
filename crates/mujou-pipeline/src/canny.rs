//! Vendored Canny edge detection from `imageproc 0.26.0`.
//!
//! This is a local copy of `imageproc::edges::{canny, non_maximum_suppression,
//! hysteresis}` with two bug fixes applied to the `hysteresis` function:
//!
//! 1. **`u32` underflow in BFS neighbor computation** — when a pixel at
//!    `x=0` or `y=0` is popped from the BFS stack, `nx - 1` wraps to
//!    `u32::MAX`, causing `get_pixel` to panic (in WASM this manifests as
//!    `RuntimeError: unreachable`). Fixed by bounds-checking each neighbor
//!    coordinate before access.
//!
//! 2. **Missing neighbors** — the original only checks 6 of 8
//!    cardinal/diagonal neighbors, omitting north `(nx, ny-1)` and
//!    northeast `(nx+1, ny-1)`. Fixed by adding the two missing entries.
//!
//! Upstream references:
//! - Issue: <https://github.com/image-rs/imageproc/issues/705>
//! - Fix PR (not yet merged): <https://github.com/image-rs/imageproc/pull/746>
//!
//! Remove this module once the upstream fix is released. Tracked by:
//! <https://github.com/altendky/mujou/issues/69>

// Vendored code — match upstream style, only vary by the fixes.
#[allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::items_after_statements,
    clippy::semicolon_if_nothing_returned,
    clippy::panic,
    unsafe_code
)]
mod inner {
    use image::{GenericImageView, GrayImage, Luma};
    use imageproc::definitions::{HasBlack, HasWhite, Image};
    use imageproc::filter::{filter_clamped, gaussian_blur_f32};
    use imageproc::kernel;
    use std::f32;

    /// Runs the canny edge detection algorithm.
    ///
    /// Identical to `imageproc::edges::canny` except that `hysteresis` is
    /// patched (see module-level docs).
    pub fn canny(image: &GrayImage, low_threshold: f32, high_threshold: f32) -> GrayImage {
        assert!(high_threshold >= low_threshold);
        // Heavily based on the implementation proposed by wikipedia.
        // 1. Gaussian blur.
        const SIGMA: f32 = 1.4;
        let blurred = gaussian_blur_f32(image, SIGMA);

        // 2. Intensity of gradients.
        let gx = filter_clamped(&blurred, kernel::SOBEL_HORIZONTAL_3X3);
        let gy = filter_clamped(&blurred, kernel::SOBEL_VERTICAL_3X3);
        let g: Vec<f32> = gx
            .iter()
            .zip(gy.iter())
            .map(|(h, v)| (*h as f32).hypot(*v as f32))
            .collect::<Vec<f32>>();

        let g = Image::from_raw(image.width(), image.height(), g).unwrap();

        // 3. Non-maximum-suppression (Make edges thinner)
        let thinned = non_maximum_suppression(&g, &gx, &gy);

        // 4. Hysteresis to filter out edges based on thresholds.
        hysteresis(&thinned, low_threshold, high_threshold)
    }

    /// Finds local maxima to make the edges thinner.
    fn non_maximum_suppression(
        g: &Image<Luma<f32>>,
        gx: &Image<Luma<i16>>,
        gy: &Image<Luma<i16>>,
    ) -> Image<Luma<f32>> {
        const RADIANS_TO_DEGREES: f32 = 180f32 / f32::consts::PI;
        let mut out = Image::from_pixel(g.width(), g.height(), Luma([0.0]));
        for y in 1..g.height() - 1 {
            for x in 1..g.width() - 1 {
                let x_gradient = gx[(x, y)][0] as f32;
                let y_gradient = gy[(x, y)][0] as f32;
                let mut angle = (y_gradient).atan2(x_gradient) * RADIANS_TO_DEGREES;
                if angle < 0.0 {
                    angle += 180.0
                }
                // Clamp angle.
                let clamped_angle = if !(22.5..157.5).contains(&angle) {
                    0
                } else if (22.5..67.5).contains(&angle) {
                    45
                } else if (67.5..112.5).contains(&angle) {
                    90
                } else if (112.5..157.5).contains(&angle) {
                    135
                } else {
                    unreachable!()
                };

                // Get the two perpendicular neighbors.
                let (cmp1, cmp2) = unsafe {
                    match clamped_angle {
                        0 => (g.unsafe_get_pixel(x - 1, y), g.unsafe_get_pixel(x + 1, y)),
                        45 => (
                            g.unsafe_get_pixel(x + 1, y + 1),
                            g.unsafe_get_pixel(x - 1, y - 1),
                        ),
                        90 => (g.unsafe_get_pixel(x, y - 1), g.unsafe_get_pixel(x, y + 1)),
                        135 => (
                            g.unsafe_get_pixel(x - 1, y + 1),
                            g.unsafe_get_pixel(x + 1, y - 1),
                        ),
                        _ => unreachable!(),
                    }
                };
                let pixel = *g.get_pixel(x, y);
                // If the pixel is not a local maximum, suppress it.
                if pixel[0] < cmp1[0] || pixel[0] < cmp2[0] {
                    out.put_pixel(x, y, Luma([0.0]));
                } else {
                    out.put_pixel(x, y, pixel);
                }
            }
        }
        out
    }

    /// Filter out edges with the thresholds.
    /// Non-recursive breadth-first search.
    ///
    /// # Changes from upstream `imageproc 0.26.0`
    ///
    /// - **Bounds check**: neighbor coordinates are checked against image
    ///   dimensions before `get_pixel`, preventing `u32` underflow panic
    ///   when BFS reaches the image border.
    ///   (<https://github.com/image-rs/imageproc/issues/705>)
    ///
    /// - **Missing neighbors**: added `(nx, ny-1)` and `(nx+1, ny-1)` to
    ///   check all 8 cardinal/diagonal neighbors (upstream only checked 6).
    ///   (<https://github.com/image-rs/imageproc/pull/746>)
    fn hysteresis(input: &Image<Luma<f32>>, low_thresh: f32, high_thresh: f32) -> Image<Luma<u8>> {
        let max_brightness = Luma::white();
        let min_brightness = Luma::black();
        // Init output image as all black.
        let mut out = Image::from_pixel(input.width(), input.height(), min_brightness);
        // Stack. Possible optimization: Use previously allocated memory, i.e. gx.
        let mut edges = Vec::with_capacity(((input.width() * input.height()) / 2) as usize);
        let (w, h) = (input.width(), input.height()); // FIX: cache for bounds checks
        for y in 1..input.height() - 1 {
            for x in 1..input.width() - 1 {
                let inp_pix = *input.get_pixel(x, y);
                let out_pix = *out.get_pixel(x, y);
                // If the edge strength is higher than high_thresh, mark it as an edge.
                if inp_pix[0] >= high_thresh && out_pix[0] == 0 {
                    out.put_pixel(x, y, max_brightness);
                    edges.push((x, y));
                    // Track neighbors until no neighbor is >= low_thresh.
                    while let Some((nx, ny)) = edges.pop() {
                        // FIX: all 8 neighbors (upstream omitted north and northeast),
                        // using wrapping_sub to avoid u32 underflow panic.
                        let neighbor_indices = [
                            (nx + 1, ny),
                            (nx + 1, ny + 1),
                            (nx, ny + 1),
                            (nx.wrapping_sub(1), ny.wrapping_sub(1)),
                            (nx.wrapping_sub(1), ny),
                            (nx.wrapping_sub(1), ny + 1),
                            (nx, ny.wrapping_sub(1)), // FIX: north (was missing)
                            (nx + 1, ny.wrapping_sub(1)), // FIX: northeast (was missing)
                        ];

                        for neighbor_idx in &neighbor_indices {
                            // FIX: bounds check — skip out-of-bounds neighbors instead
                            // of panicking on u32::MAX from wrapping_sub.
                            if neighbor_idx.0 >= w || neighbor_idx.1 >= h {
                                continue;
                            }
                            let in_neighbor = *input.get_pixel(neighbor_idx.0, neighbor_idx.1);
                            let out_neighbor = *out.get_pixel(neighbor_idx.0, neighbor_idx.1);
                            if in_neighbor[0] >= low_thresh && out_neighbor[0] == 0 {
                                out.put_pixel(neighbor_idx.0, neighbor_idx.1, max_brightness);
                                edges.push((neighbor_idx.0, neighbor_idx.1));
                            }
                        }
                    }
                }
            }
        }
        out
    }
}

pub use inner::canny;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};

    /// Regression test for imageproc#705: hysteresis panics when BFS
    /// reaches the image border due to u32 underflow on `nx - 1`.
    ///
    /// Creates an image with a strong edge at x=1 (one pixel from the
    /// left border). With low thresholds, hysteresis BFS expands to
    /// x=0, then tries to compute 0u32 - 1 = `u32::MAX` for the next
    /// iteration. Without the bounds-check fix this panics.
    #[test]
    fn border_edge_does_not_panic() {
        let mut img = GrayImage::from_pixel(10, 10, Luma([0]));
        // Place a bright column at x=1 to create a strong gradient
        // right next to the left border.
        for y in 0..10 {
            img.put_pixel(0, y, Luma([0]));
            img.put_pixel(1, y, Luma([255]));
        }
        // Low thresholds ensure the BFS will try to expand into border pixels.
        let _edges = canny(&img, 1.0, 2.0);
    }

    /// Verify output dimensions match input.
    #[test]
    fn output_dimensions_match_input() {
        let img = GrayImage::new(17, 31);
        let edges = canny(&img, 50.0, 150.0);
        assert_eq!(edges.width(), 17);
        assert_eq!(edges.height(), 31);
    }

    /// Verify edges are detected on a sharp boundary.
    #[test]
    fn sharp_edge_detected() {
        let img = GrayImage::from_fn(20, 20, |x, _y| if x < 10 { Luma([0]) } else { Luma([255]) });
        let edges = canny(&img, 50.0, 150.0);
        let edge_count: u32 = edges.pixels().map(|p| u32::from(p.0[0] > 0)).sum();
        assert!(edge_count > 0, "expected edges at sharp boundary");
    }
}
