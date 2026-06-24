use std::collections::HashMap;

use docx_rs::Docx;
use image::{load_from_memory, RgbaImage};

#[allow(dead_code)]
pub struct ImageManager {
    images: HashMap<String, RgbaImage>,
}

impl ImageManager {
    pub fn new(docx: &Docx) -> Self {
        let mut images = HashMap::new();

        for (id, _content_type, img_data, _png) in &docx.images {
            if let Ok(dynamic) = load_from_memory(&img_data.0) {
                images.insert(id.clone(), dynamic.into_rgba8());
            }
        }

        Self { images }
    }

    #[allow(dead_code)]
    pub fn get(&self, id: &str) -> Option<&RgbaImage> {
        self.images.get(id)
    }
}
