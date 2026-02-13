//! Per-stage parameter controls.
//!
//! Renders sliders, selects, and toggles appropriate to the currently
//! selected pipeline stage. Only the selected stage's controls are shown,
//! reinforcing the visual connection between what you're looking at and
//! what you can adjust.

use dioxus::prelude::*;
use mujou_pipeline::{
    ContourTracerKind, DownsampleFilter, PathJoinerKind, PipelineConfig, max_gradient_magnitude,
};

use crate::stage::StageId;

/// Props for the [`StageControls`] component.
#[derive(Props, Clone, PartialEq)]
pub struct StageControlsProps {
    /// Currently selected stage (determines which controls to show).
    stage: StageId,
    /// Current pipeline configuration (read-only).
    config: PipelineConfig,
    /// Callback fired when any parameter changes.
    on_config_change: EventHandler<PipelineConfig>,
}

/// Renders parameter controls for the currently selected pipeline stage.
///
/// Each stage shows only its relevant controls:
/// - **Original**: no controls
/// - **Grayscale**: no controls
/// - **Blur**: blur sigma slider
/// - **Edges**: Canny low/high sliders, invert toggle
/// - **Contours**: contour tracer select
/// - **Simplified**: simplify tolerance slider
/// - **Path**: path joiner select
/// - **Masked**: circular mask toggle, mask diameter slider
#[component]
#[allow(clippy::too_many_lines)]
pub fn StageControls(props: StageControlsProps) -> Element {
    let config = &props.config;
    let on_change = props.on_config_change;

    match props.stage {
        StageId::Original | StageId::Grayscale => {
            rsx! {
                p { class: "text-sm text-[var(--text-secondary)] italic",
                    "No adjustable parameters for this stage."
                }
            }
        }

        StageId::Downsampled => {
            let value = config.working_resolution;
            let config_slider = config.clone();
            let config_filter = config.clone();
            rsx! {
                div { class: "space-y-2",
                    {render_slider(
                        "working_resolution",
                        "Working Resolution",
                        f64::from(value),
                        64.0,
                        1024.0,
                        1.0,
                        0,
                        move |v: f64| {
                            let mut c = config_slider.clone();
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            { c.working_resolution = v as u32; }
                            on_change.call(c);
                        },
                    )}
                    {render_select(
                        "downsample_filter",
                        "Downsample Filter",
                        &[
                            ("None", "None (Disabled)"),
                            ("Nearest", "Nearest"),
                            ("Triangle", "Triangle (Bilinear)"),
                            ("CatmullRom", "CatmullRom (Bicubic)"),
                            ("Gaussian", "Gaussian"),
                            ("Lanczos3", "Lanczos3"),
                        ],
                        match config_filter.downsample_filter {
                            DownsampleFilter::None => "None",
                            DownsampleFilter::Nearest => "Nearest",
                            DownsampleFilter::Triangle => "Triangle",
                            DownsampleFilter::CatmullRom => "CatmullRom",
                            DownsampleFilter::Gaussian => "Gaussian",
                            DownsampleFilter::Lanczos3 => "Lanczos3",
                        },
                        move |v: String| {
                            let mut c = config_filter.clone();
                            c.downsample_filter = match v.as_str() {
                                "None" => DownsampleFilter::None,
                                "Nearest" => DownsampleFilter::Nearest,
                                "CatmullRom" => DownsampleFilter::CatmullRom,
                                "Gaussian" => DownsampleFilter::Gaussian,
                                "Lanczos3" => DownsampleFilter::Lanczos3,
                                _ => DownsampleFilter::Triangle,
                            };
                            on_change.call(c);
                        },
                    )}
                }
            }
        }

        StageId::Blur => {
            let value = config.blur_sigma;
            let config = config.clone();
            rsx! {
                div { class: "space-y-2",
                    {render_slider(
                        "blur_sigma",
                        "Blur Sigma",
                        f64::from(value),
                        0.0,
                        10.0,
                        0.1,
                        1,
                        move |v: f64| {
                            let mut c = config.clone();
                            #[allow(clippy::cast_possible_truncation)]
                            { c.blur_sigma = v as f32; }
                            on_change.call(c);
                        },
                    )}
                }
            }
        }

        StageId::Edges => {
            let canny_low = config.canny_low;
            let canny_high = config.canny_high;
            let canny_max = config.canny_max;
            let invert = config.invert;
            let config_low = config.clone();
            let config_high = config.clone();
            let config_max = config.clone();
            let config_invert = config.clone();
            let theoretical_max = f64::from(max_gradient_magnitude());
            rsx! {
                div { class: "space-y-2",
                    {render_slider(
                        "canny_low",
                        "Canny Low",
                        f64::from(canny_low),
                        1.0,
                        f64::from(canny_max),
                        1.0,
                        0,
                        move |v: f64| {
                            let mut c = config_low.clone();
                            #[allow(clippy::cast_possible_truncation)]
                            let v = v as f32;
                            // Enforce canny_low <= canny_high.
                            c.canny_low = v.min(c.canny_high);
                            on_change.call(c);
                        },
                    )}
                    {render_slider(
                        "canny_high",
                        "Canny High",
                        f64::from(canny_high),
                        1.0,
                        f64::from(canny_max),
                        1.0,
                        0,
                        move |v: f64| {
                            let mut c = config_high.clone();
                            #[allow(clippy::cast_possible_truncation)]
                            let v = v as f32;
                            // Enforce canny_low <= canny_high <= canny_max.
                            c.canny_high = v.max(c.canny_low).min(c.canny_max);
                            on_change.call(c);
                        },
                    )}
                    {render_slider(
                        "canny_max",
                        "Canny Max",
                        f64::from(canny_max),
                        0.0,
                        theoretical_max,
                        1.0,
                        0,
                        move |v: f64| {
                            let mut c = config_max.clone();
                            #[allow(clippy::cast_possible_truncation)]
                            let v = v as f32;
                            // Slider range starts at 0 so the full
                            // scale is visible, but clamp so canny_max
                            // never drops below canny_high.
                            c.canny_max = v.max(c.canny_high);
                            on_change.call(c);
                        },
                    )}
                    {render_toggle(
                        "invert",
                        "Invert",
                        invert,
                        move |v: bool| {
                            let mut c = config_invert.clone();
                            c.invert = v;
                            on_change.call(c);
                        },
                    )}
                }
            }
        }

        StageId::Contours => {
            let config = config.clone();
            rsx! {
                div { class: "space-y-2",
                    {render_select(
                        "contour_tracer",
                        "Contour Tracer",
                        &[("BorderFollowing", "Border Following")],
                        match config.contour_tracer {
                            ContourTracerKind::BorderFollowing => "BorderFollowing",
                        },
                        move |_v: String| {
                            let mut c = config.clone();
                            // Currently only one variant; when more are
                            // added, match on _v to select the right one.
                            c.contour_tracer = ContourTracerKind::BorderFollowing;
                            on_change.call(c);
                        },
                    )}
                }
            }
        }

        StageId::Simplified => {
            let value = config.simplify_tolerance;
            let config = config.clone();
            rsx! {
                div { class: "space-y-2",
                    {render_slider(
                        "simplify_tolerance",
                        "Simplify Tolerance",
                        value,
                        0.0,
                        20.0,
                        0.1,
                        1,
                        move |v: f64| {
                            let mut c = config.clone();
                            c.simplify_tolerance = v;
                            on_change.call(c);
                        },
                    )}
                }
            }
        }

        StageId::Path => {
            let config_select = config.clone();
            let config_slider = config.clone();
            let is_mst = matches!(config.path_joiner, PathJoinerKind::Mst);
            rsx! {
                div { class: "space-y-2",
                    {render_select(
                        "path_joiner",
                        "Path Joiner",
                        &[("Mst", "MST"), ("Retrace", "Retrace"), ("StraightLine", "Straight Line")],
                        match config_select.path_joiner {
                            PathJoinerKind::Mst => "Mst",
                            PathJoinerKind::StraightLine => "StraightLine",
                            PathJoinerKind::Retrace => "Retrace",
                        },
                        move |v: String| {
                            let mut c = config_select.clone();
                            c.path_joiner = match v.as_str() {
                                "Retrace" => PathJoinerKind::Retrace,
                                "StraightLine" => PathJoinerKind::StraightLine,
                                _ => PathJoinerKind::Mst,
                            };
                            on_change.call(c);
                        },
                    )}

                    if is_mst {
                        {render_slider(
                            "mst_neighbours",
                            "MST Neighbours",
                            #[allow(clippy::cast_precision_loss)]
                            { config_slider.mst_neighbours as f64 },
                            1.0,
                            100.0,
                            1.0,
                            0,
                            move |v: f64| {
                                let mut c = config_slider.clone();
                                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                                { c.mst_neighbours = v as usize; }
                                on_change.call(c);
                            },
                        )}
                    }
                }
            }
        }

        StageId::Masked => {
            let mask_enabled = config.circular_mask;
            let diameter = config.mask_diameter;
            let config_toggle = config.clone();
            let config_slider = config.clone();
            rsx! {
                div { class: "space-y-2",
                    {render_toggle(
                        "circular_mask",
                        "Circular Mask",
                        mask_enabled,
                        move |v: bool| {
                            let mut c = config_toggle.clone();
                            c.circular_mask = v;
                            on_change.call(c);
                        },
                    )}

                    if mask_enabled {
                        {render_slider(
                            "mask_diameter",
                            "Mask Diameter",
                            diameter,
                            0.1,
                            1.0,
                            0.01,
                            2,
                            move |v: f64| {
                                let mut c = config_slider.clone();
                                c.mask_diameter = v;
                                on_change.call(c);
                            },
                        )}
                    }
                }
            }
        }
    }
}

