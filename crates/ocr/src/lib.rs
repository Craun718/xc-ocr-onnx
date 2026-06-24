use std::sync::Mutex;

mod det;
mod decode;
mod rec;

use image::DynamicImage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum UpscaleFilter {
    None,
    Triangle,
    CatmullRom,
    Lanczos3,
}

impl UpscaleFilter {
    pub fn apply(&self, img: &DynamicImage) -> DynamicImage {
        match self {
            Self::None => img.clone(),
            Self::Triangle => img.resize_exact(img.width() * 2, img.height() * 2, image::imageops::FilterType::Triangle),
            Self::CatmullRom => img.resize_exact(img.width() * 2, img.height() * 2, image::imageops::FilterType::CatmullRom),
            Self::Lanczos3 => img.resize_exact(img.width() * 2, img.height() * 2, image::imageops::FilterType::Lanczos3),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextRegion {
    pub bbox: [[f32; 2]; 4],
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrBlock {
    pub text: String,
    pub confidence: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub struct OcrEngine {
    det_session: Mutex<ort::session::Session>,
    rec_session: Mutex<ort::session::Session>,
    det_output: String,
    rec_output: String,
    keys: Vec<String>,
}

impl OcrEngine {
    pub fn new(det_model: &[u8], rec_model: &[u8], keys_data: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let det_session = ort::session::Session::builder()?
            .commit_from_memory(det_model)?;
        let rec_session = ort::session::Session::builder()?
            .commit_from_memory(rec_model)?;

        let det_output = det_session.outputs()[0].name().to_string();
        let rec_output = rec_session.outputs()[0].name().to_string();

        let keys_str = std::str::from_utf8(keys_data)?;
        let keys: Vec<String> = keys_str.lines().map(|s| s.to_string()).collect();

        // debug: validate keys vs model output
        let model_classes = rec_session.outputs()[0]
            .dtype().tensor_shape()
            .and_then(|s| s.last())
            .copied()
            .unwrap_or(0) as usize;
        eprintln!(
            "[ocr] keys: {} lines, model output classes: {} (blank + {} chars)",
            keys.len(),
            model_classes,
            model_classes.saturating_sub(1),
        );
        if keys.len() + 1 != model_classes {
            eprintln!(
                "[ocr] WARNING: keys({}) + 1(blank) = {} != model_classes({})",
                keys.len(),
                keys.len() + 1,
                model_classes,
            );
        }

        Ok(Self {
            det_session: Mutex::new(det_session),
            rec_session: Mutex::new(rec_session),
            det_output,
            rec_output,
            keys,
        })
    }

    pub fn detect_text_regions(&self, image: &DynamicImage) -> Result<Vec<TextRegion>, Box<dyn std::error::Error>> {
        let mut session = self.det_session.lock().map_err(|e| format!("{}", e))?;
        det::detect_text_regions(&mut session, image, &self.det_output)
    }

    pub fn recognize_text(&self, image: &DynamicImage, region: &TextRegion) -> Result<String, Box<dyn std::error::Error>> {
        let (data, width) = rec::preprocess_region(image, region)?;
        let mut session = self.rec_session.lock().map_err(|e| format!("{}", e))?;
        let probs = rec::run_recognition(&mut session, &data, width, &self.rec_output)?;
        let text = decode::ctc_decode(&probs, &self.keys);
        Ok(text)
    }

    pub fn recognize_all(&self, image: &DynamicImage, filter: UpscaleFilter) -> Result<Vec<OcrBlock>, Box<dyn std::error::Error>> {
        let input = filter.apply(image);
        let scale = match filter {
            UpscaleFilter::None => 1.0,
            _ => 0.5,
        };

        let regions = self.detect_text_regions(&input)?;
        let mut blocks = Vec::with_capacity(regions.len());
        for region in &regions {
            let text = self.recognize_text(&input, region)?;
            let (x, y, w, h) = bbox_to_rect(&region.bbox);
            blocks.push(OcrBlock {
                text,
                confidence: region.confidence,
                x: x * scale,
                y: y * scale,
                width: w * scale,
                height: h * scale,
            });
        }
        Ok(blocks)
    }
}

fn bbox_to_rect(bbox: &[[f32; 2]; 4]) -> (f32, f32, f32, f32) {
    let min_x = bbox.iter().map(|p| p[0]).reduce(f32::min).unwrap_or(0.0);
    let min_y = bbox.iter().map(|p| p[1]).reduce(f32::min).unwrap_or(0.0);
    let max_x = bbox.iter().map(|p| p[0]).reduce(f32::max).unwrap_or(0.0);
    let max_y = bbox.iter().map(|p| p[1]).reduce(f32::max).unwrap_or(0.0);
    (min_x, min_y, max_x - min_x, max_y - min_y)
}
