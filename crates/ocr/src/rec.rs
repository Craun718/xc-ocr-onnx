use image::{DynamicImage, RgbImage};
use imageproc::geometric_transformations::{warp_into, Interpolation, Border, Projection};
use ort::session::Session;
use crate::TextRegion;
fn point_dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn order_bbox_points(bbox: [[f32; 2]; 4]) -> [[f32; 2]; 4] {
    let mut rect = [[0.0; 2]; 4];

    let sums = bbox.map(|p| p[0] + p[1]);
    let diffs = bbox.map(|p| p[0] - p[1]);

    let top_left = sums
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    let bottom_right = sums
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(idx, _)| idx)
        .unwrap_or(2);

    let remaining: Vec<usize> = (0..4)
        .filter(|idx| *idx != top_left && *idx != bottom_right)
        .collect();

    let top_right = remaining
        .iter()
        .copied()
        .max_by(|a, b| diffs[*a].partial_cmp(&diffs[*b]).unwrap())
        .unwrap_or(1);
    let bottom_left = remaining
        .iter()
        .copied()
        .min_by(|a, b| diffs[*a].partial_cmp(&diffs[*b]).unwrap())
        .unwrap_or(3);

    rect[0] = bbox[top_left];
    rect[1] = bbox[top_right];
    rect[2] = bbox[bottom_right];
    rect[3] = bbox[bottom_left];
    rect
}

pub fn preprocess_region(
    image: &DynamicImage,
    region: &TextRegion,
    rec_height: u32,
    rec_width: Option<u32>,
) -> Result<(Vec<f32>, i64), Box<dyn std::error::Error>> {
    let bbox = order_bbox_points(region.bbox);
    let rw = (point_dist(bbox[0], bbox[1]).max(point_dist(bbox[3], bbox[2])).ceil() as u32).max(1);
    let rh = (point_dist(bbox[1], bbox[2]).max(point_dist(bbox[0], bbox[3])).ceil() as u32).max(1);

    let src = [
        (bbox[0][0], bbox[0][1]),
        (bbox[1][0], bbox[1][1]),
        (bbox[2][0], bbox[2][1]),
        (bbox[3][0], bbox[3][1]),
    ];
    let dst = [
        (0.0f32, 0.0f32),
        (rw as f32, 0.0f32),
        (rw as f32, rh as f32),
        (0.0f32, rh as f32),
    ];

    let proj = Projection::from_control_points(src, dst)
        .ok_or("Failed to create projection")?;

    let rgb = image.to_rgb8();
    let mut rectified = RgbImage::new(rw, rh);
    warp_into(
        &rgb,
        proj,
        Interpolation::Bicubic,
        Border::Replicate,
        &mut rectified,
    );

    if rectified.height() as f32 / rectified.width().max(1) as f32 >= 1.5 {
        rectified = image::imageops::rotate90(&rectified);
    }

    let ratio = if rectified.height() == 0 {
        rw as f32 / rh.max(1) as f32
    } else {
        rectified.width() as f32 / rectified.height() as f32
    };
    let resized_w = (rec_height as f32 * ratio).ceil() as u32;
    let resized_w = if let Some(target_w) = rec_width {
        resized_w.max(1).min(target_w.max(4))
    } else {
        resized_w.max(4)
    };
    let input_w = rec_width.unwrap_or(resized_w.max(4));

    let resized = image::imageops::resize(
        &rectified, resized_w, rec_height,
        image::imageops::FilterType::Triangle,
    );

    let input_w_usize = input_w as usize;
    let rec_height_usize = rec_height as usize;
    let mut data = vec![0.0f32; 3 * input_w_usize * rec_height_usize];
    for c in 0..3usize {
        for y in 0..rec_height_usize {
            for x in 0..resized_w as usize {
                let pixel = resized.get_pixel(x as u32, y as u32);
                let val = pixel[c] as f32 / 255.0;
                let offset = c * input_w_usize * rec_height_usize + y * input_w_usize + x;
                data[offset] = (val - 0.5) / 0.5;
            }
        }
    }

    Ok((data, input_w as i64))
}

pub fn run_recognition(
    session: &mut Session,
    data: &[f32],
    width: i64,
    height: u32,
    input_name: &str,
    output_name: &str,
) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
    let input_tensor = ort::value::Tensor::from_array((
        [1i64, 3, height as i64, width],
        data.to_vec(),
    ))?;

    let outputs = session.run(ort::inputs![input_name => input_tensor])?;

    let (output_shape, output_slice) = outputs[output_name].try_extract_tensor::<f32>()?;
    let timesteps = output_shape[1] as usize;
    let num_classes = output_shape[2] as usize;

    let mut result = Vec::with_capacity(timesteps);
    for t in 0..timesteps {
        let mut row = Vec::with_capacity(num_classes);
        let base = t * num_classes;
        for c in 0..num_classes as usize {
            row.push(output_slice[base + c]);
        }
        result.push(row);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::order_bbox_points;

    #[test]
    fn orders_quad_points_clockwise_from_top_left() {
        let bbox = [
            [0.0, 10.0],
            [30.0, 0.0],
            [30.0, 10.0],
            [0.0, 0.0],
        ];

        let ordered = order_bbox_points(bbox);

        assert_eq!(ordered[0], [0.0, 0.0]);
        assert_eq!(ordered[1], [30.0, 0.0]);
        assert_eq!(ordered[2], [30.0, 10.0]);
        assert_eq!(ordered[3], [0.0, 10.0]);
    }
}
