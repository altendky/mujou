//! Generate a horizontal fade comparison image: original on the left,
//! processed pipeline output on the right, with a smooth linear blend.

use std::path::PathBuf;

use clap::Parser;
use image::{Rgba, RgbaImage};
use mujou_pipeline::{PipelineConfig, Polyline, process_staged};
use tiny_skia::{LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// Generate a horizontal fade comparison image: original on the left,
/// rendered pipeline vector output on the right, blended smoothly.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Input image path.
    #[arg(default_value = "assets/examples/cherry-blossoms.png")]
    input: PathBuf,

    /// Output image path (PNG recommended).
    #[arg(short, long)]
    output: PathBuf,

    /// Line width for rendered polyline paths.
    ///
    /// Absolute pixels (e.g. "3px") or percentage of image width (e.g. "0.1%").
    /// Defaults to the equivalent of 1px at the pipeline's working resolution,
    /// scaled proportionally to the full-resolution original.
    #[arg(long, value_name = "WIDTH")]
    line_width: Option<String>,

    /// Center point of the fade gradient as "X,Y" percentages of image
    /// width and height (e.g. "40,40" shifts the 50/50 blend point to
    /// 40% from the left and 40% from the top).
    #[arg(long, value_name = "X,Y", default_value = "50,50")]
    fade_center: String,

    /// Clockwise rotation of the fade gradient direction in degrees.
    /// 0 = horizontal left-to-right, 90 = top-to-bottom.
    #[arg(long, value_name = "DEG", default_value_t = 0.0)]
    fade_angle: f64,
}

// ---------------------------------------------------------------------------
// Fade parameters
// ---------------------------------------------------------------------------

/// Controls the direction, position, and orientation of the blend gradient.
struct FadeParams {
    /// Center of the fade as fractions (0.0–1.0) of image width / height.
    center_x: f64,
    center_y: f64,
    /// Clockwise rotation angle in radians.
    angle_rad: f64,
}

impl FadeParams {
    /// Parse `--fade-center "X,Y"` (percentages) and `--fade-angle` (degrees).
    fn parse(center: &str, angle_deg: f64) -> Result<Self, String> {
        let (x_str, y_str) = center
            .split_once(',')
            .ok_or_else(|| format!("fade-center must be 'X,Y', got: '{center}'"))?;

        let x_pct: f64 = x_str
            .trim()
            .parse()
            .map_err(|e| format!("invalid fade-center X '{x_str}': {e}"))?;
        let y_pct: f64 = y_str
            .trim()
            .parse()
            .map_err(|e| format!("invalid fade-center Y '{y_str}': {e}"))?;

        Ok(Self {
            center_x: x_pct / 100.0,
            center_y: y_pct / 100.0,
            angle_rad: angle_deg.to_radians(),
        })
    }
}

// ---------------------------------------------------------------------------
// Line-width parsing
// ---------------------------------------------------------------------------

/// Parsed line-width specification.
enum LineWidth {
    /// Absolute pixel count in the output image.
    Pixels(f64),
    /// Percentage of the output image width.
    Percent(f64),
}

impl LineWidth {
    fn parse(s: &str) -> Result<Self, String> {
        if let Some(px_str) = s.strip_suffix("px") {
            let val: f64 = px_str
                .parse()
                .map_err(|e| format!("invalid pixel value '{px_str}': {e}"))?;
            if val <= 0.0 {
                return Err(format!("line width must be positive, got {val}"));
            }
            Ok(Self::Pixels(val))
        } else if let Some(pct_str) = s.strip_suffix('%') {
            let val: f64 = pct_str
                .parse()
                .map_err(|e| format!("invalid percentage value '{pct_str}': {e}"))?;
            if val <= 0.0 {
                return Err(format!("line width percentage must be positive, got {val}"));
            }
            Ok(Self::Percent(val))
        } else {
            Err(format!("line width must end with 'px' or '%', got: '{s}'"))
        }
    }

    fn resolve(self, image_width: u32) -> f64 {
        match self {
            Self::Pixels(px) => px,
            Self::Percent(pct) => pct / 100.0 * f64::from(image_width),
        }
    }
}

// ---------------------------------------------------------------------------
// Polyline rendering via tiny-skia
// ---------------------------------------------------------------------------

