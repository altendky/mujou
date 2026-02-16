//! SVG export serializer.
//!
//! Converts polylines into an SVG string with `<path>` elements using
//! the [`svg`] crate for document construction, XML escaping, and path
//! data formatting.
//!
//! Each polyline becomes a separate `<path>` element using `M` (move to)
//! and `L` (line to) commands.
//!
//! Optional [`SvgMetadata`] embeds `<title>` and `<desc>` elements for
//! accessibility and to help file managers identify exported files.
//!
//! This is a pure function with no I/O -- it returns a `String`.

use std::fmt::Write;

use svg::Document;
use svg::node::element::path::Data;
use svg::node::element::{Description, Element, Path, Title};
use svg::node::{Node, Text, Value};

use mujou_pipeline::{Dimensions, MaskShape, MstEdgeInfo, Polyline};

/// Metadata to embed in the SVG document.
///
/// Both fields are optional.  When present, a `<title>` and/or `<desc>`
/// element is emitted immediately after the opening `<svg>` tag.  These
/// are standard SVG accessibility elements and are surfaced by some file
/// managers and screen readers.
///
/// Text values are XML-escaped automatically by the `svg` crate.
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

/// Build an SVG path `d` attribute string from a polyline.
///
/// Uses `M` for the first point and `L` for subsequent points.
/// Returns an empty string for polylines with fewer than 2 points.
///
/// Coordinates are formatted by the [`svg`] crate using `f32` precision
/// (sufficient for pixel-derived coordinates from the pipeline).
///
/// # Examples
///
/// ```
/// use mujou_pipeline::{Point, Polyline};
/// use mujou_export::build_path_data;
///
/// let polyline = Polyline::new(vec![
///     Point::new(10.0, 20.0),
///     Point::new(30.0, 40.0),
/// ]);
/// let d = build_path_data(&polyline);
/// assert_eq!(d, "M10,20 L30,40");
/// ```
#[must_use]
pub fn build_path_data(polyline: &Polyline) -> String {
    let points = polyline.points();
    if points.len() < 2 {
        return String::new();
    }

    let first = &points[0];
    let mut data = Data::new().move_to((first.x, first.y));
    for p in &points[1..] {
        data = data.line_to((p.x, p.y));
    }
    String::from(Value::from(data))
}

// ---------------------------------------------------------------------------
// Shared helpers for diagnostic SVG functions (manual string formatting)
// ---------------------------------------------------------------------------

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

/// Write the SVG preamble: XML declaration, opening `<svg>` tag, and
/// optional `<title>`, `<desc>`, and `<metadata>` elements.
///
/// Used by the diagnostic SVG functions which still use manual string
/// formatting (the primary [`to_svg`] uses the `svg` crate).
fn write_svg_preamble(out: &mut String, dimensions: Dimensions, metadata: &SvgMetadata<'_>) {
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
}

