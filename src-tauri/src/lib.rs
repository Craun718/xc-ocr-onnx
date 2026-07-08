use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;
use log::{info, warn};

use base64::Engine;
use serde::Serialize;

#[derive(Serialize)]
pub struct PageImage {
    pub page: usize,
    pub width: u32,
    pub height: u32,
    pub orientation: String,
    pub image_data: String,
}

struct OcrState {
    engine: Mutex<Option<ocr::OcrEngine>>,
    orientation_classifier: Mutex<Option<ocr::DocOrientationClassifier>>,
    renderer: docx_to_image::DocxRenderer,
}

#[tauri::command]
fn read_file_as_data_url(path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("读取文件失败: {}", e))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:;base64,{}", b64))
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

#[derive(Serialize)]
pub struct RecognizeResult {
    blocks: Vec<ocr::OcrBlock>,
    corrected_image: Option<String>,  // 矫正后的图像 base64（如果有旋转）
    rotation_angle: u32,              // 检测到的旋转角度
}

#[tauri::command(rename_all = "snake_case")]
fn recognize_image(
    state: tauri::State<OcrState>,
    filename: String,
    data: String,
    order_by: Option<String>,
) -> Result<RecognizeResult, String> {
    let guard = state.engine.lock().map_err(|e| e.to_string())?;
    let engine = guard.as_ref().ok_or("OCR engine not initialized")?;
    let img = decode_base64_image(&data)?;
    info!("[xc-ocr] 识别图片: {}, 尺寸: {}x{} px, base64: {} bytes",
        filename, img.width(), img.height(), data.len());

    // 自动矫正文档方向
    let (img, rotation_angle, corrected_image) = if let Ok(classifier_guard) = state.orientation_classifier.lock() {
        if let Some(classifier) = classifier_guard.as_ref() {
            match classifier.correct_orientation(&img) {
                Ok((corrected, result)) => {
                    let angle = result.orientation.angle();
                    if angle != 0 {
                        info!("[xc-ocr] 自动旋转: {}° -> 0°, 置信度: {:.3}",
                            angle, result.confidence);
                        // 返回矫正后的图像 base64
                        let corrected_b64 = encode_png_base64(&corrected.to_rgba8())
                            .map_err(|e| format!("编码矫正图像失败: {}", e))?;
                        (corrected, angle, Some(corrected_b64))
                    } else {
                        (img, 0, None)
                    }
                }
                Err(e) => {
                    warn!("[xc-ocr] 方向检测失败: {}, 使用原图", e);
                    (img, 0, None)
                }
            }
        } else {
            (img, 0, None)
        }
    } else {
        (img, 0, None)
    };

    let mut blocks = engine.recognize_all(&img).map_err(|e| e.to_string())?;
    info!("[xc-ocr] 识别完成: {}, {} 个文本块", filename, blocks.len());
    for (i, b) in blocks.iter().enumerate() {
        info!("[xc-ocr]   [{:>3}] conf={:.3} text={}", i, b.confidence, b.text);
    }
    // 结果排序
    match order_by.as_deref().unwrap_or("Horizontal") {
        "Vertical" => blocks.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y))),
        "Score" => blocks.sort_by(|a, b| b.confidence.total_cmp(&a.confidence)),
        _ => blocks.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x))),
    }
    Ok(RecognizeResult {
        blocks,
        corrected_image,
        rotation_angle,
    })
}

#[tauri::command]
fn render_docx(
    state: tauri::State<OcrState>,
    filename: String,
    data: String,
) -> Result<Vec<PageImage>, String> {
    let docx_bytes = decode_base64(&data)?;
    info!("[xc-ocr] 导入 DOCX: {}, 大小: {} bytes", filename, docx_bytes.len());
    let page_info = state.renderer.page_info(&docx_bytes);
    info!(
        "[xc-ocr] DOCX 页面信息: {}x{} TWIP ({}x{} px), 方向: {}",
        page_info.width_twip, page_info.height_twip,
        page_info.width_px, page_info.height_px,
        page_info.orientation.as_str(),
    );
    let pages = state.renderer.render(&docx_bytes).map_err(|e| e.to_string())?;

    let mut results = Vec::with_capacity(pages.len());
    for (i, img) in pages.iter().enumerate() {
        let image_data = encode_png_base64(img)?;
        results.push(PageImage {
            page: i,
            width: img.width(),
            height: img.height(),
            orientation: page_info.orientation.as_str().to_string(),
            image_data,
        });
    }
    Ok(results)
}

// ── PDF commands ─────────────────────────────────────────────────

#[tauri::command]
fn pdf_page_count(
    state: tauri::State<OcrState>,
    data: String,
) -> Result<usize, String> {
    let pdf_bytes = decode_base64(&data)?;
    info!("[xc-ocr] 获取 PDF 页数, 大小: {} bytes", pdf_bytes.len());
    let count = state.renderer.pdf_page_count(&pdf_bytes).map_err(|e| e.to_string())?;
    info!("[xc-ocr] PDF 页数: {}", count);
    Ok(count)
}

