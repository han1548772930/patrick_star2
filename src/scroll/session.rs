use anyhow::Result;

use crate::model::{RectI, RgbaFrame};
use crate::platform::{CapturedScrollFrame, ScrollDirection};

use super::orb::match_frames;
use super::{OwnedPreviewPatch, TiledImage};

const MAX_STITCHED_HEIGHT: u32 = 30_000;
const MAX_STITCHED_AREA: u64 = 149_999_999;

#[derive(Clone)]
pub(crate) struct ScrollCaptureSession {
    stitched: TiledImage,
    previous: RgbaFrame,
    bounds: RectI,
    position: i64,
    max_depth: i64,
    planned_height: u32,
}

pub(crate) struct MatchedFrame {
    pub(crate) splice_id: u64,
    frame: RgbaFrame,
    shift: i32,
    growth: Option<SpliceGrowth>,
}

#[derive(Clone, Copy)]
struct SpliceGrowth {
    rows: u32,
    from_top: bool,
}

impl ScrollCaptureSession {
    pub(crate) fn new(initial: RgbaFrame) -> Self {
        let bounds = initial.bounds();
        Self {
            stitched: TiledImage::new(initial.width(), initial.height(), initial.pixels().to_vec()),
            previous: initial,
            bounds,
            position: 0,
            max_depth: 0,
            planned_height: bounds.height(),
        }
    }

    pub(crate) fn reset_baseline(&mut self, frame: RgbaFrame) {
        if frame.width() == self.previous.width() && frame.height() == self.previous.height() {
            self.previous = frame;
        }
    }

    pub(crate) fn match_frame(
        &mut self,
        splice_id: u64,
        captured: CapturedScrollFrame,
        direction: ScrollDirection,
    ) -> Result<Option<MatchedFrame>> {
        let frame = captured.frame;
        anyhow::ensure!(
            frame.width() == self.previous.width() && frame.height() == self.previous.height(),
            "scroll frame dimensions changed during capture"
        );
        if visible_pixels_identical(&self.previous, &frame) {
            return Ok(None);
        }
        let matched = match_frames(&self.previous, &frame, direction);
        self.previous = frame.clone();
        let matched = matched?;
        if matched.rejected {
            eprintln!(
                "scroll frame offset {} exceeds 60% of the viewport and was discarded",
                matched.raw_shift
            );
            return Ok(None);
        }
        Ok(Some(MatchedFrame {
            splice_id,
            frame,
            shift: matched.shift,
            growth: None,
        }))
    }

    pub(crate) fn plan_matched_frame(&mut self, matched: &mut MatchedFrame) -> Result<()> {
        if matched.shift == 0 {
            return Ok(());
        }
        self.position -= matched.shift as i64;
        let growth = if self.position < 0 {
            let rows = (-self.position) as u32;
            self.max_depth += rows as i64;
            self.position = 0;
            Some(SpliceGrowth {
                rows,
                from_top: true,
            })
        } else {
            let rows = self.position - self.max_depth;
            if rows > 0 {
                self.max_depth = self.position;
                Some(SpliceGrowth {
                    rows: rows as u32,
                    from_top: false,
                })
            } else {
                None
            }
        };
        if let Some(growth) = growth {
            ensure_height_limit(self.stitched.width(), self.planned_height, growth.rows)?;
            self.planned_height += growth.rows;
            matched.growth = Some(growth);
        }
        Ok(())
    }

    pub(crate) fn commit_matched_frame(
        &mut self,
        matched: MatchedFrame,
    ) -> Result<Option<OwnedPreviewPatch>> {
        let Some(growth) = matched.growth else {
            self.previous = matched.frame;
            return Ok(None);
        };
        ensure_height_limit(self.stitched.width(), self.stitched.height(), growth.rows)?;
        let (strip, strip_height) = crop_splice_strip(&matched.frame, growth.rows, growth.from_top);
        self.previous = matched.frame;
        let region = if growth.from_top {
            self.stitched
                .prepend_overlapping(strip, strip_height, growth.rows)
        } else {
            self.stitched
                .append_overlapping(strip, strip_height, growth.rows)
        };
        Ok(Some(OwnedPreviewPatch {
            document_width: self.stitched.width(),
            document_height: self.stitched.height(),
            rgba: self.stitched.crop_rows(region.top, region.height),
            region,
        }))
    }

