#[cfg(target_os = "windows")]
use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
    ptr::null_mut,
    slice,
};

#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};

pub struct DesktopFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Clone, Copy)]
pub struct CaptureRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl CaptureRect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Result<Self, String> {
        const MAX_CAPTURE_EDGE: u32 = 2048;
        if width == 0 || height == 0 {
            return Err("桌面捕获尺寸不能为 0".to_string());
        }
        if width > MAX_CAPTURE_EDGE || height > MAX_CAPTURE_EDGE {
            return Err(format!("桌面捕获尺寸超过安全上限 {MAX_CAPTURE_EDGE}px"));
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }
}

fn convert_bgra_to_rgba(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        // Windows 桌面 DIB 的 Alpha 通道未定义，显式设为不透明，避免出现随机透明像素。
        pixel[3] = 255;
    }
}

#[cfg(target_os = "windows")]
pub fn capture(rect: CaptureRect) -> Result<DesktopFrame, String> {
    let byte_len = (rect.width as usize)
        .checked_mul(rect.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "桌面捕获尺寸溢出".to_string())?;

    // GDI 资源全部在本函数内创建和释放；像素复制进 Vec 后不再持有桌面句柄。
    unsafe {
        let screen_dc = GetDC(null_mut());
        if screen_dc.is_null() {
            return Err("无法取得桌面绘图上下文".to_string());
        }

        let memory_dc = CreateCompatibleDC(screen_dc);
        if memory_dc.is_null() {
            ReleaseDC(null_mut(), screen_dc);
            return Err("无法创建桌面捕获缓冲区".to_string());
        }

        let mut bitmap_info: BITMAPINFO = zeroed();
        bitmap_info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bitmap_info.bmiHeader.biWidth = rect.width as i32;
        // 负高度创建自上而下的 DIB，像素顺序可直接交给 WebGL。
        bitmap_info.bmiHeader.biHeight = -(rect.height as i32);
        bitmap_info.bmiHeader.biPlanes = 1;
        bitmap_info.bmiHeader.biBitCount = 32;
        bitmap_info.bmiHeader.biCompression = BI_RGB;

        let mut raw_pixels: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(
            screen_dc,
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut raw_pixels,
            null_mut(),
            0,
        );
        if bitmap.is_null() || raw_pixels.is_null() {
            DeleteDC(memory_dc);
            ReleaseDC(null_mut(), screen_dc);
            return Err("无法创建桌面像素缓冲区".to_string());
        }

        let previous_object = SelectObject(memory_dc, bitmap);
        if previous_object.is_null() {
            DeleteObject(bitmap);
            DeleteDC(memory_dc);
            ReleaseDC(null_mut(), screen_dc);
            return Err("无法选择桌面像素缓冲区".to_string());
        }

        let copied = BitBlt(
            memory_dc,
            0,
            0,
            rect.width as i32,
            rect.height as i32,
            screen_dc,
            rect.x,
            rect.y,
            SRCCOPY | CAPTUREBLT,
        );

        let result = if copied == 0 {
            Err("Windows 桌面区域捕获失败".to_string())
        } else {
            let mut rgba = slice::from_raw_parts(raw_pixels.cast::<u8>(), byte_len).to_vec();
            convert_bgra_to_rgba(&mut rgba);
            DesktopFrame::from_rgba(rect.width, rect.height, rgba)
        };

        SelectObject(memory_dc, previous_object);
        DeleteObject(bitmap);
        DeleteDC(memory_dc);
        ReleaseDC(null_mut(), screen_dc);
        result
    }
}

impl DesktopFrame {
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        let expected_len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "桌面帧尺寸溢出".to_string())?;
        if rgba.len() != expected_len {
            return Err(format!(
                "桌面帧像素长度不匹配：期望 {expected_len}，实际 {}",
                rgba.len()
            ));
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    pub fn into_ipc_payload(self) -> Vec<u8> {
        // 帧头固定为宽、高两个小端 u32，后面直接拼接 RGBA，避免 JSON/Base64 膨胀。
        let mut payload = Vec::with_capacity(8 + self.rgba.len());
        payload.extend_from_slice(&self.width.to_le_bytes());
        payload.extend_from_slice(&self.height.to_le_bytes());
        payload.extend_from_slice(&self.rgba);
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopFrame;

    #[test]
    fn windows_bgra_pixels_are_converted_to_opaque_rgba() {
        let mut pixels = vec![
            10, 20, 30, 0, // B, G, R, A
            40, 50, 60, 128,
        ];

        super::convert_bgra_to_rgba(&mut pixels);

        assert_eq!(pixels, vec![30, 20, 10, 255, 60, 50, 40, 255]);
    }

    #[test]
    fn desktop_frame_payload_contains_dimensions_followed_by_rgba_pixels() {
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let payload = DesktopFrame::from_rgba(2, 1, rgba.clone())
            .expect("有效 RGBA 帧应当可以构建")
            .into_ipc_payload();

        assert_eq!(&payload[0..4], &2_u32.to_le_bytes());
        assert_eq!(&payload[4..8], &1_u32.to_le_bytes());
        assert_eq!(&payload[8..], rgba);
    }

    #[test]
    fn desktop_frame_rejects_a_pixel_buffer_with_the_wrong_length() {
        let result = DesktopFrame::from_rgba(2, 1, vec![0; 4]);

        assert!(result.is_err());
    }

    #[test]
    fn capture_rect_rejects_zero_or_unreasonably_large_dimensions() {
        assert!(super::CaptureRect::new(0, 0, 0, 420).is_err());
        assert!(super::CaptureRect::new(0, 0, 420, 0).is_err());
        assert!(super::CaptureRect::new(0, 0, 2049, 420).is_err());
        assert!(super::CaptureRect::new(0, 0, 420, 2049).is_err());
    }
}
