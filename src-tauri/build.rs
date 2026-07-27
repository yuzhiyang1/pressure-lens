use std::{fs, path::Path};

fn main() {
    // Tauri 的 Windows 资源编译要求存在 ICO。这里生成一个确定性的本地图标，
    // 避免项目依赖无法追踪来源的二进制占位资产。
    let icon_path = Path::new("icons/icon.ico");
    if !icon_path.exists() {
        fs::create_dir_all("icons").expect("应可创建图标目录");
        fs::write(icon_path, build_icon()).expect("应可生成 Windows 图标");
    }
    tauri_build::build()
}

fn build_icon() -> Vec<u8> {
    const SIZE: usize = 32;
    const PIXEL_BYTES: usize = SIZE * SIZE * 4;
    const MASK_BYTES: usize = SIZE * 4;
    const IMAGE_BYTES: usize = 40 + PIXEL_BYTES + MASK_BYTES;

    let mut output = Vec::with_capacity(22 + IMAGE_BYTES);
    output.extend_from_slice(&0u16.to_le_bytes()); // ICONDIR.reserved
    output.extend_from_slice(&1u16.to_le_bytes()); // ICONDIR.type = icon
    output.extend_from_slice(&1u16.to_le_bytes()); // ICONDIR.count
    output.extend_from_slice(&[SIZE as u8, SIZE as u8, 0, 0]);
    output.extend_from_slice(&1u16.to_le_bytes()); // color planes
    output.extend_from_slice(&32u16.to_le_bytes());
    output.extend_from_slice(&(IMAGE_BYTES as u32).to_le_bytes());
    output.extend_from_slice(&22u32.to_le_bytes());

    // ICO 内部使用高度翻倍的 BGRA DIB：上半部分是图像，下半部分是透明掩码。
    output.extend_from_slice(&40u32.to_le_bytes());
    output.extend_from_slice(&(SIZE as i32).to_le_bytes());
    output.extend_from_slice(&((SIZE * 2) as i32).to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&32u16.to_le_bytes());
    output.extend_from_slice(&0u32.to_le_bytes());
    output.extend_from_slice(&(PIXEL_BYTES as u32).to_le_bytes());
    output.extend_from_slice(&[0; 16]);

    for y in (0..SIZE).rev() {
        for x in 0..SIZE {
            let dx = x as f64 - 15.5;
            let dy = (y as f64 - 15.5) * 1.7;
            let radius = (dx * dx + dy * dy).sqrt();
            let (red, green, blue, alpha) = if radius < 7.5 {
                (0, 0, 0, 255)
            } else if radius < 11.5 {
                (255, 126, 50, 255)
            } else if radius < 13.5 {
                (255, 72, 38, 130)
            } else {
                (0, 0, 0, 0)
            };
            output.extend_from_slice(&[blue, green, red, alpha]);
        }
    }
    output.extend_from_slice(&vec![0; MASK_BYTES]);
    output
}
