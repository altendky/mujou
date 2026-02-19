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

use mujou_pipeline::segment_analysis::{SEGMENT_COLORS, find_top_segments};
use mujou_pipeline::{Dimensions, MaskShape, MstEdgeInfo, Polyline};

// TODO: review these constants for different table models / sizes.
/// SVG document width and height in millimetres (square canvas).
const DOCUMENT_SIZE_MM: f64 = 200.0;

// ---------------------------------------------------------------------------
// Document mapping (pixel → mm coordinate transform)
// ---------------------------------------------------------------------------

/// Pre-computed coordinate mapping from pixel space to the SVG mm-based
/// coordinate system.
///
/// Created by [`document_mapping`] from a [`MaskShape`] and border margin.
/// Passed to [`to_svg`] so the SVG document dimensions, `viewBox`, and
/// per-path coordinate transforms are all derived from a single source of
/// truth.
///
/// # Layout
///
/// The SVG document is sized in millimetres with a `viewBox` matching those
/// dimensions.  All pixel-space polyline coordinates are transformed via:
///
/// ```text
/// mm = (px − offset) × scale
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DocumentMapping {
    /// SVG document width in millimetres.
    pub width_mm: f64,
    /// SVG document height in millimetres.
    pub height_mm: f64,
    /// Uniform scale factor: `mm = (px − offset) × scale`.
    pub scale: f64,
    /// Horizontal pixel offset (subtracted before scaling).
    pub offset_x: f64,
    /// Vertical pixel offset (subtracted before scaling).
    pub offset_y: f64,
}

/// Create a [`DocumentMapping`] from a resolved [`MaskShape`] and border
/// margin.
///
/// The `border_margin` is a fraction of the document size (0.0–0.15) that
/// pads the drawing area on all sides.  At 0.0 the canvas shape fills the
/// full document; at 0.05 there is a 5 % margin on each edge.
///
/// # Examples
///
/// ```
/// use mujou_pipeline::{MaskShape, Point};
/// use mujou_export::document_mapping;
///
/// let shape = MaskShape::Circle {
///     center: Point::new(100.0, 100.0),
///     radius: 100.0,
/// };
/// let mapping = document_mapping(&shape, 0.0);
/// assert!((mapping.width_mm - 200.0).abs() < 1e-9);
/// assert!((mapping.scale - 1.0).abs() < 1e-9);
/// ```
#[must_use]
pub fn document_mapping(shape: &MaskShape, border_margin: f64) -> DocumentMapping {
    debug_assert!(
        (0.0..0.5).contains(&border_margin),
        "border_margin must be in [0.0, 0.5), got {border_margin}",
    );

    let drawing_area = 2.0_f64.mul_add(-border_margin, 1.0) * DOCUMENT_SIZE_MM;

    // Exhaustive match on MaskShape ensures new variants get a compile
    // error here, matching the convention in mask.rs.
    match shape {
        MaskShape::Circle { center, radius } => {
            let diameter = 2.0 * radius;
            // The viewBox is in mm: `0 0 200 200`.  Pixel coordinates
            // are transformed so the circle diameter maps to the drawing
            // area, centred in the document.
            let vb_size = diameter * DOCUMENT_SIZE_MM / drawing_area;
            let margin = (vb_size - diameter) / 2.0;
            let offset_x = center.x - radius - margin;
            let offset_y = center.y - radius - margin;
            let scale = DOCUMENT_SIZE_MM / vb_size;
            DocumentMapping {
                width_mm: DOCUMENT_SIZE_MM,
                height_mm: DOCUMENT_SIZE_MM,
                scale,
                offset_x,
                offset_y,
            }
        }
        MaskShape::Rectangle {
            center,
            half_width,
            half_height,
        } => {
            let rect_width = 2.0 * half_width;
            let rect_height = 2.0 * half_height;
            // Use DOCUMENT_SIZE_MM for the longer axis, scale the
            // shorter axis proportionally.  The drawing ratio controls
            // the margin.
            let drawing_ratio = drawing_area / DOCUMENT_SIZE_MM;
            let longer = rect_width.max(rect_height);
            let vb_width = rect_width / drawing_ratio;
            let vb_height = rect_height / drawing_ratio;
            let scale = DOCUMENT_SIZE_MM / (longer / drawing_ratio);
            let doc_width_mm = vb_width * scale;
            let doc_height_mm = vb_height * scale;
            let margin_x = (vb_width - rect_width) / 2.0;
            let margin_y = (vb_height - rect_height) / 2.0;
            let offset_x = center.x - half_width - margin_x;
            let offset_y = center.y - half_height - margin_y;
            DocumentMapping {
                width_mm: doc_width_mm,
                height_mm: doc_height_mm,
                scale,
                offset_x,
                offset_y,
            }
        }
    }
}

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

