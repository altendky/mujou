//! File upload component with compact button and full-page drag overlay.

use dioxus::html::{FileData, HasFileData};
use dioxus::prelude::*;

/// Allowed file extensions for image uploads.
const ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "webp"];

/// Check whether a filename has an allowed image extension.
fn has_allowed_extension(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| {
        ALLOWED_EXTENSIONS
            .iter()
            .any(|a| a.eq_ignore_ascii_case(ext))
    })
}

/// Props for the [`FileUpload`] component.
#[derive(Props, Clone, PartialEq)]
pub struct FileUploadProps {
    /// Called with the raw file bytes and filename after a successful upload.
    on_upload: EventHandler<(Vec<u8>, String)>,
}

/// A compact upload button with a full-page drag-and-drop overlay.
///
/// Renders an "Upload Image" button (intended for placement in the header)
/// and a fixed-position drag overlay that appears only when a file is
/// dragged over the browser window. Accepts PNG, JPEG, BMP, and WebP
/// images. When a file is selected (via the picker or drag-and-drop),
/// reads the bytes and fires `on_upload` with `(bytes, filename)`.
///
/// Uses a `dragenter`/`dragleave` counter to handle the classic
/// child-element event bubbling problem where entering a child fires
/// `dragleave` on the parent.
#[component]
pub fn FileUpload(props: FileUploadProps) -> Element {
    // Counter for dragenter/dragleave balancing. Entering a child
    // element fires dragenter again before dragleave on the parent,
    // so we track the depth instead of a simple boolean.
    let mut drag_counter = use_signal(|| 0i32);
    let mut error = use_signal(|| Option::<String>::None);

    let dragging = drag_counter() > 0;

    // Validate, read, and forward the first file from a list.
    //
    // Shared by the file-picker (`handle_files`) and drag-and-drop
    // (`handle_drop`) paths so the validation/read/callback logic
    // lives in one place.
    let process_files = move |files: Vec<FileData>| async move {
        if let Some(file) = files.first() {
            let name = file.name();
            if !has_allowed_extension(&name) {
                error.set(Some(format!("Unsupported file type: {name}")));
                return;
            }
            match file.read_bytes().await {
                Ok(bytes) => {
                    error.set(None);
                    props.on_upload.call((bytes.to_vec(), name));
                }
                Err(e) => {
                    error.set(Some(format!("Failed to read file: {e}")));
                }
            }
        }
    };

    let handle_files = move |evt: FormEvent| async move {
        process_files(evt.files()).await;
    };

    let handle_drop = move |evt: DragEvent| async move {
        evt.prevent_default();
        drag_counter.set(0);
        process_files(evt.files()).await;
    };

    rsx! {
        // Compact upload button
        div { class: "flex items-center gap-3",
            label {
                class: "inline-flex items-center gap-2 px-4 py-2 bg-[var(--btn-primary)] hover:bg-[var(--btn-primary-hover)] rounded cursor-pointer text-white font-medium transition-colors",
                input {
                    r#type: "file",
                    accept: ".png,.jpg,.jpeg,.bmp,.webp",
                    class: "hidden",
                    onchange: handle_files,
                }
                "Upload Image"
            }

            if let Some(ref err) = error() {
                span { class: "text-[var(--text-error)] text-sm",
                    "{err}"
                }
            }
        }

        // Full-page drag overlay sentinel.
        // Always in the DOM but invisible and non-interactive until a
        // file is dragged over the window.
        div {
            class: "fixed inset-0 z-50 transition-opacity duration-200",
            class: if dragging { "opacity-100 pointer-events-auto" } else { "opacity-0 pointer-events-none" },

            ondragenter: move |evt| {
                evt.prevent_default();
                drag_counter += 1;
            },
            ondragover: move |evt| {
                evt.prevent_default();
            },
            ondragleave: move |_| {
                drag_counter -= 1;
            },
            ondrop: handle_drop,

            // Semi-transparent backdrop
            div { class: "absolute inset-0 bg-black/50" }

            // Centered drop prompt
            div { class: "absolute inset-8 flex items-center justify-center border-4 border-dashed border-white/70 rounded-2xl",
                div { class: "text-center",
                    p { class: "text-white text-2xl font-semibold mb-2",
                        "Drop image here"
                    }
                    p { class: "text-white/70 text-sm",
                        "PNG, JPEG, BMP, WebP"
                    }
                }
            }
        }
    }
}