/// Render a polyline as black anti-aliased strokes on a transparent background.
///
/// Coordinates are scaled from the pipeline's working resolution to the
/// full output dimensions using the provided scale factors.  `tiny-skia`
/// handles sub-pixel positioning and proper AA internally.
#[allow(clippy::cast_possible_truncation)]
fn render_polyline(
    polyline: &Polyline,
    width: u32,
    height: u32,
    scale_x: f64,
    scale_y: f64,
    line_width: f64,
) -> RgbaImage {
    let points = polyline.points();

    // Build a tiny-skia path from the polyline points.
    let mut pb = PathBuilder::new();
    if let Some(first) = points.first() {
        pb.move_to((first.x * scale_x) as f32, (first.y * scale_y) as f32);
        for p in &points[1..] {
            pb.line_to((p.x * scale_x) as f32, (p.y * scale_y) as f32);
        }
    }

    let Some(path) = pb.finish() else {
        // Empty or degenerate path — return a blank image.
        return RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    };

    // Configure stroke: round caps and joins for smooth curves.
    let stroke = Stroke {
        width: line_width as f32,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };

    // Black, fully opaque paint.
    let mut paint = Paint::default();
    paint.set_color_rgba8(0, 0, 0, 255);
    paint.anti_alias = true;

    // Render into a pixmap.
    let Some(mut pixmap) = Pixmap::new(width, height) else {
        return RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);

    // Convert the pixmap (premultiplied RGBA) to an `RgbaImage` (straight RGBA).
    let pixmap_data = pixmap.data();
    let mut img = RgbaImage::new(width, height);
    for (i, pixel) in img.pixels_mut().enumerate() {
        let off = i * 4;
        let a = pixmap_data[off + 3];
        if a == 0 {
            *pixel = Rgba([0, 0, 0, 0]);
        } else {
            // Un-premultiply: channel = premultiplied * 255 / alpha.
            let r = u16::from(pixmap_data[off]) * 255 / u16::from(a);
            let g = u16::from(pixmap_data[off + 1]) * 255 / u16::from(a);
            let b = u16::from(pixmap_data[off + 2]) * 255 / u16::from(a);
            *pixel = Rgba([r as u8, g as u8, b as u8, a]);
        }
    }
    img
}

// ---------------------------------------------------------------------------
// Image blending
// ---------------------------------------------------------------------------

/// Blend two RGBA images along a directed linear gradient.
///
/// The gradient is centred on `fade.center_x/y` (as fractions of the image
/// dimensions) and rotated by `fade.angle_rad` clockwise.  `t = 0.5` falls
/// exactly on the centre point; the gradient extends symmetrically to the
/// farthest image corner in each direction.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn blend_images(original: &RgbaImage, processed: &RgbaImage, fade: &FadeParams) -> RgbaImage {
    let (width, height) = original.dimensions();
    let mut output = RgbaImage::new(width, height);

    let w = f64::from(width);
    let h = f64::from(height);
    let cx = fade.center_x * w;
    let cy = fade.center_y * h;
    let cos_a = fade.angle_rad.cos();
    let sin_a = fade.angle_rad.sin();

    // Project the four image corners onto the gradient axis (relative to
    // the centre) and find the symmetric half-extent so that t = 0.5 at
    // the centre point.
    let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)];
    let mut half_extent: f64 = 0.0;
    for &(x, y) in &corners {
        let proj = (x - cx).mul_add(cos_a, (y - cy) * sin_a);
        half_extent = half_extent.max(proj.abs());
    }

    let inv_extent = if half_extent > f64::EPSILON {
        0.5 / half_extent
    } else {
        0.0
    };

    for y_px in 0..height {
        for x_px in 0..width {
            let proj = (f64::from(x_px) - cx).mul_add(cos_a, (f64::from(y_px) - cy) * sin_a);
            let t = proj.mul_add(inv_extent, 0.5).clamp(0.0, 1.0);

            let orig = original.get_pixel(x_px, y_px);
            let proc_px = processed.get_pixel(x_px, y_px);

            let blend = |o: u8, p: u8| -> u8 {
                let val = f64::from(o).mul_add(1.0 - t, f64::from(p) * t);
                val.round().clamp(0.0, 255.0) as u8
            };

            output.put_pixel(
                x_px,
                y_px,
                Rgba([
                    blend(orig[0], proc_px[0]),
                    blend(orig[1], proc_px[1]),
                    blend(orig[2], proc_px[2]),
                    blend(orig[3], proc_px[3]),
                ]),
            );
        }
    }
    output
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    eprintln!("Reading image from {}", args.input.display());
    let image_bytes = std::fs::read(&args.input)?;

    eprintln!("Processing with default pipeline configuration...");
    let staged = process_staged(&image_bytes, &PipelineConfig::default())?;

    let original = &staged.original;
    let (orig_w, orig_h) = original.dimensions();
    let work_w = staged.dimensions.width;
    let work_h = staged.dimensions.height;

    let scale_x = f64::from(orig_w) / f64::from(work_w);
    let scale_y = f64::from(orig_h) / f64::from(work_h);

    // Resolve line width: default is 1px at working resolution, scaled up.
    let line_width = match args.line_width {
        Some(spec) => LineWidth::parse(&spec)
            .map_err(|e| format!("--line-width: {e}"))?
            .resolve(orig_w),
        None => scale_x,
    };

    eprintln!(
        "Original: {orig_w}x{orig_h}, working: {work_w}x{work_h}, \
         scale: {scale_x:.2}x{scale_y:.2}, line width: {line_width:.1}px"
    );

    eprintln!("Rendering polyline...");
    let rendered = render_polyline(
        staged.final_polyline(),
        orig_w,
        orig_h,
        scale_x,
        scale_y,
        line_width,
    );

    let fade = FadeParams::parse(&args.fade_center, args.fade_angle)
        .map_err(|e| format!("--fade-center / --fade-angle: {e}"))?;

    eprintln!(
        "Fade center: ({:.0}%, {:.0}%), angle: {:.1}°",
        fade.center_x * 100.0,
        fade.center_y * 100.0,
        args.fade_angle,
    );

    eprintln!("Blending images...");
    let blended = blend_images(original, &rendered, &fade);

    eprintln!("Saving to {}", args.output.display());
    blended.save(&args.output)?;

    eprintln!("Done.");
    Ok(())
}
