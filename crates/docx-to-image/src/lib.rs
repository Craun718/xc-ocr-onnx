mod error;
mod renderer;

pub use error::DocxToImageError;
pub use renderer::{DocxRenderer, PageInfo, PageOrientation};
pub use renderer::render_docx_html;
