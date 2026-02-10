//! File upload component with drag-and-drop and file picker.

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

/// A drag-and-drop zone with a file picker button.
///
/// Accepts PNG, JPEG, BMP, and WebP images. When a file is selected
/// (via the picker or drag-and-drop), reads the bytes and fires
/// `on_upload` with `(bytes, filename)`.
#[component]
pub fn FileUpload(props: FileUploadProps) -> Element {
    let mut dragging = use_signal(|| false);
    let mut filename = use_signal(|| Option::<String>::None);
    let mut error = use_signal(|| Option::<String>::None);

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
                    filename.set(Some(name.clone()));
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
        dragging.set(false);
        process_files(evt.files()).await;
    };

    let border_class = if dragging() {
        "border-[var(--border-accent)] bg-[var(--surface-active)]"
    } else {
        "border-[var(--border-muted)] bg-[var(--surface)]"
    };

    rsx! {
        div {
            class: "border-2 border-dashed rounded-lg p-6 text-center transition-colors {border_class}",
            ondragover: move |evt| {
                evt.prevent_default();
                dragging.set(true);
            },
            ondragleave: move |_| {
                dragging.set(false);
            },
            ondrop: handle_drop,

            if let Some(ref name) = filename() {
                p { class: "text-[var(--text-success)] mb-2",
                    "Loaded: {name}"
                }
            }

            if let Some(ref err) = error() {
                p { class: "text-[var(--text-error)] mb-2",
                    "{err}"
                }
            }

            p { class: "text-[var(--text-secondary)] mb-3",
                "Drop an image here or "
            }

            label {
                class: "inline-block px-4 py-2 bg-[var(--btn-primary)] hover:bg-[var(--btn-primary-hover)] rounded cursor-pointer text-white font-medium transition-colors",
                input {
                    r#type: "file",
                    accept: ".png,.jpg,.jpeg,.bmp,.webp",
                    class: "hidden",
                    onchange: handle_files,
                }
                "Choose File"
            }

            p { class: "text-[var(--muted)] text-sm mt-2",
                "PNG, JPEG, BMP, WebP"
            }
        }
    }
}
