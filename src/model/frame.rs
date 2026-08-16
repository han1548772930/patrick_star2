use super::RectI;

/// A top-down BGRA8 desktop frame in physical pixels.
#[derive(Debug)]
pub struct DesktopFrame {
    pub bounds: RectI,
    pub pixels: Vec<u8>,
}

impl DesktopFrame {
    pub fn new(bounds: RectI, pixels: Vec<u8>) -> anyhow::Result<Self> {
        let expected = bounds
            .width()
            .checked_mul(bounds.height())
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| anyhow::anyhow!("desktop frame dimensions overflow"))?
            as usize;
        anyhow::ensure!(
            bounds.width() > 0 && bounds.height() > 0,
            "desktop frame must not be empty"
        );
        anyhow::ensure!(
            pixels.len() == expected,
            "invalid desktop frame length: expected {expected}, got {}",
            pixels.len()
        );
        Ok(Self { bounds, pixels })
    }

    pub fn pixel_at_local(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        if x < 0 || y < 0 || x >= self.bounds.width() as i32 || y >= self.bounds.height() as i32 {
            return None;
        }
        let offset = (y as usize * self.bounds.width() as usize + x as usize) * 4;
        self.pixels[offset..offset + 4].try_into().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_pixel_length() {
        let bounds = RectI::new(-10, 5, 2, 3);
        assert!(DesktopFrame::new(bounds, vec![0; 24]).is_ok());
        assert!(DesktopFrame::new(bounds, vec![0; 23]).is_err());
    }

    #[test]
    fn reads_top_down_bgra_pixels() {
        let frame =
            DesktopFrame::new(RectI::new(0, 0, 2, 1), vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(frame.pixel_at_local(1, 0), Some([5, 6, 7, 8]));
        assert_eq!(frame.pixel_at_local(-1, 0), None);
    }
}
