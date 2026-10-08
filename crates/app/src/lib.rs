mod app;
mod colors;
mod platform;
mod render2d;
mod render3d;
#[cfg(target_arch = "wasm32")]
mod web;

pub use app::{Args, SpaceHunter};

#[cfg(target_arch = "wasm32")]
mod entry {
    use super::*;
    use wasm_bindgen::prelude::*;

    /// Called from index.html with the id of the <canvas>.
    #[wasm_bindgen]
    pub async fn start(canvas_id: String, demo: bool) -> Result<(), JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let canvas: web_sys::HtmlCanvasElement = window
            .document()
            .and_then(|d| d.get_element_by_id(&canvas_id))
            .ok_or("canvas not found")?
            .dyn_into()?;
        let mut opts = eframe::WebOptions::default();
        opts.depth_buffer = 24;
        eframe::WebRunner::new()
            .start(canvas, opts, Box::new(move |cc| Ok(Box::new(SpaceHunter::new(cc, Args { demo, ..Default::default() })))))
            .await
    }
}
