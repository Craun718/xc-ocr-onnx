use image::{DynamicImage, GrayImage, Luma};
use imageproc::contours::find_contours;
use imageproc::geometry::convex_hull;
use ort::session::Session;
use crate::TextRegion;

const DET_LONG_SIDE: u32 = 960;
const DET_THRESHOLD: f32 = 0.3;
const BOX_THRESHOLD: f32 = 0.7;
const UNCLIP_RATIO: f32 = 1.5;
const MIN_SIZE: f32 = 3.0;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

fn preprocess(image: &DynamicImage) -> Result<(Vec<f32>, i64, i64, f32, f32), Box<dyn std::error::Error>> {
    let (w, h) = (image.width(), image.height());
    let scale = DET_LONG_SIDE as f32 / w.max(h) as f32;
    let scale = scale.min(1.0);
    let new_w = (w as f32 * scale) as u32;
    let new_h = (h as f32 * scale) as u32;

    let resized = image.resize_exact(new_w, new_h, image::imageops::FilterType::Triangle);
    let pad_w = (32 - new_w % 32) % 32;
    let pad_h = (32 - new_h % 32) % 32;
    let padded_w = new_w + pad_w;
    let padded_h = new_h + pad_h;

    let rgb = resized.to_rgb8();
    let mut padded = image::RgbImage::new(padded_w, padded_h);
    for y in 0..new_h {
        for x in 0..new_w {
            padded.put_pixel(x, y, *rgb.get_pixel(x, y));
        }
    }

    let mut data = Vec::with_capacity((3 * padded_w * padded_h) as usize);
    for c in 0..3 {
        for y in 0..padded_h {
            for x in 0..padded_w {
                let pixel = padded.get_pixel(x, y);
                let val = pixel[c] as f32 / 255.0;
                data.push((val - MEAN[c]) / STD[c]);
            }
        }
    }

    let out_h = padded_h as i64;
    let out_w = padded_w as i64;
    Ok((data, out_h, out_w, scale, scale))
}

fn postprocess(
    prob_data: &[f32],
    out_h: usize,
    out_w: usize,
    orig_w: u32,
    orig_h: u32,
    scale_x: f32,
    scale_y: f32,
) -> Vec<TextRegion> {
    let mut binary = GrayImage::new(out_w as u32, out_h as u32);
    for y in 0..out_h {
        for x in 0..out_w {
            let val = if prob_data[y * out_w + x] > DET_THRESHOLD { 255u8 } else { 0u8 };
            binary.put_pixel(x as u32, y as u32, Luma([val]));
        }
    }

    let contours = find_contours(&binary);
    let mut regions = Vec::new();

    for contour in &contours {
        if contour.points.len() < 4 {
            continue;
        }

        let hull = convex_hull(contour.points.clone());
        if hull.len() < 3 {
            continue;
        }

        let rect = min_area_rect(&hull);
        let cx = rect.iter().map(|p| p.x).sum::<f32>() / 4.0;
        let cy = rect.iter().map(|p| p.y).sum::<f32>() / 4.0;

        let area = polygon_area(&rect);
        let perimeter = polygon_perimeter(&rect);
        let d = area * UNCLIP_RATIO / perimeter.max(1.0);

        let mut unclipped = [[0.0f32; 2]; 4];
        for (k, pt) in rect.iter().enumerate() {
            let dx = pt.x - cx;
            let dy = pt.y - cy;
            let len = (dx * dx + dy * dy).sqrt().max(1.0);
            unclipped[k] = [
                (pt.x + dx / len * d).max(0.0).min(out_w as f32),
                (pt.y + dy / len * d).max(0.0).min(out_h as f32),
            ];
        }

        let box_w = (unclipped[0][0] - unclipped[1][0]).abs().max(
            (unclipped[3][0] - unclipped[2][0]).abs()
        );
        let box_h = (unclipped[0][1] - unclipped[3][1]).abs().max(
            (unclipped[1][1] - unclipped[2][1]).abs()
        );

        if box_w < MIN_SIZE || box_h < MIN_SIZE {
            continue;
        }

        let prob_val = average_probability(prob_data, out_w, out_h, &unclipped);
        if prob_val < BOX_THRESHOLD {
            continue;
        }

        let inv_sx = 1.0 / scale_x;
        let inv_sy = 1.0 / scale_y;
        let mut valid = true;
        let bbox = unclipped.map(|p| {
            let x = p[0] * inv_sx;
            let y = p[1] * inv_sy;
            if x < 0.0 || x >= orig_w as f32 || y < 0.0 || y >= orig_h as f32 {
                valid = false;
            }
            [x, y]
        });

        if !valid {
            continue;
        }

        regions.push(TextRegion { bbox, confidence: prob_val });
    }

    regions.sort_by(|a, b| {
        let ay = a.bbox.iter().map(|p| p[1]).sum::<f32>();
        let by = b.bbox.iter().map(|p| p[1]).sum::<f32>();
        ay.partial_cmp(&by).unwrap()
    });

    regions
}

