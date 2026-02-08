//! SVG export serializer.
//!
//! Converts polylines into an SVG string with `<path>` elements.
//! Each polyline becomes a separate `<path>` element using `M` (move to)
//! and `L` (line to) commands.
//!
//! This is a pure function with no I/O -- it returns a `String`.

use std::fmt::Write;

use mujou_pipeline::{Dimensions, Polyline};

/// Serialize polylines into an SVG document string.
///
/// Each [`Polyline`] with 2 or more points becomes a `<path>` element.
/// Polylines with fewer than 2 points are skipped (a single point
/// cannot form a visible line segment).
///
/// The `viewBox` is set from [`Dimensions`] so the SVG coordinate
/// space matches the source image pixel grid.
///
/// Coordinates are formatted to 1 decimal place (0.1 px precision).
///
/// # Examples
///
/// ```
/// use mujou_pipeline::{Dimensions, Point, Polyline};
/// use mujou_export::to_svg;
///
/// let polylines = vec![
///     Polyline::new(vec![Point::new(10.0, 15.0), Point::new(12.5, 18.3)]),
/// ];
/// let dims = Dimensions { width: 800, height: 600 };
/// let svg = to_svg(&polylines, dims);
/// assert!(svg.contains("viewBox=\"0 0 800 600\""));
/// assert!(svg.contains("M 10.0 15.0 L 12.5 18.3"));
/// ```
#[must_use]
pub fn to_svg(polylines: &[Polyline], dimensions: Dimensions) -> String {
    let mut out = String::new();

    // XML declaration
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);

    // Opening <svg> tag with namespace and viewBox
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}">"#,
        dimensions.width, dimensions.height,
    );

    // One <path> per polyline (skip polylines with fewer than 2 points)
    for polyline in polylines {
        let points = polyline.points();
        if points.len() < 2 {
            continue;
        }

        let _ = write!(out, r#"  <path d=""#);

        for (i, point) in points.iter().enumerate() {
            let cmd = if i == 0 { "M" } else { "L" };
            let _ = write!(out, "{cmd} {:.1} {:.1}", point.x, point.y);

            if i < points.len() - 1 {
                let _ = write!(out, " ");
            }
        }

        let _ = writeln!(out, r#"" fill="none" stroke="black" stroke-width="1"/>"#);
    }

    // Closing tag
    let _ = writeln!(out, "</svg>");

    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use mujou_pipeline::Point;

    use super::*;

    fn dims(width: u32, height: u32) -> Dimensions {
        Dimensions { width, height }
    }

    // --- Empty / degenerate inputs ---

    #[test]
    fn empty_polylines_produces_valid_svg_with_no_paths() {
        let svg = to_svg(&[], dims(100, 50));
        assert!(svg.contains(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(svg.contains(r#"viewBox="0 0 100 50""#));
        assert!(!svg.contains("<path"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn single_point_polyline_is_skipped() {
        let polylines = vec![Polyline::new(vec![Point::new(5.0, 5.0)])];
        let svg = to_svg(&polylines, dims(100, 100));
        assert!(!svg.contains("<path"));
    }

    #[test]
    fn zero_point_polyline_is_skipped() {
        let polylines = vec![Polyline::new(vec![])];
        let svg = to_svg(&polylines, dims(100, 100));
        assert!(!svg.contains("<path"));
    }

    // --- Basic output structure ---

    #[test]
    fn single_polyline_with_two_points() {
        let polylines = vec![Polyline::new(vec![
            Point::new(10.0, 20.0),
            Point::new(30.0, 40.0),
        ])];
        let svg = to_svg(&polylines, dims(800, 600));

        assert!(svg.contains(r#"viewBox="0 0 800 600""#));
        assert!(svg.contains(r#"d="M 10.0 20.0 L 30.0 40.0""#));
        assert!(svg.contains(r#"fill="none""#));
        assert!(svg.contains(r#"stroke="black""#));
        assert!(svg.contains(r#"stroke-width="1""#));
    }

    #[test]
    fn single_polyline_with_three_points() {
        let polylines = vec![Polyline::new(vec![
            Point::new(10.0, 15.0),
            Point::new(12.5, 18.3),
            Point::new(14.0, 20.1),
        ])];
        let svg = to_svg(&polylines, dims(800, 600));
        assert!(svg.contains(r#"d="M 10.0 15.0 L 12.5 18.3 L 14.0 20.1""#));
    }

    #[test]
    fn multiple_polylines_produce_multiple_paths() {
        let polylines = vec![
            Polyline::new(vec![Point::new(1.0, 2.0), Point::new(3.0, 4.0)]),
            Polyline::new(vec![Point::new(5.0, 6.0), Point::new(7.0, 8.0)]),
        ];
        let svg = to_svg(&polylines, dims(100, 100));

        // Count <path occurrences
        let path_count = svg.matches("<path").count();
        assert_eq!(path_count, 2);

        assert!(svg.contains(r#"d="M 1.0 2.0 L 3.0 4.0""#));
        assert!(svg.contains(r#"d="M 5.0 6.0 L 7.0 8.0""#));
    }

    // --- Coordinate formatting ---

    #[test]
    fn coordinates_formatted_to_one_decimal_place() {
        let polylines = vec![Polyline::new(vec![
            Point::new(1.0 / 3.0, 2.0 / 3.0),
            Point::new(10.0, 20.0),
        ])];
        let svg = to_svg(&polylines, dims(100, 100));

        // 1/3 ≈ 0.333... should be formatted as "0.3"
        // 2/3 ≈ 0.666... should be formatted as "0.7"
        assert!(svg.contains("M 0.3 0.7 L 10.0 20.0"));
    }

    #[test]
    fn integer_coordinates_show_one_decimal() {
        let polylines = vec![Polyline::new(vec![
            Point::new(5.0, 10.0),
            Point::new(15.0, 20.0),
        ])];
        let svg = to_svg(&polylines, dims(100, 100));
        assert!(svg.contains("M 5.0 10.0 L 15.0 20.0"));
    }

    // --- Mixed valid and degenerate polylines ---

    #[test]
    fn degenerate_polylines_skipped_among_valid_ones() {
        let polylines = vec![
            Polyline::new(vec![]),                                           // skipped
            Polyline::new(vec![Point::new(1.0, 1.0)]),                       // skipped
            Polyline::new(vec![Point::new(2.0, 3.0), Point::new(4.0, 5.0)]), // kept
            Polyline::new(vec![]),                                           // skipped
        ];
        let svg = to_svg(&polylines, dims(100, 100));

        let path_count = svg.matches("<path").count();
        assert_eq!(path_count, 1);
        assert!(svg.contains(r#"d="M 2.0 3.0 L 4.0 5.0""#));
    }

    // --- Dimensions ---

    #[test]
    fn viewbox_reflects_dimensions() {
        let svg = to_svg(&[], dims(1920, 1080));
        assert!(svg.contains(r#"viewBox="0 0 1920 1080""#));
    }

    // --- SVG structure ---

    #[test]
    fn svg_has_xml_declaration() {
        let svg = to_svg(&[], dims(100, 100));
        assert!(svg.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
    }

    #[test]
    fn svg_has_xmlns_namespace() {
        let svg = to_svg(&[], dims(100, 100));
        assert!(svg.contains(r#"xmlns="http://www.w3.org/2000/svg""#));
    }

    #[test]
    fn svg_ends_with_closing_tag() {
        let svg = to_svg(&[], dims(100, 100));
        let trimmed = svg.trim_end();
        assert!(trimmed.ends_with("</svg>"));
    }

    // --- End-to-end: process() -> to_svg() ---

    /// Create a test PNG with a sharp black/white vertical edge.
    fn sharp_edge_png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(width, height, |x, _y| {
            if x < width / 2 {
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
        buf
    }

    #[test]
    fn end_to_end_image_to_svg() {
        use mujou_pipeline::{PipelineConfig, process};

        let png = sharp_edge_png(40, 40);
        let result = process(&png, &PipelineConfig::default()).unwrap();
        let svg = to_svg(&[result.polyline], result.dimensions);

        // Valid SVG structure
        assert!(svg.contains(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(svg.contains(r#"viewBox="0 0 40 40""#));
        assert!(svg.contains("<path"));
        assert!(svg.contains("</svg>"));

        // At least one path with M and L commands
        assert!(svg.contains("M "));
        assert!(svg.contains("L "));
    }

    // --- Example from formats.md ---

    #[test]
    fn matches_formats_doc_example() {
        let polylines = vec![
            Polyline::new(vec![
                Point::new(10.0, 15.0),
                Point::new(12.5, 18.3),
                Point::new(14.0, 20.1),
            ]),
            Polyline::new(vec![
                Point::new(30.0, 5.0),
                Point::new(32.5, 7.8),
                Point::new(35.0, 10.2),
            ]),
        ];
        let svg = to_svg(&polylines, dims(800, 600));

        assert!(svg.contains(r#"viewBox="0 0 800 600""#));
        assert!(svg.contains(r#"d="M 10.0 15.0 L 12.5 18.3 L 14.0 20.1""#));
        assert!(svg.contains(r#"d="M 30.0 5.0 L 32.5 7.8 L 35.0 10.2""#));
        assert!(svg.contains(r#"fill="none" stroke="black" stroke-width="1""#));
    }
}