#[tauri::command]
fn render_pdf_page(
    state: tauri::State<OcrState>,
    data: String,
    page: usize,
) -> Result<PageImage, String> {
    let pdf_bytes = decode_base64(&data)?;
    info!("[xc-ocr] 渲染 PDF 第 {} 页, 大小: {} bytes", page + 1, pdf_bytes.len());
    let img = state.renderer.render_pdf_page(&pdf_bytes, page).map_err(|e| e.to_string())?;
    let image_data = encode_png_base64(&img)?;
    let orientation = if img.width() > img.height() { "landscape" } else { "portrait" };
    Ok(PageImage {
        page,
        width: img.width(),
        height: img.height(),
        orientation: orientation.to_string(),
        image_data,
    })
}

// ── model commands ─────────────────────────────────────────────────

#[tauri::command]
fn list_models(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    find_models_root(&app).map(|dir| {
        let mut variants: Vec<String> = std::fs::read_dir(&dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                let path = e.ok()?;
                if path.file_type().ok()?.is_dir() {
                    let dir_name = path.file_name().to_string_lossy().to_string();
                    // 检查是否有子目录作为规格
                    let subdirs: Vec<String> = std::fs::read_dir(path.path())
                        .ok()
                        .into_iter()
                        .flatten()
                        .filter_map(|sub| {
                            let sub_path = sub.ok()?;
                            if sub_path.file_type().ok()?.is_dir() {
                                Some(sub_path.file_name().to_string_lossy().to_string())
                            } else {
                                None
                            }
                        })
                        .collect();

                    if subdirs.is_empty() {
                        // 无子目录，返回顶层目录名
                        Some(vec![dir_name])
                    } else {
                        // 有子目录，返回复合名称 "v4/mobile" 格式
                        Some(
                            subdirs
                                .into_iter()
                                .map(|sub| format!("{}/{}", dir_name, sub))
                                .collect(),
                        )
                    }
                } else {
                    None
                }
            })
            .flatten()
            .collect();
        variants.sort();
        variants
    })
}

#[tauri::command]
fn switch_model(
    app: tauri::AppHandle,
    state: tauri::State<OcrState>,
    variant: String,
) -> Result<(), String> {
    // 解析复合名称 (支持 "v4/mobile" 格式)
    let (version, sub_variant) = if variant.contains('/') {
        let parts: Vec<&str> = variant.split('/').collect();
        (parts[0].to_string(), Some(parts[1].to_string()))
    } else {
        (variant.clone(), None)
    };

    let model_dir = find_models_root(&app)?.join(&version);

    // 根据是否指定子规格决定路径
    let (det_path, rec_path, keys_path) = if let Some(sub) = sub_variant {
        // 直接使用指定的子目录
        (
            model_dir.join(&sub).join("det.onnx"),
            model_dir.join(&sub).join("rec.onnx"),
            model_dir.join(&sub).join("keys.txt"),
        )
    } else if model_dir.join("det.onnx").is_file() {
        // 无子目录，直接在顶层目录查找
        (
            model_dir.join("det.onnx"),
            model_dir.join("rec.onnx"),
            model_dir.join("keys.txt"),
        )
    } else if model_dir.join("mobile").join("det.onnx").is_file() {
        // 兼容旧逻辑：默认使用 mobile
        (
            model_dir.join("mobile").join("det.onnx"),
            model_dir.join("mobile").join("rec.onnx"),
            model_dir.join("mobile").join("keys.txt"),
        )
    } else {
        return Err(format!("模型目录 {variant} 中未找到模型文件"));
    };

    let det_bytes = std::fs::read(&det_path)
        .map_err(|e| format!("读取 det.onnx 失败: {e}"))?;
    let rec_bytes = std::fs::read(&rec_path)
        .map_err(|e| format!("读取 rec.onnx 失败: {e}"))?;
    let keys_bytes = std::fs::read(&keys_path)
        .map_err(|e| format!("读取 keys.txt 失败: {e}"))?;

    let engine = ocr::OcrEngine::new(&det_bytes, &rec_bytes, &keys_bytes)
        .map_err(|e| format!("加载模型 {variant} 失败: {e}"))?;

    let mut guard = state.engine.lock().map_err(|e| e.to_string())?;
    *guard = Some(engine);
    Ok(())
}

// ── model path discovery ───────────────────────────────────────────

fn find_models_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    // Production: bundled resources
    if let Ok(res_dir) = app.path().resource_dir() {
        let path = res_dir.join("models").join("ocr");
        if path.is_dir() {
            return Ok(path);
        }
    }
    // Development: relative paths
    let candidates = [
        PathBuf::from("src-tauri").join("models").join("ocr"),
        PathBuf::from("models").join("ocr"),
    ];
    for path in &candidates {
        if path.is_dir() {
            return Ok(path.clone());
        }
    }
    Err("模型目录 models/ocr/ 未找到".into())
}

