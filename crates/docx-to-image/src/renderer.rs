use docx_rs::{read_docx, DocumentChild};
use image::{Rgba, RgbaImage};

use crate::error::DocxToImageError;
use crate::font::FontManager;
use crate::image::ImageManager;
use crate::table::render_table;
use crate::text::{measure_paragraph, render_paragraph};

const DEFAULT_PAGE_W: u32 = 2480;
const DEFAULT_PAGE_H: u32 = 3508;
const DEFAULT_MARGIN: u32 = 141;
const PARA_GAP: u32 = 8;

pub struct DocxRenderer {
    page_w: u32,
    page_h: u32,
    margin: u32,
    dpi: f32,
}

impl DocxRenderer {
    pub fn new() -> Self {
        Self {
            page_w: DEFAULT_PAGE_W,
            page_h: DEFAULT_PAGE_H,
            margin: DEFAULT_MARGIN,
            dpi: 300.0,
        }
    }

    pub fn with_page_size(width: u32, height: u32) -> Self {
        Self {
            page_w: width,
            page_h: height,
            margin: DEFAULT_MARGIN,
            dpi: 300.0,
        }
    }

    pub fn render(&self, docx_bytes: &[u8]) -> Result<Vec<RgbaImage>, DocxToImageError> {
        let docx = read_docx(docx_bytes)
            .map_err(|e| DocxToImageError::DocxParse(e.to_string()))?;

        let fm = FontManager::new(&docx)?;
        let im = ImageManager::new(&docx);

        let mut pages: Vec<RgbaImage> = Vec::new();
        let mut page = new_page(self.page_w, self.page_h);
        let mut cy = self.margin;
        let cw = self.page_w.saturating_sub(self.margin * 2);

        for child in &docx.document.children {
            match child {
                DocumentChild::Paragraph(p) => {
                    let ph = measure_paragraph(p, &fm, cw, self.dpi);
                    if cy + ph > self.page_h.saturating_sub(self.margin) {
                        pages.push(page);
                        page = new_page(self.page_w, self.page_h);
                        cy = self.margin;
                    }
                    let rendered = render_paragraph(&mut page, p, &fm, &im, self.margin, cy, cw, self.dpi)?;
                    cy += rendered + PARA_GAP;
                }
                DocumentChild::Table(t) => {
                    let th = render_table(&mut page, t, &fm, &im, self.margin, cy, cw, self.dpi)?;
                    cy += th + PARA_GAP;
                }
                _ => {}
            }
        }

        if has_content(&page) {
            pages.push(page);
        }

        Ok(pages)
    }
}

fn new_page(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]))
}

fn has_content(img: &RgbaImage) -> bool {
    img.pixels().any(|p| p.0 != [255, 255, 255, 255])
}
