use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use image::RgbaImage;
use tempfile::TempDir;
use log::info;

use crate::error::DocxToImageError;

// ─── public types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageOrientation {
    Portrait,
    Landscape,
}

impl PageOrientation {
    pub fn as_str(&self) -> &'static str {
        match self {
            PageOrientation::Portrait => "portrait",
            PageOrientation::Landscape => "landscape",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PageInfo {
    pub width_px: u32,
    pub height_px: u32,
    pub orientation: PageOrientation,
    pub width_twip: u32,
    pub height_twip: u32,
}

const DEFAULT_DPI: u32 = 200;

pub struct DocxRenderer {
    dpi: u32,
    tool_search_dirs: Vec<PathBuf>,

    gs_path: Option<PathBuf>,
    pandoc_path: Option<PathBuf>,
    wkhtmltoimage_path: Option<PathBuf>,
}

impl DocxRenderer {
    pub fn new() -> Self {
        Self {
            dpi: DEFAULT_DPI,
            tool_search_dirs: Vec::new(),
            gs_path: None,
            pandoc_path: None,
            wkhtmltoimage_path: None,
        }
    }

    pub fn with_dpi(mut self, dpi: u32) -> Self {
        self.dpi = dpi;
        self
    }

    pub fn add_tool_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.tool_search_dirs.push(dir.into());
        self
    }

    pub fn set_gs<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.gs_path = Some(path.into());
        self
    }

