use std::error::Error;
use std::fmt;

use crate::model::{RectI, RgbaFrame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewRegion {
    pub top: u32,
    pub height: u32,
}

#[derive(Debug)]
pub struct PreviewPatch<'a> {
    pub document_width: u32,
    pub document_height: u32,
    pub region: PreviewRegion,
    pub rgba: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchError {
    FrameWidthChanged { expected: u32, actual: u32 },
    AppendExceedsFrame { rows: u32, frame_height: u32 },
    HeightOverflow,
    PixelLengthOverflow,
}

impl fmt::Display for StitchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameWidthChanged { expected, actual } => {
                write!(
                    formatter,
                    "scroll frame width changed from {expected} to {actual}"
                )
            }
            Self::AppendExceedsFrame { rows, frame_height } => write!(
                formatter,
                "cannot append {rows} rows from a {frame_height}-row scroll frame"
            ),
            Self::HeightOverflow => formatter.write_str("stitched image height overflow"),
            Self::PixelLengthOverflow => formatter.write_str("stitched pixel length overflow"),
        }
    }
}

impl Error for StitchError {}

#[derive(Debug)]
pub struct StitchDocument {
    bounds: RectI,
    pixels: Vec<u8>,
}

impl StitchDocument {
    pub fn from_frame(frame: &RgbaFrame) -> Self {
        Self {
            bounds: frame.bounds(),
            pixels: frame.pixels().to_vec(),
        }
    }

    pub const fn width(&self) -> u32 {
        self.bounds.width()
    }

    pub const fn height(&self) -> u32 {
        self.bounds.height()
    }

    pub fn append_bottom(
        &mut self,
        frame: &RgbaFrame,
        rows: u32,
    ) -> Result<PreviewRegion, StitchError> {
        if frame.width() != self.width() {
            return Err(StitchError::FrameWidthChanged {
                expected: self.width(),
                actual: frame.width(),
            });
        }
        if rows == 0 || rows > frame.height() {
            return Err(StitchError::AppendExceedsFrame {
                rows,
                frame_height: frame.height(),
            });
        }

        let old_height = self.height();
        let new_height = old_height
            .checked_add(rows)
            .ok_or(StitchError::HeightOverflow)?;
        let row_bytes = self.width() as usize * 4;
        let added_bytes = rows as usize * row_bytes;
        self.pixels
            .len()
            .checked_add(added_bytes)
            .ok_or(StitchError::PixelLengthOverflow)?;
        self.pixels.reserve(added_bytes);
        let source_start = (frame.height() - rows) as usize * row_bytes;
        self.pixels
            .extend_from_slice(&frame.pixels()[source_start..]);
        self.bounds.height = new_height;
        Ok(PreviewRegion {
            top: old_height,
            height: rows,
        })
    }

    pub fn preview_patch(&self, region: PreviewRegion) -> Option<PreviewPatch<'_>> {
        let bottom = region.top.checked_add(region.height)?;
        if region.height == 0 || bottom > self.height() {
            return None;
        }
        let row_bytes = self.width() as usize * 4;
        let start = region.top as usize * row_bytes;
        let end = bottom as usize * row_bytes;
        Some(PreviewPatch {
            document_width: self.width(),
            document_height: self.height(),
            region,
            rgba: &self.pixels[start..end],
        })
    }

    pub fn into_frame(self) -> RgbaFrame {
        RgbaFrame::new(self.bounds, self.pixels)
            .expect("stitch document maintains a valid RGBA buffer")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_frame(rows: &[u8]) -> RgbaFrame {
        let pixels = rows
            .iter()
            .flat_map(|value| [*value, *value, *value, 255])
            .collect();
        RgbaFrame::new(RectI::new(-4, 9, 1, rows.len() as u32), pixels).unwrap()
    }

    #[test]
    fn append_uses_only_new_bottom_rows_and_preserves_origin() {
        let initial = row_frame(&[1, 2, 3, 4]);
        let next = row_frame(&[3, 4, 5, 6]);
        let mut document = StitchDocument::from_frame(&initial);
        let dirty = document.append_bottom(&next, 2).unwrap();
        let output = document.into_frame();

        assert_eq!(dirty, PreviewRegion { top: 4, height: 2 });
        assert_eq!(output.bounds(), RectI::new(-4, 9, 1, 6));
        assert_eq!(
            output
                .pixels()
                .chunks_exact(4)
                .map(|pixel| pixel[0])
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn preview_patch_borrows_only_the_dirty_rows() {
        let initial = row_frame(&[1, 2, 3]);
        let next = row_frame(&[2, 3, 4]);
        let mut document = StitchDocument::from_frame(&initial);
        let region = document.append_bottom(&next, 1).unwrap();
        let patch = document.preview_patch(region).unwrap();

        assert_eq!(patch.document_height, 4);
        assert_eq!(patch.region, PreviewRegion { top: 3, height: 1 });
        assert_eq!(patch.rgba, &[4, 4, 4, 255]);
    }
}
