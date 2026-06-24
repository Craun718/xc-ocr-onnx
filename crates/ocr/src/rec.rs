use image::{DynamicImage, Rgb, RgbImage};
use imageproc::geometric_transformations::{warp_into, Interpolation, Border, Projection};
use ort::session::Session;
use crate::TextRegion;

const REC_HEIGHT: u32 = 48;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

fn sort_bbox_points(bbox: &[[f32; 2]; 4]) -> [[f32; 2]; 4] {
    let mut pts: Vec<[f32; 2]> = bbox.to_vec();
    let cx = pts.iter().map(|p| p[0]).sum::<f32>() / 4.0;
    let cy = pts.iter().map(|p| p[1]).sum::<f32>() / 4.0;
    pts.sort_by(|a, b| {
        (a[1] - cy).atan2(a[0] - cx)
            .partial_cmp(&(b[1] - cy).atan2(b[0] - cx))
            .unwrap()
    });

    let mut min_d2 = f32::MAX;
    let mut start = 0;
    for (i, &p) in pts.iter().enumerate() {
        let d2 = (p[0] - cx).powi(2) + (p[1] - cy).powi(2);
        if d2 < min_d2 {
            min_d2 = d2;
            start = i;
        }
    }

    let mut sorted = [[0.0; 2]; 4];
    for i in 0..4 {
        sorted[i] = pts[(start + i) % 4];
    }
    sorted
}

fn rect_size(sorted: &[[f32; 2]; 4]) -> (u32, u32) {
    let top_w = dist(sorted[0], sorted[1]);
    let bot_w = dist(sorted[3], sorted[2]);
    let left_h = dist(sorted[0], sorted[3]);
    let right_h = dist(sorted[1], sorted[2]);
    let w = (top_w.max(bot_w).ceil() as u32).max(1);
    let h = (left_h.max(right_h).ceil() as u32).max(1);
    (w, h)
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

pub fn preprocess_region(
    image: &DynamicImage,
    region: &TextRegion,
) -> Result<(Vec<f32>, i64), Box<dyn std::error::Error>> {
    let sorted = sort_bbox_points(&region.bbox);
    let (rw, rh) = rect_size(&sorted);

    let src = [
        (sorted[0][0] as f32, sorted[0][1] as f32),
        (sorted[1][0] as f32, sorted[1][1] as f32),
        (sorted[2][0] as f32, sorted[2][1] as f32),
        (sorted[3][0] as f32, sorted[3][1] as f32),
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
        Interpolation::Bilinear,
        Border::Constant(Rgb([255u8; 3])),
        &mut rectified,
    );

    let aspect = rw as f32 / rh.max(1) as f32;
    let rec_w = (REC_HEIGHT as f32 * aspect).ceil() as u32;
    let rec_w = rec_w.max(4).min(320);

    let resized = image::imageops::resize(
        &rectified, rec_w, REC_HEIGHT,
        image::imageops::FilterType::Triangle,
    );

    let mut data = Vec::with_capacity((3 * rec_w * REC_HEIGHT) as usize);
    for c in 0..3 {
        for y in 0..REC_HEIGHT {
            for x in 0..rec_w {
                let pixel = resized.get_pixel(x, y);
                let val = pixel[c] as f32 / 255.0;
                data.push((val - MEAN[c]) / STD[c]);
            }
        }
    }

    Ok((data, rec_w as i64))
}

pub fn run_recognition(
    session: &mut Session,
    data: &[f32],
    width: i64,
) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
    let input_tensor = ort::value::Tensor::from_array((
        [1i64, 3, REC_HEIGHT as i64, width],
        data.to_vec(),
    ))?;

    let outputs = session.run(ort::inputs!["x" => input_tensor])?;

    let (output_shape, output_slice) = outputs["fetch_name_0"].try_extract_tensor::<f32>()?;
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