    pub fn set_pandoc<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.pandoc_path = Some(path.into());
        self
    }

    pub fn set_wkhtmltoimage<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.wkhtmltoimage_path = Some(path.into());
        self
    }

    /// Detect page info (dimensions and orientation) without rendering.
    pub fn page_info(&self, docx_bytes: &[u8]) -> PageInfo {
        detect_page_info(docx_bytes, self.dpi)
    }

    pub fn render(&self, docx_bytes: &[u8]) -> Result<Vec<RgbaImage>, DocxToImageError> {
        let tmp = TempDir::new()?;

        // 预处理：给 DOCX 空段落注入空格 run（pandoc 会丢弃无 run 的空段）
        let preprocessed_bytes = fix_empty_paragraphs_in_docx(docx_bytes)
            .unwrap_or_else(|_| docx_bytes.to_vec());
        let docx_path = tmp.path().join("input.docx");
        std::fs::write(&docx_path, &preprocessed_bytes)?;

        // detect page info from original DOCX (TWIPs → pixels, orientation)
        // 用原始 bytes 检测，方向信息不受预处理影响
        let page_info = detect_page_info(docx_bytes, self.dpi);
        info!(
            "[docx-to-image] DOCX 页面尺寸: {}x{} TWIP ({}x{} px @ {} DPI), 方向: {:?}",
            page_info.width_twip, page_info.height_twip,
            page_info.width_px, page_info.height_px,
            self.dpi, page_info.orientation,
        );

        let gs = self.find_gs();
        let pandoc = self.find_tool("pandoc", &self.pandoc_path);
        let wkhtml = self.find_tool("wkhtmltoimage", &self.wkhtmltoimage_path);
        let mut last_err = None;

        // Priority 1: Pandoc + wkhtmltoimage — fast, simple, single-page
        if let (Some(pandoc), Some(wkhtml)) = (&pandoc, &wkhtml) {
            match self.run_pandoc_wkhtml(pandoc, wkhtml, &docx_path, tmp.path(), &page_info) {
                Ok(pages) => return Ok(pages),
                Err(e) => last_err = Some(e),
            }
        }

        // Priority 2: Pandoc → HTML → PDF via wkhtmltopdf → PNG via gs
        if let Some(pandoc) = &pandoc {
            if let Some(gs) = &gs {
                let wkhtmltopdf = self.find_tool("wkhtmltopdf", &None);
                if let Some(wkpdf) = wkhtmltopdf {
                    match self.run_pandoc_wkhtmltopdf_gs(pandoc, &wkpdf, gs, &docx_path, tmp.path(), &page_info)
                    {
                        Ok(pages) => return Ok(pages),
                        Err(e) => last_err = Some(e),
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            DocxToImageError::NoTool(
                "没有找到可用的转换工具。请将工具放在以下位置之一：\n\
                 • 系统 PATH 环境变量中\n\
                 • src-tauri/tools/<平台架构>/ 目录下\n\
                 \n\
                 需要安装的工具：将 pandoc + wkhtmltopdf + Ghostscript 放入 tools/ 目录\n\
                 运行 scripts/download-tools.ps1 自动下载"
                    .into(),
            )
        }))
    }

    // ─── tool lookup ───────────────────────────────────────────────

    fn find_tool(&self, name: &str, explicit: &Option<PathBuf>) -> Option<PathBuf> {
        // 1. explicit path
        if let Some(p) = explicit {
            if p.is_file() {
                return Some(p.clone());
            }
        }
        // 2. tool search dirs (bundled tools)
        for dir in &self.tool_search_dirs {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
            let candidate_exe = dir.join(format!("{name}.exe"));
            if candidate_exe.is_file() {
                return Some(candidate_exe);
            }
        }
        // 3. system PATH
        if tool_in_path(name) {
            return Some(PathBuf::from(name));
        }
        None
    }

    fn find_gs(&self) -> Option<PathBuf> {
        let names: &[&str] = if cfg!(windows) {
            &["gswin64c", "gswin32c"]
        } else {
            &["gs"]
        };
        if let Some(ref p) = self.gs_path {
            if p.is_file() {
                return Some(p.clone());
            }
        }
        for name in names {
            for dir in &self.tool_search_dirs {
                let c = dir.join(name);
                if c.is_file() {
                    return Some(c);
                }
                let c_exe = dir.join(format!("{name}.exe"));
                if c_exe.is_file() {
                    return Some(c_exe);
                }
            }
            if tool_in_path(name) {
                return Some(PathBuf::from(name));
            }
        }
        None
    }

    // ─── ghostscript path ─────────────────────────────────────────

    fn run_gs_to_png(
        &self,
        gs: &Path,
        pdf_path: &Path,
        out_dir: &Path,
    ) -> Result<Vec<RgbaImage>, DocxToImageError> {
        let out_pattern = out_dir.join("page_%d.png");
        let output = Command::new(gs)
            .arg("-sDEVICE=png16m")
            .arg("-r")
            .arg(&self.dpi.to_string())
            .arg("-dTextAlphaBits=4")
            .arg("-dGraphicsAlphaBits=4")
            .arg("-dFirstPage=1")
            .arg("-dLastPage=1")
            .arg("-o")
            .arg(&out_pattern)
            .arg(pdf_path)
            .output()?;

        if !output.status.success() {
            return Err(DocxToImageError::CommandFailed {
                cmd: format!("{} -sDEVICE=png16m -r{}", gs.display(), self.dpi),
                code: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).into(),
            });
        }
        load_png_pages(out_dir)
    }

    // ─── PDF rendering ─────────────────────────────────────────────

    /// 获取 PDF 文件的总页数
    /// 使用 Ghostscript 渲染所有页面并统计生成的 PNG 文件数量
    pub fn pdf_page_count(&self, pdf_bytes: &[u8]) -> Result<usize, DocxToImageError> {
        let tmp = TempDir::new()?;
        let pdf_path = tmp.path().join("input.pdf");
        std::fs::write(&pdf_path, pdf_bytes)?;

        let gs = self.find_gs().ok_or_else(|| {
            DocxToImageError::NoTool("Ghostscript 未找到".into())
        })?;

        // 使用低 DPI 快速渲染来统计页数
        let out_pattern = tmp.path().join("page_%d.png");
        let args: Vec<String> = vec![
            "-sDEVICE=png16m".to_string(),
            "-r72x72".to_string(),
            "-dNOPAUSE".to_string(),
            "-dBATCH".to_string(),
            "-dQUIET".to_string(),
            "-o".to_string(),
            out_pattern.display().to_string(),
            pdf_path.display().to_string(),
        ];
        info!("[docx-to-image] 调用 Ghostscript: {} {}", gs.display(), args.join(" "));

        let output = Command::new(&gs)
            .args(&args)
            .output()?;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            info!("[docx-to-image] Ghostscript 失败: stdout={}, stderr={}", stdout, stderr);
            return Err(DocxToImageError::CommandFailed {
                cmd: format!("{} {}", gs.display(), args.join(" ")),
                code: output.status.code().unwrap_or(-1),
                stderr: if stderr.is_empty() { stdout.into() } else { stderr.into() },
            });
        }

        // 统计生成的 PNG 文件数量
        let count = std::fs::read_dir(tmp.path())?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|ext| ext == "png").unwrap_or(false))
            .count();

        info!("[docx-to-image] PDF 页数: {}", count);
        Ok(count)
    }

    /// 渲染 PDF 文件的指定页（页号从 0 开始，Ghostscript 页号从 1 开始）
    pub fn render_pdf_page(&self, pdf_bytes: &[u8], page: usize) -> Result<RgbaImage, DocxToImageError> {
        let tmp = TempDir::new()?;
        let pdf_path = tmp.path().join("input.pdf");
        std::fs::write(&pdf_path, pdf_bytes)?;

        let gs = self.find_gs().ok_or_else(|| {
            DocxToImageError::NoTool("Ghostscript 未找到".into())
        })?;

        let out_path = tmp.path().join("page.png");
        let gs_page = page + 1; // Ghostscript 页号从 1 开始

        let args: Vec<String> = vec![
            "-sDEVICE=png16m".to_string(),
            format!("-r{}x{}", self.dpi, self.dpi),
            "-dNOPAUSE".to_string(),
            "-dBATCH".to_string(),
            "-dQUIET".to_string(),
            format!("-dFirstPage={}", gs_page),
            format!("-dLastPage={}", gs_page),
            "-o".to_string(),
            out_path.display().to_string(),
            pdf_path.display().to_string(),
        ];
        info!("[docx-to-image] 调用 Ghostscript: {} {}", gs.display(), args.join(" "));

        let output = Command::new(&gs)
            .args(&args)
            .output()?;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            info!("[docx-to-image] Ghostscript 页面渲染失败: stdout={}, stderr={}", stdout, stderr);
            return Err(DocxToImageError::CommandFailed {
                cmd: format!("{} {}", gs.display(), args.join(" ")),
                code: output.status.code().unwrap_or(-1),
                stderr: if stderr.is_empty() { stdout.into() } else { stderr.into() },
            });
        }

        let img = image::open(&out_path)
            .map_err(|e| DocxToImageError::Image(e.to_string()))?
            .into_rgba8();

        info!(
            "[docx-to-image] PDF 第 {} 页渲染完成: {}x{} px @ {} DPI",
            gs_page, img.width(), img.height(), self.dpi
        );

        Ok(img)
    }

    /// 用 pandoc 将 DOCX 转 HTML5，再用 wkhtmltoimage 渲染成 PNG

    fn run_pandoc_wkhtml(
        &self,
        pandoc: &Path,
        wkhtml: &Path,
        docx_path: &Path,
        out_dir: &Path,
        page_info: &PageInfo,
    ) -> Result<Vec<RgbaImage>, DocxToImageError> {
        let html_path = out_dir.join("output.html");
        let out = Command::new(pandoc)
            .arg("-f")
            .arg("docx+empty_paragraphs")
            .arg("-t")
            .arg("html5")
            .arg("--embed-resources")
            .arg("--standalone")
            .arg("-o")
            .arg(&html_path)
            .arg(docx_path)
            .output()?;
        if !out.status.success() {
            return Err(DocxToImageError::CommandFailed {
                cmd: "pandoc -t html5 --self-contained".into(),
                code: out.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&out.stderr).into(),
            });
        }
        let content = std::fs::read_to_string(&html_path)?;

        // 移除 pandoc 默认的 max-width 和 body padding，让内容填满视口
        let content = content
            .replace("max-width: 36em;", "")
            .replace("margin: 0 auto;", "margin: 0;")
            .replace("padding-left: 50px;", "")
            .replace("padding-right: 50px;", "")
            .replace("padding-top: 50px;", "")
            .replace("padding-bottom: 50px;", "");
        // 覆盖 p 样式：保留空格缩进/空段换行，紧凑间距
        let content = content.replace(
            "p {",
            "p { white-space: pre-wrap; line-height: 100%; margin: 0;",
        );
        // 基础字体大小
        let content = content.replace(
            "text-rendering: optimizeLegibility;",
            "text-rendering: optimizeLegibility;font-size:14pt;",
        );
        std::fs::write(&html_path, &content)?;

        // debug: save HTML for inspection
        {
            let p_count = content.matches("<p").count();
            let empty_p_count = content.matches("<p></p>").count();
            let nbsp_p_count = content.matches("<p>&nbsp;").count();
            let br_count = content.matches("<br").count();
            info!(
                "[docx-to-image] HTML 统计: {} 个 <p>, {} 个空 <p>, {} 个 <p>&nbsp;, {} 个 <br>",
                p_count, empty_p_count, nbsp_p_count, br_count,
            );
            let debug_path = std::env::temp_dir().join("xc-ocr-debug_output.html");
            let _ = std::fs::write(&debug_path, &content);
            info!("[docx-to-image] HTML 已保存到: {}", debug_path.display());
        }

        let png_path = out_dir.join("output.png");
        // wkhtmltoimage uses CSS pixels (96 DPI): TWIP → CSS px = TWIP * 96 / 1440 = TWIP / 15
        let css_w = page_info.width_twip / 15;
        let css_h = page_info.height_twip / 15;
        info!(
            "[docx-to-image] wkhtmltoimage viewport: {}x{} px (from {}x{} TWIP)",
            css_w, css_h, page_info.width_twip, page_info.height_twip,
        );
        let out = Command::new(wkhtml)
            .arg("--format")
            .arg("png")
            .arg("--width")
            .arg(css_w.to_string())
            .arg("--height")
            .arg(css_h.to_string())
            .arg(&html_path)
            .arg(&png_path)
            .output()?;
        if !out.status.success() {
            return Err(DocxToImageError::CommandFailed {
                cmd: format!("{} --format png", wkhtml.display()),
                code: out.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&out.stderr).into(),
            });
        }

        let img = image::open(&png_path)
            .map_err(|e| DocxToImageError::Image(e.to_string()))?
            .into_rgba8();
        info!(
            "[docx-to-image] wkhtmltoimage 输出: {}x{} px (CSS 宽度: {} px)",
            img.width(), img.height(), css_w,
        );
        Ok(vec![img])
    }

    // ─── pandoc → wkhtmltopdf → gs (multi-page) ─────────────────

    fn run_pandoc_wkhtmltopdf_gs(
        &self,
        pandoc: &Path,
        wkpdf: &Path,
        gs: &Path,
        docx_path: &Path,
        out_dir: &Path,
        page_info: &PageInfo,
    ) -> Result<Vec<RgbaImage>, DocxToImageError> {
        let html_path = out_dir.join("output.html");
        let out = Command::new(pandoc)
            .arg("-t")
            .arg("html5")
            .arg("--self-contained")
            .arg("-o")
            .arg(&html_path)
            .arg(docx_path)
            .output()?;
        if !out.status.success() {
            return Err(DocxToImageError::CommandFailed {
                cmd: "pandoc -t html5 --self-contained".into(),
                code: out.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&out.stderr).into(),
            });
        }

        // 移除 pandoc 默认的 max-width: 36em 限制
        if let Ok(html) = std::fs::read_to_string(&html_path) {
            let _ = std::fs::write(&html_path, html.replace("max-width: 36em;", ""));
        }

        let pdf_path = out_dir.join("output.pdf");

        // Convert TWIPs → mm: 1 TWIP = 1/1440 inch, 1 inch = 25.4 mm
        let w_mm = ((page_info.width_twip  as f64 * 25.4) / 1440.0).round() as u32;
        let h_mm = ((page_info.height_twip as f64 * 25.4) / 1440.0).round() as u32;

        let mut cmd = Command::new(wkpdf);
        cmd.arg("--page-width").arg(format!("{}mm", w_mm));
        cmd.arg("--page-height").arg(format!("{}mm", h_mm));
        cmd.arg(&html_path).arg(&pdf_path);
        let out = cmd.output()?;
        if !out.status.success() {
            return Err(DocxToImageError::CommandFailed {
                cmd: format!("{} --page-width {}mm --page-height {}mm", wkpdf.display(), w_mm, h_mm),
                code: out.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&out.stderr).into(),
            });
        }

        let pages = self.run_gs_to_png(gs, &pdf_path, out_dir)?;
        for (i, p) in pages.iter().enumerate() {
            info!(
                "[docx-to-image] GS 输出第 {} 页: {}x{} px",
                i + 1, p.width(), p.height(),
            );
        }
        Ok(pages)
    }
}