/// Like [`build_path_data`] but applies a uniform scale-and-translate to
/// every coordinate before emitting it.  Used by [`to_svg`] to convert
/// pixel-space polylines into the mm-based `viewBox` coordinate system.
fn build_path_data_transformed(
    polyline: &Polyline,
    scale: f64,
    offset_x: f64,
    offset_y: f64,
) -> String {
    let points = polyline.points();
    if points.len() < 2 {
        return String::new();
    }

    let tx = |p: &mujou_pipeline::Point| ((p.x - offset_x) * scale, (p.y - offset_y) * scale);

    let first = tx(&points[0]);
    let mut data = Data::new().move_to(first);
    for p in &points[1..] {
        data = data.line_to(tx(p));
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
/// The [`DocumentMapping`] (created by [`document_mapping`]) controls the
/// SVG document dimensions, `viewBox`, and pixel→mm coordinate transform.
/// All path coordinates are transformed from pixel space into the mm-based
/// `viewBox`.
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
/// use mujou_pipeline::{MaskShape, Point, Polyline};
/// use mujou_export::{SvgMetadata, document_mapping, to_svg};
///
/// let shape = MaskShape::Circle {
///     center: Point::new(100.0, 100.0),
///     radius: 100.0,
/// };
/// let mapping = document_mapping(&shape, 0.0);
/// let polylines = vec![
///     Polyline::new(vec![Point::new(10.0, 15.0), Point::new(12.5, 18.3)]),
/// ];
/// let metadata = SvgMetadata {
///     title: Some("cherry-blossoms"),
///     description: Some("Exported by mujou"),
///     ..SvgMetadata::default()
/// };
/// let svg = to_svg(&polylines, &metadata, &mapping);
/// assert!(svg.contains("<title>cherry-blossoms</title>"));
/// assert!(svg.contains("<desc>Exported by mujou</desc>"));
/// assert!(svg.contains("M10,15 L12.5,18.3"));
/// ```
#[must_use]
pub fn to_svg(
    polylines: &[Polyline],
    metadata: &SvgMetadata<'_>,
    mapping: &DocumentMapping,
) -> String {
    let w = mapping.width_mm;
    let h = mapping.height_mm;

    let mut doc = Document::new()
        .set("width", format!("{w}mm"))
        .set("height", format!("{h}mm"))
        .set("viewBox", format!("0 0 {w} {h}"))
        .set("preserveAspectRatio", "xMidYMid meet");

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

    // One <path> per polyline (skip polylines with fewer than 2 points).
    // Coordinates are mapped from pixel space into the mm-based viewBox.
    for polyline in polylines {
        let d = build_path_data_transformed(
            polyline,
            mapping.scale,
            mapping.offset_x,
            mapping.offset_y,
        );
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
    let all_segments = find_top_segments(polylines, top_n);

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

    /// Shorthand: no metadata (most existing tests don't care about it).
    fn no_meta() -> SvgMetadata<'static> {
        SvgMetadata::default()
    }

    /// Identity-like mapping: circle at (100,100) with radius=100 and
    /// `border_margin=0`.  Gives scale=1.0, offset=(0,0), 200×200 mm.
    /// Pixel coordinates pass through to mm coordinates unchanged.
    fn identity_mapping() -> DocumentMapping {
        document_mapping(
            &MaskShape::Circle {
                center: Point::new(100.0, 100.0),
                radius: 100.0,
            },
            0.0,
        )
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
        let mapping = identity_mapping();
        let svg = to_svg(&[], &no_meta(), &mapping);
        assert!(svg.contains(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(svg.contains(r#"width="200mm""#));
        assert!(svg.contains(r#"height="200mm""#));
        assert!(svg.contains(r#"viewBox="0 0 200 200""#));
        assert!(!svg.contains("<path"));
        // Empty SVGs use self-closing <svg .../> tag
        assert!(svg.contains("<svg "));
        assert!(svg.trim_end().ends_with("/>"));
    }

    #[test]
    fn single_point_polyline_is_skipped() {
        let polylines = vec![Polyline::new(vec![Point::new(5.0, 5.0)])];
        let svg = to_svg(&polylines, &no_meta(), &identity_mapping());
        assert!(!svg.contains("<path"));
    }

    #[test]
    fn zero_point_polyline_is_skipped() {
        let polylines = vec![Polyline::new(vec![])];
        let svg = to_svg(&polylines, &no_meta(), &identity_mapping());
        assert!(!svg.contains("<path"));
    }

    // --- Basic output structure ---

    #[test]
    fn single_polyline_with_two_points() {
        // identity_mapping: scale=1.0, offset=(0,0), so pixel coords
        // pass through to mm coords unchanged.
        let polylines = vec![Polyline::new(vec![
            Point::new(10.0, 20.0),
            Point::new(30.0, 40.0),
        ])];
        let svg = to_svg(&polylines, &no_meta(), &identity_mapping());

        assert!(svg.contains(r#"width="200mm""#));
        assert!(svg.contains(r#"height="200mm""#));
        assert!(svg.contains(r#"viewBox="0 0 200 200""#));
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
        let svg = to_svg(&polylines, &no_meta(), &identity_mapping());
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
        let svg = to_svg(&polylines, &no_meta(), &identity_mapping());

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
        let svg = to_svg(&polylines, &no_meta(), &identity_mapping());

        let path_count = svg.matches("<path").count();
        assert_eq!(path_count, 1);
        assert!(svg.contains(r#"d="M2,3 L4,5""#));
    }

    // --- Document mapping dimensions ---

    #[test]
    fn viewbox_reflects_mapping_dimensions() {
        // Rectangle mapping produces non-square mm dimensions.
        let mapping = document_mapping(
            &MaskShape::Rectangle {
                center: Point::new(100.0, 50.0),
                half_width: 100.0,
                half_height: 50.0,
            },
            0.0,
        );
        let svg = to_svg(&[], &no_meta(), &mapping);
        assert!(
            svg.contains(r#"viewBox="0 0 200 100""#),
            "viewBox should match mapping dimensions, got:\n{svg}",
        );
        assert!(svg.contains(r#"width="200mm""#));
        assert!(svg.contains(r#"height="100mm""#));
    }

    // --- SVG structure ---

    #[test]
    fn svg_has_xml_declaration() {
        let svg = to_svg(&[], &no_meta(), &identity_mapping());
        assert!(svg.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
    }

    #[test]
    fn svg_has_xmlns_namespace() {
        let svg = to_svg(&[], &no_meta(), &identity_mapping());
        assert!(svg.contains(r#"xmlns="http://www.w3.org/2000/svg""#));
    }

    #[test]
    fn svg_has_preserve_aspect_ratio() {
        let svg = to_svg(&[], &no_meta(), &identity_mapping());
        assert!(svg.contains(r#"preserveAspectRatio="xMidYMid meet""#));
    }

    #[test]
    fn svg_ends_with_closing_tag() {
        // With children, the svg crate emits </svg>
        let polylines = vec![Polyline::new(vec![
            Point::new(1.0, 2.0),
            Point::new(3.0, 4.0),
        ])];
        let svg = to_svg(&polylines, &no_meta(), &identity_mapping());
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
        let svg = to_svg(&[], &meta, &identity_mapping());
        assert!(svg.contains("<title>cherry-blossoms</title>"));
        assert!(!svg.contains("<desc>"));
    }

    #[test]
    fn desc_element_emitted_when_present() {
        let meta = SvgMetadata {
            description: Some("Exported by mujou"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], &meta, &identity_mapping());
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
        let svg = to_svg(&[], &meta, &identity_mapping());
        assert!(svg.contains("<title>my-image</title>"));
        assert!(svg.contains("<desc>blur=1.4, canny=15/40</desc>"));
    }

    #[test]
    fn title_and_desc_omitted_when_none() {
        let svg = to_svg(&[], &no_meta(), &identity_mapping());
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
        let svg = to_svg(&polylines, &meta, &identity_mapping());

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
        let svg = to_svg(&[], &meta, &identity_mapping());
        assert!(svg.contains("<title>A &lt;B&gt; &amp; C</title>"));
    }

    #[test]
    fn special_characters_in_desc_are_escaped() {
        let meta = SvgMetadata {
            description: Some("x < y & z > w"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], &meta, &identity_mapping());
        assert!(svg.contains("<desc>x &lt; y &amp; z &gt; w</desc>"));
    }

    // --- Config JSON / <metadata> ---

    #[test]
    fn metadata_element_emitted_when_config_json_present() {
        let meta = SvgMetadata {
            config_json: Some(r#"{"blur_sigma":1.4}"#),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], &meta, &identity_mapping());
        assert!(svg.contains("<metadata>"));
        assert!(svg.contains("</metadata>"));
        assert!(svg.contains(r#"<mujou:pipeline xmlns:mujou="https://mujou.app/ns/1">"#));
        assert!(svg.contains("</mujou:pipeline>"));
    }

    #[test]
    fn metadata_element_omitted_when_config_json_none() {
        let svg = to_svg(&[], &no_meta(), &identity_mapping());
        assert!(!svg.contains("<metadata>"));
    }

    #[test]
    fn config_json_special_characters_are_escaped() {
        let meta = SvgMetadata {
            config_json: Some(r#"{"note":"a < b & c > d"}"#),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], &meta, &identity_mapping());
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
        let svg = to_svg(&polylines, &meta, &identity_mapping());

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

    // --- End-to-end: process_staged() -> to_svg() ---

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
        use mujou_pipeline::{PipelineConfig, process_staged};

        // Use scale=0.5 so the canvas (radius=40) covers the full
        // 40×40 test image.
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            scale: 0.5,
            ..PipelineConfig::default()
        };
        let result = process_staged(&png, &config).unwrap();
        let mapping = document_mapping(&result.canvas.shape, config.border_margin);
        let svg = to_svg(&[result.final_polyline().clone()], &no_meta(), &mapping);

        // Valid SVG structure
        assert!(svg.contains(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(svg.contains(r#"width="200mm""#));
        assert!(svg.contains(r#"viewBox="0 0 200 200""#));
        assert!(svg.contains("<path"));
        assert!(svg.contains("</svg>"));

        // At least one path with M and L commands
        assert!(svg.contains('M'));
        assert!(svg.contains('L'));
    }

    // --- Example from formats.md ---

    #[test]
    fn matches_formats_doc_example() {
        // Use the identity mapping so pixel coords pass through unchanged.
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
        let svg = to_svg(&polylines, &no_meta(), &identity_mapping());

        assert!(svg.contains(r#"width="200mm""#));
        assert!(svg.contains(r#"height="200mm""#));
        assert!(svg.contains(r#"viewBox="0 0 200 200""#));
        // Both paths should be present with correct commands
        assert!(svg.contains("M10,15"));
        assert!(svg.contains("M30,5"));
        assert!(svg.contains(r#"fill="none""#));
        assert!(svg.contains(r#"stroke="black""#));
        assert!(svg.contains(r#"stroke-width="1""#));
    }

    // --- DocumentMapping + coordinate transform ---

    #[test]
    fn circle_mapping_produces_square_viewbox() {
        // radius=50, center=(100,80), border_margin=0
        // diameter=100, drawing_area=200
        // vb_size=100*200/200=100, margin=0
        // offset_x=100-50=50, offset_y=80-50=30
        // scale=200/100=2.0
        let mapping = document_mapping(
            &MaskShape::Circle {
                center: Point::new(100.0, 80.0),
                radius: 50.0,
            },
            0.0,
        );
        let polylines = vec![Polyline::new(vec![
            Point::new(60.0, 40.0),
            Point::new(140.0, 120.0),
        ])];
        let svg = to_svg(&polylines, &no_meta(), &mapping);

        assert!(svg.contains(r#"width="200mm""#), "width should be 200mm");
        assert!(svg.contains(r#"height="200mm""#), "height should be 200mm");
        assert!(
            svg.contains(r#"viewBox="0 0 200 200""#),
            "viewBox should be 200×200, got:\n{svg}",
        );
        assert!(svg.contains(r#"preserveAspectRatio="xMidYMid meet""#));
        // (60,40) → ((60-50)*2, (40-30)*2) = (20, 20)
        // (140,120) → ((140-50)*2, (120-30)*2) = (180, 180)
        assert!(
            svg.contains("M20,20 L180,180"),
            "path coordinates should be in mm, got:\n{svg}",
        );
    }

    #[test]
    fn circle_mapping_with_border_margin() {
        // radius=100, center=(100,100), border_margin=0.1 (10%)
        // drawing_area = 0.8 * 200 = 160
        // diameter=200, vb_size=200*200/160=250
        // margin=(250-200)/2=25
        // offset_x=100-100-25=-25, offset_y=-25
        // scale=200/250=0.8
        let mapping = document_mapping(
            &MaskShape::Circle {
                center: Point::new(100.0, 100.0),
                radius: 100.0,
            },
            0.1,
        );
        assert!((mapping.width_mm - 200.0).abs() < 1e-9);
        assert!((mapping.height_mm - 200.0).abs() < 1e-9);
        assert!((mapping.scale - 0.8).abs() < 1e-9);
        assert!((mapping.offset_x - -25.0).abs() < 1e-9);
        assert!((mapping.offset_y - -25.0).abs() < 1e-9);

        // Point at center (100,100) → ((100-(-25))*0.8, same) = (100, 100) ✓
        // Point at edge (0,100) → ((0-(-25))*0.8, (100-(-25))*0.8) = (20, 100)
        let polylines = vec![Polyline::new(vec![
            Point::new(0.0, 100.0),
            Point::new(200.0, 100.0),
        ])];
        let svg = to_svg(&polylines, &no_meta(), &mapping);
        assert!(
            svg.contains("M20,100 L180,100"),
            "10% margin should inset coordinates, got:\n{svg}",
        );
    }

    #[test]
    fn large_circle_still_uses_mm_viewbox() {
        // Circle exceeds image — viewBox is always 0 0 200 200 in mm.
        let mapping = document_mapping(
            &MaskShape::Circle {
                center: Point::new(500.0, 500.0),
                radius: 600.0,
            },
            0.0,
        );
        let svg = to_svg(&[], &no_meta(), &mapping);
        assert!(
            svg.contains(r#"viewBox="0 0 200 200""#),
            "viewBox should be mm-based, got:\n{svg}",
        );
        assert!(svg.contains(r#"preserveAspectRatio="xMidYMid meet""#));
    }

    #[test]
    fn circle_mapping_still_renders_paths() {
        // radius=50, center=(50,50), border_margin=0
        // scale=200/100=2.0, offset=(0,0)
        let mapping = document_mapping(
            &MaskShape::Circle {
                center: Point::new(50.0, 50.0),
                radius: 50.0,
            },
            0.0,
        );
        let polylines = vec![Polyline::new(vec![
            Point::new(25.0, 25.0),
            Point::new(75.0, 75.0),
        ])];
        let svg = to_svg(&polylines, &no_meta(), &mapping);
        assert!(svg.contains("<path"));
        // (25,25) → (50,50), (75,75) → (150,150)
        assert!(
            svg.contains(r#"d="M50,50 L150,150""#),
            "path coordinates should be in mm, got:\n{svg}",
        );
        assert!(svg.contains(r#"viewBox="0 0 200 200""#));
    }

    #[test]
    fn mapping_with_metadata() {
        let mapping = document_mapping(
            &MaskShape::Circle {
                center: Point::new(100.0, 100.0),
                radius: 100.0,
            },
            0.0,
        );
        let meta = SvgMetadata {
            title: Some("test"),
            description: Some("masked export"),
            config_json: Some(r#"{"circular_mask":true}"#),
        };
        let svg = to_svg(&[], &meta, &mapping);

        assert!(svg.contains("<title>test</title>"));
        assert!(svg.contains("<desc>masked export</desc>"));
        assert!(svg.contains("<metadata>"));
        assert!(svg.contains(r#"viewBox="0 0 200 200""#));
    }

    // --- document_mapping unit tests ---

    #[test]
    fn identity_mapping_has_unit_scale() {
        let m = identity_mapping();
        assert!((m.width_mm - 200.0).abs() < 1e-9);
        assert!((m.height_mm - 200.0).abs() < 1e-9);
        assert!((m.scale - 1.0).abs() < 1e-9);
        assert!(m.offset_x.abs() < 1e-9);
        assert!(m.offset_y.abs() < 1e-9);
    }

    #[test]
    fn rectangle_mapping_produces_correct_aspect_ratio() {
        // 200×100 pixel rectangle: longer axis maps to 200mm.
        let mapping = document_mapping(
            &MaskShape::Rectangle {
                center: Point::new(100.0, 50.0),
                half_width: 100.0,
                half_height: 50.0,
            },
            0.0,
        );
        assert!((mapping.width_mm - 200.0).abs() < 1e-9);
        assert!((mapping.height_mm - 100.0).abs() < 1e-9);
        // scale = 200 / (200/1.0) = 1.0
        assert!((mapping.scale - 1.0).abs() < 1e-9);
    }

    #[test]
    fn rectangle_mapping_taller_than_wide() {
        // 100×200 pixel rectangle (taller): longer axis (height=200) maps
        // to 200mm.
        let mapping = document_mapping(
            &MaskShape::Rectangle {
                center: Point::new(50.0, 100.0),
                half_width: 50.0,
                half_height: 100.0,
            },
            0.0,
        );
        assert!((mapping.width_mm - 100.0).abs() < 1e-9);
        assert!((mapping.height_mm - 200.0).abs() < 1e-9);
        assert!((mapping.scale - 1.0).abs() < 1e-9);
    }
}
