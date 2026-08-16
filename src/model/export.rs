use std::error::Error;
use std::fmt;

use super::{Rect, RectI};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureIntent {
    Clipboard,
    Save,
    Pin,
    ExtractText,
    ScrollCapture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportError {
    EmptyDesktop,
    NonFiniteSelection,
    EmptySelection,
    CoordinateOverflow,
    PixelLengthOverflow,
    InvalidPixelLength { expected: usize, actual: usize },
}

impl fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDesktop => formatter.write_str("export desktop must not be empty"),
            Self::NonFiniteSelection => {
                formatter.write_str("export selection coordinates must be finite")
            }
            Self::EmptySelection => formatter.write_str("export selection must not be empty"),
            Self::CoordinateOverflow => formatter.write_str("export coordinates overflow"),
            Self::PixelLengthOverflow => formatter.write_str("export pixel length overflow"),
            Self::InvalidPixelLength { expected, actual } => write!(
                formatter,
                "invalid RGBA frame length: expected {expected}, got {actual}"
            ),
        }
    }
}

impl Error for ExportError {}

/// Integer crop derived from a physical-pixel selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportRegion {
    local: RectI,
    desktop: RectI,
}

impl ExportRegion {
    pub fn from_selection(selection: Rect, desktop: RectI) -> Result<Self, ExportError> {
        if desktop.width() == 0 || desktop.height() == 0 {
            return Err(ExportError::EmptyDesktop);
        }
        if ![
            selection.left,
            selection.top,
            selection.right,
            selection.bottom,
        ]
        .into_iter()
        .all(f32::is_finite)
        {
            return Err(ExportError::NonFiniteSelection);
        }

        let clipped = selection.normalized().clamped(desktop.local_bounds());
        let left = clipped.left.floor() as i64;
        let top = clipped.top.floor() as i64;
        let right = clipped.right.ceil() as i64;
        let bottom = clipped.bottom.ceil() as i64;
        if right <= left || bottom <= top {
            return Err(ExportError::EmptySelection);
        }

        let left = i32::try_from(left).map_err(|_| ExportError::CoordinateOverflow)?;
        let top = i32::try_from(top).map_err(|_| ExportError::CoordinateOverflow)?;
        let width =
            u32::try_from(right - i64::from(left)).map_err(|_| ExportError::CoordinateOverflow)?;
        let height =
            u32::try_from(bottom - i64::from(top)).map_err(|_| ExportError::CoordinateOverflow)?;
        let desktop_left = desktop
            .left
            .checked_add(left)
            .ok_or(ExportError::CoordinateOverflow)?;
        let desktop_top = desktop
            .top
            .checked_add(top)
            .ok_or(ExportError::CoordinateOverflow)?;

        Ok(Self {
            local: RectI::new(left, top, width, height),
            desktop: RectI::new(desktop_left, desktop_top, width, height),
        })
    }

    pub const fn local(self) -> RectI {
        self.local
    }

    pub const fn desktop(self) -> RectI {
        self.desktop
    }

    pub fn local_rect(self) -> Rect {
        Rect::new(
            self.local.left as f32,
            self.local.top as f32,
            self.local.right() as f32,
            self.local.bottom() as f32,
        )
    }
}

/// A top-down RGBA8 image positioned in virtual-desktop physical pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaFrame {
    bounds: RectI,
    pixels: Vec<u8>,
}

impl RgbaFrame {
    pub fn new(bounds: RectI, pixels: Vec<u8>) -> Result<Self, ExportError> {
        if bounds.width() == 0 || bounds.height() == 0 {
            return Err(ExportError::EmptySelection);
        }
        let expected = (bounds.width() as usize)
            .checked_mul(bounds.height() as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(ExportError::PixelLengthOverflow)?;
        if pixels.len() != expected {
            return Err(ExportError::InvalidPixelLength {
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Self { bounds, pixels })
    }

    pub const fn width(&self) -> u32 {
        self.bounds.width()
    }

    pub const fn bounds(&self) -> RectI {
        self.bounds
    }

    pub const fn height(&self) -> u32 {
        self.bounds.height()
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_clipped_and_rounded_outward() {
        let desktop = RectI::new(-1920, -200, 3840, 1280);
        let region =
            ExportRegion::from_selection(Rect::new(-10.0, 20.2, 120.1, 80.01), desktop).unwrap();

        assert_eq!(region.local(), RectI::new(0, 20, 121, 61));
        assert_eq!(region.desktop(), RectI::new(-1920, -180, 121, 61));
        assert_eq!(region.local_rect(), Rect::new(0.0, 20.0, 121.0, 81.0));
    }

    #[test]
    fn negative_virtual_desktop_origin_is_preserved() {
        let desktop = RectI::new(-2560, -1440, 5120, 2880);
        let region =
            ExportRegion::from_selection(Rect::new(2500.0, 1400.0, 2600.0, 1500.0), desktop)
                .unwrap();

        assert_eq!(region.local(), RectI::new(2500, 1400, 100, 100));
        assert_eq!(region.desktop(), RectI::new(-60, -40, 100, 100));
    }

    #[test]
    fn empty_and_non_finite_selections_are_rejected() {
        let desktop = RectI::new(0, 0, 100, 100);
        assert_eq!(
            ExportRegion::from_selection(Rect::new(10.0, 10.0, 10.0, 20.0), desktop),
            Err(ExportError::EmptySelection)
        );
        assert_eq!(
            ExportRegion::from_selection(Rect::new(f32::NAN, 0.0, 10.0, 10.0), desktop),
            Err(ExportError::NonFiniteSelection)
        );
    }

    #[test]
    fn rgba_frame_requires_exact_top_down_storage() {
        let bounds = RectI::new(-10, 20, 2, 2);
        let frame = RgbaFrame::new(bounds, vec![0; 16]).unwrap();
        assert_eq!(frame.bounds, bounds);
        assert_eq!(frame.width(), 2);
        assert_eq!(frame.height(), 2);
        assert_eq!(frame.pixels().len(), 16);
        assert!(matches!(
            RgbaFrame::new(bounds, vec![0; 15]),
            Err(ExportError::InvalidPixelLength {
                expected: 16,
                actual: 15
            })
        ));
    }
}