const DEFAULT_TWIP_W: u32 = 11906; // A4 default width
const DEFAULT_TWIP_H: u32 = 16838; // A4 default height

fn detect_page_info(docx_bytes: &[u8], dpi: u32) -> PageInfo {
    let docx = docx_rs::read_docx(docx_bytes).ok();

    // Multi-section DOCX files store the first section's properties in a
    // paragraph's section_property field, while document.section_property
    // only holds the LAST section. Search children first for section breaks.
    let page_size = docx
        .as_ref()
        .and_then(|docx| {
            // Walk children in order to find the first paragraph that contains
            // a sectPr (section property). This is the first section's page setup.
            docx.document.children.iter().find_map(|child| {
                if let docx_rs::DocumentChild::Paragraph(para) = child {
                    para.property.section_property.as_ref()
                } else {
                    None
                }
            })
        })
        .map(|sp| &sp.page_size);

    // Fall back to document-level section_property (single-section,
    // or no paragraph-level sectPr found).
    let fallback = docx
        .as_ref()
        .map(|d| &d.document.section_property.page_size);
    let page_size = page_size.or(fallback);

    let page_size_json = serde_json::to_value(page_size).ok();

    let twip_w = page_size_json
        .as_ref()
        .and_then(|v| v.get("w").and_then(|w| w.as_u64()))
        .map(|w| w as u32)
        .unwrap_or(DEFAULT_TWIP_W);

    let twip_h = page_size_json
        .as_ref()
        .and_then(|v| v.get("h").and_then(|h| h.as_u64()))
        .map(|h| h as u32)
        .unwrap_or(DEFAULT_TWIP_H);

    // docx_rs 0.4.x reader ignores the "orient" attribute on <w:pgSz>, so
    // we read it from JSON when present, and fall back to comparing width/height.
    let orientation = page_size_json
        .as_ref()
        .and_then(|v| v.get("orient").and_then(|o| o.as_str()))
        .map(|o| match o {
            "landscape" => PageOrientation::Landscape,
            _ => PageOrientation::Portrait,
        })
        .unwrap_or_else(|| {
            // orient not explicitly set — compare w/h
            if twip_w > twip_h {
                PageOrientation::Landscape
            } else {
                PageOrientation::Portrait
            }
        });

    let w_px = ((twip_w * dpi) / 1440).max(800).min(4000);
    let h_px = ((twip_h * dpi) / 1440).max(800).min(4000);

    PageInfo {
        width_px: w_px,
        height_px: h_px,
        orientation,
        width_twip: twip_w,
        height_twip: twip_h,
    }
}

