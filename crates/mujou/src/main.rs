use dioxus::prelude::*;

fn main() {
    dioxus::launch(app);
}

fn app() -> Element {
    rsx! {
        div { class: "min-h-screen bg-gray-900 text-white flex items-center justify-center",
            div { class: "text-center",
                h1 { class: "text-4xl font-bold mb-4", "mujou" }
                p { class: "text-gray-400",
                    "Image to vector path converter for sand tables and CNC devices"
                }
            }
        }
    }
}
