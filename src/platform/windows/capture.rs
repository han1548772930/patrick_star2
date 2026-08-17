use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;

use anyhow::{Context, Result, anyhow};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SRCCOPY,
    SelectObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use crate::model::{DesktopFrame, RectI, RgbaFrame};

pub fn capture_virtual_desktop() -> Result<DesktopFrame> {
    let bounds = virtual_desktop_bounds();
    anyhow::ensure!(
        bounds.width() > 0 && bounds.height() > 0,
        "Windows reported an empty virtual desktop"
    );
    let mut capture = RegionCapture::new(bounds)?;
    DesktopFrame::new(bounds, capture.capture_bgra()?)
}

pub struct RegionCapture {
    bounds: RectI,
    byte_len: usize,
    bits: *mut u8,
    resources: CaptureResources,
}

impl RegionCapture {
    pub fn new(bounds: RectI) -> Result<Self> {
        let desktop = virtual_desktop_bounds();
        anyhow::ensure!(
            bounds.width() > 0 && bounds.height() > 0,
            "scroll capture region is empty"
        );
        anyhow::ensure!(
            bounds.intersection(desktop) == Some(bounds),
            "scroll capture region lies outside the virtual desktop"
        );
        let width = i32::try_from(bounds.width()).context("capture width exceeds i32")?;
        let height = i32::try_from(bounds.height()).context("capture height exceeds i32")?;
        let byte_len = bounds
            .width()
            .checked_mul(bounds.height())
            .and_then(|value| value.checked_mul(4))
            .ok_or_else(|| anyhow!("capture dimensions overflow"))? as usize;

        let mut resources = CaptureResources {
            screen: unsafe { GetDC(null_mut()) },
            ..Default::default()
        };
        anyhow::ensure!(!resources.screen.is_null(), "GetDC(NULL) failed");
        resources.memory = unsafe { CreateCompatibleDC(resources.screen) };
        anyhow::ensure!(!resources.memory.is_null(), "CreateCompatibleDC failed");

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: u32::try_from(byte_len).context("capture byte length exceeds u32")?,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = null_mut();
        resources.bitmap = unsafe {
            CreateDIBSection(
                resources.screen,
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                null_mut(),
                0,
            )
        };
        anyhow::ensure!(
            !resources.bitmap.is_null() && !bits.is_null(),
            "CreateDIBSection failed"
        );
        resources.previous = unsafe { SelectObject(resources.memory, resources.bitmap) };
        anyhow::ensure!(!resources.previous.is_null(), "SelectObject failed");

        Ok(Self {
            bounds,
            byte_len,
            bits: bits.cast(),
            resources,
        })
    }

    pub fn capture_rgba(&mut self) -> Result<RgbaFrame> {
        let mut pixels = self.capture_bgra()?;
        bgra_to_rgba(&mut pixels);
        RgbaFrame::new(self.bounds, pixels).map_err(Into::into)
    }

    fn capture_bgra(&mut self) -> Result<Vec<u8>> {
        let copied = unsafe {
            BitBlt(
                self.resources.memory,
                0,
                0,
                self.bounds.width() as i32,
                self.bounds.height() as i32,
                self.resources.screen,
                self.bounds.left,
                self.bounds.top,
                SRCCOPY | CAPTUREBLT,
            )
        };
        anyhow::ensure!(copied != 0, "BitBlt desktop capture failed");
        Ok(unsafe { std::slice::from_raw_parts(self.bits, self.byte_len) }.to_vec())
    }
}

fn virtual_desktop_bounds() -> RectI {
    unsafe {
        RectI::new(
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(0) as u32,
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(0) as u32,
        )
    }
}

fn bgra_to_rgba(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
}

#[derive(Default)]
struct CaptureResources {
    screen: HDC,
    memory: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}

impl Drop for CaptureResources {
    fn drop(&mut self) {
        unsafe {
            if !self.memory.is_null() && !self.previous.is_null() {
                SelectObject(self.memory, self.previous);
            }
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap);
            }
            if !self.memory.is_null() {
                DeleteDC(self.memory);
            }
            if !self.screen.is_null() {
                ReleaseDC(null_mut::<c_void>() as HWND, self.screen);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gdi_pixels_are_converted_to_opaque_rgba() {
        let mut pixels = [3, 2, 1, 0, 30, 20, 10, 123];
        bgra_to_rgba(&mut pixels);
        assert_eq!(pixels, [1, 2, 3, 255, 10, 20, 30, 255]);
    }
}
