use docx_rs::{TableCellContent, TableChild};
use image::{Rgba, RgbaImage};
use imageproc::drawing::draw_line_segment_mut;

use crate::error::DocxToImageError;
use crate::font::FontManager;
use crate::image::ImageManager;
use crate::text::{measure_paragraph, render_paragraph};

const CELL_PAD: u32 = 6;
const MIN_CELL_HEIGHT: u32 = 24;

pub fn render_table(
    image: &mut RgbaImage,
    table: &docx_rs::Table,
    font_manager: &FontManager,
    image_manager: &ImageManager,
    x: u32,
    y: u32,
    max_width: u32,
    dpi: f32,
) -> Result<u32, DocxToImageError> {
    if table.rows.is_empty() {
        return Ok(0);
    }

    let num_cols = table.rows.iter()
        .map(|r| {
            let TableChild::TableRow(row) = r;
            row.cells.len()
        })
        .max()
        .unwrap_or(0);

    if num_cols == 0 {
        return Ok(0);
    }

    let col_w = max_width / num_cols as u32;
    let border = Rgba([0, 0, 0, 255]);
    let start_y = y;
    let mut cy = y;

    for row_child in &table.rows {
        let docx_rs::TableChild::TableRow(row) = row_child;
        let mut cx = x;
        let mut row_h = 0u32;

        for cell_child in &row.cells {
            let docx_rs::TableRowChild::TableCell(cell) = cell_child;
            let cell_x = cx;
            let cell_y = cy;

            let mut content_h = 0u32;
            for content in &cell.children {
                if let TableCellContent::Paragraph(p) = content {
                    let ph = measure_paragraph(p, font_manager, col_w.saturating_sub(CELL_PAD * 2), dpi);
                    content_h = content_h.max(ph);
                }
            }
            let cell_h = (content_h + CELL_PAD * 2).max(MIN_CELL_HEIGHT);

            let mut inner_y = cell_y + CELL_PAD;
            for content in &cell.children {
                if let TableCellContent::Paragraph(p) = content {
                    let rendered = render_paragraph(
                        image, p, font_manager, image_manager,
                        cell_x + CELL_PAD, inner_y,
                        col_w.saturating_sub(CELL_PAD * 2),
                        dpi,
                    )?;
                    inner_y += rendered;
                }
            }

            let x1 = cell_x as f32;
            let y1 = cell_y as f32;
            let x2 = (cell_x + col_w) as f32;
            let y2 = (cell_y + cell_h) as f32;

            draw_line_segment_mut(image, (x1, y1), (x2, y1), border);
            draw_line_segment_mut(image, (x1, y2), (x2, y2), border);
            draw_line_segment_mut(image, (x1, y1), (x1, y2), border);
            if cx + col_w >= x + max_width {
                draw_line_segment_mut(image, (x2, y1), (x2, y2), border);
            }

            row_h = row_h.max(cell_h);
            cx += col_w;
        }

        cy += row_h;
    }

    for col in 1..num_cols {
        let cx = x + (col as u32) * col_w;
        let xf = cx as f32;
        draw_line_segment_mut(image, (xf, start_y as f32), (xf, cy as f32), border);
    }

    Ok(cy.saturating_sub(start_y))
}
