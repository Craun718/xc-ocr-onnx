use thiserror::Error;

#[derive(Error, Debug)]
pub enum DocxToImageError {
    #[error("Failed to parse DOCX: {0}")]
    DocxParse(String),

    #[error("Font error: {0}")]
    Font(String),

    #[error("Image error: {0}")]
    Image(String),

    #[error("Render error: {0}")]
    Render(String),
}
