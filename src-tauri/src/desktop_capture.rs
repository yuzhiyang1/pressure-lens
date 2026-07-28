#[cfg(target_os = "windows")]
use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
    ptr::null_mut,
    slice,
};

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
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

#[cfg(target_os = "windows")]
struct CaptureExclusionGuard {
    window: HWND,
}

#[cfg(target_os = "windows")]
impl CaptureExclusionGuard {
    fn enable(window_handle: usize) -> Result<Self, String> {
        let window = window_handle as HWND;
        // 仅在内部 BitBlt 的极短窗口内排除悬浮窗，避免把上一帧黑洞递归采回折射纹理。
        let changed = unsafe { SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE) };
        if changed == 0 {
            return Err("无法在桌面采样期间排除悬浮窗".to_string());
        }
        Ok(Self { window })
    }
}

#[cfg(target_os = "windows")]
impl Drop for CaptureExclusionGuard {
    fn drop(&mut self) {
        // 抓取结束立即恢复，用户日常截图和录屏仍然能够看到悬浮黑洞。
        let restored = unsafe { SetWindowDisplayAffinity(self.window, WDA_NONE) };
        if restored == 0 {
            log::warn!("桌面采样结束后未能恢复悬浮窗截图状态");
        }
    }
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

#[cfg(target_os = "windows")]
pub fn capture_behind_window(
    rect: CaptureRect,
    window_handle: usize,
) -> Result<DesktopFrame, String> {
    let exclusion = CaptureExclusionGuard::enable(window_handle)?;
    let frame = capture(rect);
    // JPEG 压缩不需要继续排除窗口，尽量缩短对其他截图工具的影响时间。
    drop(exclusion);
    frame
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

    pub fn into_jpeg(self, quality: u8) -> Result<Vec<u8>, String> {
        let mut rgb = Vec::with_capacity(self.width as usize * self.height as usize * 3);
        for pixel in self.rgba.chunks_exact(4) {
            rgb.extend_from_slice(&pixel[..3]);
        }
        let mut encoded = Vec::with_capacity(self.rgba.len() / 6);
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, quality)
            .encode(
                &rgb,
                self.width,
                self.height,
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|error| format!("桌面帧 JPEG 编码失败：{error}"))?;
        Ok(encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopFrame;

    #[cfg(target_os = "windows")]
    #[test]
    fn capture_exclusion_is_restored_after_the_guard_is_dropped() {
        use std::ptr::{null, null_mut};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
            WDA_NONE, WS_OVERLAPPED,
        };

        // 复用 Windows 自带的 STATIC 窗口类，测试真实 display-affinity API，而不是模拟状态。
        let class_name: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let window = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                null(),
                WS_OVERLAPPED,
                0,
                0,
                32,
                32,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            )
        };
        assert!(!window.is_null());

        {
            let _guard =
                super::CaptureExclusionGuard::enable(window as usize).expect("应能排除自有窗口");
            let mut affinity = WDA_NONE;
            let read = unsafe { GetWindowDisplayAffinity(window, &mut affinity) };
            assert_ne!(read, 0);
            assert_eq!(affinity, WDA_EXCLUDEFROMCAPTURE);
        }

        let mut restored_affinity = WDA_EXCLUDEFROMCAPTURE;
        let read = unsafe { GetWindowDisplayAffinity(window, &mut restored_affinity) };
        assert_ne!(read, 0);
        assert_eq!(restored_affinity, WDA_NONE);
        unsafe {
            DestroyWindow(window);
        }
    }

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
    fn desktop_frame_rejects_a_pixel_buffer_with_the_wrong_length() {
        let result = DesktopFrame::from_rgba(2, 1, vec![0; 4]);

        assert!(result.is_err());
    }

    #[test]
    fn compressed_desktop_frame_is_a_valid_jpeg_payload() {
        let rgba = vec![80; 8 * 8 * 4];
        let payload = DesktopFrame::from_rgba(8, 8, rgba)
            .expect("测试帧有效")
            .into_jpeg(72)
            .expect("测试帧可压缩");

        assert_eq!(&payload[0..2], &[0xff, 0xd8]);
        assert_eq!(&payload[payload.len() - 2..], &[0xff, 0xd9]);
    }

    #[test]
    fn capture_rect_rejects_zero_or_unreasonably_large_dimensions() {
        assert!(super::CaptureRect::new(0, 0, 0, 420).is_err());
        assert!(super::CaptureRect::new(0, 0, 420, 0).is_err());
        assert!(super::CaptureRect::new(0, 0, 2049, 420).is_err());
        assert!(super::CaptureRect::new(0, 0, 420, 2049).is_err());
    }
}