/// Render a labeled range slider.
#[allow(clippy::too_many_arguments)]
fn render_slider(
    id: &str,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    decimals: usize,
    on_input: impl Fn(f64) + 'static,
) -> Element {
    let display = format!("{value:.decimals$}");
    let id = id.to_string();
    let label = label.to_string();

    rsx! {
        div { class: "flex flex-col gap-1",
            div { class: "flex justify-between text-sm",
                label { r#for: "{id}",
                    class: "text-[var(--text-heading)] font-medium",
                    "{label}"
                }
                span { class: "text-[var(--text-secondary)] tabular-nums",
                    "{display}"
                }
            }
            input {
                r#type: "range",
                id: "{id}",
                min: "{min}",
                max: "{max}",
                step: "{step}",
                value: "{value}",
                class: "w-full accent-[var(--btn-primary)]",
                oninput: move |e| {
                    match e.value().parse::<f64>() {
                        Ok(v) => on_input(v),
                        Err(err) => {
                            web_sys::console::warn_1(
                                &format!("slider parse failure: {err:?} from {:?}", e.value())
                                    .into(),
                            );
                        }
                    }
                },
            }
        }
    }
}

/// Render a labeled toggle (checkbox styled as switch).
fn render_toggle(
    id: &str,
    label: &str,
    checked: bool,
    on_change: impl Fn(bool) + 'static,
) -> Element {
    let id = id.to_string();
    let label = label.to_string();

    rsx! {
        div { class: "flex items-center justify-between",
            label { r#for: "{id}",
                class: "text-sm text-[var(--text-heading)] font-medium",
                "{label}"
            }
            input {
                r#type: "checkbox",
                id: "{id}",
                checked: checked,
                class: "w-5 h-5 accent-[var(--btn-primary)]",
                onchange: move |e| {
                    on_change(e.checked());
                },
            }
        }
    }
}

/// Render a labeled select dropdown.
fn render_select(
    id: &str,
    label: &str,
    options: &[(&str, &str)],
    selected: &str,
    on_change: impl Fn(String) + 'static,
) -> Element {
    let id = id.to_string();
    let label = label.to_string();
    let options: Vec<(String, String)> = options
        .iter()
        .map(|(v, l)| ((*v).to_string(), (*l).to_string()))
        .collect();
    let selected = selected.to_string();

    rsx! {
        div { class: "flex flex-col gap-1",
            label { r#for: "{id}",
                class: "text-sm text-[var(--text-heading)] font-medium",
                "{label}"
            }
            select {
                id: "{id}",
                class: "px-2 py-1 rounded border border-[var(--border)] bg-[var(--surface)]
                        text-[var(--text)] text-sm",
                value: "{selected}",
                onchange: move |e| {
                    on_change(e.value());
                },

                for (value, display) in options.iter() {
                    option {
                        value: "{value}",
                        selected: value == &selected,
                        "{display}"
                    }
                }
            }
        }
    }
}
