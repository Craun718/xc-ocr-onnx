use std::path::{Path, PathBuf};
use std::process::Command;

use image::RgbaImage;
use tempfile::TempDir;

use crate::error::DocxToImageError;

// ─── public types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageOrientation {
    Portrait,
    Landscape,
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
        let docx_path = tmp.path().join("input.docx");
        std::fs::write(&docx_path, docx_bytes)?;

        // detect page info from DOCX (TWIPs → pixels, orientation)
        let page_info = detect_page_info(docx_bytes, self.dpi);
        eprintln!(
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

    // ─── pandoc + wkhtmltoimage (single-page) ───────────────────

    fn run_pandoc_wkhtml(
        &self,
        pandoc: &Path,
        wkhtml: &Path,
        docx_path: &Path,
        out_dir: &Path,
        page_info: &PageInfo,
    ) -> Result<Vec<RgbaImage>, DocxToImageError> {
        let html_path = out_dir.join("output.html");
        let docx_bytes = std::fs::read(docx_path)?;
        let content = if let Some(content) = render_docx_html(&docx_bytes) {
            std::fs::write(&html_path, &content)?;
            content
        } else {
            let out = Command::new(pandoc)
                .arg("-f")
                .arg("docx+empty_paragraphs")
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
            std::fs::read_to_string(&html_path)?
        };

        // debug: save HTML for inspection
        {
            let p_count = content.matches("<p").count();
            let empty_p_count = content.matches("<p></p>").count();
            let nbsp_p_count = content.matches("<p>&nbsp;").count();
            let br_count = content.matches("<br").count();
            eprintln!(
                "[docx-to-image] HTML 统计: {} 个 <p>, {} 个空 <p>, {} 个 <p>&nbsp;, {} 个 <br>",
                p_count, empty_p_count, nbsp_p_count, br_count,
            );
            let debug_path = std::env::temp_dir().join("xc-ocr-debug_output.html");
            let _ = std::fs::write(&debug_path, &content);
            eprintln!("[docx-to-image] HTML 已保存到: {}", debug_path.display());
        }

        let png_path = out_dir.join("output.png");
        // wkhtmltoimage uses CSS pixels (96 DPI): TWIP → CSS px = TWIP * 96 / 1440 = TWIP / 15
        // Only set --width for correct line wrapping; omit --height so full content is rendered
        let css_w = page_info.width_twip / 15;
        let out = Command::new(wkhtml)
            .arg("--format")
            .arg("png")
            .arg("--width")
            .arg(css_w.to_string())
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
        eprintln!(
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
            eprintln!(
                "[docx-to-image] GS 输出第 {} 页: {}x{} px",
                i + 1, p.width(), p.height(),
            );
        }
        Ok(pages)
    }
}

pub fn render_docx_html(docx_bytes: &[u8]) -> Option<String> {
    let docx = docx_rs::read_docx(docx_bytes).ok()?;
    let mut body = String::new();

    for child in &docx.document.children {
        match child {
            docx_rs::DocumentChild::Paragraph(p) => {
                body.push_str("<p>");
                if !render_paragraph(p, &mut body) {
                    return None;
                }
                body.push_str("</p>\n");
            }
            docx_rs::DocumentChild::Section(_) => {}
            docx_rs::DocumentChild::BookmarkStart(_) => {}
            docx_rs::DocumentChild::BookmarkEnd(_) => {}
            docx_rs::DocumentChild::CommentStart(_) => {}
            docx_rs::DocumentChild::CommentEnd(_) => {}
            docx_rs::DocumentChild::StructuredDataTag(_) => {}
            docx_rs::DocumentChild::Table(_) => return None,
            docx_rs::DocumentChild::TableOfContents(_) => return None,
        }
    }

    Some(format!(
        r#"<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=yes" />
  <style>
    html {{
      color: #1a1a1a;
      background-color: #fdfdfd;
    }}
    body {{
      margin: 0;
      padding: 0;
      width: 100%;
      box-sizing: border-box;
      hyphens: auto;
      overflow-wrap: break-word;
      text-rendering: optimizeLegibility;
      font-kerning: normal;
      font-family: "Noto Serif CJK SC", "SimSun", serif;
    }}
    p {{
      margin: 0;
      line-height: 100%;
      white-space: pre-wrap;
    }}
    @media (max-width: 600px) {{
      body {{
        font-size: 0.9em;
        padding: 12px;
      }}
    }}
  </style>
</head>
<body>
{}
</body>
</html>"#,
        body
    ))
}

fn render_paragraph(paragraph: &docx_rs::Paragraph, out: &mut String) -> bool {
    let start_len = out.len();
    for child in &paragraph.children {
        if !render_paragraph_child(child, out) {
            return false;
        }
    }
    if out.len() == start_len {
        out.push_str("<br />");
    }
    true
}

fn render_paragraph_child(child: &docx_rs::ParagraphChild, out: &mut String) -> bool {
    match child {
        docx_rs::ParagraphChild::Run(run) => render_run(run, out),
        docx_rs::ParagraphChild::Hyperlink(link) => {
            for child in &link.children {
                if !render_paragraph_child(child, out) {
                    return false;
                }
            }
            true
        }
        docx_rs::ParagraphChild::Insert(insert) => {
            for child in &insert.children {
                if !render_insert_child(child, out) {
                    return false;
                }
            }
            true
        }
        docx_rs::ParagraphChild::Delete(delete) => {
            for child in &delete.children {
                if !render_delete_child(child, out) {
                    return false;
                }
            }
            true
        }
        docx_rs::ParagraphChild::BookmarkStart(_) => true,
        docx_rs::ParagraphChild::BookmarkEnd(_) => true,
        docx_rs::ParagraphChild::CommentStart(_) => true,
        docx_rs::ParagraphChild::CommentEnd(_) => true,
        docx_rs::ParagraphChild::StructuredDataTag(_) => true,
        docx_rs::ParagraphChild::PageNum(_) => true,
        docx_rs::ParagraphChild::NumPages(_) => true,
    }
}

