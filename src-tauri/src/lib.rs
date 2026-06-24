use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;

use base64::Engine;
use serde::Serialize;

#[derive(Serialize)]
pub struct PageImage {
    pub page: usize,
    pub width: u32,
    pub height: u32,
    pub image_data: String,
}

struct OcrState {
    engine: Mutex<Option<ocr::OcrEngine>>,
    renderer: docx_to_image::DocxRenderer,
}

fn decode_base64(data: &str) -> Result<Vec<u8>, String> {
    let b64 = if let Some(pos) = data.find(',') { &data[pos + 1..] } else { data };
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| e.to_string())
}

fn decode_base64_image(data: &str) -> Result<image::DynamicImage, String> {
    let bytes = decode_base64(data)?;
    image::load_from_memory(&bytes).map_err(|e| e.to_string())
}

fn encode_png_base64(img: &image::RgbaImage) -> Result<String, String> {
    let dyn_img = image::DynamicImage::ImageRgba8(img.clone());
    let mut buf = std::io::Cursor::new(Vec::new());
    dyn_img.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(buf.into_inner());
    Ok(format!("data:image/png;base64,{}", b64))
}

#[tauri::command]
fn recognize_image(
    state: tauri::State<OcrState>,
    data: String,
) -> Result<Vec<ocr::OcrBlock>, String> {
    let guard = state.engine.lock().map_err(|e| e.to_string())?;
    let engine = guard.as_ref().ok_or("OCR engine not initialized")?;
    let img = decode_base64_image(&data)?;
    engine.recognize_all(&img).map_err(|e| e.to_string())
}

#[tauri::command]
fn render_docx(
    state: tauri::State<OcrState>,
    data: String,
) -> Result<Vec<PageImage>, String> {
    let docx_bytes = decode_base64(&data)?;
    let pages = state.renderer.render(&docx_bytes).map_err(|e| e.to_string())?;

    let mut results = Vec::with_capacity(pages.len());
    for (i, img) in pages.iter().enumerate() {
        let image_data = encode_png_base64(img)?;
        results.push(PageImage {
            page: i,
            width: img.width(),
            height: img.height(),
            image_data,
        });
    }
    Ok(results)
}

fn load_model_bytes(name: &str) -> Result<Vec<u8>, String> {
    let candidates = [
        format!("models/{}", name),
        format!("src-tauri/models/{}", name),
    ];
    for path in &candidates {
        if let Ok(data) = std::fs::read(path) {
            return Ok(data);
        }
    }
    Err(format!("Cannot find model file: {} (tried {:?})", name, candidates))
}

fn find_bundled_tools_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    // Tauri v2: resources are in resource_dir
    let res_dir = app.path().resource_dir().ok()?;

    // Try platform-specific subdirectory first (e.g. tools/windows-x86_64/)
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let triple = format!("{os}-{arch}");
    let platform_dir = res_dir.join("tools").join(&triple);
    if platform_dir.is_dir() {
        return Some(platform_dir);
    }

    // Fallback: flat tools/ directory
    let flat_dir = res_dir.join("tools");
    if flat_dir.is_dir() {
        return Some(flat_dir);
    }

    None
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let det_bytes = load_model_bytes("det.onnx")?;
            let rec_bytes = load_model_bytes("rec.onnx")?;
            let keys_bytes = load_model_bytes("keys.txt")?;

            let engine = ocr::OcrEngine::new(&det_bytes, &rec_bytes, &keys_bytes)
                .map_err(|e| format!("Failed to init OCR: {}", e))?;

            // ── discover bundled conversion tools ──
            let mut renderer = docx_to_image::DocxRenderer::new();
            if let Some(tools_dir) = find_bundled_tools_dir(&app.handle()) {
                renderer = renderer.add_tool_dir(tools_dir);
            }

            app.manage(OcrState {
                engine: Mutex::new(Some(engine)),
                renderer,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![recognize_image, render_docx])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
