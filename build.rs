fn draw_icon_dot(pixels: &mut [u8], size: usize, cx: f32, cy: f32, radius: f32) {
    let min_x = (cx - radius).floor() as i32;
    let max_x = (cx + radius).ceil() as i32;
    let min_y = (cy - radius).floor() as i32;
    let max_y = (cy + radius).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
                continue;
            }
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            if dx * dx + dy * dy <= radius * radius {
                let index = ((y as usize * size + x as usize) * 4) as usize;
                pixels[index] = 0xf1;
                pixels[index + 1] = 0x6f;
                pixels[index + 2] = 0x63;
                pixels[index + 3] = 0xff;
            }
        }
    }
}

fn draw_icon_line(pixels: &mut [u8], size: usize, x1: f32, y1: f32, x2: f32, y2: f32, stroke: f32) {
    let steps = ((x2 - x1).abs().max((y2 - y1).abs()) * 2.0).max(1.0) as i32;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = x1 + (x2 - x1) * t;
        let y = y1 + (y2 - y1) * t;
        draw_icon_dot(pixels, size, x, y, stroke / 2.0);
    }
}

fn generate_ico_file() {
    let size: usize = 64;
    let mut pixels = vec![0u8; size * size * 4];
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let radius = size as f32 * 0.30;
    let stroke = (size as f32 * 0.13).max(4.0);
    draw_icon_line(&mut pixels, size, cx, cy - radius, cx, cy + radius, stroke);
    draw_icon_line(
        &mut pixels,
        size,
        cx - radius * 0.9,
        cy - radius * 0.5,
        cx + radius * 0.9,
        cy + radius * 0.5,
        stroke,
    );
    draw_icon_line(
        &mut pixels,
        size,
        cx + radius * 0.9,
        cy - radius * 0.5,
        cx - radius * 0.9,
        cy + radius * 0.5,
        stroke,
    );

    let mut bmp_pixels = vec![0u8; size * size * 4];
    for y in 0..size {
        let src_y = size - 1 - y;
        let src_offset = src_y * size * 4;
        let dest_offset = y * size * 4;
        bmp_pixels[dest_offset..(dest_offset + size * 4)]
            .copy_from_slice(&pixels[src_offset..(src_offset + size * 4)]);
    }

    let and_mask_size = ((size + 31) / 32) * 4 * size;
    let img_data_size = 40 + bmp_pixels.len() + and_mask_size;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    bytes.push(size as u8);
    bytes.push(size as u8);
    bytes.push(0);
    bytes.push(0);
    bytes.extend_from_slice(&[1, 0]);
    bytes.extend_from_slice(&[32, 0]);
    bytes.extend_from_slice(&(img_data_size as u32).to_le_bytes());
    bytes.extend_from_slice(&22u32.to_le_bytes());

    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&(size as i32).to_le_bytes());
    bytes.extend_from_slice(&(size as i32 * 2).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(bmp_pixels.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());

    bytes.extend_from_slice(&bmp_pixels);
    bytes.extend_from_slice(&vec![0u8; and_mask_size]);

    let _ = std::fs::create_dir_all("assets");
    let _ = std::fs::write("assets/aster.ico", bytes);
}

fn main() {
    println!("cargo:rustc-link-lib=advapi32");
    generate_ico_file();
    embed_resource::compile("aster.rc", embed_resource::NONE);
}