// ─── helpers ───────────────────────────────────────────────────────

/// Pandoc 丢弃没有 `<w:r>` 的空段落（即使有 `+empty_paragraphs`）。
/// 此函数在 ZIP 层修改 `word/document.xml`，给空 `<w:p>` 注入空格 run，
/// 让 pandoc 能够保留它们。
fn fix_empty_paragraphs_xml(xml: &str) -> String {
    // 匹配 <w:p ...> ... </w:p>（非贪心，匹配到第一个 </w:p>）
    let re = regex::Regex::new(r"(?s)(<w:p[^>]*>)(.*?)(</w:p>)").unwrap();
    let mut injected = 0usize;
    let result = re
        .replace_all(xml, |caps: &regex::Captures| {
            let content = caps.get(2).unwrap().as_str();
            // 注意：w:rPr（run 属性）也包含子串 "<w:r"，所以要精确匹配 run 元素
            if content.contains("<w:r>") || content.contains("<w:r ") || content.contains("<w:r/") {
                // 有 run 元素 —— 保留原样
                caps.get(0).unwrap().as_str().to_string()
            } else {
                injected += 1;
                // 无 run —— 保留原有内容（如 w:pPr），注入 U+00A0 空格 run
                // 普通空格 ' ' 也会被 pandoc 丢弃，必须用 non-breaking space
                format!(
                    "{}{}<w:r><w:t xml:space=\"preserve\">\u{a0}</w:t></w:r>{}",
                    caps.get(1).unwrap().as_str(),
                    content,
                    caps.get(3).unwrap().as_str(),
                )
            }
        })
        .to_string();
    info!("[docx-to-image] fix_empty_paragraphs_xml: 注入 {} 个空格 run", injected);
    result
}

