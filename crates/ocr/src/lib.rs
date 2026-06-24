use std::sync::Mutex;

mod det;
mod decode;
mod rec;

use image::DynamicImage;
use serde::{Deserialize, Serialize};

const OCR_THRESHOLD: f32 = 0.3;

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
    det_input: String,
    det_output: String,
    rec_input: String,
    rec_output: String,
    rec_height: u32,
    rec_width: Option<u32>,
    keys: Vec<String>,
}

impl OcrEngine {
    pub fn new(det_model: &[u8], rec_model: &[u8], keys_data: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let det_session = ort::session::Session::builder()?
            .commit_from_memory(det_model)?;
        let rec_session = ort::session::Session::builder()?
            .commit_from_memory(rec_model)?;

        let det_input = det_session.inputs()[0].name().to_string();
        let det_output = det_session.outputs()[0].name().to_string();
        let rec_input = rec_session.inputs()[0].name().to_string();
        let rec_output = rec_session.outputs()[0].name().to_string();
        let rec_shape = rec_session.inputs()[0]
            .dtype()
            .tensor_shape()
            .cloned()
            .unwrap_or_else(|| vec![1_i64, 3, 48, 320].into());
        let rec_height = rec_shape.get(2).copied().unwrap_or(48).max(1) as u32;
        let rec_width = rec_shape
            .get(3)
            .copied()
            .filter(|dim| *dim > 0)
            .map(|dim| dim as u32);

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
            det_input,
            det_output,
            rec_input,
            rec_output,
            rec_height,
            rec_width,
            keys,
        })
    }

    pub fn detect_text_regions(&self, image: &DynamicImage) -> Result<Vec<TextRegion>, Box<dyn std::error::Error>> {
        let mut session = self.det_session.lock().map_err(|e| format!("{}", e))?;
        det::detect_text_regions(&mut session, image, &self.det_input, &self.det_output)
    }

    pub fn recognize_text(&self, image: &DynamicImage, region: &TextRegion) -> Result<decode::DecodedText, Box<dyn std::error::Error>> {
        let (data, width) = rec::preprocess_region(image, region, self.rec_height, self.rec_width)?;
        let mut session = self.rec_session.lock().map_err(|e| format!("{}", e))?;
        let probs = rec::run_recognition(&mut session, &data, width, self.rec_height, &self.rec_input, &self.rec_output)?;
        Ok(decode::ctc_decode(&probs, &self.keys))
    }

    pub fn recognize_all(&self, image: &DynamicImage) -> Result<Vec<OcrBlock>, Box<dyn std::error::Error>> {
        let regions = self.detect_text_regions(image)?;
        if regions.is_empty() {
            if let Some(block) = self.recognize_full_image(image)? {
                return Ok(vec![block]);
            }
            return Ok(Vec::new());
        }

        let mut blocks = Vec::with_capacity(regions.len());
        for region in &regions {
            let decoded = self.recognize_text(image, region)?;
            if decoded.score < OCR_THRESHOLD {
                continue;
            }

            let (x, y, w, h) = bbox_to_rect(&region.bbox);
            blocks.push(OcrBlock {
                text: decoded.text.trim().to_string(),
                confidence: decoded.score,
                x,
                y,
                width: w,
                height: h,
            });
        }
        blocks.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        Ok(blocks)
    }

    fn recognize_full_image(&self, image: &DynamicImage) -> Result<Option<OcrBlock>, Box<dyn std::error::Error>> {
        let width = image.width() as f32;
        let height = image.height() as f32;
        if width < 1.0 || height < 1.0 {
            return Ok(None);
        }

        let full_region = TextRegion {
            bbox: [
                [0.0, 0.0],
                [width - 1.0, 0.0],
                [width - 1.0, height - 1.0],
                [0.0, height - 1.0],
            ],
            confidence: 0.0,
        };
        let decoded = self.recognize_text(image, &full_region)?;
        if decoded.score < OCR_THRESHOLD {
            return Ok(None);
        }

        Ok(Some(OcrBlock {
            text: decoded.text.trim().to_string(),
            confidence: decoded.score,
            x: 0.0,
            y: 0.0,
            width,
            height,
        }))
    }
}

fn bbox_to_rect(bbox: &[[f32; 2]; 4]) -> (f32, f32, f32, f32) {
    let min_x = bbox.iter().map(|p| p[0]).reduce(f32::min).unwrap_or(0.0);
    let min_y = bbox.iter().map(|p| p[1]).reduce(f32::min).unwrap_or(0.0);
    let max_x = bbox.iter().map(|p| p[0]).reduce(f32::max).unwrap_or(0.0);
    let max_y = bbox.iter().map(|p| p[1]).reduce(f32::max).unwrap_or(0.0);
    (min_x, min_y, max_x - min_x, max_y - min_y)
}