    pub(crate) fn finish(self) -> RgbaFrame {
        let bounds = RectI::new(
            self.bounds.left,
            self.bounds.top,
            self.stitched.width(),
            self.stitched.height(),
        );
        RgbaFrame::new(bounds, self.stitched.into_pixels())
            .expect("scroll stitcher maintains a valid RGBA document")
    }
}

fn visible_pixels_identical(left: &RgbaFrame, right: &RgbaFrame) -> bool {
    left.pixels().len() == right.pixels().len()
        && left
            .pixels()
            .chunks_exact(4)
            .zip(right.pixels().chunks_exact(4))
            .all(|(left, right)| left[..3] == right[..3])
}

fn crop_splice_strip(frame: &RgbaFrame, grow: u32, from_top: bool) -> (Vec<u8>, u32) {
    let height = frame.height();
    let crop = (height / 2)
        .max(height.div_ceil(4).saturating_add(grow))
        .clamp(1, height);
    let top = if from_top { 0 } else { height - crop };
    let row_bytes = frame.width() as usize * 4;
    let start = top as usize * row_bytes;
    let end = start + crop as usize * row_bytes;
    (frame.pixels()[start..end].to_vec(), crop)
}

fn ensure_height_limit(width: u32, current_height: u32, added_height: u32) -> Result<()> {
    let height = current_height.saturating_add(added_height);
    anyhow::ensure!(
        height <= MAX_STITCHED_HEIGHT
            && width <= MAX_STITCHED_HEIGHT
            && width as u64 * height as u64 <= MAX_STITCHED_AREA,
        "scrolling screenshot reached the maximum height of {MAX_STITCHED_HEIGHT}px"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(rows: &[u8]) -> RgbaFrame {
        let pixels = rows
            .iter()
            .flat_map(|value| [*value, *value, *value, 255])
            .collect();
        RgbaFrame::new(RectI::new(0, 0, 1, rows.len() as u32), pixels).unwrap()
    }

    fn matched(id: u64, frame: RgbaFrame, shift: i32) -> MatchedFrame {
        MatchedFrame {
            splice_id: id,
            frame,
            shift,
            growth: None,
        }
    }

    #[test]
    fn scrolling_back_over_captured_rows_does_not_duplicate_them() {
        let mut session = ScrollCaptureSession::new(frame(&[1, 2, 3, 4]));
        for (id, shift, rows) in [
            (1, -2, &[3, 4, 5, 6][..]),
            (2, 2, &[1, 2, 3, 4][..]),
            (3, -2, &[3, 4, 5, 6][..]),
        ] {
            let mut next = matched(id, frame(rows), shift);
            session.plan_matched_frame(&mut next).unwrap();
            session.commit_matched_frame(next).unwrap();
        }
        assert_eq!(session.finish().height(), 6);
    }

    #[test]
    fn scrolling_above_the_first_frame_prepends_new_rows() {
        let mut session = ScrollCaptureSession::new(frame(&[3, 4, 5, 6]));
        let mut next = matched(1, frame(&[1, 2, 3, 4]), 2);
        session.plan_matched_frame(&mut next).unwrap();
        session.commit_matched_frame(next).unwrap();
        let output = session.finish();
        let rows = output
            .pixels()
            .chunks_exact(4)
            .map(|pixel| pixel[0])
            .collect::<Vec<_>>();
        assert_eq!(rows, [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn discontinuity_can_replace_the_matcher_baseline_without_growing() {
        let mut session = ScrollCaptureSession::new(frame(&[1, 2, 3, 4]));
        let replacement = frame(&[9, 8, 7, 6]);
        session.reset_baseline(replacement.clone());
        assert!(visible_pixels_identical(&session.previous, &replacement));
    }
}
