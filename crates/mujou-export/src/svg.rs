//! SVG export serializer.
//!
//! Converts polylines into an SVG string with `<path>` elements.
//! Each polyline becomes a separate `<path>` element using `M` (move to)
//! and `L` (line to) commands.
//!
//! Optional [`SvgMetadata`] embeds `<title>` and `<desc>` elements for
//! accessibility and to help file managers identify exported files.
//!
//! This is a pure function with no I/O -- it returns a `String`.

use std::fmt::Write;

use mujou_pipeline::{Dimensions, Polyline};

/// Metadata to embed in the SVG document.
///
/// Both fields are optional.  When present, a `<title>` and/or `<desc>`
/// element is emitted immediately after the opening `<svg>` tag.  These
/// are standard SVG accessibility elements and are surfaced by some file
/// managers and screen readers.
///
/// Text values are XML-escaped automatically (see [`xml_escape`]).
#[derive(Debug, Clone, Default)]
pub struct SvgMetadata<'a> {
    /// Document title — emitted as `<title>`.
    ///
    /// Typically the source image filename (without extension).
    pub title: Option<&'a str>,

    /// Document description — emitted as `<desc>`.
    ///
    /// Typically contains pipeline parameters and a timestamp so
    /// exported files are distinguishable.
    pub description: Option<&'a str>,

    /// Structured pipeline configuration JSON — emitted inside a
    /// `<metadata>` element wrapped in a namespaced `<mujou:pipeline>`
    /// element.
    ///
    /// When present, the full serialized [`PipelineConfig`] is embedded
    /// so exported files carry machine-parseable settings for
    /// reproducibility.  The human-readable `description` is retained
    /// separately.
    pub config_json: Option<&'a str>,
}