fn min_area_rect(points: &[imageproc::point::Point<i32>]) -> [imageproc::point::Point<f32>; 4] {
    let n = points.len();
    if n < 3 {
        return [imageproc::point::Point::new(0.0, 0.0); 4];
    }

    let mut min_area = f32::MAX;
    let mut best = [imageproc::point::Point::new(0.0, 0.0); 4];

    for i in 0..n {
        let j = (i + 1) % n;
        let dx = (points[j].x - points[i].x) as f32;
        let dy = (points[j].y - points[i].y) as f32;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1.0 {
            continue;
        }
        let cos_a = dx / len;
        let sin_a = dy / len;

        let (mut min_proj, mut max_proj) = (f32::MAX, f32::MIN);
        let (mut min_perp, mut max_perp) = (f32::MAX, f32::MIN);

        for p in points {
            let px = p.x as f32;
            let py = p.y as f32;
            let proj = px * cos_a + py * sin_a;
            let perp = -px * sin_a + py * cos_a;
            if proj < min_proj { min_proj = proj; }
            if proj > max_proj { max_proj = proj; }
            if perp < min_perp { min_perp = perp; }
            if perp > max_perp { max_perp = perp; }
        }

        let area = (max_proj - min_proj) * (max_perp - min_perp);
        if area < min_area {
            min_area = area;
            best = [
                imageproc::point::Point::new(min_proj * cos_a - min_perp * sin_a, min_proj * sin_a + min_perp * cos_a),
                imageproc::point::Point::new(max_proj * cos_a - min_perp * sin_a, max_proj * sin_a + min_perp * cos_a),
                imageproc::point::Point::new(max_proj * cos_a - max_perp * sin_a, max_proj * sin_a + max_perp * cos_a),
                imageproc::point::Point::new(min_proj * cos_a - max_perp * sin_a, min_proj * sin_a + max_perp * cos_a),
            ];
        }
    }
    best
}

fn polygon_area(poly: &[imageproc::point::Point<f32>; 4]) -> f32 {
    let mut area = 0.0;
    for i in 0..4 {
        let j = (i + 1) % 4;
        area += poly[i].x * poly[j].y - poly[j].x * poly[i].y;
    }
    area.abs() / 2.0
}

fn polygon_perimeter(poly: &[imageproc::point::Point<f32>; 4]) -> f32 {
    let mut perim = 0.0;
    for i in 0..4 {
        let j = (i + 1) % 4;
        let dx = poly[i].x - poly[j].x;
        let dy = poly[i].y - poly[j].y;
        perim += (dx * dx + dy * dy).sqrt();
    }
    perim
}

fn average_probability(prob_data: &[f32], w: usize, h: usize, poly: &[[f32; 2]; 4]) -> f32 {
    let min_x = (poly.iter().map(|p| p[0] as isize).min().unwrap_or(0)).max(0);
    let max_x = (poly.iter().map(|p| p[0] as isize).max().unwrap_or(0)).min(w as isize - 1);
    let min_y = (poly.iter().map(|p| p[1] as isize).min().unwrap_or(0)).max(0);
    let max_y = (poly.iter().map(|p| p[1] as isize).max().unwrap_or(0)).min(h as isize - 1);

    let mut sum = 0.0f64;
    let mut count = 0;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if point_in_polygon(x as f32, y as f32, poly) {
                sum += prob_data[(y * w as isize + x) as usize] as f64;
                count += 1;
            }
        }
    }
    if count > 0 { (sum / count as f64) as f32 } else { 0.0 }
}

fn point_in_polygon(px: f32, py: f32, poly: &[[f32; 2]; 4]) -> bool {
    let mut inside = false;
    let mut j = 3;
    for i in 0..4 {
        if ((poly[i][1] > py) != (poly[j][1] > py))
            && (px < (poly[j][0] - poly[i][0]) * (py - poly[i][1]) / (poly[j][1] - poly[i][1]) + poly[i][0])
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn detect_text_regions(
    session: &mut Session,
    image: &DynamicImage,
    output_name: &str,
) -> Result<Vec<TextRegion>, Box<dyn std::error::Error>> {
    let (data, padded_h, padded_w, scale_x, scale_y) = preprocess(image)?;

    let input_tensor = ort::value::Tensor::from_array((
        [1i64, 3, padded_h, padded_w],
        data,
    ))?;

    let outputs = session.run(ort::inputs!["x" => input_tensor])?;

    let (output_shape, output_slice) = outputs[output_name].try_extract_tensor::<f32>()?;
    let out_h = output_shape[2] as usize;
    let out_w = output_shape[3] as usize;

    let regions = postprocess(output_slice, out_h, out_w, image.width(), image.height(), scale_x, scale_y);
    Ok(regions)
}
