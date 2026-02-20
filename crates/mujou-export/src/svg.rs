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
use mujou_pipeline::{MaskShape, MstEdgeInfo, Polyline};

// TODO: review these constants for different table models / sizes.
/// SVG document width and height in millimetres (square canvas).
const DOCUMENT_SIZE_MM: f64 = 200.0;

// ---------------------------------------------------------------------------
// Document mapping (pixel → mm coordinate transform)
// ---------------------------------------------------------------------------

/// Pre-computed coordinate mapping from normalized space to the SVG
/// mm-based coordinate system.
///
/// Created by [`document_mapping`] from a [`MaskShape`] and border margin.
/// Passed to [`to_svg`] so the SVG document dimensions, `viewBox`, and
/// per-path coordinate transforms are all derived from a single source of
/// truth.
///
/// # Layout
///
/// The SVG document is sized in millimetres with a `viewBox` matching those
/// dimensions.  Normalized coordinates are transformed via:
///
/// ```text
/// mm_x = norm_x × scale_factor + offset_x
/// mm_y = norm_y × scale_factor + offset_y
/// ```
///
/// No Y-flip is needed because normalized space preserves the image
/// convention of +Y pointing downward, matching SVG.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DocumentMapping {
    /// SVG document width in millimetres.
    pub width_mm: f64,
    /// SVG document height in millimetres.
    pub height_mm: f64,
    /// Scale factor from normalized to mm: `mm = norm × scale_factor`.
    pub scale_factor: f64,
    /// Horizontal offset in mm (center of the SVG document).
    pub offset_x: f64,
    /// Vertical offset in mm (center of the SVG document).
    pub offset_y: f64,
}