/// Escape the five XML special characters for safe embedding in element
/// text content and attribute values.
///
/// Handles `&` (must be first), `<`, `>`, `"`, and `'`.
fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

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
/// If [`SvgMetadata::title`] or [`SvgMetadata::description`] is
/// provided, the corresponding `<title>` / `<desc>` element is emitted
/// after the opening `<svg>` tag.  If [`SvgMetadata::config_json`] is
/// provided, a `<metadata>` element is emitted containing the JSON
/// wrapped in a namespaced `<mujou:pipeline>` element.
///
/// # Examples
///
/// ```
/// use mujou_pipeline::{Dimensions, Point, Polyline};
/// use mujou_export::{SvgMetadata, to_svg};
///
/// let polylines = vec![
///     Polyline::new(vec![Point::new(10.0, 15.0), Point::new(12.5, 18.3)]),
/// ];
/// let dims = Dimensions { width: 800, height: 600 };
/// let metadata = SvgMetadata {
///     title: Some("cherry-blossoms"),
///     description: Some("Exported by mujou"),
///     ..SvgMetadata::default()
/// };
/// let svg = to_svg(&polylines, dims, &metadata);
/// assert!(svg.contains("width=\"800\" height=\"600\""));
/// assert!(svg.contains("viewBox=\"0 0 800 600\""));
/// assert!(svg.contains("<title>cherry-blossoms</title>"));
/// assert!(svg.contains("M 10.0 15.0 L 12.5 18.3"));
/// ```
#[must_use]
pub fn to_svg(
    polylines: &[Polyline],
    dimensions: Dimensions,
    metadata: &SvgMetadata<'_>,
) -> String {
    let mut out = String::new();

    // XML declaration
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);

    // Opening <svg> tag with namespace, explicit dimensions, and viewBox
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
        dimensions.width, dimensions.height, dimensions.width, dimensions.height,
    );

    // Optional <title> element
    if let Some(title) = metadata.title {
        let _ = writeln!(out, "  <title>{}</title>", xml_escape(title));
    }

    // Optional <desc> element
    if let Some(description) = metadata.description {
        let _ = writeln!(out, "  <desc>{}</desc>", xml_escape(description));
    }

    // Optional <metadata> element with structured pipeline config
    if let Some(config_json) = metadata.config_json {
        let _ = writeln!(out, "  <metadata>");
        let _ = writeln!(
            out,
            "    <mujou:pipeline xmlns:mujou=\"https://mujou.app/ns/1\">{}</mujou:pipeline>",
            xml_escape(config_json),
        );
        let _ = writeln!(out, "  </metadata>");
    }

    // One <path> per polyline (skip polylines with fewer than 2 points)
    for polyline in polylines {
        let points = polyline.points();
        if points.len() < 2 {
            continue;
        }

        let d: String = points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let cmd = if i == 0 { "M" } else { "L" };
                format!("{cmd} {:.1} {:.1}", p.x, p.y)
            })
            .collect::<Vec<_>>()
            .join(" ");

        let _ = writeln!(
            out,
            r#"  <path d="{d}" fill="none" stroke="black" stroke-width="1"/>"#,
        );
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

    /// Shorthand: no metadata (most existing tests don't care about it).
    fn no_meta() -> SvgMetadata<'static> {
        SvgMetadata::default()
    }

    // --- Empty / degenerate inputs ---

    #[test]
    fn empty_polylines_produces_valid_svg_with_no_paths() {
        let svg = to_svg(&[], dims(100, 50), &no_meta());
        assert!(svg.contains(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(svg.contains(r#"width="100" height="50""#));
        assert!(svg.contains(r#"viewBox="0 0 100 50""#));
        assert!(!svg.contains("<path"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn single_point_polyline_is_skipped() {
        let polylines = vec![Polyline::new(vec![Point::new(5.0, 5.0)])];
        let svg = to_svg(&polylines, dims(100, 100), &no_meta());
        assert!(!svg.contains("<path"));
    }

    #[test]
    fn zero_point_polyline_is_skipped() {
        let polylines = vec![Polyline::new(vec![])];
        let svg = to_svg(&polylines, dims(100, 100), &no_meta());
        assert!(!svg.contains("<path"));
    }

    // --- Basic output structure ---

    #[test]
    fn single_polyline_with_two_points() {
        let polylines = vec![Polyline::new(vec![
            Point::new(10.0, 20.0),
            Point::new(30.0, 40.0),
        ])];
        let svg = to_svg(&polylines, dims(800, 600), &no_meta());

        assert!(svg.contains(r#"width="800" height="600""#));
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
        let svg = to_svg(&polylines, dims(800, 600), &no_meta());
        assert!(svg.contains(r#"d="M 10.0 15.0 L 12.5 18.3 L 14.0 20.1""#));
    }

    #[test]
    fn multiple_polylines_produce_multiple_paths() {
        let polylines = vec![
            Polyline::new(vec![Point::new(1.0, 2.0), Point::new(3.0, 4.0)]),
            Polyline::new(vec![Point::new(5.0, 6.0), Point::new(7.0, 8.0)]),
        ];
        let svg = to_svg(&polylines, dims(100, 100), &no_meta());

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
        let svg = to_svg(&polylines, dims(100, 100), &no_meta());

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
        let svg = to_svg(&polylines, dims(100, 100), &no_meta());
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
        let svg = to_svg(&polylines, dims(100, 100), &no_meta());

        let path_count = svg.matches("<path").count();
        assert_eq!(path_count, 1);
        assert!(svg.contains(r#"d="M 2.0 3.0 L 4.0 5.0""#));
    }

    // --- Dimensions ---

    #[test]
    fn viewbox_reflects_dimensions() {
        let svg = to_svg(&[], dims(1920, 1080), &no_meta());
        assert!(svg.contains(r#"width="1920" height="1080""#));
        assert!(svg.contains(r#"viewBox="0 0 1920 1080""#));
    }

    // --- SVG structure ---

    #[test]
    fn svg_has_xml_declaration() {
        let svg = to_svg(&[], dims(100, 100), &no_meta());
        assert!(svg.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
    }

    #[test]
    fn svg_has_xmlns_namespace() {
        let svg = to_svg(&[], dims(100, 100), &no_meta());
        assert!(svg.contains(r#"xmlns="http://www.w3.org/2000/svg""#));
    }

    #[test]
    fn svg_ends_with_closing_tag() {
        let svg = to_svg(&[], dims(100, 100), &no_meta());
        let trimmed = svg.trim_end();
        assert!(trimmed.ends_with("</svg>"));
    }

    // --- Metadata ---

    #[test]
    fn title_element_emitted_when_present() {
        let meta = SvgMetadata {
            title: Some("cherry-blossoms"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta);
        assert!(svg.contains("  <title>cherry-blossoms</title>"));
        assert!(!svg.contains("<desc>"));
    }

    #[test]
    fn desc_element_emitted_when_present() {
        let meta = SvgMetadata {
            description: Some("Exported by mujou"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta);
        assert!(svg.contains("  <desc>Exported by mujou</desc>"));
        assert!(!svg.contains("<title>"));
    }

    #[test]
    fn title_and_desc_both_emitted() {
        let meta = SvgMetadata {
            title: Some("my-image"),
            description: Some("blur=1.4, canny=15/40"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta);
        assert!(svg.contains("  <title>my-image</title>"));
        assert!(svg.contains("  <desc>blur=1.4, canny=15/40</desc>"));
    }

    #[test]
    fn title_and_desc_omitted_when_none() {
        let svg = to_svg(&[], dims(100, 100), &no_meta());
        assert!(!svg.contains("<title>"));
        assert!(!svg.contains("<desc>"));
    }

    #[test]
    fn title_appears_before_paths() {
        let polylines = vec![Polyline::new(vec![
            Point::new(1.0, 2.0),
            Point::new(3.0, 4.0),
        ])];
        let meta = SvgMetadata {
            title: Some("test"),
            description: Some("desc"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&polylines, dims(100, 100), &meta);

        let title_pos = svg.find("<title>").unwrap();
        let desc_pos = svg.find("<desc>").unwrap();
        let path_pos = svg.find("<path").unwrap();
        assert!(title_pos < desc_pos, "title should come before desc");
        assert!(desc_pos < path_pos, "desc should come before paths");
    }

    #[test]
    fn special_characters_in_title_are_escaped() {
        let meta = SvgMetadata {
            title: Some("A <B> & C \"D\" 'E'"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta);
        assert!(svg.contains("<title>A &lt;B&gt; &amp; C &quot;D&quot; &apos;E&apos;</title>"));
    }

    #[test]
    fn special_characters_in_desc_are_escaped() {
        let meta = SvgMetadata {
            description: Some("x < y & z > w"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta);
        assert!(svg.contains("<desc>x &lt; y &amp; z &gt; w</desc>"));
    }

    // --- Config JSON / <metadata> ---

    #[test]
    fn metadata_element_emitted_when_config_json_present() {
        let meta = SvgMetadata {
            config_json: Some(r#"{"blur_sigma":1.4}"#),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta);
        assert!(svg.contains("<metadata>"));
        assert!(svg.contains("</metadata>"));
        assert!(svg.contains(r#"<mujou:pipeline xmlns:mujou="https://mujou.app/ns/1">"#));
        // JSON quotes are XML-escaped
        assert!(svg.contains(r"{&quot;blur_sigma&quot;:1.4}</mujou:pipeline>"));
    }

    #[test]
    fn metadata_element_omitted_when_config_json_none() {
        let svg = to_svg(&[], dims(100, 100), &no_meta());
        assert!(!svg.contains("<metadata>"));
    }

    #[test]
    fn config_json_special_characters_are_escaped() {
        // JSON with quotes won't normally contain XML specials, but
        // verify the escaping works if they do appear.
        let meta = SvgMetadata {
            config_json: Some(r#"{"note":"a < b & c > d"}"#),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta);
        assert!(
            svg.contains(
                r"{&quot;note&quot;:&quot;a &lt; b &amp; c &gt; d&quot;}</mujou:pipeline>"
            )
        );
    }

    #[test]
    fn metadata_appears_after_desc_and_before_paths() {
        let polylines = vec![Polyline::new(vec![
            Point::new(1.0, 2.0),
            Point::new(3.0, 4.0),
        ])];
        let meta = SvgMetadata {
            title: Some("test"),
            description: Some("desc"),
            config_json: Some(r#"{"blur_sigma":1.4}"#),
        };
        let svg = to_svg(&polylines, dims(100, 100), &meta);

        let desc_pos = svg.find("<desc>").unwrap();
        let metadata_pos = svg.find("<metadata>").unwrap();
        let path_pos = svg.find("<path").unwrap();
        assert!(desc_pos < metadata_pos, "desc should come before metadata");
        assert!(metadata_pos < path_pos, "metadata should come before paths");
    }

    // --- XML escaping ---

    #[test]
    fn xml_escape_handles_all_special_chars() {
        assert_eq!(xml_escape("&<>\"'"), "&amp;&lt;&gt;&quot;&apos;");
    }

    #[test]
    fn xml_escape_passes_through_plain_text() {
        assert_eq!(xml_escape("hello world 123"), "hello world 123");
    }

    #[test]
    fn xml_escape_empty_string() {
        assert_eq!(xml_escape(""), "");
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
        let svg = to_svg(&[result.polyline], result.dimensions, &no_meta());

        // Valid SVG structure
        assert!(svg.contains(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(svg.contains(r#"width="40" height="40""#));
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
        let svg = to_svg(&polylines, dims(800, 600), &no_meta());

        assert!(svg.contains(r#"width="800" height="600""#));
        assert!(svg.contains(r#"viewBox="0 0 800 600""#));
        assert!(svg.contains(r#"d="M 10.0 15.0 L 12.5 18.3 L 14.0 20.1""#));
        assert!(svg.contains(r#"d="M 30.0 5.0 L 32.5 7.8 L 35.0 10.2""#));
        assert!(svg.contains(r#"fill="none" stroke="black" stroke-width="1""#));
    }
}