fn load_model_for_variant(app: &tauri::AppHandle, variant: &str) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
    let root = find_models_root(app)?;
    let dir = root.join(variant);

    // Check for subdirectories (mobile/server) or direct files
    let (det_path, rec_path, keys_path) = if dir.join("det.onnx").is_file() {
        (
            dir.join("det.onnx"),
            dir.join("rec.onnx"),
            dir.join("keys.txt"),
        )
    } else if dir.join("mobile").join("det.onnx").is_file() {
        (
            dir.join("mobile").join("det.onnx"),
            dir.join("mobile").join("rec.onnx"),
            dir.join("mobile").join("keys.txt"),
        )
    } else if dir.join("server").join("rec.onnx").is_file() {
        (
            dir.join("mobile").join("det.onnx"),  // det is always from mobile
            dir.join("server").join("rec.onnx"),
            dir.join("server").join("keys.txt"),
        )
    } else {
        return Err(format!("模型目录 {variant} 中未找到模型文件"));
    };

    let det = std::fs::read(&det_path)
        .map_err(|e| format!("读取 det.onnx 失败: {e}"))?;
    let rec = std::fs::read(&rec_path)
        .map_err(|e| format!("读取 rec.onnx 失败: {e}"))?;
    let keys = std::fs::read(&keys_path)
        .map_err(|e| format!("读取 keys.txt 失败: {e}"))?;
    Ok((det, rec, keys))
}

fn find_doc_ori_model(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    // Production: bundled resources
    if let Ok(res_dir) = app.path().resource_dir() {
        let path = res_dir.join("models").join("PP-LCNet_x1_0_doc_ori").join("PP-LCNet_x1_0_doc_ori.onnx");
        if path.is_file() {
            return Ok(path);
        }
    }
    // Development: relative paths
    let candidates = [
        PathBuf::from("src-tauri").join("models").join("PP-LCNet_x1_0_doc_ori").join("PP-LCNet_x1_0_doc_ori.onnx"),
        PathBuf::from("models").join("PP-LCNet_x1_0_doc_ori").join("PP-LCNet_x1_0_doc_ori.onnx"),
    ];
    for path in &candidates {
        if path.is_file() {
            return Ok(path.clone());
        }
    }
    Err("方向分类模型 PP-LCNet_x1_0_doc_ori.onnx 未找到".into())
}

fn load_doc_ori_classifier(app: &tauri::AppHandle) -> Result<ocr::DocOrientationClassifier, String> {
    let model_path = find_doc_ori_model(app)?;
    let model_bytes = std::fs::read(&model_path)
        .map_err(|e| format!("读取方向分类模型失败: {}", e))?;
    ocr::DocOrientationClassifier::new(&model_bytes)
        .map_err(|e| format!("初始化方向分类器失败: {}", e))
}

fn find_bundled_tools_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let triple = format!("{os}-{arch}");

    // 1. Resource dir (used in production bundle)
    if let Ok(res_dir) = app.path().resource_dir() {
        let platform_dir = res_dir.join("tools").join(&triple);
        if platform_dir.is_dir() {
            return Some(platform_dir);
        }
        let flat_dir = res_dir.join("tools");
        if flat_dir.is_dir() {
            return Some(flat_dir);
        }
    }

    // 2. Dev-mode: search under repo root `tools/` (outside src-tauri/,
    //    so Tauri's file watcher never triggers a rebuild loop)
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap_or(&manifest_dir);
    let dev_platform = repo_root.join("tools").join(&triple);
    if dev_platform.is_dir() {
        return Some(dev_platform);
    }
    let dev_flat = repo_root.join("tools");
    if dev_flat.is_dir() {
        return Some(dev_flat);
    }

    None
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::default()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir { file_name: None }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                ])
                .build(),
        )
        .setup(|app| {
            let default_variant = "v4";
            let (det_bytes, rec_bytes, keys_bytes) =
                load_model_for_variant(&app.handle(), default_variant)?;

            let engine = ocr::OcrEngine::new(&det_bytes, &rec_bytes, &keys_bytes)
                .map_err(|e| format!("Failed to init OCR: {}", e))?;

            // 加载文档方向分类器
            let orientation_classifier = load_doc_ori_classifier(&app.handle()).ok();
            if orientation_classifier.is_some() {
                info!("[xc-ocr] 方向分类器加载成功");
            } else {
                warn!("[xc-ocr] 方向分类器加载失败，将跳过自动旋转");
            }

            let mut renderer = docx_to_image::DocxRenderer::new();
            if let Some(tools_dir) = find_bundled_tools_dir(&app.handle()) {
                renderer = renderer.add_tool_dir(tools_dir);
            }

            app.manage(OcrState {
                engine: Mutex::new(Some(engine)),
                orientation_classifier: Mutex::new(orientation_classifier),
                renderer,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            read_file_as_data_url,
            recognize_image,
            render_docx,
            pdf_page_count,
            render_pdf_page,
            list_models,
            switch_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