/// Build the SVG `d` attribute string for a polyline (manual formatting).
///
/// Returns `None` if the polyline has fewer than 2 points (cannot form
/// a visible line segment).  Coordinates are formatted to 1 decimal
/// place (0.1 px precision).
fn polyline_to_path_d(polyline: &Polyline) -> Option<String> {
    let points = polyline.points();
    if points.len() < 2 {
        return None;
    }
    Some(
        points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let cmd = if i == 0 { "M" } else { "L" };
                format!("{cmd} {:.1} {:.1}", p.x, p.y)
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Write polyline `<path>` elements at the given indentation level.
///
/// Each polyline with 2+ points becomes a `<path>` element.  When
/// `attrs` is non-empty it is appended after the `d` attribute
/// (e.g. `fill="none" stroke="black" stroke-width="1"`).
fn write_polyline_paths(out: &mut String, polylines: &[Polyline], indent: &str, attrs: &str) {
    for polyline in polylines {
        if let Some(d) = polyline_to_path_d(polyline) {
            if attrs.is_empty() {
                let _ = writeln!(out, r#"{indent}<path d="{d}"/>"#);
            } else {
                let _ = writeln!(out, r#"{indent}<path d="{d}" {attrs}/>"#);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Primary SVG export (uses `svg` crate)
// ---------------------------------------------------------------------------

/// Serialize polylines into an SVG document string.
///
/// Each [`Polyline`] with 2 or more points becomes a `<path>` element.
/// Polylines with fewer than 2 points are skipped (a single point
/// cannot form a visible line segment).
///
/// When `mask_shape` is `Some`, the `viewBox` is set to a **square**
/// bounding box centered on the mask circle, with explicit
/// `preserveAspectRatio="xMidYMid meet"`.  This ensures the output
/// is correctly centered and fills the drawing area on circular
/// devices (e.g. Oasis Mini).  When `None`, the `viewBox` is set
/// from [`Dimensions`] so the SVG coordinate space matches the source
/// image pixel grid.
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
/// let svg = to_svg(&polylines, dims, &metadata, None);
/// assert!(svg.contains("<title>cherry-blossoms</title>"));
/// assert!(svg.contains("<desc>Exported by mujou</desc>"));
/// assert!(svg.contains("M10,15 L12.5,18.3"));
/// ```
#[must_use]
pub fn to_svg(
    polylines: &[Polyline],
    dimensions: Dimensions,
    metadata: &SvgMetadata<'_>,
    mask_shape: Option<&MaskShape>,
) -> String {
    let mut doc = if let Some(MaskShape::Circle { center, radius }) = mask_shape {
        let size = 2.0 * radius;
        let min_x = center.x - radius;
        let min_y = center.y - radius;
        Document::new()
            .set("width", size)
            .set("height", size)
            .set("viewBox", format!("{min_x} {min_y} {size} {size}"))
            .set("preserveAspectRatio", "xMidYMid meet")
    } else {
        let w = dimensions.width;
        let h = dimensions.height;
        Document::new()
            .set("width", w)
            .set("height", h)
            .set("viewBox", (0, 0, w, h))
    };

    // Optional <title> element
    if let Some(title) = metadata.title {
        doc = doc.add(Title::new(title));
    }

    // Optional <desc> element
    if let Some(description) = metadata.description {
        doc = doc.add(Description::new().add(Text::new(description)));
    }

    // Optional <metadata> element with structured pipeline config
    if let Some(config_json) = metadata.config_json {
        let mut pipeline_el = Element::new("mujou:pipeline");
        pipeline_el.assign("xmlns:mujou", "https://mujou.app/ns/1");
        pipeline_el.append(Text::new(config_json));
        let mut metadata_el = Element::new("metadata");
        metadata_el.append(pipeline_el);
        doc = doc.add(metadata_el);
    }

    // One <path> per polyline (skip polylines with fewer than 2 points)
    for polyline in polylines {
        let d = build_path_data(polyline);
        if d.is_empty() {
            continue;
        }

        let path = Path::new()
            .set("d", d)
            .set("fill", "none")
            .set("stroke", "black")
            .set("stroke-width", 1);
        doc = doc.add(path);
    }

    // The svg crate omits the XML declaration, so we prepend it.
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{doc}\n")
}

// ---------------------------------------------------------------------------
// Diagnostic SVG exports (manual string formatting)
// ---------------------------------------------------------------------------

/// Serialize polylines into a diagnostic SVG with MST edges highlighted.
///
/// Produces the same output as [`to_svg`] but additionally renders each
/// MST connecting edge as a red `<line>` element, grouped under
/// `<g id="mst-edges">`.  This makes it visually obvious which lines
/// are new connections introduced by the MST joiner vs. original contour
/// geometry.
///
/// Each MST edge line element includes a `data-weight` attribute with
/// the edge weight and a `data-polys` attribute identifying the
/// connected polyline indices.
#[must_use]
pub fn to_diagnostic_svg(
    polylines: &[Polyline],
    dimensions: Dimensions,
    metadata: &SvgMetadata<'_>,
    mst_edges: &[MstEdgeInfo],
) -> String {
    let mut out = String::new();

    write_svg_preamble(&mut out, dimensions, metadata);

    // Dark background for visibility
    let _ = writeln!(
        out,
        "  <rect width=\"{}\" height=\"{}\" fill=\"#1a1a1a\"/>",
        dimensions.width, dimensions.height,
    );

    // Contour paths in white
    let _ = writeln!(
        out,
        r#"  <g id="contours" stroke="white" stroke-width="1" fill="none">"#
    );
    write_polyline_paths(&mut out, polylines, "    ", "");
    let _ = writeln!(out, "  </g>");

    // MST connecting edges in red
    if !mst_edges.is_empty() {
        let _ = writeln!(
            out,
            r"  <!-- MST connecting edges: {} total -->",
            mst_edges.len(),
        );
        let _ = writeln!(
            out,
            r#"  <g id="mst-edges" stroke="red" stroke-width="1.5" opacity="0.9">"#,
        );
        for (i, edge) in mst_edges.iter().enumerate() {
            let _ = writeln!(
                out,
                r#"    <line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" data-weight="{:.2}" data-polys="{},{}" data-index="{}"/>"#,
                edge.point_a.0,
                edge.point_a.1,
                edge.point_b.0,
                edge.point_b.1,
                edge.weight,
                edge.poly_a,
                edge.poly_b,
                i,
            );
        }
        let _ = writeln!(out, "  </g>");
    }

    // Closing tag
    let _ = writeln!(out, "</svg>");

    out
}

// ---------------------------------------------------------------------------
// Segment diagnostic SVG
// ---------------------------------------------------------------------------

/// Distinct colors for the top-N highlighted segments.
///
/// Chosen for visibility against a dark background and mutual
/// distinguishability. The palette cycles if `top_n` exceeds the
/// array length.
const SEGMENT_COLORS: &[&str] = &[
    "#ff3333", // red
    "#ff8800", // orange
    "#ffdd00", // yellow
    "#33cc33", // green
    "#3399ff", // blue
];

/// A single segment identified for highlighting.
struct RankedSegment {
    /// Polyline index within the input slice.
    poly_idx: usize,
    /// Segment index within the polyline (from point `seg_idx` to `seg_idx + 1`).
    seg_idx: usize,
    /// Start point.
    from: (f64, f64),
    /// End point.
    to: (f64, f64),
    /// Euclidean length in pixels.
    length: f64,
}

/// Generate a diagnostic SVG highlighting the longest segments.
///
/// Renders all polylines in white on a dark background, then overlays
/// the `top_n` longest individual segments in distinct colors with a
/// legend.  This makes it easy to visually identify unexpectedly long
/// segments — whether they are MST connecting edges, retrace artifacts,
/// contour segments, or algorithmic bugs.
///
/// Each highlighted segment `<line>` element includes `data-rank`,
/// `data-length`, `data-from`, and `data-to` attributes for
/// programmatic inspection.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn to_segment_diagnostic_svg(
    polylines: &[Polyline],
    dimensions: Dimensions,
    metadata: &SvgMetadata<'_>,
    top_n: usize,
) -> String {
    let mut out = String::new();

    write_svg_preamble(&mut out, dimensions, metadata);

    // Dark background for visibility
    let _ = writeln!(
        out,
        "  <rect width=\"{}\" height=\"{}\" fill=\"#1a1a1a\"/>",
        dimensions.width, dimensions.height,
    );

    // Contour paths in white
    let _ = writeln!(
        out,
        r#"  <g id="contours" stroke="white" stroke-width="1" fill="none">"#
    );
    write_polyline_paths(&mut out, polylines, "    ", "");
    let _ = writeln!(out, "  </g>");

    // Find the top N longest segments across all polylines.
    let mut all_segments: Vec<RankedSegment> = Vec::new();
    for (poly_idx, polyline) in polylines.iter().enumerate() {
        let pts = polyline.points();
        for seg_idx in 0..pts.len().saturating_sub(1) {
            let from = pts[seg_idx];
            let to = pts[seg_idx + 1];
            let length = from.distance(to);
            all_segments.push(RankedSegment {
                poly_idx,
                seg_idx,
                from: (from.x, from.y),
                to: (to.x, to.y),
                length,
            });
        }
    }

    // Sort descending by length, take top N.
    all_segments.sort_by(|a, b| {
        b.length
            .partial_cmp(&a.length)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all_segments.truncate(top_n);

    // Highlighted segments
    if !all_segments.is_empty() {
        let _ = writeln!(
            out,
            r"  <!-- Top {} longest segments -->",
            all_segments.len(),
        );
        let _ = writeln!(
            out,
            r#"  <g id="top-segments" stroke-width="3" opacity="0.9" fill="none">"#,
        );
        for (rank, seg) in all_segments.iter().enumerate() {
            let color = SEGMENT_COLORS[rank % SEGMENT_COLORS.len()];
            let _ = writeln!(
                out,
                r#"    <line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" data-rank="{}" data-length="{:.2}" data-from="{:.1},{:.1}" data-to="{:.1},{:.1}"/>"#,
                seg.from.0,
                seg.from.1,
                seg.to.0,
                seg.to.1,
                color,
                rank + 1,
                seg.length,
                seg.from.0,
                seg.from.1,
                seg.to.0,
                seg.to.1,
            );
        }
        let _ = writeln!(out, "  </g>");

        // Legend in the top-left corner.
        //
        // Compute an adaptive width based on the longest label text so
        // the background rect always encloses the labels, even for
        // images with large coordinate values.
        let labels: Vec<String> = all_segments
            .iter()
            .enumerate()
            .map(|(rank, seg)| {
                format!(
                    "#{}: {:.1}px  ({:.0},{:.0})->({:.0},{:.0})  poly={} seg={}",
                    rank + 1,
                    seg.length,
                    seg.from.0,
                    seg.from.1,
                    seg.to.0,
                    seg.to.1,
                    seg.poly_idx,
                    seg.seg_idx,
                )
            })
            .collect();
        let header_text = format!("Top {} longest segments", all_segments.len());
        let max_label_chars = labels
            .iter()
            .map(String::len)
            .chain(std::iter::once(header_text.len()))
            .max()
            .unwrap_or(0);
        // Monospace font-size 12 ≈ 7.2px per character.  Add padding
        // for the color swatch (18px) and left/right margins (16+8).
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let legend_width = (max_label_chars as f64).mul_add(7.2, 42.0) as usize;

        let _ = writeln!(
            out,
            r#"  <g id="legend" font-family="monospace" font-size="12">"#
        );
        let legend_height = 20 + all_segments.len() * 18;
        let _ = writeln!(
            out,
            "    <rect x=\"8\" y=\"8\" width=\"{legend_width}\" height=\"{legend_height}\" rx=\"4\" fill=\"#000000\" opacity=\"0.7\"/>",
        );
        let _ = writeln!(
            out,
            r#"    <text x="16" y="24" fill="white" font-weight="bold">{header_text}</text>"#,
        );
        for (rank, label) in labels.iter().enumerate() {
            let color = SEGMENT_COLORS[rank % SEGMENT_COLORS.len()];
            let y = 42 + rank * 18;
            // Color swatch
            let _ = writeln!(
                out,
                r#"    <rect x="16" y="{}" width="12" height="12" fill="{}"/>"#,
                y - 9,
                color,
            );
            // Label
            let _ = writeln!(
                out,
                r#"    <text x="34" y="{y}" fill="white">{label}</text>"#,
            );
        }
        let _ = writeln!(out, "  </g>");
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

    // --- build_path_data ---

    #[test]
    fn build_path_data_empty_polyline() {
        let polyline = Polyline::new(vec![]);
        assert_eq!(build_path_data(&polyline), "");
    }

    #[test]
    fn build_path_data_single_point() {
        let polyline = Polyline::new(vec![Point::new(5.0, 5.0)]);
        assert_eq!(build_path_data(&polyline), "");
    }

    #[test]
    fn build_path_data_two_points() {
        let polyline = Polyline::new(vec![Point::new(10.0, 20.0), Point::new(30.0, 40.0)]);
        assert_eq!(build_path_data(&polyline), "M10,20 L30,40");
    }

    #[test]
    fn build_path_data_three_points() {
        let polyline = Polyline::new(vec![
            Point::new(10.0, 15.0),
            Point::new(12.5, 18.3),
            Point::new(14.0, 20.1),
        ]);
        let d = build_path_data(&polyline);
        assert_eq!(d, "M10,15 L12.5,18.3 L14,20.1");
    }

    #[test]
    fn build_path_data_integer_coords() {
        let polyline = Polyline::new(vec![Point::new(5.0, 10.0), Point::new(15.0, 20.0)]);
        assert_eq!(build_path_data(&polyline), "M5,10 L15,20");
    }

    // --- Empty / degenerate inputs ---

    #[test]
    fn empty_polylines_produces_valid_svg_with_no_paths() {
        let svg = to_svg(&[], dims(100, 50), &no_meta(), None);
        assert!(svg.contains(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(svg.contains(r#"width="100""#));
        assert!(svg.contains(r#"height="50""#));
        assert!(svg.contains(r#"viewBox="0 0 100 50""#));
        assert!(!svg.contains("<path"));
        // Empty SVGs use self-closing <svg .../> tag
        assert!(svg.contains("<svg "));
        assert!(svg.trim_end().ends_with("/>"));
    }

    #[test]
    fn single_point_polyline_is_skipped() {
        let polylines = vec![Polyline::new(vec![Point::new(5.0, 5.0)])];
        let svg = to_svg(&polylines, dims(100, 100), &no_meta(), None);
        assert!(!svg.contains("<path"));
    }

    #[test]
    fn zero_point_polyline_is_skipped() {
        let polylines = vec![Polyline::new(vec![])];
        let svg = to_svg(&polylines, dims(100, 100), &no_meta(), None);
        assert!(!svg.contains("<path"));
    }

    // --- Basic output structure ---

    #[test]
    fn single_polyline_with_two_points() {
        let polylines = vec![Polyline::new(vec![
            Point::new(10.0, 20.0),
            Point::new(30.0, 40.0),
        ])];
        let svg = to_svg(&polylines, dims(800, 600), &no_meta(), None);

        assert!(svg.contains(r#"width="800""#));
        assert!(svg.contains(r#"height="600""#));
        assert!(svg.contains(r#"viewBox="0 0 800 600""#));
        assert!(svg.contains(r#"d="M10,20 L30,40""#));
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
        let svg = to_svg(&polylines, dims(800, 600), &no_meta(), None);
        // Verify move-to and line-to commands are present
        assert!(svg.contains("M10,15"));
        assert!(svg.contains("L12.5,"));
        assert!(svg.contains("L14,"));
    }

    #[test]
    fn multiple_polylines_produce_multiple_paths() {
        let polylines = vec![
            Polyline::new(vec![Point::new(1.0, 2.0), Point::new(3.0, 4.0)]),
            Polyline::new(vec![Point::new(5.0, 6.0), Point::new(7.0, 8.0)]),
        ];
        let svg = to_svg(&polylines, dims(100, 100), &no_meta(), None);

        // Count <path occurrences
        let path_count = svg.matches("<path").count();
        assert_eq!(path_count, 2);

        assert!(svg.contains(r#"d="M1,2 L3,4""#));
        assert!(svg.contains(r#"d="M5,6 L7,8""#));
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
        let svg = to_svg(&polylines, dims(100, 100), &no_meta(), None);

        let path_count = svg.matches("<path").count();
        assert_eq!(path_count, 1);
        assert!(svg.contains(r#"d="M2,3 L4,5""#));
    }

    // --- Dimensions ---

    #[test]
    fn viewbox_reflects_dimensions() {
        let svg = to_svg(&[], dims(1920, 1080), &no_meta(), None);
        assert!(svg.contains(r#"width="1920""#));
        assert!(svg.contains(r#"height="1080""#));
        assert!(svg.contains(r#"viewBox="0 0 1920 1080""#));
    }

    // --- SVG structure ---

    #[test]
    fn svg_has_xml_declaration() {
        let svg = to_svg(&[], dims(100, 100), &no_meta(), None);
        assert!(svg.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
    }

    #[test]
    fn svg_has_xmlns_namespace() {
        let svg = to_svg(&[], dims(100, 100), &no_meta(), None);
        assert!(svg.contains(r#"xmlns="http://www.w3.org/2000/svg""#));
    }

    #[test]
    fn svg_ends_with_closing_tag() {
        // With children, the svg crate emits </svg>
        let polylines = vec![Polyline::new(vec![
            Point::new(1.0, 2.0),
            Point::new(3.0, 4.0),
        ])];
        let svg = to_svg(&polylines, dims(100, 100), &no_meta(), None);
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
        let svg = to_svg(&[], dims(100, 100), &meta, None);
        assert!(svg.contains("<title>cherry-blossoms</title>"));
        assert!(!svg.contains("<desc>"));
    }

    #[test]
    fn desc_element_emitted_when_present() {
        let meta = SvgMetadata {
            description: Some("Exported by mujou"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta, None);
        assert!(svg.contains("<desc>Exported by mujou</desc>"));
        assert!(!svg.contains("<title>"));
    }

    #[test]
    fn title_and_desc_both_emitted() {
        let meta = SvgMetadata {
            title: Some("my-image"),
            description: Some("blur=1.4, canny=15/40"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta, None);
        assert!(svg.contains("<title>my-image</title>"));
        assert!(svg.contains("<desc>blur=1.4, canny=15/40</desc>"));
    }

    #[test]
    fn title_and_desc_omitted_when_none() {
        let svg = to_svg(&[], dims(100, 100), &no_meta(), None);
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
        let svg = to_svg(&polylines, dims(100, 100), &meta, None);

        let title_pos = svg.find("<title>").unwrap();
        let desc_pos = svg.find("<desc>").unwrap();
        let path_pos = svg.find("<path").unwrap();
        assert!(title_pos < desc_pos, "title should come before desc");
        assert!(desc_pos < path_pos, "desc should come before paths");
    }

    #[test]
    fn special_characters_in_title_are_escaped() {
        let meta = SvgMetadata {
            title: Some("A <B> & C"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta, None);
        assert!(svg.contains("<title>A &lt;B&gt; &amp; C</title>"));
    }

    #[test]
    fn special_characters_in_desc_are_escaped() {
        let meta = SvgMetadata {
            description: Some("x < y & z > w"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta, None);
        assert!(svg.contains("<desc>x &lt; y &amp; z &gt; w</desc>"));
    }

    // --- Config JSON / <metadata> ---

    #[test]
    fn metadata_element_emitted_when_config_json_present() {
        let meta = SvgMetadata {
            config_json: Some(r#"{"blur_sigma":1.4}"#),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta, None);
        assert!(svg.contains("<metadata>"));
        assert!(svg.contains("</metadata>"));
        assert!(svg.contains(r#"<mujou:pipeline xmlns:mujou="https://mujou.app/ns/1">"#));
        assert!(svg.contains("</mujou:pipeline>"));
    }

    #[test]
    fn metadata_element_omitted_when_config_json_none() {
        let svg = to_svg(&[], dims(100, 100), &no_meta(), None);
        assert!(!svg.contains("<metadata>"));
    }

    #[test]
    fn config_json_special_characters_are_escaped() {
        let meta = SvgMetadata {
            config_json: Some(r#"{"note":"a < b & c > d"}"#),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], dims(100, 100), &meta, None);
        // The svg crate escapes <, >, & in text content
        assert!(svg.contains("&lt;"));
        assert!(svg.contains("&amp;"));
        assert!(svg.contains("&gt;"));
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
        let svg = to_svg(&polylines, dims(100, 100), &meta, None);

        let desc_pos = svg.find("<desc>").unwrap();
        let metadata_pos = svg.find("<metadata>").unwrap();
        let path_pos = svg.find("<path").unwrap();
        assert!(desc_pos < metadata_pos, "desc should come before metadata");
        assert!(metadata_pos < path_pos, "metadata should come before paths");
    }

    // --- XML escaping (used by diagnostic SVG helpers) ---

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
        let svg = to_svg(&[result.polyline], result.dimensions, &no_meta(), None);

        // Valid SVG structure
        assert!(svg.contains(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(svg.contains(r#"width="40""#));
        assert!(svg.contains(r#"height="40""#));
        assert!(svg.contains(r#"viewBox="0 0 40 40""#));
        assert!(svg.contains("<path"));
        assert!(svg.contains("</svg>"));

        // At least one path with M and L commands
        assert!(svg.contains('M'));
        assert!(svg.contains('L'));
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
        let svg = to_svg(&polylines, dims(800, 600), &no_meta(), None);

        assert!(svg.contains(r#"width="800""#));
        assert!(svg.contains(r#"height="600""#));
        assert!(svg.contains(r#"viewBox="0 0 800 600""#));
        // Both paths should be present with correct commands
        assert!(svg.contains("M10,15"));
        assert!(svg.contains("M30,5"));
        assert!(svg.contains(r#"fill="none""#));
        assert!(svg.contains(r#"stroke="black""#));
        assert!(svg.contains(r#"stroke-width="1""#));
    }

    // --- Mask-aware square viewBox ---

    #[test]
    fn mask_shape_produces_square_viewbox() {
        let shape = MaskShape::Circle {
            center: Point::new(500.0, 333.5),
            radius: 450.0,
        };
        let polylines = vec![Polyline::new(vec![
            Point::new(100.0, 100.0),
            Point::new(200.0, 200.0),
        ])];
        let svg = to_svg(&polylines, dims(1000, 667), &no_meta(), Some(&shape));

        // Square dimensions: 2 * 450 = 900
        assert!(svg.contains(r#"width="900""#), "width should be 900");
        assert!(svg.contains(r#"height="900""#), "height should be 900");
        // viewBox origin: (500-450, 333.5-450) = (50, -116.5)
        assert!(
            svg.contains("viewBox=\"50 -116.5 900 900\""),
            "viewBox should be square centered on mask circle, got:\n{svg}",
        );
        assert!(
            svg.contains(r#"preserveAspectRatio="xMidYMid meet""#),
            "should have explicit preserveAspectRatio",
        );
    }

    #[test]
    fn mask_shape_none_uses_dimensions_viewbox() {
        let svg = to_svg(&[], dims(800, 600), &no_meta(), None);
        assert!(svg.contains(r#"width="800""#));
        assert!(svg.contains(r#"height="600""#));
        assert!(svg.contains(r#"viewBox="0 0 800 600""#));
        // No preserveAspectRatio for non-mask export.
        assert!(!svg.contains("preserveAspectRatio"));
    }

    #[test]
    fn mask_shape_square_image_viewbox_extends_beyond() {
        // For a square 1000x1000 image with default mask_diameter=0.75:
        // diagonal = 1414.21, radius = 1414.21 * 0.75 / 2 = 530.33
        // The circle diameter (1060.66) exceeds the image (1000).
        let radius = 530.33;
        let shape = MaskShape::Circle {
            center: Point::new(500.0, 500.0),
            radius,
        };
        let svg = to_svg(&[], dims(1000, 1000), &no_meta(), Some(&shape));

        // viewBox origin: (500-530.33, 500-530.33) = (-30.33, -30.33)
        // viewBox size: 1060.66
        assert!(
            svg.contains("viewBox=\"-30.33"),
            "viewBox should have negative origin"
        );
        assert!(svg.contains(r#"preserveAspectRatio="xMidYMid meet""#));
    }

    #[test]
    fn mask_shape_viewbox_still_renders_paths() {
        let shape = MaskShape::Circle {
            center: Point::new(50.0, 50.0),
            radius: 40.0,
        };
        let polylines = vec![Polyline::new(vec![
            Point::new(30.0, 30.0),
            Point::new(70.0, 70.0),
        ])];
        let svg = to_svg(&polylines, dims(100, 100), &no_meta(), Some(&shape));

        assert!(svg.contains("<path"));
        assert!(svg.contains(r#"d="M30,30 L70,70""#));
        // Square viewBox: side = 80, origin = (10, 10)
        assert!(svg.contains("viewBox=\"10 10 80 80\""));
    }

    #[test]
    fn mask_shape_with_metadata() {
        let shape = MaskShape::Circle {
            center: Point::new(100.0, 100.0),
            radius: 80.0,
        };
        let meta = SvgMetadata {
            title: Some("test"),
            description: Some("masked export"),
            config_json: Some(r#"{"circular_mask":true}"#),
        };
        let svg = to_svg(&[], dims(200, 200), &meta, Some(&shape));

        assert!(svg.contains("<title>test</title>"));
        assert!(svg.contains("<desc>masked export</desc>"));
        assert!(svg.contains("<metadata>"));
        assert!(svg.contains("viewBox=\"20 20 160 160\""));
    }
}
