use std::path::{Path, PathBuf};
use std::process::Command;

use image::RgbaImage;
use tempfile::TempDir;

use crate::error::DocxToImageError;

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

    pub fn render(&self, docx_bytes: &[u8]) -> Result<Vec<RgbaImage>, DocxToImageError> {
        let tmp = TempDir::new()?;
        let docx_path = tmp.path().join("input.docx");
        std::fs::write(&docx_path, docx_bytes)?;

        // detect page width from DOCX (TWIPs → pixels)
        let page_w_px = detect_page_width(docx_bytes, self.dpi);

        let gs = self.find_gs();
        let pandoc = self.find_tool("pandoc", &self.pandoc_path);
        let wkhtml = self.find_tool("wkhtmltoimage", &self.wkhtmltoimage_path);
        let mut last_err = None;

        // Priority 1: Pandoc + wkhtmltoimage — decent quality, lighter bundle
        if let (Some(pandoc), Some(wkhtml)) = (&pandoc, &wkhtml) {
            match self.run_pandoc_wkhtml(pandoc, wkhtml, &docx_path, tmp.path(), page_w_px) {
                Ok(pages) => return Ok(pages),
                Err(e) => last_err = Some(e),
            }
        }

        // Priority 2: Pandoc → HTML → PDF via wkhtmltopdf → PNG via gs
        if let Some(pandoc) = &pandoc {
            if let Some(gs) = &gs {
                let wkhtmltopdf = self.find_tool("wkhtmltopdf", &None);
                if let Some(wkpdf) = wkhtmltopdf {
                    match self.run_pandoc_wkhtmltopdf_gs(pandoc, &wkpdf, gs, &docx_path, tmp.path())
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

    // ─── pandoc + wkhtmltoimage ──────────────────────────────────

    fn run_pandoc_wkhtml(
        &self,
        pandoc: &Path,
        wkhtml: &Path,
        docx_path: &Path,
        out_dir: &Path,
        page_w_px: u32,
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

        let png_path = out_dir.join("output.png");
        let out = Command::new(wkhtml)
            .arg("--format")
            .arg("png")
            .arg("--width")
            .arg(page_w_px.to_string())
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
        let out = Command::new(wkpdf)
            .arg("--page-size")
            .arg("A4")
            .arg(&html_path)
            .arg(&pdf_path)
            .output()?;
        if !out.status.success() {
            return Err(DocxToImageError::CommandFailed {
                cmd: format!("{} --page-size A4", wkpdf.display()),
                code: out.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&out.stderr).into(),
            });
        }

        self.run_gs_to_png(gs, &pdf_path, out_dir)
    }
}

// ─── page size detection ───────────────────────────────────────────

const DEFAULT_TWIP_W: u32 = 11906; // A4 default

fn detect_page_width(docx_bytes: &[u8], dpi: u32) -> u32 {
    let twip_w = docx_rs::read_docx(docx_bytes)
        .ok()
        .and_then(|docx| serde_json::to_value(&docx.document.section_property.page_size).ok())
        .and_then(|v| v.get("w").and_then(|w| w.as_u64()).map(|w| w as u32))
        .unwrap_or(DEFAULT_TWIP_W);

    let px = (twip_w * dpi) / 1440;
    px.max(800).min(4000)
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
