use std::collections::HashMap;
use std::path::PathBuf;

use ab_glyph::FontArc;
use docx_rs::{Docx, DocumentChild, ParagraphChild};

use crate::error::DocxToImageError;

pub struct FontManager {
    fonts: HashMap<String, FontArc>,
    default: FontArc,
}

impl FontManager {
    pub fn new(docx: &Docx) -> Result<Self, DocxToImageError> {
        let names = collect_font_names(docx);
        let mut fonts = HashMap::new();

        for name in &names {
            if let Some(font) = load_system_font(name) {
                fonts.insert(name.clone(), font);
            } else {
                eprintln!("[docx-to-image] Font '{name}' not found, default will be used");
            }
        }

        let default = load_system_font("Microsoft YaHei")
            .or_else(|| load_system_font("SimSun"))
            .or_else(|| load_system_font("Arial"))
            .ok_or_else(|| DocxToImageError::Font(
                "No suitable system font found (tried: Microsoft YaHei, SimSun, Arial)".into(),
            ))?;

        Ok(Self { fonts, default })
    }

    pub fn get(&self, name: &str) -> &FontArc {
        if name.is_empty() {
            return &self.default;
        }
        self.fonts.get(name).unwrap_or(&self.default)
    }
}

fn collect_font_names(docx: &Docx) -> Vec<String> {
    let mut names = Vec::new();

    fn walk_paragraph(p: &docx_rs::Paragraph, names: &mut Vec<String>) {
        for child in &p.children {
            if let ParagraphChild::Run(run) = child {
                if let Some(ref fonts) = run.run_property.fonts {
                    if let Ok(v) = serde_json::to_value(fonts) {
                        for key in &["ascii", "eastAsia", "hiAnsi", "cs"] {
                            if let Some(name) = v.get(key).and_then(|s| s.as_str()) {
                                if !name.is_empty() && !names.contains(&name.to_string()) {
                                    names.push(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    for child in &docx.document.children {
        match child {
            DocumentChild::Paragraph(p) => walk_paragraph(p, &mut names),
            DocumentChild::Table(t) => {
                for row_child in &t.rows {
                    let docx_rs::TableChild::TableRow(row) = row_child;
                    for cell_child in &row.cells {
                        let docx_rs::TableRowChild::TableCell(cell) = cell_child;
                        for content in &cell.children {
                            if let docx_rs::TableCellContent::Paragraph(p) = content {
                                walk_paragraph(p, &mut names);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    names
}

fn load_system_font(name: &str) -> Option<FontArc> {
    #[cfg(windows)]
    let font_dir = PathBuf::from(r"C:\Windows\Fonts");
    #[cfg(target_os = "macos")]
    let font_dir = PathBuf::from("/System/Library/Fonts");
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let font_dir = PathBuf::from("/usr/share/fonts");

    if !font_dir.exists() {
        return None;
    }

    let candidates = font_candidates(name);
    let entries = std::fs::read_dir(&font_dir).ok()?;

    for entry in entries.flatten() {
        let path = entry.path();
        let fname = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();

        let matched = candidates.iter().any(|c| fname == *c);
        if !matched {
            continue;
        }

        let data = std::fs::read(&path).ok()?;

        // Try direct Vec load (handles TTF/OTF)
        if let Ok(font) = FontArc::try_from_vec(data.clone()) {
            return Some(font);
        }

        // Try TTC extraction
        if let Some(font) = try_load_ttc(&data) {
            return Some(font);
        }
    }

    None
}

fn font_candidates(name: &str) -> Vec<String> {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "microsoft yahei" => vec!["msyh.ttf".into(), "msyh.ttc".into()],
        "microsoft yahei bold" => vec!["msyhbd.ttf".into(), "msyhbd.ttc".into()],
        "simsun" => vec!["simsun.ttc".into()],
        "simhei" => vec!["simhei.ttf".into()],
        "simkai" => vec!["simkai.ttf".into()],
        "fangsong" => vec!["simfang.ttf".into()],
        "arial" => vec!["arial.ttf".into()],
        "times new roman" => vec!["times.ttf".into()],
        "calibri" => vec!["calibri.ttf".into()],
        _ => {
            let stem = lower.replace(' ', "");
            vec![format!("{stem}.ttf"), format!("{stem}.ttc")]
        }
    }
}

fn try_load_ttc(data: &[u8]) -> Option<FontArc> {
    if data.len() < 12 || &data[0..4] != b"ttcf" {
        return None;
    }
    let n = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;
    if n == 0 {
        return None;
    }
    let off = u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize;
    if off >= data.len() {
        return None;
    }
    FontArc::try_from_vec(data[off..].to_vec()).ok()
}
