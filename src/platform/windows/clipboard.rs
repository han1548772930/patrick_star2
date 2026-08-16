use std::mem::size_of;
use std::ptr::{copy_nonoverlapping, null_mut};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use windows_sys::Win32::Foundation::{GetLastError, GlobalFree, HGLOBAL};
use windows_sys::Win32::Graphics::Gdi::{BI_BITFIELDS, BITMAPV5HEADER, LCS_GM_IMAGES};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::CF_DIBV5;
use windows_sys::Win32::UI::ColorSystem::LCS_sRGB;

use crate::model::RgbaFrame;

const CLIPBOARD_RETRIES: usize = 8;
const RETRY_DELAY: Duration = Duration::from_millis(5);

pub fn write_image(image: &RgbaFrame) -> Result<()> {
    let header = bitmap_header(image)?;
    let header_bytes = size_of::<BITMAPV5HEADER>();
    let allocation_size = header_bytes
        .checked_add(image.pixels().len())
        .ok_or_else(|| anyhow!("clipboard image allocation overflow"))?;
    let mut memory = GlobalMemory::allocate(allocation_size)?;

    unsafe {
        let destination = GlobalLock(memory.handle);
        if destination.is_null() {
            return Err(last_error("GlobalLock clipboard image"));
        }
        copy_nonoverlapping(
            (&header as *const BITMAPV5HEADER).cast::<u8>(),
            destination.cast::<u8>(),
            header_bytes,
        );
        let bitmap = std::slice::from_raw_parts_mut(
            destination.cast::<u8>().add(header_bytes),
            image.pixels().len(),
        );
        write_bgra(bitmap, image.pixels());
        GlobalUnlock(memory.handle);
    }

    let _clipboard = ClipboardGuard::open()?;
    if unsafe { EmptyClipboard() } == 0 {
        return Err(last_error("EmptyClipboard"));
    }
    if unsafe { SetClipboardData(CF_DIBV5 as u32, memory.handle) }.is_null() {
        return Err(last_error("SetClipboardData(CF_DIBV5)"));
    }
    memory.release_to_clipboard();
    Ok(())
}

pub fn write_text(text: &str) -> Result<()> {
    let payload = unicode_text_payload(text);
    let allocation_size = payload
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| anyhow!("clipboard text allocation overflow"))?;
    let mut memory = GlobalMemory::allocate(allocation_size)?;

    unsafe {
        let destination = GlobalLock(memory.handle);
        if destination.is_null() {
            return Err(last_error("GlobalLock clipboard text"));
        }
        copy_nonoverlapping(
            payload.as_ptr().cast::<u8>(),
            destination.cast::<u8>(),
            allocation_size,
        );
        GlobalUnlock(memory.handle);
    }

    let _clipboard = ClipboardGuard::open()?;
    if unsafe { EmptyClipboard() } == 0 {
        return Err(last_error("EmptyClipboard"));
    }
    const CF_UNICODETEXT: u32 = 13;
    if unsafe { SetClipboardData(CF_UNICODETEXT, memory.handle) }.is_null() {
        return Err(last_error("SetClipboardData(CF_UNICODETEXT)"));
    }
    memory.release_to_clipboard();
    Ok(())
}

fn unicode_text_payload(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn bitmap_header(image: &RgbaFrame) -> Result<BITMAPV5HEADER> {
    let width = i32::try_from(image.width()).context("clipboard image width exceeds i32")?;
    let height = i32::try_from(image.height()).context("clipboard image height exceeds i32")?;
    let image_size =
        u32::try_from(image.pixels().len()).context("clipboard image byte length exceeds DIBV5")?;
    Ok(BITMAPV5HEADER {
        bV5Size: size_of::<BITMAPV5HEADER>() as u32,
        bV5Width: width,
        bV5Height: -height,
        bV5Planes: 1,
        bV5BitCount: 32,
        bV5Compression: BI_BITFIELDS,
        bV5SizeImage: image_size,
        bV5RedMask: 0x00ff_0000,
        bV5GreenMask: 0x0000_ff00,
        bV5BlueMask: 0x0000_00ff,
        bV5AlphaMask: 0xff00_0000,
        bV5CSType: LCS_sRGB as u32,
        bV5Intent: LCS_GM_IMAGES as u32,
        ..Default::default()
    })
}

fn write_bgra(destination: &mut [u8], source: &[u8]) {
    debug_assert_eq!(destination.len(), source.len());
    for (target, pixel) in destination.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
        target.copy_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
}

struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> Result<Self> {
        for attempt in 0..CLIPBOARD_RETRIES {
            if unsafe { OpenClipboard(null_mut()) } != 0 {
                return Ok(Self);
            }
            if attempt + 1 < CLIPBOARD_RETRIES {
                std::thread::sleep(RETRY_DELAY);
            }
        }
        Err(last_error("OpenClipboard"))
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

struct GlobalMemory {
    handle: HGLOBAL,
}

impl GlobalMemory {
    fn allocate(bytes: usize) -> Result<Self> {
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
        if handle.is_null() {
            return Err(last_error("GlobalAlloc clipboard image"));
        }
        Ok(Self { handle })
    }

    fn release_to_clipboard(&mut self) {
        self.handle = null_mut();
    }
}

impl Drop for GlobalMemory {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { GlobalFree(self.handle) };
        }
    }
}

fn last_error(operation: &str) -> anyhow::Error {
    anyhow!("{operation} failed with Win32 error {}", unsafe {
        GetLastError()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RectI;

    #[test]
    fn dibv5_header_describes_top_down_bgra_with_alpha() {
        let image = RgbaFrame::new(RectI::new(-10, 20, 2, 3), vec![0; 24]).unwrap();
        let header = bitmap_header(&image).unwrap();

        assert_eq!(header.bV5Size as usize, size_of::<BITMAPV5HEADER>());
        assert_eq!(header.bV5Width, 2);
        assert_eq!(header.bV5Height, -3);
        assert_eq!(header.bV5BitCount, 32);
        assert_eq!(header.bV5Compression, BI_BITFIELDS);
        assert_eq!(header.bV5SizeImage, 24);
        assert_eq!(header.bV5RedMask, 0x00ff_0000);
        assert_eq!(header.bV5AlphaMask, 0xff00_0000);
    }

    #[test]
    fn rgba_pixels_are_written_as_standard_bgra_dib_pixels() {
        let mut destination = [0; 8];
        write_bgra(&mut destination, &[1, 2, 3, 4, 10, 20, 30, 40]);
        assert_eq!(destination, [3, 2, 1, 4, 30, 20, 10, 40]);
    }

    #[test]
    fn unicode_text_has_one_terminal_nul_and_preserves_non_bmp_characters() {
        let payload = unicode_text_payload("OCR \u{1f680}");
        assert_eq!(payload.last(), Some(&0));
        assert_eq!(payload.iter().filter(|unit| **unit == 0).count(), 1);
        assert_eq!(
            String::from_utf16(&payload[..payload.len() - 1]).unwrap(),
            "OCR \u{1f680}"
        );
    }
}
