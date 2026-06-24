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

// ── model commands ─────────────────────────────────────────────────

#[tauri::command]
fn list_models() -> Result<Vec<String>, String> {
    find_models_root()
        .map(|dir| {
            let mut variants: Vec<String> = std::fs::read_dir(&dir)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|e| {
                    let path = e.ok()?;
                    if path.file_type().ok()?.is_dir() {
                        Some(path.file_name().to_string_lossy().to_string())
                    } else {
                        None
                    }
                })
                .collect();
            variants.sort();
            variants
        })
}

#[tauri::command]
fn switch_model(
    state: tauri::State<OcrState>,
    variant: String,
) -> Result<(), String> {
    let model_dir = find_models_root()?.join(&variant);

    let det_path = model_dir.join("det.onnx");
    let rec_path = model_dir.join("rec.onnx");
    let keys_path = model_dir.join("keys.txt");

    let det_bytes = std::fs::read(&det_path)
        .map_err(|e| format!("读取 {variant}/det.onnx 失败: {e}"))?;
    let rec_bytes = std::fs::read(&rec_path)
        .map_err(|e| format!("读取 {variant}/rec.onnx 失败: {e}"))?;
    let keys_bytes = std::fs::read(&keys_path)
        .map_err(|e| format!("读取 {variant}/keys.txt 失败: {e}"))?;

    let engine = ocr::OcrEngine::new(&det_bytes, &rec_bytes, &keys_bytes)
        .map_err(|e| format!("加载模型 {variant} 失败: {e}"))?;

    let mut guard = state.engine.lock().map_err(|e| e.to_string())?;
    *guard = Some(engine);
    Ok(())
}

// ── model path discovery ───────────────────────────────────────────

fn find_models_root() -> Result<PathBuf, String> {
    let candidates = [
        PathBuf::from("models").join("ocr"),
        PathBuf::from("src-tauri").join("models").join("ocr"),
    ];
    for path in &candidates {
        if path.is_dir() {
            return Ok(path.clone());
        }
    }
    Err("模型目录 models/ocr/ 未找到".into())
}

fn load_model_for_variant(variant: &str) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
    let root = find_models_root()?;
    let dir = root.join(variant);
    let det = std::fs::read(dir.join("det.onnx"))
        .map_err(|e| format!("读取 {variant}/det.onnx 失败: {e}"))?;
    let rec = std::fs::read(dir.join("rec.onnx"))
        .map_err(|e| format!("读取 {variant}/rec.onnx 失败: {e}"))?;
    let keys = std::fs::read(dir.join("keys.txt"))
        .map_err(|e| format!("读取 {variant}/keys.txt 失败: {e}"))?;
    Ok((det, rec, keys))
}

fn find_bundled_tools_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    let res_dir = app.path().resource_dir().ok()?;

    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let triple = format!("{os}-{arch}");
    let platform_dir = res_dir.join("tools").join(&triple);
    if platform_dir.is_dir() {
        return Some(platform_dir);
    }

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
            let default_variant = "v4";
            let (det_bytes, rec_bytes, keys_bytes) =
                load_model_for_variant(default_variant)?;

            let engine = ocr::OcrEngine::new(&det_bytes, &rec_bytes, &keys_bytes)
                .map_err(|e| format!("Failed to init OCR: {}", e))?;

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
        .invoke_handler(tauri::generate_handler![
            recognize_image,
            render_docx,
            list_models,
            switch_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