/// Pandoc 丢弃没有 `<w:r>` 的空段落（即使有 `+empty_paragraphs`）。
/// 此函数在 ZIP 层修改 DOCX，给空 `<w:p>` 注入空格 run，
/// 让 pandoc 能够保留它们。
/// 解压→修改→重新打包。
fn fix_empty_paragraphs_in_docx(docx_bytes: &[u8]) -> Result<Vec<u8>, DocxToImageError> {
    let cursor = std::io::Cursor::new(docx_bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;

    let n = archive.len();
    struct RawEntry {
        name: String,
        data: Vec<u8>,
    }
    let mut raw_entries: Vec<RawEntry> = Vec::with_capacity(n);

    // 全部读取（by_index 自动解压）
    for i in 0..n {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        let mut data: Vec<u8> = Vec::new();
        entry.read_to_end(&mut data)?;

        if name == "word/document.xml" {
            let xml = String::from_utf8_lossy(&data);
            let fixed = fix_empty_paragraphs_xml(&xml);
            raw_entries.push(RawEntry {
                name,
                data: fixed.into_bytes(),
            });
        } else {
            raw_entries.push(RawEntry { name, data });
        }
    }

    drop(archive);

    // 重新打包（全用 Deflated）
    let mut out_buf = std::io::Cursor::new(Vec::new());
    {
        let mut out_zip = zip::ZipWriter::new(&mut out_buf);
        for entry in &raw_entries {
            let options = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            out_zip.start_file(&entry.name, options)?;
            out_zip.write_all(&entry.data)?;
        }
        out_zip.finish()?;
    }
    Ok(out_buf.into_inner())
}

fn tool_in_path(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn load_png_pages(dir: &Path) -> Result<Vec<RgbaImage>, DocxToImageError> {
    let mut pages: Vec<(usize, RgbaImage)> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if !matches!(path.extension(), Some(e) if e == "png") {
            continue;
        }
        let img = image::open(&path)
            .map_err(|e| DocxToImageError::Image(e.to_string()))?
            .into_rgba8();
        let idx = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.rsplit('-').next().or_else(|| s.rsplit('_').next()))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        pages.push((idx, img));
    }
    pages.sort_by_key(|(i, _)| *i);
    Ok(pages.into_iter().map(|(_, i)| i).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
}
