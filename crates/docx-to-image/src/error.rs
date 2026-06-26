use thiserror::Error;

#[derive(Error, Debug)]
pub enum DocxToImageError {
    #[error("{0}")]
    NoTool(String),

    #[error("Command failed: {cmd}\n  exit code: {code}\n  stderr: {stderr}")]
    CommandFailed { cmd: String, code: i32, stderr: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image error: {0}")]
    Image(String),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
}