/// Create a [`DocumentMapping`] from a resolved [`MaskShape`] and border
/// margin.
///
/// The `border_margin` is a fraction of the document size (0.0–0.15) that
/// pads the drawing area on all sides.  At 0.0 the canvas shape fills the
/// full document; at 0.05 there is a 5 % margin on each edge.
///
/// In normalized space, the circle has radius 1.0 at the origin, and
/// rectangles have half-short-side = 1.0.  The mapping transforms these
/// to the SVG mm-based coordinate system (Oasis template: 200mm × 200mm,
/// 195mm diameter circle drawing area by default).
///
/// # Panics
///
/// Panics if `border_margin` is outside `[0.0, 0.5)`.
///
/// # Examples
///
/// ```
/// use mujou_pipeline::{MaskShape, Point};
/// use mujou_export::document_mapping;
///
/// let shape = MaskShape::Circle {
///     center: Point::new(0.0, 0.0),
///     radius: 1.0,
/// };
/// let mapping = document_mapping(&shape, 0.0);
/// assert!((mapping.width_mm - 200.0).abs() < 1e-9);
/// // scale_factor = drawing_area / (2 * radius) = 200 / 2 = 100
/// assert!((mapping.scale_factor - 100.0).abs() < 1e-9);
/// ```
#[must_use]
pub fn document_mapping(shape: &MaskShape, border_margin: f64) -> DocumentMapping {
    assert!(
        (0.0..0.5).contains(&border_margin),
        "border_margin must be in [0.0, 0.5), got {border_margin}",
    );

    let drawing_frac = 2.0_f64.mul_add(-border_margin, 1.0);
    let drawing_area = drawing_frac * DOCUMENT_SIZE_MM;

    // Exhaustive match on MaskShape ensures new variants get a compile
    // error here, matching the convention in mask.rs.
    match shape {
        MaskShape::Circle { .. } => {
            // Normalized circle: radius=1.0, so diameter=2.0 in norm units.
            // Map diameter → drawing_area mm, centred in DOCUMENT_SIZE_MM.
            let scale_factor = drawing_area / 2.0;
            let center_mm = DOCUMENT_SIZE_MM / 2.0;
            DocumentMapping {
                width_mm: DOCUMENT_SIZE_MM,
                height_mm: DOCUMENT_SIZE_MM,
                scale_factor,
                offset_x: center_mm,
                offset_y: center_mm,
            }
        }
        MaskShape::Rectangle {
            half_width,
            half_height,
            ..
        } => {
            // Normalized rectangle: extents are half_width × half_height.
            let rect_norm_w = 2.0 * half_width;
            let rect_norm_h = 2.0 * half_height;
            let longer_norm = rect_norm_w.max(rect_norm_h);
            // Map the longer normalized axis to DOCUMENT_SIZE_MM.
            let scale_factor = drawing_area / longer_norm;
            let doc_width_mm = rect_norm_w * scale_factor / drawing_frac;
            let doc_height_mm = rect_norm_h * scale_factor / drawing_frac;
            DocumentMapping {
                width_mm: doc_width_mm,
                height_mm: doc_height_mm,
                scale_factor,
                offset_x: doc_width_mm / 2.0,
                offset_y: doc_height_mm / 2.0,
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

/// Like [`build_path_data`] but applies the normalized→mm transform to
/// every coordinate before emitting it.  Used by [`to_svg`] to convert
/// normalized-space polylines into the mm-based `viewBox` coordinate
/// system.
///
/// The transform is:
/// ```text
/// mm_x = norm_x × scale_factor + offset_x
/// mm_y = norm_y × scale_factor + offset_y
/// ```
///
/// No Y-flip is needed because normalized space preserves the image
/// convention of +Y pointing downward, which matches SVG's coordinate
/// system.
fn build_path_data_transformed(polyline: &Polyline, mapping: &DocumentMapping) -> String {
    let points = polyline.points();
    if points.len() < 2 {
        return String::new();
    }

    let tx = |p: &mujou_pipeline::Point| {
        (
            p.x.mul_add(mapping.scale_factor, mapping.offset_x),
            p.y.mul_add(mapping.scale_factor, mapping.offset_y),
        )
    };

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
/// Used by the diagnostic SVG functions which use manual string
/// formatting (the primary [`to_svg`] uses the `svg` crate).
///
/// `view_box` is a pre-formatted viewBox attribute value (e.g.
/// `"-1.5 -1.5 3 3"`).  `display_width` and `display_height` set the
/// SVG element's pixel dimensions for browser rendering.
fn write_svg_preamble(
    out: &mut String,
    view_box: &str,
    display_width: u32,
    display_height: u32,
    metadata: &SvgMetadata<'_>,
) {
    // XML declaration
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);

    // Opening <svg> tag with namespace, explicit dimensions, and viewBox
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{display_width}" height="{display_height}" viewBox="{view_box}">"#,
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
/// a visible line segment).  Coordinates are formatted to 4 decimal
/// places (sufficient for normalized coordinates).
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
                format!("{cmd} {:.4} {:.4}", p.x, p.y)
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
///     center: Point::new(0.0, 0.0),
///     radius: 1.0,
/// };
/// let mapping = document_mapping(&shape, 0.0);
/// let polylines = vec![
///     Polyline::new(vec![Point::new(0.1, 0.15), Point::new(0.2, 0.3)]),
/// ];
/// let metadata = SvgMetadata {
///     title: Some("cherry-blossoms"),
///     description: Some("Exported by mujou"),
///     ..SvgMetadata::default()
/// };
/// let svg = to_svg(&polylines, &metadata, &mapping);
/// assert!(svg.contains("<title>cherry-blossoms</title>"));
/// assert!(svg.contains("<desc>Exported by mujou</desc>"));
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
    // Coordinates are mapped from normalized space into the mm-based viewBox.
    for polyline in polylines {
        let d = build_path_data_transformed(polyline, mapping);
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

/// Display size for diagnostic SVGs (pixels).
const DIAG_DISPLAY_SIZE: u32 = 800;

/// Compute a padded viewBox from polyline bounds and optional extra
/// points (e.g. MST edge endpoints).
///
/// Returns `(min_x, min_y, width, height)`.  When no data is present,
/// defaults to a view covering the unit circle with padding.
fn compute_diagnostic_view_box(
    polylines: &[Polyline],
    extra_points: &[(f64, f64)],
) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for poly in polylines {
        for p in poly.points() {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
    }

    for &(x, y) in extra_points {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    // Default to unit circle extent if no data.
    if min_x > max_x {
        min_x = -1.5;
        min_y = -1.5;
        max_x = 1.5;
        max_y = 1.5;
    }

    // Pad by 5% of the larger dimension.
    let dx = max_x - min_x;
    let dy = max_y - min_y;
    let pad = 0.05 * dx.max(dy).max(0.1);
    min_x -= pad;
    min_y -= pad;

    (
        min_x,
        min_y,
        2.0_f64.mul_add(pad, dx),
        2.0_f64.mul_add(pad, dy),
    )
}

/// Format a viewBox tuple as an SVG attribute value string.
fn format_view_box(vb: (f64, f64, f64, f64)) -> String {
    format!("{:.4} {:.4} {:.4} {:.4}", vb.0, vb.1, vb.2, vb.3)
}

/// Serialize polylines into a diagnostic SVG with MST edges highlighted.
///
/// Produces the same output as [`to_svg`] but additionally renders each
/// MST connecting edge as a red `<line>` element, grouped under
/// `<g id="mst-edges">`.  This makes it visually obvious which lines
/// are new connections introduced by the MST joiner vs. original contour
/// geometry.
///
/// The viewBox is computed automatically from the polyline and MST edge
/// data.  All coordinates are in normalized space (center-origin, +Y up).
///
/// Each MST edge line element includes a `data-weight` attribute with
/// the edge weight and a `data-polys` attribute identifying the
/// connected polyline indices.
#[must_use]
pub fn to_diagnostic_svg(
    polylines: &[Polyline],
    metadata: &SvgMetadata<'_>,
    mst_edges: &[MstEdgeInfo],
) -> String {
    // Collect MST edge points for viewBox computation.
    let extra_points: Vec<(f64, f64)> = mst_edges
        .iter()
        .flat_map(|e| [e.point_a, e.point_b])
        .collect();

    let vb = compute_diagnostic_view_box(polylines, &extra_points);
    let vb_str = format_view_box(vb);
    let extent = vb.2.max(vb.3);

    // Scale stroke widths proportionally to the viewBox extent.
    let sw_thin = extent / 800.0;
    let sw_thick = extent / 500.0;

    let mut out = String::new();

    write_svg_preamble(
        &mut out,
        &vb_str,
        DIAG_DISPLAY_SIZE,
        DIAG_DISPLAY_SIZE,
        metadata,
    );

    // Dark background for visibility (covers the full viewBox).
    let _ = writeln!(
        out,
        "  <rect x=\"{:.4}\" y=\"{:.4}\" width=\"{:.4}\" height=\"{:.4}\" fill=\"#1a1a1a\"/>",
        vb.0, vb.1, vb.2, vb.3,
    );

    // Contour paths in white
    let _ = writeln!(
        out,
        r#"  <g id="contours" stroke="white" stroke-width="{sw_thin:.4}" fill="none">"#,
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
            r#"  <g id="mst-edges" stroke="red" stroke-width="{sw_thick:.4}" opacity="0.9">"#,
        );
        for (i, edge) in mst_edges.iter().enumerate() {
            let _ = writeln!(
                out,
                r#"    <line x1="{:.4}" y1="{:.4}" x2="{:.4}" y2="{:.4}" data-weight="{:.4}" data-polys="{},{}" data-index="{}"/>"#,
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
/// The viewBox is computed automatically from the polyline data.
/// All coordinates are in normalized space (center-origin, +Y up).
///
/// Each highlighted segment `<line>` element includes `data-rank`,
/// `data-length`, `data-from`, and `data-to` attributes for
/// programmatic inspection.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn to_segment_diagnostic_svg(
    polylines: &[Polyline],
    metadata: &SvgMetadata<'_>,
    top_n: usize,
) -> String {
    let vb = compute_diagnostic_view_box(polylines, &[]);
    let vb_str = format_view_box(vb);
    let extent = vb.2.max(vb.3);

    // Scale sizes proportionally to the viewBox extent.
    let sw_thin = extent / 800.0;
    let sw_thick = extent / 250.0;
    let font_size = extent / 60.0;
    let line_height = font_size * 1.5;
    let padding = extent * 0.01;
    let swatch_size = font_size;

    let mut out = String::new();

    write_svg_preamble(
        &mut out,
        &vb_str,
        DIAG_DISPLAY_SIZE,
        DIAG_DISPLAY_SIZE,
        metadata,
    );

    // Dark background for visibility (covers the full viewBox).
    let _ = writeln!(
        out,
        "  <rect x=\"{:.4}\" y=\"{:.4}\" width=\"{:.4}\" height=\"{:.4}\" fill=\"#1a1a1a\"/>",
        vb.0, vb.1, vb.2, vb.3,
    );

    // Contour paths in white
    let _ = writeln!(
        out,
        r#"  <g id="contours" stroke="white" stroke-width="{sw_thin:.4}" fill="none">"#,
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
            r#"  <g id="top-segments" stroke-width="{sw_thick:.4}" opacity="0.9" fill="none">"#,
        );
        for (rank, seg) in all_segments.iter().enumerate() {
            let color = SEGMENT_COLORS[rank % SEGMENT_COLORS.len()];
            let _ = writeln!(
                out,
                r#"    <line x1="{:.4}" y1="{:.4}" x2="{:.4}" y2="{:.4}" stroke="{}" data-rank="{}" data-length="{:.4}" data-from="{:.4},{:.4}" data-to="{:.4},{:.4}"/>"#,
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
        // Sizes are proportional to the viewBox extent so the legend
        // looks consistent regardless of coordinate system.
        let labels: Vec<String> = all_segments
            .iter()
            .enumerate()
            .map(|(rank, seg)| {
                format!(
                    "#{}: {:.4}  ({:.3},{:.3})->({:.3},{:.3})  poly={} seg={}",
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
        // Approximate character width ≈ 0.6 × font_size for monospace.
        #[allow(clippy::cast_precision_loss)]
        let legend_width = (max_label_chars as f64)
            .mul_add(font_size * 0.6, 4.0_f64.mul_add(padding, swatch_size));

        let legend_x = vb.0 + padding;
        let legend_y = vb.1 + padding;

        let _ = writeln!(
            out,
            r#"  <g id="legend" font-family="monospace" font-size="{font_size:.4}">"#,
        );
        #[allow(clippy::cast_precision_loss)]
        let legend_height = (all_segments.len() as f64).mul_add(line_height, line_height) + padding;
        #[allow(clippy::uninlined_format_args)]
        let _ = writeln!(
            out,
            "    <rect x=\"{:.4}\" y=\"{:.4}\" width=\"{:.4}\" height=\"{:.4}\" rx=\"{:.4}\" fill=\"#000000\" opacity=\"0.7\"/>",
            legend_x, legend_y, legend_width, legend_height, padding,
        );
        let _ = writeln!(
            out,
            r#"    <text x="{:.4}" y="{:.4}" fill="white" font-weight="bold">{header_text}</text>"#,
            legend_x + padding,
            legend_y + line_height,
        );
        for (rank, label) in labels.iter().enumerate() {
            let color = SEGMENT_COLORS[rank % SEGMENT_COLORS.len()];
            #[allow(clippy::cast_precision_loss)]
            let row_y = (rank as f64 + 1.0).mul_add(line_height, legend_y + line_height);
            // Color swatch
            let _ = writeln!(
                out,
                r#"    <rect x="{:.4}" y="{:.4}" width="{:.4}" height="{:.4}" fill="{}"/>"#,
                legend_x + padding,
                swatch_size.mul_add(-0.75, row_y),
                swatch_size,
                swatch_size,
                color,
            );
            // Label
            let _ = writeln!(
                out,
                r#"    <text x="{:.4}" y="{row_y:.4}" fill="white">{label}</text>"#,
                legend_x + padding + swatch_size + padding,
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

    /// Standard normalized mapping: unit circle at origin, no margin.
    /// Gives `scale_factor`=100.0, offset=(100,100), 200×200 mm.
    ///
    /// Transform: `mm_x` = `norm_x`*100+100, `mm_y` = -`norm_y`*100+100
    fn test_mapping() -> DocumentMapping {
        document_mapping(
            &MaskShape::Circle {
                center: Point::new(0.0, 0.0),
                radius: 1.0,
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
        let mapping = test_mapping();
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
        let svg = to_svg(&polylines, &no_meta(), &test_mapping());
        assert!(!svg.contains("<path"));
    }

    #[test]
    fn zero_point_polyline_is_skipped() {
        let polylines = vec![Polyline::new(vec![])];
        let svg = to_svg(&polylines, &no_meta(), &test_mapping());
        assert!(!svg.contains("<path"));
    }

    // --- Basic output structure ---

    #[test]
    fn single_polyline_with_two_points() {
        // test_mapping: sf=100, ox=100, oy=100.
        // No Y-flip: (0.5, 0.3) → mm(150, 130), (−0.5, −0.3) → mm(50, 70)
        let polylines = vec![Polyline::new(vec![
            Point::new(0.5, 0.3),
            Point::new(-0.5, -0.3),
        ])];
        let svg = to_svg(&polylines, &no_meta(), &test_mapping());

        assert!(svg.contains(r#"width="200mm""#));
        assert!(svg.contains(r#"height="200mm""#));
        assert!(svg.contains(r#"viewBox="0 0 200 200""#));
        assert!(svg.contains("M150,130 L50,70"));
        assert!(svg.contains(r#"fill="none""#));
        assert!(svg.contains(r#"stroke="black""#));
        assert!(svg.contains(r#"stroke-width="1""#));
    }

    #[test]
    fn single_polyline_with_three_points() {
        let polylines = vec![Polyline::new(vec![
            Point::new(0.1, 0.15),
            Point::new(0.2, 0.3),
            Point::new(0.3, 0.45),
        ])];
        let svg = to_svg(&polylines, &no_meta(), &test_mapping());
        // Verify move-to and line-to commands are present (M and L)
        assert!(svg.contains('M'));
        assert!(svg.contains('L'));
        // Should have exactly one <path> element
        assert_eq!(svg.matches("<path").count(), 1);
    }

    #[test]
    fn multiple_polylines_produce_multiple_paths() {
        let polylines = vec![
            Polyline::new(vec![Point::new(0.1, 0.2), Point::new(0.3, 0.4)]),
            Polyline::new(vec![Point::new(0.5, 0.6), Point::new(0.7, 0.8)]),
        ];
        let svg = to_svg(&polylines, &no_meta(), &test_mapping());

        // Count <path occurrences
        let path_count = svg.matches("<path").count();
        assert_eq!(path_count, 2);
    }

    // --- Mixed valid and degenerate polylines ---

    #[test]
    fn degenerate_polylines_skipped_among_valid_ones() {
        let polylines = vec![
            Polyline::new(vec![]),                                           // skipped
            Polyline::new(vec![Point::new(0.1, 0.1)]),                       // skipped
            Polyline::new(vec![Point::new(0.2, 0.3), Point::new(0.4, 0.5)]), // kept
            Polyline::new(vec![]),                                           // skipped
        ];
        let svg = to_svg(&polylines, &no_meta(), &test_mapping());

        let path_count = svg.matches("<path").count();
        assert_eq!(path_count, 1);
    }

    // --- Document mapping dimensions ---

    #[test]
    fn viewbox_reflects_mapping_dimensions() {
        // Rectangle mapping produces non-square mm dimensions.
        // Normalized rect: half_width=2.0, half_height=1.0 (landscape 2:1).
        let mapping = document_mapping(
            &MaskShape::Rectangle {
                center: Point::new(0.0, 0.0),
                half_width: 2.0,
                half_height: 1.0,
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
        let svg = to_svg(&[], &no_meta(), &test_mapping());
        assert!(svg.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
    }

    #[test]
    fn svg_has_xmlns_namespace() {
        let svg = to_svg(&[], &no_meta(), &test_mapping());
        assert!(svg.contains(r#"xmlns="http://www.w3.org/2000/svg""#));
    }

    #[test]
    fn svg_has_preserve_aspect_ratio() {
        let svg = to_svg(&[], &no_meta(), &test_mapping());
        assert!(svg.contains(r#"preserveAspectRatio="xMidYMid meet""#));
    }

    #[test]
    fn svg_ends_with_closing_tag() {
        // With children, the svg crate emits </svg>
        let polylines = vec![Polyline::new(vec![
            Point::new(1.0, 2.0),
            Point::new(3.0, 4.0),
        ])];
        let svg = to_svg(&polylines, &no_meta(), &test_mapping());
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
        let svg = to_svg(&[], &meta, &test_mapping());
        assert!(svg.contains("<title>cherry-blossoms</title>"));
        assert!(!svg.contains("<desc>"));
    }

    #[test]
    fn desc_element_emitted_when_present() {
        let meta = SvgMetadata {
            description: Some("Exported by mujou"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], &meta, &test_mapping());
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
        let svg = to_svg(&[], &meta, &test_mapping());
        assert!(svg.contains("<title>my-image</title>"));
        assert!(svg.contains("<desc>blur=1.4, canny=15/40</desc>"));
    }

    #[test]
    fn title_and_desc_omitted_when_none() {
        let svg = to_svg(&[], &no_meta(), &test_mapping());
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
        let svg = to_svg(&polylines, &meta, &test_mapping());

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
        let svg = to_svg(&[], &meta, &test_mapping());
        assert!(svg.contains("<title>A &lt;B&gt; &amp; C</title>"));
    }

    #[test]
    fn special_characters_in_desc_are_escaped() {
        let meta = SvgMetadata {
            description: Some("x < y & z > w"),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], &meta, &test_mapping());
        assert!(svg.contains("<desc>x &lt; y &amp; z &gt; w</desc>"));
    }

    // --- Config JSON / <metadata> ---

    #[test]
    fn metadata_element_emitted_when_config_json_present() {
        let meta = SvgMetadata {
            config_json: Some(r#"{"blur_sigma":1.4}"#),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], &meta, &test_mapping());
        assert!(svg.contains("<metadata>"));
        assert!(svg.contains("</metadata>"));
        assert!(svg.contains(r#"<mujou:pipeline xmlns:mujou="https://mujou.app/ns/1">"#));
        assert!(svg.contains("</mujou:pipeline>"));
    }

    #[test]
    fn metadata_element_omitted_when_config_json_none() {
        let svg = to_svg(&[], &no_meta(), &test_mapping());
        assert!(!svg.contains("<metadata>"));
    }

    #[test]
    fn config_json_special_characters_are_escaped() {
        let meta = SvgMetadata {
            config_json: Some(r#"{"note":"a < b & c > d"}"#),
            ..SvgMetadata::default()
        };
        let svg = to_svg(&[], &meta, &test_mapping());
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
        let svg = to_svg(&polylines, &meta, &test_mapping());

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

        // Use zoom=0.5 so the canvas covers the full 40×40 test image.
        let png = sharp_edge_png(40, 40);
        let config = PipelineConfig {
            zoom: 0.5,
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
    fn multiple_polylines_correct_structure() {
        // Two polylines in normalized space, verify SVG structure.
        let polylines = vec![
            Polyline::new(vec![
                Point::new(0.1, 0.15),
                Point::new(0.125, 0.183),
                Point::new(0.14, 0.201),
            ]),
            Polyline::new(vec![
                Point::new(0.3, 0.05),
                Point::new(0.325, 0.078),
                Point::new(0.35, 0.102),
            ]),
        ];
        let svg = to_svg(&polylines, &no_meta(), &test_mapping());

        assert!(svg.contains(r#"width="200mm""#));
        assert!(svg.contains(r#"height="200mm""#));
        assert!(svg.contains(r#"viewBox="0 0 200 200""#));
        // Both paths should be present
        assert_eq!(svg.matches("<path").count(), 2);
        assert!(svg.contains(r#"fill="none""#));
        assert!(svg.contains(r#"stroke="black""#));
        assert!(svg.contains(r#"stroke-width="1""#));
    }

    // --- DocumentMapping + coordinate transform ---

    #[test]
    fn circle_mapping_produces_square_viewbox() {
        // Normalized circle: radius=1.0 at origin, border_margin=0.
        // scale_factor = 200/2 = 100, offset = (100, 100).
        let mapping = test_mapping();
        // No Y-flip: (0.5, 0.5) → mm(150, 150), (-0.5, -0.5) → mm(50, 50)
        let polylines = vec![Polyline::new(vec![
            Point::new(0.5, 0.5),
            Point::new(-0.5, -0.5),
        ])];
        let svg = to_svg(&polylines, &no_meta(), &mapping);

        assert!(svg.contains(r#"width="200mm""#), "width should be 200mm");
        assert!(svg.contains(r#"height="200mm""#), "height should be 200mm");
        assert!(
            svg.contains(r#"viewBox="0 0 200 200""#),
            "viewBox should be 200×200, got:\n{svg}",
        );
        assert!(svg.contains(r#"preserveAspectRatio="xMidYMid meet""#));
        assert!(
            svg.contains("M150,150 L50,50"),
            "path coordinates should be in mm, got:\n{svg}",
        );
    }

    #[test]
    fn circle_mapping_with_border_margin() {
        // Normalized circle: radius=1.0 at origin, border_margin=0.1 (10%).
        // drawing_frac = 0.8, drawing_area = 160mm.
        // scale_factor = 160 / 2 = 80, offset = (100, 100).
        let mapping = document_mapping(
            &MaskShape::Circle {
                center: Point::new(0.0, 0.0),
                radius: 1.0,
            },
            0.1,
        );
        assert!((mapping.width_mm - 200.0).abs() < 1e-9);
        assert!((mapping.height_mm - 200.0).abs() < 1e-9);
        assert!((mapping.scale_factor - 80.0).abs() < 1e-9);
        assert!((mapping.offset_x - 100.0).abs() < 1e-9);
        assert!((mapping.offset_y - 100.0).abs() < 1e-9);

        // No Y-flip:
        // norm (-1.0, 0.0) → mm(-1*80+100, 0*80+100) = (20, 100)
        // norm (1.0, 0.0) → mm(1*80+100, 0*80+100) = (180, 100)
        let polylines = vec![Polyline::new(vec![
            Point::new(-1.0, 0.0),
            Point::new(1.0, 0.0),
        ])];
        let svg = to_svg(&polylines, &no_meta(), &mapping);
        assert!(
            svg.contains("M20,100 L180,100"),
            "10% margin should inset coordinates, got:\n{svg}",
        );
    }

    #[test]
    fn any_circle_uses_mm_viewbox() {
        // Any circle mapping produces a 200×200 mm viewBox.
        let mapping = test_mapping();
        let svg = to_svg(&[], &no_meta(), &mapping);
        assert!(
            svg.contains(r#"viewBox="0 0 200 200""#),
            "viewBox should be mm-based, got:\n{svg}",
        );
        assert!(svg.contains(r#"preserveAspectRatio="xMidYMid meet""#));
    }

    #[test]
    fn circle_mapping_still_renders_paths() {
        // test_mapping: sf=100, ox=100, oy=100.
        // No Y-flip: (-0.5, 0.5) → mm(50, 150), (0.5, -0.5) → mm(150, 50)
        let mapping = test_mapping();
        let polylines = vec![Polyline::new(vec![
            Point::new(-0.5, 0.5),
            Point::new(0.5, -0.5),
        ])];
        let svg = to_svg(&polylines, &no_meta(), &mapping);
        assert!(svg.contains("<path"));
        assert!(
            svg.contains(r#"d="M50,150 L150,50""#),
            "path coordinates should be in mm, got:\n{svg}",
        );
        assert!(svg.contains(r#"viewBox="0 0 200 200""#));
    }

    #[test]
    fn mapping_with_metadata() {
        let mapping = test_mapping();
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
    fn test_mapping_has_expected_values() {
        let m = test_mapping();
        assert!((m.width_mm - 200.0).abs() < 1e-9);
        assert!((m.height_mm - 200.0).abs() < 1e-9);
        // scale_factor = drawing_area / 2 = 200 / 2 = 100
        assert!((m.scale_factor - 100.0).abs() < 1e-9);
        assert!((m.offset_x - 100.0).abs() < 1e-9);
        assert!((m.offset_y - 100.0).abs() < 1e-9);
    }

    #[test]
    fn rectangle_mapping_produces_correct_aspect_ratio() {
        // Normalized rect: half_width=2.0, half_height=1.0 (landscape 2:1).
        // Longer normalized axis = 4.0. drawing_area=200. scale_factor=200/4=50.
        let mapping = document_mapping(
            &MaskShape::Rectangle {
                center: Point::new(0.0, 0.0),
                half_width: 2.0,
                half_height: 1.0,
            },
            0.0,
        );
        assert!((mapping.width_mm - 200.0).abs() < 1e-9);
        assert!((mapping.height_mm - 100.0).abs() < 1e-9);
        assert!((mapping.scale_factor - 50.0).abs() < 1e-9);
    }

    #[test]
    fn rectangle_mapping_taller_than_wide() {
        // Normalized rect: half_width=1.0, half_height=2.0 (portrait 2:1).
        // Longer normalized axis = 4.0. scale_factor = 200/4 = 50.
        let mapping = document_mapping(
            &MaskShape::Rectangle {
                center: Point::new(0.0, 0.0),
                half_width: 1.0,
                half_height: 2.0,
            },
            0.0,
        );
        assert!((mapping.width_mm - 100.0).abs() < 1e-9);
        assert!((mapping.height_mm - 200.0).abs() < 1e-9);
        assert!((mapping.scale_factor - 50.0).abs() < 1e-9);
    }
}
