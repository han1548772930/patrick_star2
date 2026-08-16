use crate::model::RgbaFrame;

const SAMPLE_COLUMNS: usize = 32;
const SAMPLE_ROWS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameFingerprint {
    width: u32,
    height: u32,
    luminance: Box<[u8]>,
}

impl FrameFingerprint {
    pub fn from_frame(frame: &RgbaFrame) -> Self {
        let mut luminance = Vec::with_capacity(SAMPLE_COLUMNS * SAMPLE_ROWS);
        for sample_y in 0..SAMPLE_ROWS {
            let y = sample_coordinate(sample_y, SAMPLE_ROWS, frame.height());
            for sample_x in 0..SAMPLE_COLUMNS {
                let x = sample_coordinate(sample_x, SAMPLE_COLUMNS, frame.width());
                let offset = ((y * frame.width() + x) * 4) as usize;
                let pixel = &frame.pixels()[offset..offset + 3];
                luminance.push(rgb_luminance(pixel[0], pixel[1], pixel[2]));
            }
        }
        Self {
            width: frame.width(),
            height: frame.height(),
            luminance: luminance.into_boxed_slice(),
        }
    }

    /// Mean absolute luminance difference in the range 0..=255.
    pub fn difference(&self, other: &Self) -> Option<f32> {
        if self.width != other.width || self.height != other.height {
            return None;
        }
        let total = self
            .luminance
            .iter()
            .zip(other.luminance.iter())
            .map(|(left, right)| left.abs_diff(*right) as u64)
            .sum::<u64>();
        Some(total as f32 / self.luminance.len() as f32)
    }
}

fn sample_coordinate(index: usize, samples: usize, extent: u32) -> u32 {
    (((index * 2 + 1) as u64 * extent as u64) / (samples as u64 * 2))
        .min(extent.saturating_sub(1) as u64) as u32
}

fn rgb_luminance(red: u8, green: u8, blue: u8) -> u8 {
    ((red as u32 * 77 + green as u32 * 150 + blue as u32 * 29 + 128) >> 8) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RectI;

    fn frame(width: u32, height: u32, value: u8) -> RgbaFrame {
        let mut pixels = vec![value; width as usize * height as usize * 4];
        for alpha in pixels.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
        RgbaFrame::new(RectI::new(0, 0, width, height), pixels).unwrap()
    }

    #[test]
    fn identical_frames_have_zero_difference() {
        let left = FrameFingerprint::from_frame(&frame(80, 50, 42));
        let right = FrameFingerprint::from_frame(&frame(80, 50, 42));
        assert_eq!(left.difference(&right), Some(0.0));
    }

    #[test]
    fn dimensions_must_match() {
        let left = FrameFingerprint::from_frame(&frame(80, 50, 42));
        let right = FrameFingerprint::from_frame(&frame(81, 50, 42));
        assert_eq!(left.difference(&right), None);
    }

    #[test]
    fn luminance_difference_is_measured_in_pixel_units() {
        let left = FrameFingerprint::from_frame(&frame(80, 50, 10));
        let right = FrameFingerprint::from_frame(&frame(80, 50, 30));
        assert!((left.difference(&right).unwrap() - 20.0).abs() < f32::EPSILON);
    }
}