fn render_insert_child(child: &docx_rs::InsertChild, out: &mut String) -> bool {
    match child {
        docx_rs::InsertChild::Run(run) => render_run(run, out),
        docx_rs::InsertChild::Delete(delete) => {
            for child in &delete.children {
                if !render_delete_child(child, out) {
                    return false;
                }
            }
            true
        }
        docx_rs::InsertChild::CommentStart(_) => true,
        docx_rs::InsertChild::CommentEnd(_) => true,
    }
}

fn render_delete_child(child: &docx_rs::DeleteChild, out: &mut String) -> bool {
    match child {
        docx_rs::DeleteChild::Run(run) => render_run(run, out),
        docx_rs::DeleteChild::CommentStart(_) => true,
        docx_rs::DeleteChild::CommentEnd(_) => true,
    }
}

fn render_run(run: &docx_rs::Run, out: &mut String) -> bool {
    for child in &run.children {
        if !render_run_child(child, out) {
            return false;
        }
    }
    true
}

fn render_run_child(child: &docx_rs::RunChild, out: &mut String) -> bool {
    match child {
        docx_rs::RunChild::Text(text) => {
            out.push_str(&escape_html(&text.text));
            true
        }
        docx_rs::RunChild::Break(break_item) => {
            if is_text_wrapping_break(break_item) {
                out.push_str("<br />");
                true
            } else if is_page_break(break_item) {
                out.push_str("<div style=\"page-break-after: always;\"></div>");
                true
            } else {
                true
            }
        }
        docx_rs::RunChild::Tab(_) => {
            out.push_str("    ");
            true
        }
        docx_rs::RunChild::PTab(_) => {
            out.push_str("    ");
            true
        }
        docx_rs::RunChild::Sym(_) => true,
        docx_rs::RunChild::DeleteText(_) => true,
        docx_rs::RunChild::Drawing(_) => false,
        docx_rs::RunChild::Shape(_) => false,
        docx_rs::RunChild::CommentStart(_) => true,
        docx_rs::RunChild::CommentEnd(_) => true,
        docx_rs::RunChild::FieldChar(_) => true,
        docx_rs::RunChild::InstrText(_) => true,
        docx_rs::RunChild::DeleteInstrText(_) => true,
        docx_rs::RunChild::InstrTextString(_) => true,
        docx_rs::RunChild::FootnoteReference(_) => false,
        docx_rs::RunChild::Shading(_) => true,
    }
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn is_text_wrapping_break(break_item: &docx_rs::Break) -> bool {
    matches!(break_kind(break_item).as_deref(), Some("textWrapping"))
}

fn is_page_break(break_item: &docx_rs::Break) -> bool {
    matches!(break_kind(break_item).as_deref(), Some("page"))
}

fn break_kind(break_item: &docx_rs::Break) -> Option<String> {
    let value = serde_json::to_value(break_item).ok()?;
    value.get("breakType")?.as_str().map(|s| s.to_string())
}

// ─── page info detection ──────────────────────────────────────────────

const DEFAULT_TWIP_W: u32 = 11906; // A4 default width
const DEFAULT_TWIP_H: u32 = 16838; // A4 default height

fn detect_page_info(docx_bytes: &[u8], dpi: u32) -> PageInfo {
    let page_size = docx_rs::read_docx(docx_bytes)
        .ok()
        .and_then(|docx| serde_json::to_value(&docx.document.section_property.page_size).ok());

    let twip_w = page_size
        .as_ref()
        .and_then(|v| v.get("w").and_then(|w| w.as_u64()))
        .map(|w| w as u32)
        .unwrap_or(DEFAULT_TWIP_W);

    let twip_h = page_size
        .as_ref()
        .and_then(|v| v.get("h").and_then(|h| h.as_u64()))
        .map(|h| h as u32)
        .unwrap_or(DEFAULT_TWIP_H);

    let orientation = page_size
        .as_ref()
        .and_then(|v| v.get("orient").and_then(|o| o.as_str()))
        .map(|o| match o {
            "landscape" => PageOrientation::Landscape,
            _ => PageOrientation::Portrait,
        })
        .unwrap_or(PageOrientation::Portrait);

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
    use std::io::Cursor;

    #[test]
    fn renders_text_and_page_breaks() {
        let mut cursor = Cursor::new(Vec::new());
        docx_rs::Docx::new()
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(
                    docx_rs::Run::new()
                        .add_text("Hello")
                        .add_break(docx_rs::BreakType::TextWrapping)
                        .add_text("World")
                        .add_break(docx_rs::BreakType::Page)
                        .add_text("Next"),
                ),
            )
            .build()
            .pack(&mut cursor)
            .unwrap();

        let html = render_docx_html(&cursor.into_inner()).unwrap();
        assert!(html.contains("Hello<br />World"));
        assert!(html.contains("page-break-after: always"));
    }

    #[test]
    fn renders_empty_paragraph_as_blank_line() {
        let mut cursor = Cursor::new(Vec::new());
        docx_rs::Docx::new()
            .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("A")))
            .add_paragraph(docx_rs::Paragraph::new())
            .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("B")))
            .build()
            .pack(&mut cursor)
            .unwrap();

        let html = render_docx_html(&cursor.into_inner()).unwrap();
        assert!(html.contains("<p><br /></p>"));
        assert!(!html.contains("max-width: 36em"));
    }
}
