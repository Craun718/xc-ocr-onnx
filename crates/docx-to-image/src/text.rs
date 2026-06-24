use ab_glyph::PxScale;
use docx_rs::{Paragraph, ParagraphChild, Run, RunChild};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_text_mut, text_size};

use crate::error::DocxToImageError;
use crate::font::FontManager;
use crate::image::ImageManager;

const DEFAULT_FONT_SIZE: f32 = 11.0;
const FONT_SIZE_MULT: f32 = 2.0;

pub fn pt_to_px(pt: f32, dpi: f32) -> f32 {
    pt * dpi / 72.0
}

fn parse_color(hex: &str) -> Rgba<u8> {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
        Rgba([r, g, b, 255])
    } else {
        Rgba([0, 0, 0, 255])
    }
}

fn run_font_size(run: &Run) -> f32 {
    if let Some(ref sz) = run.run_property.sz {
        if let Ok(v) = serde_json::to_value(sz) {
            if let Some(val) = v.as_u64() {
                return val as f32 / FONT_SIZE_MULT;
            }
        }
    }
    DEFAULT_FONT_SIZE
}

fn run_font_name<'a>(run: &'a Run, font_manager: &'a FontManager) -> &'a ab_glyph::FontArc {
    if let Some(ref fonts) = run.run_property.fonts {
        if let Ok(v) = serde_json::to_value(fonts) {
            let candidates = ["eastAsia", "ascii", "hiAnsi", "cs"];
            for key in &candidates {
                if let Some(name) = v.get(key).and_then(|s| s.as_str()) {
                    if !name.is_empty() {
                        return font_manager.get(name);
                    }
                }
            }
        }
    }
    font_manager.get("")
}

fn run_color(run: &Run) -> Rgba<u8> {
    if let Some(ref c) = run.run_property.color {
        if let Ok(v) = serde_json::to_value(c) {
            if let Some(hex) = v.as_str() {
                return parse_color(hex);
            }
        }
    }
    Rgba([0, 0, 0, 255])
}

fn collect_run_text(run: &Run) -> String {
    let mut text = String::new();
    for child in &run.children {
        if let RunChild::Text(t) = child {
            text.push_str(&t.text);
        }
    }
    text
}

fn scale_for_run(run: &Run, dpi: f32) -> PxScale {
    PxScale::from(pt_to_px(run_font_size(run), dpi))
}

fn measure_run(run: &Run, font_manager: &FontManager, dpi: f32) -> (u32, u32) {
    let text = collect_run_text(run);
    if text.is_empty() {
        return (0, 0);
    }
    let scale = scale_for_run(run, dpi);
    let font = run_font_name(run, font_manager);
    text_size(scale, font, &text)
}

fn render_run(
    image: &mut RgbaImage,
    run: &Run,
    font_manager: &FontManager,
    x: u32,
    y: u32,
    max_width: u32,
    dpi: f32,
) -> u32 {
    let text = collect_run_text(run);
    if text.is_empty() {
        return 0;
    }

    let scale = scale_for_run(run, dpi);
    let font = run_font_name(run, font_manager);
    let color = run_color(run);

    let (tw, th) = text_size(scale, font, &text);
    let line_h = th.max(1);

    if tw as u32 <= max_width {
        draw_text_mut(image, color, x as i32, y as i32, scale, font, &text);
        return line_h;
    }

    let mut cx = 0u32;
    let mut cy = y;

    for ch in text.chars() {
        let s = ch.to_string();
        let (cw, _) = text_size(scale, font, &s);

        if cx + cw > max_width {
            cy += line_h;
            cx = 0;
        }

        draw_text_mut(image, color, (x + cx) as i32, cy as i32, scale, font, &s);
        cx += cw;
    }

    (cy + line_h).saturating_sub(y)
}

pub fn measure_paragraph(
    paragraph: &Paragraph,
    font_manager: &FontManager,
    max_width: u32,
    dpi: f32,
) -> u32 {
    let mut h = 0u32;
    for child in &paragraph.children {
        if let ParagraphChild::Run(run) = child {
            let (w, lh) = measure_run(run, font_manager, dpi);
            let lines = ((w as u32).max(1) + max_width - 1) / max_width;
            h += lh * lines;
        }
    }
    h.max(1)
}

pub fn render_paragraph(
    image: &mut RgbaImage,
    paragraph: &Paragraph,
    font_manager: &FontManager,
    _image_manager: &ImageManager,
    x: u32,
    y: u32,
    max_width: u32,
    dpi: f32,
) -> Result<u32, DocxToImageError> {
    let mut cy = y;

    for child in &paragraph.children {
        match child {
            ParagraphChild::Run(run) => {
                cy += render_run(image, run, font_manager, x, cy, max_width, dpi);
            }
            _ => {}
        }
    }

    Ok(cy.saturating_sub(y))
}
